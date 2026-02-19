from __future__ import annotations
from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum, auto
import typing
from builders import builder_stack, current_module, runtime
from helpers import BOOL, CHAR, POINTER, INT, get_type_size, cmp_function
from llvmlite.ir import (
    CastInstr,
    Constant,
    Function,
    FunctionType,
    IRBuilder,
    IdentifiedStructType,
    LiteralStructType,
    Value,
    Type,
)
from loguru import logger

type_db: dict[str, BaseMetatype] = {}


def get_type(trepr: TypeRepr) -> BaseMetatype:
    str_repr = str(trepr)
    if str_repr in type_db:
        return type_db[str_repr]

    generics: list[BaseMetatype] = []
    for generic in trepr.generics:
        if isinstance(generic, list):
            generics.append(TupleMetatype(*[get_type(sub) for sub in generic]))
        else: 
            generics.append(get_type(generic))
    
    out = trepr.base.create_metatype(generics)
    type_db[str_repr] = out 
    return out


class TypeRepr:
    def __init__(self, base: type[ObjectType], generics: list[TypeRepr | list[TypeRepr]] = []) -> None:
        self.base = base 
        self.generics = generics

    def __str__(self) -> str:
        generics = f"<{", ".join([str(t) for t in self.generics])}>" if self.generics else ""
        return f"{self.base.NAME}{generics}"
    

@dataclass
class TypeHeader:
    """
    Specifies layout but not implementation of a class. 

    In memory, only parameters and *then* members will be stored in the
    order defined in their respective dictionaries. 
    """

    # TODO: Replace parameters and members with AnnotatedType wrapper.

    name: str
    generics: dict[str, BaseMetatype]
    inherits: list[BaseMetatype]
    parameters: dict[str, BaseMetatype]
    members: dict[str, BaseMetatype]
    static_members: dict[str, ObjectType]
    is_abstract: bool


