from __future__ import annotations
from abc import ABC
from copy import deepcopy
from dataclasses import dataclass
from enum import Enum, auto
import typing
from llvmlite.ir import (
    CastInstr,
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

from hydro.builders import builder_stack, current_module, runtime
from hydro.helpers import BOOL, LONG, NULL, POINTER, INT, arith_function, functions_into_struct, get_type_size, cmp_function
from hydro.loggers import create_logger
from src.hydro.compiler import Scope


logger = create_logger("Types")


type_db: dict[str, BaseMetatype] = {}


def get_type(base: type[ObjectType], generics: typing.Sequence[TypeRepr | BaseMetatype | typing.Sequence[TypeRepr | BaseMetatype]] = []) -> BaseMetatype:
    str_repr = str(TypeRepr(base, generics))
    if str_repr in type_db:
        return type_db[str_repr]

    metatype_generics: list[BaseMetatype] = []
    for generic in generics:
        if isinstance(generic, list):
            metatype_generics.append(TupleMetatype(*[get_type(sub.base, sub.generics) for sub in generic]))
        else:
            metatype_generics.append(get_type(generic.base, generic.generics) if isinstance(generic, TypeRepr) else generic)

    logger.debug(f"Compiling {str_repr} ...")
    out = base.create_metatype(type_db, metatype_generics)
    return out


class TypeRepr:
    def __init__(
        self, base: type[ObjectType], generics: typing.Sequence[TypeRepr | BaseMetatype | list[TypeRepr]] = []
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
        self.value: Value
        if not self.reference:
            if self.IS_REFCOUNTED:
                if allocate:
                    builder.comment(f"Allocating a reference counted variable in memory for {dbg_name}:")
                    memory = builder.call(
                        runtime.rc_alloc_func,
                        [INT(get_type_size(self.typ.llvm_type))],
                        f"{self.name}_memory",
                    )
                    self.value = builder.bitcast(memory, self.LLVM_TYPE.as_pointer(), f"{self.name}_pointer")  # type: ignore
                else:
                    self.value = value
                builder.store(value, self.value)
            else:
                builder.comment(f"Allocating stack memory for {dbg_name} (possibly copying).")
                self.value = builder.alloca(self.typ.llvm_type, name=f"{self.name}_stack_ptr")
                builder.store(value, self.value)
        else:
            self.value = value

    @property
    def storage_type(self) -> Type:
        return self.typ.llvm_type.as_pointer() if self.IS_REFCOUNTED else self.typ.llvm_type

    def extract_values(self, into: Scope) -> None:
        """
        Extracts all members (public and private) into scope.
        """
        # TODO: BaseMetatype extract_values.

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
        assert name in self.members
        member = self.members[name]
        internal_mem = self._get_index(member.struct_index, into_name)
        internal_ptr: CastInstr = builder.bitcast(internal_mem, member.typ.llvm_type.as_pointer(), f"{into_name}_ptr")  # type: ignore
        return member.typ.bound.from_value(internal_ptr, member.typ, reference, into_name)

    def set_to(self, other: ObjectType) -> None:
        assert self.reference
        # TODO: set_to implementation
        pass

    def call_on(self, name: str, generics: list[BaseMetatype], arguments: list[ObjectType], reference: bool = False, into_name: str = "unnamed_object") -> ObjectType:
        # TODO: call_on implementation
        pass

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 0
        header = TypeHeader("Object", {}, [], {}, {}, {}, False)
        out = BaseMetatype(ObjectType, header)
        out.finalize()
        return out

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        return ObjectType._initializer_builder(metatype)

    @staticmethod
    def _initializer_builder(
        metatype: BaseMetatype,
        member_builder: typing.Callable[
            [BaseMetatype, list[ObjectType]], list[Value]
        ] = lambda typ, params: [],
    ) -> Callable:
        logger.debug(f"Creating {metatype.header.name} initializer")

        initializer_ir_type = FunctionType(
            POINTER, [param.typ.storage_type for param in metatype.header.parameters.values()]
        )
        initializer_value = Function(current_module, initializer_ir_type, "Object__init")
        initializer_type = get_type(Callable, [[], TypeRepr(ObjectType)])
        initializer = Callable(initializer_value, initializer_ir_type, initializer_type, [], get_type(ObjectType))

        block = initializer_value.append_basic_block("entry")
        builder = IRBuilder(block)
        builder_stack.append(builder)

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
        members = member_builder(metatype, parameters)

        # TODO: Unnecessary now.
        member_index = 0
        for member in members:
            field_ptr = builder.gep(new_ptr, [INT(0), INT(member_index)], name=f"field_ptr_{member_index}")
            builder.store(member, field_ptr)
            member_index += 1

        if metatype.has_member("init"):
            builder.comment("Call user created init function.")
            init = metatype.get_member("init", into_name="init_func")  # This is the static function.
            assert isinstance(init, Callable)
            init.call([as_object])

        builder.ret(new_ptr)
        builder_stack.pop()

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
            builder.comment("Dereference twice because this is a reference.")
        indices = [INT(0), INT(index)] if not self.reference else [INT(0), INT(0), INT(index)]
        return builder.gep(self.value, indices, name=f"{name}_mem")


class VoidType(ObjectType):
    NAME = "Void"

    def __init__(self) -> None:
        super().__init__(NULL, get_type(VoidType))


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
    IS_REFCOUNTED = False

    def __init__(self, bound: type[ObjectType], header: TypeHeader) -> None:

        # TODO: Finalize this type generation.
        logger.debug(f"Creating a new type object named {header.name}")
        self.llvm_type: IdentifiedStructType = current_module.context.get_identified_type(header.name) # TODO: Correct internal naming
        self.static_llvm_type: IdentifiedStructType = current_module.context.get_identified_type(f"{header.name}__type")

        self.type_global = GlobalVariable(current_module, self.static_llvm_type, f"{self.name}__global")
        super().__init__(self.type_global, self, allocate=False)

        self.bound = bound
        self.header = header
        self.static_members = self.header.static_members
        self.subclasses: list[BaseMetatype] = []
        self.generic_name = f"{self.name}<{", ".join(t.name for t in self.header.generics.values())}"
        if self.header.has_constructor or self.header.is_abstract:
            self.static_members["()"] = self.bound.get_initializer(self)

        self.object_members: dict[str, MemberInfo] = {}
        self.struct: list[Type] = []
        if len(self.header.inherits) > 0:
            for inherits in self.header.inherits:
                self.struct.append(inherits.storage_type)
        else:
            self.struct.append(POINTER)  # vptr

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

    @property
    def generics(self) -> dict[str, BaseMetatype]:
        return self.header.generics

    # TODO: Validation has weird stuff with generics.
    def validate(self, obj: ObjectType) -> bool:
        if isinstance(obj, self.bound):
            return True
        for subclass in self.subclasses:
            if subclass.validate(obj):
                return True
        return False

    def validate_type(self, typ: BaseMetatype) -> bool:
        if typ.name == self.name:
            return True
        for subclass in self.subclasses:
            if subclass.validate_type(typ):
                return True
        return False

    def has_member(self, name: str) -> bool:
        return name in self.static_members

    def get_member(self, name: str, reference: bool = False, into_name: str = "unnamed_object") -> ObjectType:
        # TODO: This probably looks really weird in the IR.
        assert name in self.static_members
        return self.static_members[name]

    def add_parameter(self, name: str, typ: BaseMetatype) -> None:
        info = MemberInfo(typ, len(self.struct))
        self.struct.append(typ.storage_type)
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
        info = MemberInfo(typ, len(self.struct), private, const, internal, abstract)
        self.struct.append(typ.storage_type)
        self.header.members[name] = info
        self.object_members[name] = info

    def add_internal(self, typ: Type):
        self.struct.append(typ)

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
        static_struct = self._generate_static()

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

        logger.debug(f"Finalizing type {self.name} with struct {self.struct} and static struct {static_struct}")

        self.llvm_type.set_body(self.struct)
        self.static_llvm_type.set_body(static_struct)

    def get_vptr_constant(self, type_name: str) -> Value:
        builder = builder_stack[-1]
        for i, inherits in enumerate(self.header.inherits):
            if inherits.name == type_name:
                return builder.gep(self.vtable_global, [INT(0), INT(i)])
        assert False

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 0
        header = TypeHeader("Type", {}, [get_type(ObjectType)], {})
        out = BaseMetatype(BaseMetatype, header)
        db["Type"] = out
        out.finalize()
        return out

    def _search_vtables(self, name: str) -> tuple[str, int]:
        for typ, (vtable, _) in self.vtables.items():
            for i, entry in enumerate(vtable):
                if entry.name == name:
                    return (typ, i)
        assert False

    def _generate_static(self) -> list[Type]:
        out: list[Type] = []

        statics = list(self.static_members.values())
        for static in statics:
            out.append(static.typ.storage_type)

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
        combined_type.set_body(*[t.storage_type for t in element_types])

        mapped_generics = {f"{i}": element for i, element in enumerate(element_types)}
        mapped_types = {
            f"{i}": MemberInfo(element, i, internal=True) for i, element in enumerate(element_types)
        }

        super().__init__(
            TupleType,
            TypeHeader(
                "Tuple",
                mapped_generics,
                [get_type(ObjectType)],
                mapped_types,
                {},
                {},
            ),
        )


class BoolType(ObjectType):
    NAME = "Bool"
    IS_REFCOUNTED = False

    eq = cmp_function(current_module, "Bool", "==", BOOL, BOOL)
    neq = cmp_function(current_module, "Bool", "!=", BOOL, BOOL)
    initializer: Function | None = None

    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_bool",
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        cmp_type = get_type(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)])
        self.members = {
            "==": (cmp_type, 1),
            "!=": (cmp_type, 2),
        }

    def get_raw(self) -> Value:
        pass

    @staticmethod
    def from_literal(value: bool, dbg_name: str = "unnamed_bool") -> BoolType:
        typ = get_type(BoolType)

        if not BoolType.initializer:
            initializer_typ = FunctionType(POINTER, [BOOL])
            initializer = Function(current_module, initializer_typ, "Bool__initializer")
            block = initializer.append_basic_block("entry")
            builder = IRBuilder(block)
            val = initializer.args[0]
            eq_ptr: Value = builder.bitcast(BoolType.eq, POINTER, "eq_ptr")  # type: ignore
            neq_ptr: Value = builder.bitcast(BoolType.eq, POINTER, "neq_ptr")  # type: ignore

            struct_ptr = builder.alloca(typ.llvm_type, name="struct_ptr")
            val_field_ptr = builder.gep(struct_ptr, [INT(0), INT(0)], name="val_field_ptr")
            eq_field_ptr = builder.gep(struct_ptr, [INT(0), INT(1)], name="eq_field_ptr")
            neq_field_ptr = builder.gep(struct_ptr, [INT(0), INT(2)], name="neq_field_ptr")
            builder.store(val, val_field_ptr)
            builder.store(eq_ptr, eq_field_ptr)
            builder.store(neq_ptr, neq_field_ptr)

            builder.ret(val)
            BoolType.initializer = initializer

        builder = builder_stack[-1]
        val = builder.call(BoolType.initializer, [BOOL(value)], name=dbg_name)

        return BoolType(val, typ, allocate=False, dbg_name=dbg_name)

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype] = []) -> BaseMetatype:
        out = BaseMetatype(
            BoolType,
            TypeHeader("Bool", {}, [get_type(ObjectType)], {}, has_constructor=False),
        )
        db["Bool"] = out

        cmp_type = get_type(InstanceCallable, [TypeRepr(BoolType), [TypeRepr(BoolType)], TypeRepr(BoolType)])
        out.add_internal(BOOL)
        out.add_member("==", cmp_type)
        out.add_member("!=", cmp_type)

        out.finalize()
        return out

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        raise RuntimeError("Bool type has no initializer, this function should not be called.")


