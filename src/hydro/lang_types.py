from __future__ import annotations
from abc import ABC, abstractmethod
from dataclasses import dataclass
from encodings.punycode import T
from enum import Enum, auto
import typing
from builders import builder_stack, current_module, runtime
from helpers import BOOL, CHAR, POINTER, INT, arith_function, get_type_size, cmp_function
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
            generics.append(get_type(generic) if isinstance(generic, TypeRepr) else generic)
    
    logger.debug(f"Compiling {str_repr} ...")
    out = trepr.base.create_metatype(generics)
    type_db[str_repr] = out 
    trepr.base.fill_metatype(out)
    return out


class TypeRepr:
    def __init__(self, base: type[ObjectType], generics: list[TypeRepr | BaseMetatype | list[TypeRepr]] = []) -> None:
        self.base = base 
        self.generics = generics

    def __str__(self) -> str:
        generics = f"<{", ".join([str(t) for t in self.generics])}>" if self.generics else ""
        return f"{self.base.NAME}{generics}"
    

@dataclass
class MemberInfo:
    typ: BaseMetatype
    struct_index: int
    private: bool = False 
    const: bool = False
    internal: bool = False
    abstract: bool = False


@dataclass
class TypeHeader:
    """
    Specifies layout but not implementation of a class. 

    In memory, only parameters and *then* members will be stored in the
    order defined in their respective dictionaries. 
    """

    name: str
    generics: dict[str, BaseMetatype]
    inherits: list[BaseMetatype]
    parameters: dict[str, MemberInfo]
    members: dict[str, MemberInfo] = {}
    static_members: dict[str, ObjectType] = {}
    is_abstract: bool = False
    has_constructor: bool = True


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
        self.members = self.typ.object_members

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
        member = self.members[name]
        assert name in self.members
        internal_mem = self._get_index(member.struct_index, into_name)
        internal_ptr: CastInstr = builder.bitcast(internal_mem, member.typ.llvm_type.as_pointer(), f"{into_name}_ptr")  # type: ignore
        return member.typ.bound.from_value(internal_ptr, member.typ, reference, into_name)

    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 0
        header = TypeHeader("Object", {}, [], {}, {}, {}, False)
        return BaseMetatype(ObjectType, header)
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        pass
    
    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> BasicCallable:
        return ObjectType._initializer_builder(metatype)
    
    @staticmethod 
    def _initializer_builder(metatype: BaseMetatype, member_builder: typing.Callable[[IRBuilder, list[ObjectType]], list[Value]] = lambda builder, params: []) -> BasicCallable:
        logger.debug(f"Creating {metatype.header.name} initializer")

        initializer_ir_type = FunctionType(POINTER, [param.typ.llvm_type for param in metatype.header.parameters.values()])
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

        parameters = [info.typ.bound.from_value(arg, info.typ) for arg, info in zip(initializer_value.args, metatype.header.parameters.values())]

        # TODO: Put all parameters into whatever scope system.
        
        builder.comment("Initialize all member variables.")
        members = member_builder(builder, parameters)

        # TODO: Unnecessary now.
        member_index = 0
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
        self.struct_index = len(self.object_members)
        self.static_members = self.header.static_members
        self.subclasses: list[BaseMetatype] = []
        self.generic_name = f"{self.name}<{", ".join(t.name for t in self.header.generics.values())}"
        if self.header.has_constructor or self.header.is_abstract:
            self.static_members["()"] = self.bound.get_initializer(self) 

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
        info = MemberInfo(typ, self.struct_index)
        self.struct_index += 1
        self.header.parameters[name] = info 
        self.object_members[name] = info
    
    def add_member(self, name: str, typ: BaseMetatype, private: bool = False, const: bool = False, internal: bool = False, abstract: bool = False) -> None:
        info = MemberInfo(typ, self.struct_index, private, const, internal, abstract)
        self.struct_index += 1
        self.header.members[name] = info
        self.object_members[name] = info
    
    def add_static(self, name: str, val: ObjectType) -> None:
        self.header.static_members[name] = val 
        self.static_members[name] = val
    
    def add_subclass(self, typ: BaseMetatype) -> None:
        self.subclasses.append(typ)
    
    def finalize(self) -> None:
        members = list(self.object_members.values())
        members.sort(key=lambda x: x.struct_index)
        for i, member in enumerate(members):
            assert i == member.struct_index
        
        # TODO: Internal types.
        self.llvm_type.set_body(*[member.typ.llvm_type for member in members])
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        raise RuntimeError
    

