from dataclasses import dataclass
from pathlib import Path
import llvmlite.binding as llvm
from llvmlite.ir import Function, FunctionType, IRBuilder, Module
from llvmlite.ir.types import Type

import hydro.builders as builders
from hydro.builders import current_module, builder_stack
from hydro.helpers import INT, NULL, POINTER
from hydro.lang_types import BaseMetatype, BoolType, Callable, InstanceCallable, IntType, ListType, ObjectType, TupleMetatype, TupleType, VoidType, get_type, type_db
from hydro.loggers import create_logger
from hydro.parser.nodes import Array, Atom, Binary, Block, Call, ClassDecl, CustomStatement, Declaration, Expression, FunctionDecl, Grouping, Identifier, Literal, Member, Primary, Program, Slice, Span, Statement, Ternary, Tuple, TypeNode, Unary, VarDecl, VarSet
from hydro.runtime import Runtime
from src.hydro.tokens import Lexeme


logger = create_logger("Compiler")
errors = create_logger("Compiler", False)


@dataclass
class Header:
    node: Declaration
    generics_num: int
    params: list[BaseMetatype]
    inside: BaseMetatype | None


class Implementations:
    """
    Represents multiple implementations of some function, either by
    generics or different specified implementations.
    """

    def __init__(self, name: Lexeme) -> None:
        self.name = name
        self.headers: list[Header] = []
        self.implementations: list[tuple[list[BaseMetatype], Callable]] = []

    def add_impl(self, impl: Header) -> None:
        self.headers.append(impl)

    def has_impl(self, generics: list[BaseMetatype], args: list[BaseMetatype]) -> bool:
        for impl_generics, impl in self.implementations:
            if len(impl.params) != len(args) or len(impl_generics) != len(generics):
                continue

            same = True
            for (param, arg), (generic_param, generic_arg) in zip(zip(impl.params, args), zip(impl_generics, generics)):
                if not param.typ.validate(arg) or generic_param != generic_arg:
                    same = False
                    break
            if not same:
                continue

            return True
        return False

    def get_requires_compile(self, generics: list[BaseMetatype], args: list[BaseMetatype]) -> Header | None:
        if self.has_impl(generics, args):
            return None

        for header in self.headers:
            assert isinstance(header.node, (FunctionDecl, ClassDecl))
            if header.generics_num != len(generics) or len(header.params) != len(args):
                continue

            valid = True
            for param, arg in zip(header.params, args):
                if param != arg:
                    valid = False
                    break
            if not valid:
                continue

            return header

        raise RuntimeError("No available implementation for function found.")

    def call(
        self,
        generics: list[BaseMetatype],
        arguments: list[ObjectType],
        reference: bool = False,
        var_name: str = "unknown_callable",
    ) -> ObjectType:
        if not self.has_impl(generics, [arg.typ for arg in arguments]):
            raise RuntimeError("Need to check for and provide an implementation for function.")

        for generics, impl in self.implementations:
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


Scope = dict[Lexeme, ObjectType | Header]