class ObjectType:
    """
    Represents a *specific* object, such as a local variable, or an
    argument to a function.
    """

    NAME = "Object"
    IS_REFCOUNTED = True

    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_object",
    ) -> None:
        """
        Docstring for __init__

        :param value: The value, which is a pointer to the place in memory where the value is stored.
        :param allocate: If this is true, value is not a pointer and new memory will be allocated. Otherwise, value is a pointer.
        :param reference: When setting a member or result of a function, this is true, and value is a pointer to the element, regardless if it is refcounted.
        :param dbg_name: An optional name for the variable.
        """

        self.name = dbg_name
        self.reference = reference
        self.typ = typ

        # This ends up with a valid pointer to the variable, no matter
        # the method.
        builder = builder_stack[-1]
        if not self.reference:
            if self.IS_REFCOUNTED:
                if allocate:
                    builder.comment(f"Allocating a reference counted variable in memory for {dbg_name}:")
                    memory = builder.call(
                        runtime.rc_alloc_func,
                        [INT(get_type_size(self.typ.llvm_type))],
                        f"{self.name}_memory",
                    )
                    self.value: Value = builder.bitcast(memory, self.LLVM_TYPE.as_pointer(), f"{self.name}_pointer")  # type: ignore
                else:
                    self.value: Value = value
                builder.store(value, self.value)
            else:
                builder.comment(f"Allocating stack memory for {dbg_name} (possibly copying).")
                self.value: Value = builder.alloca(self.typ.llvm_type, name=f"{self.name}_stack_ptr")
                builder.store(value, self.value)
        else:
            self.value = value

        #: A dictionary mapping a name to a type and an index.
        self.members: dict[str, tuple[BaseMetatype, int]] = {}

    @property 
    def storage_type(self) -> Type:
        return self.typ.llvm_type.as_pointer() if self.IS_REFCOUNTED else self.typ.llvm_type
    
    def has_member(self, name: str) -> bool:
        return self.typ.has_member(name)

    def get_member(self, name: str, reference: bool = False, into_name: str = "unnamed_object") -> ObjectType:
        """
        Gets a member by name from the internal struct.

        :param name: The name of the member.
        :param into_name: Optional, the name of the variable this will be loaded into.
        """

        builder = builder_stack[-1]
        builder.comment(f"Load value named '{name}' from '{self.name}'")
        member_type, index = self.members[name]
        assert name in self.members
        internal_mem = self._get_index(index, into_name)
        internal_ptr: CastInstr = builder.bitcast(internal_mem, member_type.llvm_type.as_pointer(), f"{into_name}_ptr")  # type: ignore
        return member_type.bound.from_value(internal_ptr, member_type, reference, into_name)

    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 0
        header = TypeHeader("Object", {}, [], {}, {}, {}, False)
        return BaseMetatype(ObjectType, header)
    
    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> BasicCallable:
        return ObjectType._initializer_builder(metatype)
    
    @staticmethod 
    def _initializer_builder(metatype: BaseMetatype, member_builder: typing.Callable[[IRBuilder], list[Value]] = lambda x: []) -> BasicCallable:
        logger.debug(f"Creating {metatype.header.name} initializer")

        initializer_ir_type = FunctionType(POINTER, [param.llvm_type for param in metatype.header.parameters.values()])
        initializer_value = Function(current_module, initializer_ir_type, f"Object__init")
        initializer_type = get_type(TypeRepr(Callable, [[], TypeRepr(ObjectType)]))
        initializer = BasicCallable(initializer_value, initializer_type, [], get_type(TypeRepr(ObjectType)))

        block = initializer_value.append_basic_block("entry")
        builder = IRBuilder(block)

        size = get_type_size(metatype.llvm_type)
        builder.comment(f"Initializer for {metatype.header.name} (size {size} bytes).")

        builder.comment("Initial memory allocation.")
        new_mem = builder.call(runtime.rc_alloc_func, [size], "new_mem")
        new_ptr: Value = builder.bitcast(new_mem, metatype.llvm_type.as_pointer(), "new_ptr") # type: ignore
        as_object = metatype.bound.from_value(new_ptr, metatype)

        # TODO: Put all parameters into whatever scope system.

        builder.comment("First, store all class parameters into the struct.")
        member_index = 0
        for arg in initializer_value.args:
            field_ptr = builder.gep(new_ptr, [INT(0), INT(member_index)], name=f"field_ptr_{member_index}")
            builder.store(arg, field_ptr)
            member_index += 1
        
        builder.comment("(not implemented) Initialize all member variables.")
        members = member_builder(builder)

        for member in members:
            field_ptr = builder.gep(new_ptr, [INT(0), INT(member_index)], name=f"field_ptr_{member_index}")
            builder.store(member, field_ptr)
            member_index += 1
        
        if metatype.has_member("init"):
            builder.comment("Call user created init function.")
            init = metatype.get_member("init", into_name="init_func") # This is the static function.
            assert isinstance(init, BasicCallable)
            init.call([as_object])

        builder.ret(new_ptr)

        return initializer
    
    @staticmethod
    def from_value(value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_object") -> ObjectType:
        """
        Generates a type container from an LLVM value *pointer*.
        """

        return ObjectType(value, val_type, False, reference, name)

    def _get_index(self, index: int, name: str) -> Value:
        builder = builder_stack[-1]
        if self.reference:
            builder.comment(f"Dereference twice because this is a reference.")
        indices = [INT(0), INT(index)] if not self.reference else [INT(0), INT(0), INT(index)]
        return builder.gep(self.value, indices, name=f"{name}_mem")


class BaseMetatype(ObjectType):
    """
    A Metatype is an Object representing a type. For example, if you
    create a type using `class Name`, a Metatype will be created in the
    global registry with the name "Name". Metatype overrides the ()
    operator so you can use it as a constructor.

    The Metatype system is heavily inspired from python's, and is
    essential in creating a complete type system.
    """

    NAME = "Type"
    LLVM_TYPE = current_module.context.get_identified_type("Type")

    def __init__(self, bound: type[ObjectType], header: TypeHeader) -> None:
        if not self.LLVM_TYPE.elements:
            # Because setting members dynamically at runtime is complex
            # and Types are always global, getters are implemented 
            # statically within this instance instead of in code. In 
            # the future this should probably be fixed.
            self.LLVM_TYPE.set_body()

        # TODO: Once above implemented, move to type llvm initializer.
        # TODO: Finalize this type generation.
        logger.debug(f"Creating a new type object named {header.name}")
        self.llvm_type: IdentifiedStructType = current_module.context.get_identified_type(header.name)

        super().__init__(self.LLVM_TYPE([]), self)

        self.bound = bound
        self.header = header
        self.object_members = self.header.parameters | self.header.members
        self.static_members = self.header.static_members
        self.subclasses: list[BaseMetatype] = []
        self.generic_name = f"{self.name}<{", ".join(t.name for t in self.header.generics.values())}"

    def validate(self, obj: ObjectType) -> bool:
        if isinstance(obj, self.bound):
            return True
        for subclass in self.subclasses:
            if subclass.validate(obj):
                return True
        return False
    
    def has_member(self, name: str) -> bool:
        return name in self.object_members
    
    def add_parameter(self, name: str, typ: BaseMetatype) -> None:
        self.header.parameters[name] = typ 
        self.object_members[name] = typ
    
    def add_member(self, name: str, typ: BaseMetatype) -> None:
        self.header.members[name] = typ 
        self.object_members[name] = typ
    
    def add_static(self, name: str, val: ObjectType) -> None:
        self.header.static_members[name] = val 
        self.static_members[name] = val
    
    def add_subclass(self, typ: BaseMetatype) -> None:
        self.subclasses.append(typ)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        raise RuntimeError
    

class TupleMetatype(BaseMetatype):
    """
    Useful for specifying function parameters.
    """

    NAME = "TupleType"

    def __init__(self, *element_types: BaseMetatype) -> None:
        self.element_types = list(element_types)
        combined_type = current_module.context.get_identified_type(f"Tuple<{", ".join(t.name for t in element_types)}")
        combined_type.set_body(*[t.llvm_type for t in element_types])

        mapped_types = {f"{i}": element for i, element in enumerate(element_types)}

        super().__init__(
            TupleType,
            TypeHeader(
                "Tuple",
                mapped_types,
                [get_type(TypeRepr(ObjectType))],
                mapped_types,
                {},
                {},
                False
            )
        )


class BoolType(ObjectType):
    NAME = "Bool"
    IS_REFCOUNTED = False

    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_object",
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)]))
        self.members = {
            "==": (cmp_type, 0),
            "!=": (cmp_type, 1)
        }
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        return BaseMetatype(
            BoolType, 
            TypeHeader(
                "Bool", 
                {}, 
                [get_type(TypeRepr(ObjectType))],
                {},
                {},
                {},
                False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)]))
        typ.add_parameter("value", typ)
        typ.add_member("==", cmp_type)
        typ.add_member("!=", cmp_type)
    
    @staticmethod 
    def member_builder(builder: IRBuilder) -> list[Value]:
        _, eq = cmp_function(current_module, "Bool", "==", BOOL, BOOL)
        _, neq = cmp_function(current_module, "Bool", "!=", BOOL, BOOL)

        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr") # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr") # type: ignore

        return [BOOL(0), eq_ptr, neq_ptr]
    
    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> BasicCallable:
        return ObjectType._initializer_builder(metatype, BoolType.member_builder)


