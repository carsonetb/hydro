from __future__ import annotations
from abc import ABC, abstractmethod
from copy import copy, deepcopy
from dataclasses import dataclass
from encodings.punycode import T
from enum import Enum, auto
from typing import cast
import typing
from hydro.builders import builder_stack, current_module, runtime
from hydro.helpers import BOOL, CHAR, POINTER, INT, arith_function, get_type_size, cmp_function
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
    GlobalVariable,
    ArrayType,
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
    def __init__(
        self, base: type[ObjectType], generics: list[TypeRepr | BaseMetatype | list[TypeRepr]] = []
    ) -> None:
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
    def _initializer_builder(
        metatype: BaseMetatype,
        member_builder: typing.Callable[
            [IRBuilder, list[ObjectType]], list[Value]
        ] = lambda builder, params: [],
    ) -> BasicCallable:
        logger.debug(f"Creating {metatype.header.name} initializer")

        initializer_ir_type = FunctionType(
            POINTER, [param.typ.llvm_type for param in metatype.header.parameters.values()]
        )
        initializer_value = Function(current_module, initializer_ir_type, f"Object__init")
        initializer_type = get_type(TypeRepr(Callable, [[], TypeRepr(ObjectType)]))
        initializer = BasicCallable(initializer_value, initializer_type, [], get_type(TypeRepr(ObjectType)))

        block = initializer_value.append_basic_block("entry")
        builder = IRBuilder(block)

        size = get_type_size(metatype.llvm_type)
        builder.comment(f"Initializer for {metatype.header.name} (size {size} bytes).")

        builder.comment("Initial memory allocation.")
        new_mem = builder.call(runtime.rc_alloc_func, [size], "new_mem")
        new_ptr: Value = builder.bitcast(new_mem, metatype.llvm_type.as_pointer(), "new_ptr")  # type: ignore
        as_object = metatype.bound.from_value(new_ptr, metatype)

        builder.comment("Store vptr constants into memory.")
        for base in metatype.base_classes:
            vptr_path = [0] + metatype.index_paths[base.name] + [0]
            vptr_ptr = builder.gep(new_ptr, [INT(i) for i in vptr_path])
            vptr = metatype.get_vptr_constant(base.name)
            builder.store(vptr, vptr_ptr)

        parameters = [
            info.typ.bound.from_value(arg, info.typ)
            for arg, info in zip(initializer_value.args, metatype.header.parameters.values())
        ]

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
            init = metatype.get_member("init", into_name="init_func")  # This is the static function.
            assert isinstance(init, BasicCallable)
            init.call([as_object])

        builder.ret(new_ptr)

        return initializer

    @staticmethod
    def from_value(
        value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_object"
    ) -> ObjectType:
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