class TupleMetatype(BaseMetatype):
    """
    Useful for specifying function parameters.
    """

    NAME = "TupleType"
    LLVM_TYPE = current_module.context.get_identified_type("TupleType")

    def __init__(self, *element_types: BaseMetatype) -> None:
        self.element_types = list(element_types)
        combined_type = current_module.context.get_identified_type(f"Tuple<{", ".join(t.name for t in element_types)}")
        combined_type.set_body(*[t.llvm_type for t in element_types])

        mapped_generics = {f"{i}": element for i, element in enumerate(element_types)}
        mapped_types = {f"{i}": MemberInfo(element, i, internal=True) for i, element in enumerate(element_types)}

        super().__init__(
            TupleType,
            TypeHeader(
                "Tuple",
                mapped_generics,
                [get_type(TypeRepr(ObjectType))],
                mapped_types,
                {},
                {},
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
                has_constructor=False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)]))
        typ.add_member("value", typ, internal=True)
        typ.add_member("==", cmp_type)
        typ.add_member("!=", cmp_type)
    
    @staticmethod 
    def member_builder(builder: IRBuilder, params: list[ObjectType]) -> list[Value]:
        assert len(params) == 1
        arg = params[0]
        assert isinstance(arg, BoolType)

        raw_ptr = builder.gep(arg, [INT(0), INT(0)], name="raw_ptr")
        raw = builder.load(raw_ptr, "raw")

        _, eq = cmp_function(current_module, "Bool", "==", BOOL, BOOL)
        _, neq = cmp_function(current_module, "Bool", "!=", BOOL, BOOL)

        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr") # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr") # type: ignore

        return [raw, eq_ptr, neq_ptr]
    
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

        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)]))
        arith_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)]))
        self.members = {
            "==": (cmp_type, 1),
            "!=": (cmp_type, 2),
            "<": (cmp_type, 3),
            ">": (cmp_type, 4),
            "<=": (cmp_type, 5),
            ">=": (cmp_type, 6),
            "+": (arith_type, 7),
            "-": (arith_type, 8),
            "*": (arith_type, 9),
            "/": (arith_type, 10),
        }
    
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
                has_constructor=False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)]))
        arith_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)]))
        typ.add_parameter("value", typ)
        typ.add_member("==", cmp_type)
        typ.add_member("!=", cmp_type)
        typ.add_member("<", cmp_type)
        typ.add_member(">", cmp_type)
        typ.add_member("<=", cmp_type)
        typ.add_member(">=", cmp_type)
        typ.add_member("+", arith_type)
        typ.add_member("-", arith_type)
        typ.add_member("*", arith_type)
        typ.add_member("/", arith_type)
        # TODO: Rest of operators.
    
    @staticmethod 
    def member_builder(builder: IRBuilder, params: list[ObjectType]) -> list[Value]:
        assert len(params) == 1
        arg = params[0]
        assert isinstance(arg, BoolType)

        raw_ptr = builder.gep(arg, [INT(0), INT(0)], name="raw_ptr")
        raw = builder.load(raw_ptr, "raw")

        builder.comment("Integer operator functions:")

        eq_type, eq = cmp_function(current_module, "Int", "==", INT, INT)
        neq_type, neq = cmp_function(current_module, "Int", "!=", INT, INT)
        less_type, less = cmp_function(current_module, "Int", "<", INT, INT)
        greater_type, greater = cmp_function(current_module, "Int", ">", INT, INT)
        leq_type, leq = cmp_function(current_module, "Int", "<=", INT, INT)
        geq_type, geq = cmp_function(current_module, "Int", ">=", INT, INT)

        # TODO: Overload for unary minus.
        add_type, add = arith_function(current_module, "Int", "+", lambda builder, lhs, rhs: builder.add(lhs, rhs, "arith_res"), INT, INT, INT) # type: ignore
        sub_type, sub = arith_function(current_module, "Int", "-", lambda builder, lhs, rhs: builder.sub(lhs, rhs, "arith_res"), INT, INT, INT) # type: ignore
        mul_type, mul = arith_function(current_module, "Int", "*", lambda builder, lhs, rhs: builder.mul(lhs, rhs, "arith_res"), INT, INT, INT) # type: ignore
        div_type, div = arith_function(current_module, "Int", "/", lambda builder, lhs, rhs: builder.div(lhs, rhs, "arith_res"), INT, INT, INT) # type: ignore

        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr") # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr") # type: ignore
        less_ptr: Value = builder.bitcast(less, POINTER, "less_ptr") # type: ignore
        greater_ptr: Value = builder.bitcast(greater, POINTER, "greater_ptr") # type: ignore
        leq_ptr: Value = builder.bitcast(leq, POINTER, "leq_ptr") # type: ignore
        geq_ptr: Value = builder.bitcast(geq, POINTER, "geq_ptr") # type: ignore
        add_ptr: Value = builder.bitcast(add, POINTER, "add_ptr") # type: ignore
        sub_ptr: Value = builder.bitcast(sub, POINTER, "add_ptr") # type: ignore
        mul_ptr: Value = builder.bitcast(mul, POINTER, "add_ptr") # type: ignore
        div_ptr: Value = builder.bitcast(div, POINTER, "add_ptr") # type: ignore

        return [raw, eq_ptr, neq_ptr, less_ptr, greater_ptr, leq_ptr, geq_ptr, add_ptr, sub_ptr, mul_ptr, div_ptr]
    
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
        return BaseMetatype(
            Callable, 
            TypeHeader(
                "Callable",
                {
                    "Params": generics[0],
                    "Returns": generics[1],
                },
                [get_type(TypeRepr(ObjectType))],
                {},
                {},
                {},
                is_abstract=True,
                has_constructor=False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(Callable), typ.header.generics["Params"], typ.header.generics["Returns"]]))
        typ.add_member("call", call_type, abstract=True)


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
        params = val_type.header.generics["Params"]
        returns = val_type.header.generics["Returns"]
        assert isinstance(params, TupleMetatype)
        return BasicCallable(value, val_type, params.element_types, returns, False, reference, name)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)
        # llvm_type = current_module.context.get_identified_type("BasicCallable")
        # llvm_type.set_body(POINTER)
        return BaseMetatype(
            BasicCallable,
            TypeHeader(
                "BasicCallable",
                {
                    "Params": generics[0],
                    "Returns": generics[1],
                },
                [get_type(TypeRepr(Callable))],
                {},
                {},
                {},
                has_constructor=False
            )
        )
    
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]]))
        typ.add_member("call", call_type)


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
        params = val_type.header.generics["Params"]
        returns = val_type.header.generics["Returns"]
        assert isinstance(params, TupleMetatype)
        assert len(params.element_types) == 1
        instance = params.element_types[0]
        rest = params.element_types[1:]
        return InstanceCallable(value, val_type, instance, rest, returns, set(), False, reference, name)
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 3
        assert isinstance(generics[1], TupleMetatype)
        # llvm_type = current_module.context.get_identified_type("InstanceCallable")
        # llvm_type.set_body(POINTER)
        return BaseMetatype(
            InstanceCallable,
            TypeHeader(
                "InstanceCallable",
                {
                    "On": generics[0],
                    "Params": generics[1],
                    "Returns": generics[2],
                },
                [get_type(TypeRepr(BasicCallable))],
                {},
                {},
                {},
                has_constructor=False
            )
        )
    
    # TODO: This should probably be able to bind the instance.
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]]))
        typ.add_member("call", call_type)