class IntType(ObjectType):
    NAME = "Int"
    IS_REFCOUNTED = False 

    def __init__(
        self, 
        value: Value, 
        typ: BaseMetatype, 
        allocate: bool = True, 
        reference: bool = False, 
        dbg_name: str = "unnamed_object"
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        self.eq_type, self.eq = cmp_function(current_module, "Int", "==", INT, INT)
        self.neq_type, self.neq = cmp_function(current_module, "Int", "!=", INT, INT)
        self.less_type, self.less = cmp_function(current_module, "Int", "<", INT, INT)
        self.greater_type, self.greater = cmp_function(current_module, "Int", ">", INT, INT)
        self.leq_type, self.leq = cmp_function(current_module, "Int", "<=", INT, INT)
        self.geq_type, self.geq = cmp_function(current_module, "Int", ">=", INT, INT)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        return BaseMetatype(
            IntType, 
            TypeHeader(
                "Int", 
                {}, 
                [get_type(TypeRepr(ObjectType))],
                {},
                {},
                {},
                False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)]))
        typ.add_parameter("value", typ)
        typ.add_member("==", cmp_type)
        typ.add_member("!=", cmp_type)
        # TODO: Rest of operators.
    
    @staticmethod 
    def member_builder(builder: IRBuilder) -> list[Value]:
        _, eq = cmp_function(current_module, "Int", "==", INT, INT)
        _, neq = cmp_function(current_module, "Int", "!=", INT, INT)

        # TODO: Rest of operators
        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr") # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr") # type: ignore

        return [INT(0), eq_ptr, neq_ptr]
    
    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> BasicCallable:
        return ObjectType._initializer_builder(metatype, IntType.member_builder)
    