@dataclass
class VTableEntry:
    name: str
    obj: InstanceCallable | None
    abstract: bool
    virtual: bool


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
        self.static_members = self.header.static_members
        self.subclasses: list[BaseMetatype] = []
        self.generic_name = f"{self.name}<{", ".join(t.name for t in self.header.generics.values())}"
        if self.header.has_constructor or self.header.is_abstract:
            self.static_members["()"] = self.bound.get_initializer(self)

        self.object_members: dict[str, MemberInfo] = {}
        self.struct_index = 0

        self.is_root_class = len(self.header.inherits) == 0

        self.base_classes: list[BaseMetatype] = []
        if self.is_root_class:
            self.base_classes = [self]
        else:
            for inherits in self.header.inherits:
                self.base_classes += inherits.base_classes

        self.primary_vtbale: list[VTableEntry] = []
        self.vtables: dict[str, tuple[list[VTableEntry], BaseMetatype]] = {}
        if self.is_root_class:
            self.vtables[self.name] = (self.primary_vtbale, self)
        else:
            main_inherits = self.header.inherits[0]
            self.primary_vtbale = deepcopy(main_inherits.primary_vtbale)

            for inherits in self.header.inherits:
                self.vtables |= deepcopy(inherits.vtables)

        self.index_paths: dict[str, list[int]] = {base_class.name: self._parent_path(self, base_class.name) for base_class in self.base_classes}  # type: ignore

        self.vtable_global: GlobalVariable | None = None

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

    def add_member(
        self,
        name: str,
        typ: BaseMetatype,
        private: bool = False,
        const: bool = False,
        internal: bool = False,
        abstract: bool = False,
    ) -> None:
        info = MemberInfo(typ, self.struct_index, private, const, internal, abstract)
        self.struct_index += 1
        self.header.members[name] = info
        self.object_members[name] = info

    def add_virtual(
        self,
        name: str,
        meth: InstanceCallable | None,
        virtual: bool = False,
        abstract: bool = False,
        override: bool = False,
    ) -> None:
        assert virtual or abstract or override
        assert (meth is None and abstract) or (meth is not None and not abstract)

        entry = VTableEntry(name, meth, abstract, virtual)

        if abstract:
            assert not (virtual or override)
            self.primary_vtbale.append(entry)
        elif override:
            typ, index = self._search_vtables(name)
            super_entry = self.vtables[typ][0][index]
            assert super_entry.virtual or super_entry.abstract
            self.vtables[typ][0][index] = entry
        elif virtual:
            self.primary_vtbale.append(entry)

    def add_static(self, name: str, val: ObjectType) -> None:
        self.header.static_members[name] = val
        self.static_members[name] = val

    def add_subclass(self, typ: BaseMetatype) -> None:
        self.subclasses.append(typ)

    def finalize(self) -> None:
        struct = self._generate_struct()

        # for name, (vtable, typ) in self.vtables.items():
        #     table_typ = ArrayType(POINTER, len(vtable))
        #     self.table_val = GlobalVariable(current_module, table_typ, f"vtable_{self.name}__{name}")
        #     self.table_val.initializer = table_typ([cast(InstanceCallable, entry.obj).function_pointer for entry in vtable]) # type: ignore
        #     self.internal_struct.append(table_typ)

        # TODO: Deadly diamond of death

        # Generate vtable global (doesn't effect struct)
        vtable_struct: list[ArrayType] = []
        vtable_values: list[Value] = []
        for typ in self.base_classes:
            this_vtable, _ = self.vtables[typ.name]
            this_typ = ArrayType(POINTER, len(this_vtable))
            vtable_struct.append(this_typ)
            this_ptrs: list[Value] = []
            for entry in this_vtable:
                assert entry.obj is not None
                this_ptrs.append(entry.obj.function_pointer)
            this_value = this_typ(this_ptrs)
            vtable_values.append(this_value)

        vtable_group_type = LiteralStructType(vtable_struct)
        vtable_group = vtable_group_type(vtable_values)
        self.vtable_global = GlobalVariable(current_module, vtable_group_type, name=f"vtable__{self.name}")
        self.vtable_global.global_constant = True
        self.vtable_global.initializer = vtable_group  # type: ignore

        self.llvm_type.set_body(struct)

    def get_vptr_constant(self, type_name: str) -> Value:
        builder = builder_stack[-1]
        for i, inherits in enumerate(self.header.inherits):
            if inherits.name == type_name:
                return builder.gep(self.vtable_global, [INT(0), INT(i)])
        assert False

    @staticmethod
    def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
        raise RuntimeError

    def _search_vtables(self, name: str) -> tuple[str, int]:
        for typ, (vtable, _) in self.vtables.items():
            for i, entry in enumerate(vtable):
                if entry.name == name:
                    return (typ, i)
        assert False

    def _generate_struct(self) -> list[Type]:
        out: list[Type] = []

        struct_index = 0
        if len(self.header.inherits) > 0:
            for inherits in self.header.inherits:
                out.append(inherits.llvm_type)
                struct_index += 1
        else:
            out.append(POINTER)  # vptr
            struct_index += 1

        prefix_index = struct_index

        members = list(self.object_members.values())
        members.sort(key=lambda x: x.struct_index)
        for member in members:
            member.struct_index += prefix_index
            out.append(member.typ.llvm_type)

        return out

    @staticmethod
    def _parent_path(current: BaseMetatype, target_name: str) -> list[int] | None:
        if current.name == target_name:
            return []

        # Because all the inherited classes are at the start of the
        # struct, we can literally just use the index.
        for i, inherits in enumerate(current.header.inherits):
            path_rest = BaseMetatype._parent_path(inherits, target_name)
            if path_rest is not None:
                return [i] + path_rest

        return None