class CompileError(RuntimeError):
    def __init__(self, lexeme: Lexeme | Span, msg: str, code: str = "-1") -> None:
        super().__init__()
        if isinstance(lexeme, Lexeme):
            errors.error(f"[{lexeme.pos}] [{lexeme}] {f"[{code}]" if code != "-1" else ""} {msg}")
        self.lexeme = lexeme
        self.msg = msg
        self.code = code


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
            self.declaration(decl, None)

        self.pop_scope()

    def declaration(self, decl: Declaration, inside: BaseMetatype | None):
        if isinstance(decl, ClassDecl):
            self.class_header(decl, inside)
        if isinstance(decl, FunctionDecl):
            self.function_header(decl, inside)
        if isinstance(decl, VarDecl):
            self.var_decl(decl)
        logger.error("Unexpected declaration type.")

    def statement(self, stmt: Statement) -> ObjectType | None:
        if isinstance(stmt, VarDecl):
            return self.var_decl(stmt)
        if isinstance(stmt, CustomStatement):
            return self.custom_statement(stmt)
        if isinstance(stmt, VarSet):
            return self.var_set(stmt)
        logger.error("Unexpected statement type.")

    def expression(self, expr: Expression, into_name: str = "unnamed_expression") -> ObjectType:
        if isinstance(expr, Ternary):
            return self.ternary(expr, into_name)
        if isinstance(expr, Unary):
            return self.unary(expr, into_name)
        if isinstance(expr, Binary):
            return self.binary(expr, into_name)
        if isinstance(expr, Primary):
            return self.primary(expr, False, into_name)
        logger.error("Unexpected expression type.")
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

    def class_header(self, decl: ClassDecl, inside: BaseMetatype | None) -> None:
        if decl.name.raw in self.scope:
            raise CompileError(decl.name, "Class name already exists in the current scope.")
        self.scope[decl.name] = Header(decl, inside)

    def function_header(self, fn: FunctionDecl, inside: BaseMetatype | None) -> None:
        if fn.name.raw in self.scope:
            raise CompileError(fn.name, "Name already exists in the current scope.")
        self.scope[fn.name] = Header(fn, inside)

    def gen_member_function(self, header: Header, generics: list[TypeNode]) -> ObjectType:
        assert header.inside is not None

        node = header.node

        assert isinstance(node, FunctionDecl)
        if len(generics) != len(node.generics):
            span = Span(generics[0].spans.start, generics[-1].spans.end)
            raise CompileError(span, f"Length of generic arguments doesn't match expected count ({len(node.generics)})")

        params: list[tuple[BaseMetatype, Lexeme]] = []
        returns = self.get_type(node.returns) if node.returns else get_type(VoidType)

        ir_type = FunctionType(returns.storage_type, [header.inside.storage_type] + [param.storage_type for param, _ in params])
        ir_function = Function(current_module, ir_type, node.name.raw)

        block = ir_function.append_basic_block("entry")
        builder_stack.append(IRBuilder(block))
        self.push_scope()

        obj = header.inside.bound.from_value(ir_function.args[0], header.inside)
        obj.extract_values(self.scope)

        for (typ, name), value in zip(params, ir_function.args):
            self.scope[name] = typ.bound.from_value(value, typ)

        assert obj is not None
        param_types = [param for param, _ in params[1:]]
        function_type = get_type(InstanceCallable, [header.inside, param_types, returns])
        return InstanceCallable(ir_function, ir_type, function_type, obj, param_types, returns, set())

    def gen_static_function(self, header: Header, generics: list[TypeNode]) -> ObjectType:
        node = header.node
        assert isinstance(node, FunctionDecl)
        if len(generics) != len(node.generics):
            span = Span(generics[0].spans.start, generics[-1].spans.end)
            raise CompileError(span, f"Length of generic arguments doesn't match expected count ({len(node.generics)})")

        params: list[tuple[BaseMetatype, Lexeme]] = []
        for param in node.params.pos:
            params.append((self.get_type(param.typ), param.name))
        returns = self.get_type(node.returns) if node.returns else get_type(VoidType)

        ir_type = FunctionType(returns.storage_type, [param.storage_type for param, _ in params])
        ir_function = Function(current_module, ir_type, node.name.raw)

        block = ir_function.append_basic_block("entry")
        builder_stack.append(IRBuilder(block))
        self.push_scope()

        for (typ, name), value in zip(params, ir_function.args):
            self.scope[name] = typ.bound.from_value(value, typ)

        param_types = [param for param, _ in params]
        function_type = get_type(Callable, [param_types, returns])
        return Callable(ir_function, ir_type, function_type, param_types, returns)

    def var_decl(self, decl: VarDecl) -> None:
        typ = self.get_type(decl.typ)
        assert decl.name.raw not in self.scope
        value = self.expression(decl.value, decl.name.raw)
        if not typ.validate(value):
            raise CompileError(decl.value.spans, f"Expression evaluates to '{value.typ.name}' but expected '{typ.name}'.")
        self.scope[decl.name] = value

    def custom_statement(self, stmt: CustomStatement) -> ObjectType | None:
        match stmt.name.raw:
            case "if": return self.if_stmt(stmt)
            case "while": return self.while_stmt(stmt)
            case "for": return self.for_stmt(stmt)
            case "return": return self.return_stmt(stmt)
        logger.warning(f"Unkown statement '{stmt.name.raw}'. No code generated.")

    def if_stmt(self, stmt: CustomStatement) -> ObjectType | None:
        condition_node = stmt.expressions["condition"]
        body_node = stmt.expressions["body"]
        assert isinstance(condition_node, Expression)
        assert isinstance(body_node, Block)

        condition = self.expression(condition_node)
        if not isinstance(condition, BoolType):
            raise CompileError(condition_node.spans, f"Condition of if statement must evaluate to type 'Bool', but instead got type '{condition.typ}'")

        truthy_block = self.builder.append_basic_block("if")
        falsey_block = self.builder.append_basic_block("else")
        continue_block = self.builder.append_basic_block("continue")
        self.builder.cbranch(condition.get_raw(), truthy_block, falsey_block)

        self.builder.position_at_start(truthy_block)
        returns = self.block(body_node.stmts)
        if returns is None:
            self.builder.branch(continue_block)

        self.builder.position_at_start(falsey_block)
        if len(stmt.following) == 0:
            self.builder.branch(continue_block)
            self.builder.position_at_start(falsey_block)
            return None # Cannot gurantee a return if if statement isn't exhaustive.

        assert len(stmt.following) == 1
        following = stmt.following[0]
        if following.name == "elif":
            returns = self.if_stmt(following) if returns else None
        elif following.name == "else":
            returns = self.else_stmt(following) if returns else None
        else:
            logger.error("Unexpected following statement after 'if'.")

        if not returns:
            self.builder.branch(continue_block)
            self.builder.position_at_start(falsey_block)
        return returns

    def else_stmt(self, stmt: CustomStatement) -> ObjectType | None:
        body_node = stmt.expressions["body"]
        assert isinstance(body_node, Block)

        return self.block(body_node.stmts)

    def while_stmt(self, stmt: CustomStatement) -> ObjectType | None:
        pass

    def for_stmt(self, stmt: CustomStatement) -> ObjectType:
        pass

    def return_stmt(self, stmt: CustomStatement) -> ObjectType:
        if "expression" not in stmt.expressions:
            return VoidType()
        expression = stmt.expressions["expression"]
        assert isinstance(expression, Expression)
        value = self.expression(expression)
        self.builder.ret(value.value)
        return value

    def var_set(self, stmt: VarSet) -> None:
        into = self.primary(stmt.into, True, "varset_into")
        value = self.expression(stmt.value, "varset_val")
        into.set_to(value)

    def ternary(self, ternary: Ternary, into_name: str = "unnamed_ternary") -> ObjectType:
        switch = self.expression(ternary.switch, f"{into_name}_switch")
        if not isinstance(switch, BoolType):
            raise CompileError(ternary.switch.spans, "In a ternary, this expression must evaluate to type bool.")
        truthy = self.expression(ternary.truthy, f"{into_name}_truthy")
        falsey = self.expression(ternary.falsey, f"{into_name}_falsey")
        assert truthy.typ == falsey.typ

        value = switch.get_raw()
        out_mem = self.builder.alloca(POINTER, name=f"{into_name}_ternary_res")

        truthy_block = self.builder.append_basic_block(f"ternary_{into_name}_truthy")
        falsey_block = self.builder.append_basic_block(f"ternary_{into_name}_falsey")
        continued_block = self.builder.append_basic_block(f"ternary_{into_name}_continue")

        self.builder.cbranch(value, truthy_block, falsey_block)

        self.builder.position_at_start(truthy_block)
        self.builder.store(truthy.value, out_mem)
        self.builder.branch(continued_block)

        self.builder.position_at_start(falsey_block)
        self.builder.store(falsey.value, out_mem)
        self.builder.branch(continued_block)

        return truthy.typ.from_value(out_mem, truthy.typ, name=into_name)

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
        logger.error("Unexpected primary type.")
        assert False

    def member(self, member: Member, reference: bool = False, into_name: str = "unnamed_member") -> ObjectType:
        on = self.primary(member.on, False, f"{into_name}_geton")
        return on.get_member(member.name.raw, reference, into_name)

    def call(self, call: Call, reference: bool = False, into_name: str = "unnamed_call") -> ObjectType:
        on = self.primary(call.on, False, f"{into_name}_callon")
        if not isinstance(on, Callable): # TODO: Check for () operator instead.
            raise CompileError(call.spans, f"Type '{on.typ.name}' cannot be called.")
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
        logger.error("Unexpected atom type.")
        assert False

    def identifier(self, id: Identifier, generics: list[TypeNode] = []) -> ObjectType:
        return self.get_field(id.text, generics)

    def literal(self, literal: Literal, dbg_name: str = "unnamed_literal") -> ObjectType:
        if isinstance(literal.value, bool):
            return BoolType.from_literal(literal.value, dbg_name)
        if isinstance(literal.value, int):
            return IntType.from_literal(literal.value, dbg_name)
        # TODO: Rest of literals
        logger.error("Unexpected literal type.")
        assert False

    def grouping(self, grouping: Grouping) -> ObjectType:
        return self.expression(grouping.expr)

    def gen_tuple(self, node: Tuple) -> ObjectType:
        if len(node.values) == 0:
            raise CompileError(node.spans, "Tuple types cannot be empty.")
        values = [self.expression(value) for value in node.values]
        generics = [value.typ for value in values]
        typ = get_type(TupleType, generics)
        return TupleType.from_values(typ, values)

    def array(self, array: Array) -> ObjectType:
        if len(array.values) == 0:
            raise CompileError(array.spans, "Array cannot be empty (will be fixed with type inference).")
        values = [self.expression(value) for value in array.values]
        item_typ = values[0].typ
        for value in values:
            assert value.typ == item_typ
        typ = get_type(ListType, [item_typ])
        return ListType.from_values(typ, values)

    def get_header(self, header: Header, generics: list[TypeNode]) -> ObjectType:
        if isinstance(header.node, FunctionDecl):
            return self.gen_member_function(header, generics) if header.inside else self.gen_static_function(header, generics)
        if isinstance(header.node, ClassDecl):
            pass
        assert False

    def get_field(self, name: Lexeme, generics: list[TypeNode]) -> ObjectType:
        for scope in reversed(self.scopes):
            if name in scope:
                value = scope[name]
                assert not (len(generics) > 0 and isinstance(value, ObjectType))
                if isinstance(value, Header):
                    obj = self.get_header(value, generics)
                    if len(generics) == 0: # TODO: Save each generic implementation somehow.
                        scope[name] = obj
                return value if isinstance(value, ObjectType) else self.get_header(value, generics)
        raise CompileError(name, "Not found in current scope.")

    def get_type(self, typ: TypeNode) -> BaseMetatype:
        if typ.name.raw not in self.headers:
            raise CompileError(typ.name, "Type not found in the current scope.")
        header = self.headers[typ.name.raw]
        generics = [self.get_type(generic) for generic in typ.generics]
        return get_type(header, generics)

    def push_scope(self):
        self.scopes.append({})

    def pop_scope(self) -> Scope:
        # TODO: Other stuff here (freeing and memory)
        return self.scopes.pop()
