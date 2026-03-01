from dataclasses import dataclass
from pathlib import Path
import llvmlite.binding as llvm
from llvmlite.ir import Function, FunctionType, IRBuilder, Module

import hydro.builders as builders
from hydro.builders import current_module, runtime, builder_stack
from hydro.helpers import INT
from hydro.lang_types import BaseMetatype, BoolType, Callable, IntType, ObjectType, type_db
from hydro.loggers import create_logger
from hydro.parser.nodes import Array, Atom, Binary, Block, Call, ClassDecl, CustomStatement, Declaration, Expression, Grouping, Identifier, Literal, Member, Primary, Program, Slice, Statement, Ternary, Tuple, Type, Unary, VarDecl, VarSet
from hydro.runtime import Runtime
from src.hydro.tokens import Lexeme


logger = create_logger("Compiler")
errors = create_logger("Compiler", False)


Scope = dict[Lexeme, ObjectType]


class Compiler:
    def __init__(self, program: Program, build_dir: Path) -> None:
        logger.debug(f"Starting compiler for {program.path}. Building into {build_dir}")

        self.program = program
        self.build_dir = build_dir
        self.scopes: list[Scope] = [{}]
        self.headers: dict[str, type[ObjectType]] = {
            "Bool": BoolType,
        }

        llvm.initialize_native_target()
        llvm.initialize_native_asmprinter()
        self.target = llvm.Target.from_default_triple().create_target_machine(reloc="pic")
        logger.debug("LLVM Targets initialized.")

        builders.current_module = Module(program.path.stem)
        builders.runtime = Runtime(builders.current_module)
        logger.debug("Runtime initialized.")

        self.scope[Lexeme.make_id("Bool")] = BoolType.create_metatype(type_db)
        logger.debug("Builtin types initialized.")

        main_ty = FunctionType(INT, [])
        self.main = Function(current_module, main_ty, "main")

    @property
    def scope(self) -> Scope:
        return self.scopes[-1]

    @property
    def builder(self) -> IRBuilder:
        return builder_stack[-1]

    @property
    def module_name(self) -> str:
        return self.program.path.stem

    def gen_program(self) -> None:
        logger.info(f"Compiling {self.program.path}")

        for imp in self.program.imports:
            # TODO: Imports
            pass

        self.push_scope()

        for decl in self.program.declarations:
            self.declaration(decl)

        self.pop_scope()

    def declaration(self, decl: Declaration):
        if isinstance(decl, ClassDecl):
            self.class_decl(decl)
        if isinstance(decl, Function):
            self.function(decl)
        if isinstance(decl, VarDecl):
            self.var_decl(decl)

    def statement(self, stmt: Statement) -> ObjectType | None:
        if isinstance(stmt, VarDecl):
            return self.var_decl(stmt)
        if isinstance(stmt, CustomStatement):
            return self.custom_statement(stmt)
        if isinstance(stmt, VarSet):
            return self.var_set(stmt)

    def expression(self, expr: Expression, into_name: str = "unnamed_expression") -> ObjectType:
        if isinstance(expr, Ternary):
            return self.ternary(expr, into_name)
        if isinstance(expr, Unary):
            return self.unary(expr, into_name)
        if isinstance(expr, Binary):
            return self.binary(expr, into_name)
        if isinstance(expr, Primary):
            return self.primary(expr, False, into_name)
        assert False

    def block(self, stmts: list[Statement]) -> ObjectType | None:
        self.push_scope()

        for stmt in stmts:
            possibly_ret = self.statement(stmt)
            if possibly_ret is not None:
                self.pop_scope()
                return possibly_ret

        self.pop_scope()
        self.builder.ret_void()

    def class_decl(self, decl: ClassDecl) -> None:
        assert decl.name.raw not in self.scope
        # TODO: Classes

    def function(self, fn: Function) -> None:
        assert fn.name.raw not in self.scope
        # TODO: Functions

    def var_decl(self, decl: VarDecl) -> None:
        typ = self.get_type(decl.typ)
        assert decl.name.raw not in self.scope
        value = self.expression(decl.value, decl.name.raw)
        assert typ.validate(value)
        self.scope[decl.name] = value

    def custom_statement(self, stmt: CustomStatement) -> ObjectType | None:
        pass

    def var_set(self, stmt: VarSet) -> None:
        into = self.primary(stmt.into, True, "varset_into")
        value = self.expression(stmt.value, "varset_val")
        into.set_to(value)

    def ternary(self, ternary: Ternary, into_name: str = "unnamed_ternary") -> ObjectType:
        switch = self.expression(ternary.switch)
        assert isinstance(switch, BoolType)
        truthy = self.expression(ternary.truthy)
        falsey = self.expression(ternary.falsey)
        value = switch.get_raw()

        truthy_block = self.builder.append_basic_block(f"ternary_{into_name}_truthy")
        falsey_block = self.builder.append_basic_block(f"ternary_{into_name}_falsey")
        continued_block = self.builder.append_basic_block(f"ternary_{into_name}_continue")

        self.builder.cbranch(value, truthy_block, falsey_block)

        self.builder.position_at_start(truthy_block)
        # TODO: bah ternary

    def unary(self, unary: Unary, into_name: str = "unnamed_unary") -> ObjectType:
        right = self.expression(unary.right, f"{into_name}_opright")
        return right.call_on(unary.op.raw, [], [], into_name=into_name)

    def binary(self, binary: Binary, into_name: str = "unnamed_binary") -> ObjectType:
        left = self.expression(binary.left, f"{into_name}_opleft")
        right = self.expression(binary.right, f"{into_name}_opright")
        return left.call_on(binary.op.raw, [], [right], into_name=into_name)

    def primary(self, primary: Primary, reference: bool = False, into_name: str = "unnamed_primary") -> ObjectType:
        if isinstance(primary, Member):
            return self.member(primary, reference, into_name)
        if isinstance(primary, Call):
            return self.call(primary, reference, into_name)
        if isinstance(primary, Slice):
            return self.slice(primary, reference, into_name)
        if isinstance(primary, Atom):
            return self.atom(primary)
        assert False

    def member(self, member: Member, reference: bool = False, into_name: str = "unnamed_member") -> ObjectType:
        on = self.primary(member.on, False, f"{into_name}_geton")
        return on.get_member(member.name.raw, reference, into_name)

    def call(self, call: Call, reference: bool = False, into_name: str = "unnamed_call") -> ObjectType:
        on = self.primary(call.on, False, f"{into_name}_callon")
        assert isinstance(on, Callable)
        generics = [self.get_type(generic) for generic in call.generics]
        args = [self.expression(arg) for arg in call.args.pos]
        # TODO: kwargs and generics
        return on.call(args, reference, into_name)

    def slice(self, slice: Slice, reference: bool = False, into_name: str = "unnamed_slice") -> ObjectType:
        on = self.primary(slice.on, False, f"{into_name}_sliced")
        using = self.expression(slice.using)
        return on.call_on("[]", [], [using], reference, into_name)

    def atom(self, atom: Atom) -> ObjectType:
        if isinstance(atom, Identifier):
            return self.identifier(atom)
        if isinstance(atom, Literal):
            return self.literal(atom)
        if isinstance(atom, Grouping):
            return self.grouping(atom)
        if isinstance(atom, Tuple):
            return self.gen_tuple(atom)
        assert False

    def identifier(self, id: Identifier) -> ObjectType:
        return self.get_field(id.text)

    def literal(self, literal: Literal, dbg_name: str = "unnamed_literal") -> ObjectType:
        if isinstance(literal.value, bool):
            return BoolType.from_literal(literal.value, dbg_name)
        if isinstance(literal.value, int):
            return IntType.from_literal(literal.value, dbg_name)
        # TODO: Rest of literals
        assert False

    def grouping(self, grouping: Grouping) -> ObjectType:
        return self.expression(grouping.expr)

    def gen_tuple(self, node: Tuple) -> ObjectType:
        assert len(node.values) > 0

    def array(self, array: Array) -> ObjectType:
        pass

    def get_field(self, name: Lexeme) -> ObjectType:
        pass

    def get_type(self, typ: Type) -> BaseMetatype:
        pass

    def push_scope(self):
        self.scopes.append({})

    def pop_scope(self) -> Scope:
        # TODO: Other stuff here (freeing and memory)
        return self.scopes.pop()