class TupleMetatype(BaseMetatype):
    """
    Useful for specifying function parameters.
    """

    NAME = "TupleType"
    LLVM_TYPE = current_module.context.get_identified_type("TupleType")

    def __init__(self, *element_types: BaseMetatype) -> None:
        self.element_types = list(element_types)
        combined_type = current_module.context.get_identified_type(
            f"Tuple<{", ".join(t.name for t in element_types)}"
        )
        combined_type.set_body(*[t.llvm_type for t in element_types])

        mapped_generics = {f"{i}": element for i, element in enumerate(element_types)}
        mapped_types = {
            f"{i}": MemberInfo(element, i, internal=True) for i, element in enumerate(element_types)
        }

        super().__init__(
            TupleType,
            TypeHeader(
                "Tuple",
                mapped_generics,
                [get_type(TypeRepr(ObjectType))],
                mapped_types,
                {},
                {},
            ),
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
            TypeHeader("Bool", {}, [get_type(TypeRepr(ObjectType))], {}, {}, {}, has_constructor=False),
        )

    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(
            TypeRepr(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)])
        )
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

        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr")  # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr")  # type: ignore

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
        dbg_name: str = "unnamed_object",
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        cmp_type = get_type(
            TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)])
        )
        arith_type = get_type(
            TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)])
        )
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
            TypeHeader("Int", {}, [get_type(TypeRepr(ObjectType))], {}, {}, {}, has_constructor=False),
        )

    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        cmp_type = get_type(
            TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)])
        )
        arith_type = get_type(
            TypeRepr(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)])
        )
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
        add_type, add = arith_function(current_module, "Int", "+", lambda builder, lhs, rhs: builder.add(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
        sub_type, sub = arith_function(current_module, "Int", "-", lambda builder, lhs, rhs: builder.sub(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
        mul_type, mul = arith_function(current_module, "Int", "*", lambda builder, lhs, rhs: builder.mul(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
        div_type, div = arith_function(current_module, "Int", "/", lambda builder, lhs, rhs: builder.div(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore

        eq_ptr: Value = builder.bitcast(eq, POINTER, "eq_ptr")  # type: ignore
        neq_ptr: Value = builder.bitcast(neq, POINTER, "neq_ptr")  # type: ignore
        less_ptr: Value = builder.bitcast(less, POINTER, "less_ptr")  # type: ignore
        greater_ptr: Value = builder.bitcast(greater, POINTER, "greater_ptr")  # type: ignore
        leq_ptr: Value = builder.bitcast(leq, POINTER, "leq_ptr")  # type: ignore
        geq_ptr: Value = builder.bitcast(geq, POINTER, "geq_ptr")  # type: ignore
        add_ptr: Value = builder.bitcast(add, POINTER, "add_ptr")  # type: ignore
        sub_ptr: Value = builder.bitcast(sub, POINTER, "add_ptr")  # type: ignore
        mul_ptr: Value = builder.bitcast(mul, POINTER, "add_ptr")  # type: ignore
        div_ptr: Value = builder.bitcast(div, POINTER, "add_ptr")  # type: ignore

        return [
            raw,
            eq_ptr,
            neq_ptr,
            less_ptr,
            greater_ptr,
            leq_ptr,
            geq_ptr,
            add_ptr,
            sub_ptr,
            mul_ptr,
            div_ptr,
        ]

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
                has_constructor=False,
            ),
        )

    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(
            TypeRepr(
                InstanceCallable,
                [TypeRepr(Callable), typ.header.generics["Params"], typ.header.generics["Returns"]],
            )
        )
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
        value: Value | Function,
        typ: BaseMetatype,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        function_index: int = 0,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        builder = builder_stack[-1]
        if not isinstance(value, Function):
            function_value = builder.extract_value(typ, function_index, "function_value")
        else:
            function_value = value
        builder.comment("Cast function to pointer, store.")
        self.function_pointer: Value = builder.bitcast(function_value, POINTER, "function_ptr")  # type: ignore

        # TODO: Weird, probably should be refactored.
        super().__init__(
            typ.llvm_type([value]) if isinstance(value, Function) else value,
            typ,
            allocate,
            reference,
            dbg_name,
        )
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
        return self.returns.bound.from_value(
            returns, self.returns, reference, f"{var_name}_return_transferred"
        )

    @staticmethod
    def from_value(
        value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_callable"
    ) -> ObjectType:
        assert isinstance(value, Function)
        assert issubclass(val_type.bound, BasicCallable)
        params = val_type.header.generics["Params"]
        returns = val_type.header.generics["Returns"]
        assert isinstance(params, TupleMetatype)
        return BasicCallable(value, val_type, params.element_types, returns, 0, False, reference, name)

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
                has_constructor=False,
            ),
        )

    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(
            TypeRepr(
                InstanceCallable,
                [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]],
            )
        )
        typ.add_member("call", call_type)


class InstanceCallable(BasicCallable):
    """
    Represents a callable class member function.

    Type signature: InstanceCallable<Instance, Params : Tuple, Returns>
    """

    class Flags(Enum):
        OVERRIDE = auto()
        VIRTUAL = auto()
        ABSTRACT = auto()
        PRIVATE = auto()
        CONST = auto()
        OPERATOR = auto()

    NAME = "InstanceCallable"

    def __init__(
        self,
        value: Function,
        typ: BaseMetatype,
        instance: ObjectType,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        flags: set[Flags],
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        # TODO: Might need to convert to POINTER?
        # TODO: Load the function from the vtable if virtual.
        struct = typ.llvm_type([instance.value, value])

        super().__init__(struct, typ, params, returns, 1, allocate, reference, dbg_name)
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
        assert self.instance.typ.validate(on)
        return super().call(arguments, reference, var_name)

    @staticmethod
    def from_value(
        value: Value, val_type: BaseMetatype, reference: bool = False, name: str = "unnamed_callable"
    ) -> ObjectType:
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
                has_constructor=False,
            ),
        )

    # TODO: This should probably be able to bind the instance.
    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(
            TypeRepr(
                InstanceCallable,
                [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]],
            )
        )
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
        super().__init__(value, typ, params, returns, 0, allocate, reference, dbg_name)

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
                has_constructor=False,
            ),
        )

    # TODO: This should probably be able to bind the instance.
    @staticmethod
    def fill_metatype(typ: BaseMetatype) -> None:
        typ.add_member("value", typ, internal=True)

        call_type = get_type(
            TypeRepr(
                InstanceCallable,
                [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]],
            )
        )
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

    type_db = {"Object": object_type, "Bool": bool_type, "Int": int_type}


init_type_db()