class IntType(ObjectType):
    NAME = "Int"
    IS_REFCOUNTED = False

    eq = cmp_function(current_module, "Int", "==", INT, INT)
    neq = cmp_function(current_module, "Int", "!=", INT, INT)
    less = cmp_function(current_module, "Int", "<", INT, INT)
    greater = cmp_function(current_module, "Int", ">", INT, INT)
    leq = cmp_function(current_module, "Int", "<=", INT, INT)
    geq = cmp_function(current_module, "Int", ">=", INT, INT)

    # TODO: Overload for unary minus.
    add = arith_function(current_module, "Int", "+", lambda builder, lhs, rhs: builder.add(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
    sub = arith_function(current_module, "Int", "-", lambda builder, lhs, rhs: builder.sub(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
    mul = arith_function(current_module, "Int", "*", lambda builder, lhs, rhs: builder.mul(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore
    div = arith_function(current_module, "Int", "/", lambda builder, lhs, rhs: builder.div(lhs, rhs, "arith_res"), INT, INT, INT)  # type: ignore

    initializer: Function | None = None

    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_object",
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        cmp_type = get_type(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)])
        arith_type = get_type(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)])
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
    def from_literal(value: int, dbg_name: str = "unnamed_int") -> IntType:
        typ = get_type(IntType)

        if not IntType.initializer:
            initializer_typ = FunctionType(POINTER, [INT])
            initializer = Function(current_module, initializer_typ, "Int__initializer")
            block = initializer.append_basic_block("entry")
            builder = IRBuilder(block)
            val = initializer.args[0]

            functions = [
                ("eq", IntType.eq),
                ("neq", IntType.neq),
                ("less", IntType.less),
                ("greater", IntType.greater),
                ("leq", IntType.leq),
                ("geq", IntType.geq),
                ("add", IntType.add),
                ("sub", IntType.sub),
                ("mul", IntType.mul),
                ("div", IntType.div),
            ]

            struct_ptr = builder.alloca(typ.llvm_type, name="struct_ptr")
            val_field_ptr = builder.gep(struct_ptr, [INT(0), INT(0)], name="val_field_ptr")
            builder.store(val, val_field_ptr)

            functions_into_struct(builder, functions, struct_ptr)

            builder.ret(val)

            IntType.initializer = initializer

        builder = builder_stack[-1]
        val = builder.call(IntType.initializer, [INT(value)], name=dbg_name)

        return IntType(val, typ, allocate=False, dbg_name=dbg_name)

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        out = BaseMetatype(
            IntType,
            TypeHeader("Int", {}, [get_type(ObjectType)], {}, {}, {}, has_constructor=False),
        )
        db["Int"] = out

        cmp_type = get_type(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(BoolType)])
        arith_type = get_type(InstanceCallable, [TypeRepr(IntType), [TypeRepr(IntType)], TypeRepr(IntType)])

        out.add_internal(INT)
        out.add_member("==", cmp_type)
        out.add_member("!=", cmp_type)
        out.add_member("<", cmp_type)
        out.add_member(">", cmp_type)
        out.add_member("<=", cmp_type)
        out.add_member(">=", cmp_type)
        out.add_member("+", arith_type)
        out.add_member("-", arith_type)
        out.add_member("*", arith_type)
        out.add_member("/", arith_type)
        # TODO: Rest of operators

        out.finalize()
        return out

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        raise RuntimeError("Int type has no initializer, this function should not be called.")


class FloatType(ObjectType):
    pass


class StringType(ObjectType):
    pass


class ListType(ObjectType):
    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_object"
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

    @staticmethod
    def from_values(typ: BaseMetatype, values: list[ObjectType]) -> ListType:
        # TODO: ListType from_values
        pass


class TupleType(ObjectType):
    def __init__(
        self, value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_tuple"
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

    @staticmethod
    def from_values(typ: BaseMetatype, values: list[ObjectType]) -> TupleType:
        # TODO: TupleType from_values
        pass

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) > 0
        out = BaseMetatype(
            TupleType,
            TypeHeader(
                "Tuple",
                {
                    str(i): generic for i, generic in enumerate(generics)
                },
                [get_type(ObjectType)],
                {}
            )
        )

        for i, generic in enumerate(generics):
            out.add_parameter(str(i), generic)

        out.finalize()
        return out

    @staticmethod
    def member_builder(typ: BaseMetatype, params: list[ObjectType]) -> list[Value]:
        # TODO: TupleType member builder
        pass

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        return ObjectType._initializer_builder(metatype, TupleType.member_builder)


class DictType(ObjectType):
    def __init__(
        self,
        value: Value,
        typ: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_dict"
    ) -> None:
        super().__init__(value, typ, allocate, reference, dbg_name)

        self.key_type = self.typ.header.generics["Key"]
        self.value_type = self.typ.header.generics["Value"]

        at_type = get_type(InstanceCallable, [TypeRepr(DictType), [TypeRepr(self.key_type.bound)], TypeRepr(self.value_type.bound)])

        self.members = {
            "at": (at_type, 1),
            "[]": (at_type, 1),
        }

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 0
        out = BaseMetatype(
            DictType,
            TypeHeader(
                "Dict",
                {
                    "Key": generics[0],
                    "Value": generics[1],
                },
                [get_type(ObjectType)],
                {},
                {},
                {},
                has_constructor=False
            ),
        )
        db["Dict"] = out

        at_type = get_type(InstanceCallable, [TypeRepr(DictType), [TypeRepr(generics[0].bound)], TypeRepr(generics[1].bound)])

        out.add_internal(POINTER)
        out.add_member("at", at_type)
        out.add_member("[]", at_type)
        # TODO: Rest of operators.

        out.finalize()
        return out

    @staticmethod
    def member_builder(typ: BaseMetatype, params: list[ObjectType]) -> list[Value]:
        assert len(params) == 0

        key = typ.generics["Key"]
        value = typ.generics["Value"]
        item_struct = LiteralStructType([key.storage_type, value.storage_type])

        hash_typ = FunctionType(LONG, [POINTER, LONG, LONG])
        hash = Function(current_module, hash_typ, f"{typ.generic_name}__hash")
        # TODO: Hash implementation

        compare_typ = FunctionType(INT, [POINTER, POINTER, POINTER])
        compare = Function(current_module, compare_typ, f"{typ.generic_name}__compare")
        # TODO: Compare implementation (equality)

        builder = builder_stack[-1]

        hash_ptr = builder.bitcast(hash, POINTER, "hash_ptr")
        compare_ptr = builder.bitcast(compare, POINTER, "compare_ptr")

        hashmap_ptr = builder.call(runtime.hashmap_new, [
            LONG(get_type_size(item_struct)), LONG(0), LONG(0), LONG(0),
            hash_ptr, compare_ptr, NULL, NULL
        ])

        # TODO: Also return function pointers.

        return [hashmap_ptr]

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        return ObjectType._initializer_builder(metatype, DictType.member_builder)


class Callable(ObjectType, ABC):
    """
    Represents a basic function which can be called with arguments.
    This is a complete function which has already been generated, and
    so generic arguments cannot be passed in. `CallableGroup` handles
    dynamic compilation of functions with generics.
    """

    NAME = "BasicCallable"

    initializer: Function | None = None

    def __init__(
        self,
        function: Value,
        function_type: Type,
        typ: BaseMetatype,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        if not self.initializer:
            initializer_typ = FunctionType(POINTER, [POINTER])
            self.initializer = Function(current_module, initializer_typ, "Callable__initializer")
            block = self.initializer.append_basic_block("entry")
            builder = IRBuilder(block)
            function_ptr = self.initializer.args[0]

            struct_mem = builder.call(runtime.rc_alloc_func, [INT(get_type_size(typ.llvm_type))], "struct_mem")
            struct_ptr: Value = builder.bitcast(struct_mem, typ.llvm_type, "struct_ptr") # type: ignore

            value_ptr = builder.gep(struct_ptr, [INT(0), INT(0)])
            builder.store(function_ptr, value_ptr)

            builder.ret(value_ptr)

        if allocate:
            builder = builder_stack[-1]
            self.function_pointer: Value = builder.bitcast(function, POINTER, "function_ptr")  # type: ignore
            ptr = builder.call(self.initializer, [self.function_pointer], dbg_name)

            super().__init__(ptr, typ, False, reference, dbg_name)
        else:
            super().__init__(function, typ, False, reference, dbg_name)

        self.params = params
        self.returns = returns
        self.function_type = function_type

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
        assert issubclass(val_type.bound, Callable)
        params = val_type.header.generics["Params"]
        returns = val_type.header.generics["Returns"]
        assert isinstance(params, TupleMetatype)
        return Callable(
            value,
            FunctionType(
                returns.llvm_type,
                [param.llvm_type for param in params.generics.values()]
            ),
            val_type,
            params.element_types,
            returns,
            False,
            reference,
            name
        )

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 2
        assert isinstance(generics[0], TupleMetatype)

        params = generics[0]
        returns = generics[1]

        # llvm_type = current_module.context.get_identified_type("BasicCallable")
        # llvm_type.set_body(POINTER)
        out = BaseMetatype(
            Callable,
            TypeHeader(
                "Callable",
                {
                    "Params": params,
                    "Returns": returns,
                },
                [get_type(ObjectType)],
                {},
                {},
                {},
                has_constructor=False,
            ),
        )

        out.add_internal(POINTER)

        out.finalize()
        return out

    @staticmethod
    def get_initializer(metatype: BaseMetatype) -> Callable:
        raise RuntimeError("Callable type has no initializer, this function should not be called.")


class InstanceCallable(Callable):
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

    initializer: Function | None = None

    def __init__(
        self,
        function: Value,
        function_type: Type,
        typ: BaseMetatype,
        instance: ObjectType,
        params: list[BaseMetatype],
        returns: BaseMetatype,
        flags: set[Flags],
        allocate: bool = True,
        reference: bool = False,
        dbg_name: str = "unnamed_callable",
    ) -> None:
        if not self.initializer:
            initializer_typ = FunctionType(POINTER, [POINTER, POINTER])
            self.initializer = Function(current_module, initializer_typ, "InstanceCallable__init")
            block = self.initializer.append_basic_block("entry")
            builder = IRBuilder(block)
            on_ptr = self.initializer.args[0]
            function_ptr = self.initializer.args[1]

            struct_mem = builder.call(runtime.rc_alloc_func, [INT(get_type_size(typ.llvm_type))], "struct_mem")
            struct_ptr: Value = builder.bitcast(struct_mem, typ.llvm_type, "struct_ptr") # type: ignore

            on_ptr_field = builder.gep(struct_ptr, [INT(0), INT(0)], name="on_ptr_field")
            builder.store(on_ptr, on_ptr_field)

            value_ptr = builder.gep(struct_ptr, [INT(0), INT(1)], name="value_ptr")
            builder.store(function_ptr, value_ptr)

            builder.ret(value_ptr)

        # TODO: RC Retain for instance.

        if allocate:
            assert isinstance(function, Function)
            builder = builder_stack[-1]
            self.function_pointer: Value = builder.bitcast(function, POINTER, "function_ptr")  # type: ignore
            ptr = builder.call(self.initializer, [instance.value, self.function_pointer], dbg_name)

            super().__init__(ptr, function_type, typ, params, returns, False)
        else:
            assert function_type is not None
            super().__init__(function, function_type, typ, params, returns, False)

        self.instance = instance
        self.params = params
        self.returns = returns
        self.function_type = function_type
        self.flags = flags

    def call(
        self,
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        assert self.Flags.ABSTRACT not in self.flags
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
        return InstanceCallable(
            value,
            FunctionType(
                returns.llvm_type,
                [param.llvm_type for param in params.generics.values()]
            ),
            val_type,
            instance,
            rest,
            returns,
            set(),
            False,
            reference,
            name
        )

    @staticmethod
    def create_metatype(db: dict[str, BaseMetatype], generics: list[BaseMetatype]) -> BaseMetatype:
        assert len(generics) == 3
        assert isinstance(generics[1], TupleMetatype)

        on = generics[0]
        params = generics[1]
        returns = generics[2]

        # llvm_type = current_module.context.get_identified_type("InstanceCallable")
        # llvm_type.set_body(POINTER)
        out = BaseMetatype(
            InstanceCallable,
            TypeHeader(
                "InstanceCallable",
                {
                    "On": on,
                    "Params": params,
                    "Returns": returns,
                },
                [get_type(Callable)],
                {},
                {},
                {},
                has_constructor=False,
            ),
        )

        out.add_internal(POINTER) # Object pointer
        out.add_internal(POINTER) # Function pointer

        out.finalize()
        return out


# TODO: AnonymousCallable needs some way to store bound_values that probably isn't generics (although that wouldn't actually be that bad)
class AnonymousCallable(Callable):
    NAME = "AnonymousCallable"

    # def __init__(
    #     self,
    #     value: Function,
    #     typ: BaseMetatype,
    #     params: list[BaseMetatype],
    #     returns: BaseMetatype,
    #     bound_values: list[ObjectType],
    #     allocate: bool = True,
    #     reference: bool = False,
    #     dbg_name: str = "unnamed_callable",
    # ) -> None:
    #     #super().__init__(value, typ, params, returns, 0, allocate, reference, dbg_name)

    #     # struct_type = LiteralStructType([val.storage_type for val in bound_values])

    #     # builder = builder_stack[-1]
    #     # builder.comment(f"Transferring bound values to the struct for {dbg_name} anonymous lambda.")
    #     # struct_ptr = builder.alloca(struct_type, name=f"{dbg_name}_bindings")
    #     # for i, binding in enumerate(bound_values):
    #     #     field_ptr = builder.gep(struct_ptr, [INT(0), INT(i)])
    #     #     builder.store(binding.value, field_ptr)

    # # TODO: from_value for AnonymousCallable

    # @staticmethod
    # def create_metatype(generics: list[BaseMetatype]) -> BaseMetatype:
    #     assert len(generics) == 2
    #     assert isinstance(generics[0], TupleMetatype)
    #     # llvm_type = current_module.context.get_identified_type("AnonymousCallable")
    #     # llvm_type.set_body(POINTER, POINTER)
    #     return BaseMetatype(
    #         AnonymousCallable,
    #         TypeHeader(
    #             "AnonymousCallable",
    #             {
    #                 "Params": generics[0],
    #                 "Returns": generics[1],
    #             },
    #             [get_type(TypeRepr(BasicCallable))],
    #             {},
    #             {},
    #             {},
    #             has_constructor=False,
    #         ),
    #     )

    # # TODO: This should probably be able to bind the instance.
    # @staticmethod
    # def fill_metatype(typ: BaseMetatype) -> None:
    #     typ.add_member("value", typ, internal=True)

    #     call_type = get_type(
    #         TypeRepr(
    #             InstanceCallable,
    #             [TypeRepr(BasicCallable), typ.header.generics["Params"], typ.header.generics["Returns"]],
    #         )
    #     )
    #     typ.add_member("call", call_type)


class CallableGroup:
    """
    Represents multiple functions which have the same name and return
    type.
    """

    def __init__(self, implementations: list[Callable]) -> None:
        self.implementations = implementations

    def call(
        self,
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        for impl in self.implementations:
            if len(impl.params) != len(arguments):
                continue

            same = True
            for param, arg in zip(impl.params, arguments):
                if not param.typ.validate(arg):
                    same = False
                    break

            if not same:
                continue

            return impl.call(arguments, reference, var_name)

        raise RuntimeError("No available implementation for function found.")