class FloatType(ObjectType):
    pass


class StringType(ObjectType):
    pass


class ListType(ObjectType):
    pass


class TupleType(ObjectType):
    pass


class Callable(ObjectType, ABC):
    """
    Callable is an abstract class that represents any type that can be
    called with arguments.

    TODO: Maybe should be merged into BasicCallable.
    """

    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

    @abstractmethod
    def call(
        self,
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        pass
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)
        llvm_type = current_module.context.get_identified_type("Callable")
        return BaseMetatype("Callable", Callable, llvm_type, True, generics, [get_type(TypeRepr(ObjectType))])


class BasicCallable(Callable):
    """
    Represents a basic function which can be called with arguments. 
    This is a complete function which has already been generated, and 
    so generic arguments cannot be passed in. `CallableGroup` handles
    dynamic compilation of functions with generics. 
    """

    NAME = "BasicCallable"

    def __init__(
        self,
        value: Function,
        typ: BaseMetatype,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
        extra_struct_args: list[Value] = []
    ) -> None:
        builder = builder_stack[-1]
        builder.comment("Cast function to pointer, store.")
        function_memory = builder.bitcast(value, POINTER)

        super().__init__(typ.llvm_type([function_memory] + extra_struct_args), typ, allocate, reference, dbg_name)
        self.params = params
        self.returns = returns

        self.function_type = FunctionType(returns.llvm_type, [param_type.llvm_type for param_type in params])

    def call(
        self,
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        assert len(arguments) == len(self.params)
        for param_type, argument in zip(self.params, arguments):
            assert param_type.validate(argument)

        builder = builder_stack[-1]
        builder.comment(f"Load and call the {var_name} callable.")
        function_mem_ptr = builder.gep(self.value, [INT(0), INT(0)], name=f"{var_name}_mem_ptr")
        function_mem = builder.load(function_mem_ptr, f"{var_name}_mem")
        function_ptr = builder.bitcast(function_mem, self.function_type, f"{var_name}_ptr")
        returns = builder.call(function_ptr, [arg.value for arg in arguments], f"{var_name}_returns")
        return self.returns.bound.from_value(returns, self.returns, reference, f"{var_name}_return_transferred")
    
    @staticmethod
    def from_value(value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_callable") -> ObjectType:
        assert isinstance(value, Function)
        assert issubclass(val_type.bound, BasicCallable)
        assert len(val_type.generics) == 2
        params, returns = val_type.generics
        assert isinstance(params, TupleMetatype)
        return BasicCallable(value, val_type, params.element_types, returns, False, reference, name)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)
        llvm_type = current_module.context.get_identified_type("BasicCallable")
        llvm_type.set_body(POINTER)
        return BaseMetatype("BasicCallable", BasicCallable, llvm_type, False, generics, [get_type(TypeRepr(Callable))])


class InstanceCallable(BasicCallable):
    """
    Represents a callable class member function.

    Type signature: InstanceCallable<Instance, Params : Tuple, Returns> 
    """

    class Flags(Enum):
        ABSTRACT = auto()
        PRIVATE = auto()
        CONST = auto()
        OPERATOR = auto()

    NAME = "InstanceCallable"

    def __init__(
        self,
        value: Function,
        typ: BaseMetatype,
        instance: BaseMetatype,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        flags: set[Flags],
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        super().__init__(value, typ, [instance] + params, returns, allocate, reference, dbg_name)
        self.instance = instance
        self.flags = flags

    def call(
        self,
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        assert not self.Flags.ABSTRACT in self.flags
        assert len(arguments) > 0
        on = arguments[0]
        assert self.instance.validate(on)
        return super().call(arguments, reference, var_name)
    
    @staticmethod
    def from_value(value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_callable") -> ObjectType:
        assert isinstance(value, Function)
        assert issubclass(val_type.bound, InstanceCallable)
        assert len(val_type.generics) == 2
        params, returns = val_type.generics
        assert isinstance(params, TupleMetatype)
        assert len(params.element_types) == 1
        instance = params.element_types[0]
        rest = params.element_types[1:]
        return InstanceCallable(value, val_type, instance, rest, returns, set(), False, reference, name)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 3
        assert isinstance(generics[1], TupleMetatype)
        llvm_type = current_module.context.get_identified_type("InstanceCallable")
        llvm_type.set_body(POINTER)
        return BaseMetatype("InstanceCallable", InstanceCallable, llvm_type, False, generics, [get_type(TypeRepr(BasicCallable))])


class AnonymousCallable(BasicCallable):
    NAME = "AnonymousCallable"

    def __init__(
        self,
        value: Function,
        typ: BaseMetatype,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        bound_values: list[ObjectType],
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        super().__init__(value, typ, params, returns, allocate, reference, dbg_name, [POINTER(None)])

        struct_type = LiteralStructType([val.storage_type for val in bound_values])

        # TODO: Use TupleType here.
        # builder = builder_stack[-1]
        # builder.comment(f"Transferring bound values to the struct for {dbg_name} anonymous lambda.")
        # struct_ptr = builder.alloca(struct_type, name=f"{dbg_name}_bindings")
        # for i, binding in enumerate(bound_values):
        #     field_ptr = builder.gep(struct_ptr, [INT(0), INT(i)])
        #     builder.store(binding.value, field_ptr)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)
        llvm_type = current_module.context.get_identified_type("AnonymousCallable")
        llvm_type.set_body(POINTER, POINTER)
        return BaseMetatype("AnonymousCallable", AnonymousCallable, llvm_type, False, generics, [get_type(TypeRepr(BasicCallable))])
        


class CallableGroup:
    """
    Represents multiple functions which have the same name and return
    type.
    """

    # def __init__(self, implimentations: list[BasicCallable]) -> None:
    #     self.implimentations = implimentations

    # def call(
    #     self,
    #     arguments: list[ObjectType],
    #     reference: bool = False,
    #     var_name: str = "unknown_callable",
    # ) -> ObjectType:
    #     # TODO: Dynamic compilation based on generics.
    #     pass


# TODO: GLOBALS
def init_type_db() -> None:
    global type_db

    object_type = ObjectType.create_metatype([])
    bool_type = BoolType.create_metatype([])
    int_type = IntType.create_metatype([])

    type_db = {
        "Object": object_type,
        "Bool": bool_type,
        "Int": int_type
    }

init_type_db()