# TODO: AnonymousCallable needs some way to store bound_values that probably isn't generics (although that wouldn't actually be that bad)
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

        # struct_type = LiteralStructType([val.storage_type for val in bound_values])

        # builder = builder_stack[-1]
        # builder.comment(f"Transferring bound values to the struct for {dbg_name} anonymous lambda.")
        # struct_ptr = builder.alloca(struct_type, name=f"{dbg_name}_bindings")
        # for i, binding in enumerate(bound_values):
        #     field_ptr = builder.gep(struct_ptr, [INT(0), INT(i)])
        #     builder.store(binding.value, field_ptr)
    
    # TODO: from_value for AnonymousCallable
    
    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)
        # llvm_type = current_module.context.get_identified_type("AnonymousCallable")
        # llvm_type.set_body(POINTER, POINTER)
        return BaseMetatype(
            AnonymousCallable,
            TypeHeader(
                "AnonymousCallable",
                {
                    "Params": generics[0],
                    "Returns": generics[1],
                },
                [get_type(TypeRepr(BasicCallable))],
                {},
                {},
                {},
                has_constructor=False
            )
        )
    
    # TODO: This should probably be able to bind the instance.
    @staticmethod 
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(TypeRepr(InstanceCallable, [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]]))
        typ.add_member("call", call_type)


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