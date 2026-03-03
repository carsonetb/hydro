from ast import Expr
from enum import Enum, auto
from pathlib import Path
from typing import Callable

from hydro.loggers import create_logger
from hydro.parser.interface import ParseError
from hydro.parser.nodes import Annotation, Arguments, Array, Atom, Binary, Block, Call, ClassDecl, Declaration, DefaultParam, Expression, FunctionDecl, Generic, Grouping, Identifier, Import, Literal, Member, Param, Parameters, Primary, Program, Slice, Span, Statement, Ternary, Tuple, TypeNode, Unary, VarDecl, VarSet
from hydro.parser.rules import FullParser, Rule
from hydro.scanner import Lexeme, Scanner
from hydro.tokens import Token


logger = create_logger("Parser")


class Parser(FullParser):
    class DeclType(Enum):
        VAR = auto()
        FN = auto()
        CLASS = auto()

    def __init__(self, srcdir: Path, file: Path, tokens: list[Lexeme], rules: list[Rule], handled_imports: dict[str, Program]) -> None:
        super().__init__(srcdir, file, tokens)

        self.rules = rules
        self.imports: list[Import] = []
        self.handled_imports = handled_imports

        logger.debug(f"Initialized parser at {file}.")

    def parse(self) -> Program:
        self.process_imports()

        declarations: list[Declaration] = []
        while not self.match(Token.EOF):
            declarations.append(self.declaration())

        logger.debug("Parser finished. Dispatching parsers to imports.")

        out = Program(self.file, [], declarations)

        imports: list[Program] = []
        for im in self.imports:
            base = self.srcdir
            for segment in im.path:
                base /= segment.raw
            if not base.exists():
                raise ParseError(im.spans, f"Invalid path to import: '{base}'")
            scanner = Scanner(base)
            lexemes = scanner.scan_source()
            parser = Parser(self.srcdir, base, lexemes, self.rules, self.handled_imports | {str(base): out})
            imports.append(parser.parse())

        out.imports = imports
        return out

    def body(self) -> Block:
        self.begin_node()
        self.consume(Token.LEFT_CURLY, "Expected '{' to start a block.")

        statements: list[Statement] = []
        while not self.match(Token.RIGHT_CURLY):
            statements.append(self.statement())

        span = self.end_node()
        return Block(span, statements)

    def declaration(self) -> Declaration:
        decltype = self.find_decltype()
        match decltype:
            case self.DeclType.VAR:
                return self.var_decl()
            case self.DeclType.FN:
                return self.function_decl()
            case self.DeclType.CLASS:
                return self.class_decl()

    def statement(self) -> Statement:
        for rule in self.rules:
            if self.match_kw(rule.name):
                return rule.generator(self)
        if self.check(Token.LEFT_ANGLE, 2) or self.check(Token.EQUAL, 3):
            return self.var_decl()
        elif self.check(Token.EQUAL, 1) or self.check(Token.DOT, 1) or self.check(Token.LEFT_PAREN, 1) or self.check(Token.LEFT_BRACKET, 1):
            return self.var_set()
        else:
            return self.expression()

    def expression(self) -> Expression:
        return self.ternary()

    def binary(self, ops: list[Token], lower: Callable[[], Expression]):
        self.begin_node(True)
        expr = lower()
        while self.match(ops):
            op = self.previous
            right = lower()
            expr = Binary(Span(self._stack[-1], self.position), expr, op, right)
        self.end_node()
        return expr

    def ternary(self) -> Expression:
        self.begin_node(True)
        switch_or_expr = self.disjunction()

        if self.match(Token.QUESTION):
            truthy = self.disjunction()
            self.consume(Token.COLON, "Expected ':' to seperate expressions in ternary.")
            falsey = self.expression()
            span = self.end_node()
            return Ternary(span, switch_or_expr, truthy, falsey)

        self.end_node()
        return switch_or_expr

    def disjunction(self) -> Expression:
        return self.binary([Token.PIPE_PIPE], self.conjunction)

    def conjunction(self) -> Expression:
        return self.binary([Token.AND_AND], self.equality)

    def equality(self) -> Expression:
        return self.binary([Token.EQUAL_EQUAL, Token.BANG_EQUAL], self.comparison)

    def comparison(self) -> Expression:
        return self.binary(
            [
                Token.LEFT_ANGLE,
                Token.RIGHT_ANGLE,
                # TODO: Less and greater equal
            ],
            self.bitwise_or
        )

    def bitwise_or(self) -> Expression:
        return self.binary([Token.PIPE], self.bitwise_xor)

    def bitwise_xor(self) -> Expression:
        return self.binary([Token.CARET], self.bitwise_and)

    def bitwise_and(self) -> Expression:
        return self.binary([Token.AND], self.shift_expr)

    def shift_expr(self) -> Expression:
        return self.binary([Token.LEFT_ANGLE_ANGLE, Token.RIGHT_ANGLE_ANGLE], self.term)

    def term(self) -> Expression:
        return self.binary([Token.PLUS, Token.MINUS], self.factor)

    def factor(self) -> Expression:
        return self.binary([Token.STAR, Token.SLASH, Token.MODULO, Token.AT], self.unary)

    def unary(self) -> Expression:
        if self.match([Token.BANG, Token.MINUS]):
            self.begin_node()
            op = self.previous
            right = self.unary()
            span = self.end_node()
            return Unary(span, op, right)
        return self.power()

    def power(self) -> Expression:
        return self.binary([Token.STAR_STAR], self.primary)

    def primary(self, prefix: Primary | None = None) -> Primary:
        if prefix is not None:
            self.begin_node(True)
            if self.match(Token.DOT):
                name = self.consume(Token.IDENTIFIER, "Expected identifier after '.'.")
                return self.primary(Member(self.end_node(), prefix, name))
            elif self.check(Token.LEFT_PAREN) or self.check(Token.LEFT_ANGLE):
                generics = self.generics()
                arguments = self.arguments()
                return self.primary(Call(self.end_node(), prefix, generics, arguments))
            elif self.match(Token.LEFT_BRACKET):
                expr = self.expression()
                return self.primary(Slice(self.end_node(), prefix, expr))
            return prefix
        return self.primary(self.atom())

    def atom(self) -> Atom:
        self.begin_node(True)
        if self.match(Token.IDENTIFIER):
            return Identifier(self.end_node(), self.previous)
        if self.match([Token.INT, Token.FLOAT, Token.STRING, Token.CHARACTER, Token.BOOL]):
            assert self.previous.literal is not None
            return Literal(self.end_node(), self.previous, self.previous.literal)
        if self.check(Token.LEFT_CURLY):
            return self.body()
        if self.check(Token.LEFT_PAREN):
            return self.paren_expr()
        if self.check(Token.LEFT_BRACKET):
            return self.array()
        raise ParseError(self.peek(), "Expected identifier, int, float, string, character, boolean, '(', '{', '['")

    def paren_expr(self) -> Atom:
        self.begin_node(True)
        if self.match(Token.RIGHT_PAREN):
            return Tuple(self.end_node(), [])
        expr = self.expression()
        if self.match(Token.RIGHT_PAREN):
            return Grouping(self.end_node(), expr)
        values = [expr]
        while True:
            self.consume(Token.COMMA, "Expected ',' or ')'.")
            values.append(self.expression())
            if self.match(Token.RIGHT_PAREN):
                break
        span = self.end_node()
        return Tuple(span, values)

    def array(self) -> Array:
        self.begin_node(True)
        if self.match(Token.RIGHT_BRACKET):
            return Array(self.end_node(), [])
        values: list[Expression] = []
        while True:
            values.append(self.expression())
            if self.match(Token.RIGHT_PAREN):
                break
            self.consume(Token.COMMA, "Expected ',' or ')'.")
        span = self.end_node()
        return Array(span, values)

    def var_set(self) -> VarSet:
        self.begin_node(True)
        into = self.primary()
        self.consume(Token.EQUAL, "Expected '=' after identifier.")
        value = self.expression()
        span = self.end_node()
        return VarSet(span, into, value)

    def var_decl(self) -> VarDecl:
        self.begin_node(True)
        annotations: list[Annotation] = []
        while not self.check(Token.LEFT_ANGLE, 2) or self.check(Token.EQUAL, 3):
            annotations.append(self.annotation())
        typ = self.typ()
        name = self.consume(Token.IDENTIFIER, f"Expected variable name after variable type '{typ}'.", "ERR00100")
        self.consume(Token.EQUAL, f"Expected '=' after variable name '{name}'.", "ERR00101")
        value = self.expression()
        self.consume(Token.SEMICOLON, "Expected ';' after expression in variable declaration.", "ERR00102")
        span = self.end_node()
        return VarDecl(span, typ, name, value)

    def function_decl(self) -> FunctionDecl:
        self.begin_node(True)

        annotations: list[Annotation] = []
        while not self.match(Token.FN_KW):
            annotations.append(self.annotation())

        name = self.consume(Token.IDENTIFIER, "Expected identifier after 'fn' keyword", "ERR00200")
        generics = self.generics_def()

        if self.check(Token.LEFT_PAREN):
            params = self.parameters()
        else:
            params = Parameters(Span(self.position, self.position), [], [])

        if self.match(Token.RETURNS):
            returns = self.typ()
        else:
            returns = None

        if self.check(Token.LEFT_CURLY):
            body = self.body()
        else:
            body = None

        span = self.end_node()
        return FunctionDecl(span, annotations, name, generics, params, returns, body)

    def class_decl(self) -> ClassDecl:
        self.begin_node(True)

        annotations: list[Annotation] = []
        while not self.match(Token.CLASS_KW):
            annotations.append(self.annotation())

        name = self.consume(Token.IDENTIFIER, "Expected identifier after 'class' keyword.")
        generics = self.generics_def()

        inherits: list[TypeNode] = []
        if self.match(Token.COLON):
            if self.match(Token.LEFT_PAREN):
                inherits.append(self.typ())
                while self.match(Token.COMMA):
                    inherits.append(self.typ())
                self.consume(Token.RIGHT_PAREN, "Expected ')' to close inheritance list.")
            else:
                inherits = [self.typ()]

        if self.check(Token.LEFT_PAREN):
            params = self.parameters()
        else:
            params = Parameters(Span(self.position, self.position), [], [])

        self.consume(Token.LEFT_CURLY, "Expected '{' after class arguments.")
        members: list[Declaration] = []
        while not self.match(Token.RIGHT_CURLY):
            members.append(self.declaration())

        span = self.end_node()
        return ClassDecl(span, annotations, name, generics, inherits, params, members)

    def typ(self) -> TypeNode:
        self.begin_node(True)
        name = self.consume(Token.IDENTIFIER, "Expected type name.")
        generics: list[TypeNode] = []
        if self.match(Token.LEFT_ANGLE):
            generics = [self.typ()]
            while self.match(Token.COMMA):
                generics.append(self.typ())
            self.consume(Token.RIGHT_ANGLE, "Expected '>' or ',' after generics.")
        span = self.end_node()
        return TypeNode(span, name, generics)

    def parameters(self) -> Parameters:
        self.begin_node(True)

        positionals: list[Param] = []
        defaults: list[DefaultParam] = []

        while not self.check(Token.EQUAL, 3):
            self.begin_node(True)
            typ = self.typ()
            name = self.consume(Token.IDENTIFIER, "Expected name of parameter after its type.")
            span = self.end_node()
            positionals.append(Param(span, typ, name))
            if not self.match(Token.COMMA):
                break

        while not self.match(Token.RIGHT_PAREN):
            self.consume(Token.COMMA, "Expected ',' or ')' after parameter.")
            self.begin_node(True)
            typ = self.typ()
            name = self.consume(Token.IDENTIFIER, "Expected name of parameter after its type.")
            self.consume(Token.EQUAL, "Function parameters may not have positional arguments after keyword arguments (ones with defaults) (or '=' is missing).")
            default = self.expression()
            span = self.end_node()
            defaults.append(DefaultParam(span, typ, name, default))

        span = self.end_node()
        return Parameters(span, positionals, defaults)

    def arguments(self) -> Arguments:
        self.begin_node(True)

        positionals: list[Expression] = []
        kwargs: dict[Lexeme, Expression] = {}

        while not self.check(Token.EQUAL, 2):
            positionals.append(self.expression())
            if not self.match(Token.COMMA):
                break

        while not self.match(Token.RIGHT_PAREN):
            self.consume(Token.COMMA, "Expected ',' or ')' after argument.")
            name = self.consume(Token.IDENTIFIER, "Expected name of parameter for keyword argument.")
            self.consume(Token.EQUAL, "Expected '=' after keyword argument name.")
            kwargs[name] = self.expression()

        span = self.end_node()
        return Arguments(span, positionals, kwargs)

    def generics(self) -> list[TypeNode]:
        if not self.match(Token.LEFT_ANGLE):
            return []

        out: list[TypeNode] = [self.typ()]
        while not self.match(Token.RIGHT_ANGLE):
            self.consume(Token.COMMA, "Expected ',' or '>' to end generics.")
            out.append(self.typ())

        return out

    def generics_def(self) -> list[Generic]:
        if not self.match(Token.LEFT_ANGLE):
            return []

        out: list[Generic] = []
        while True:
            self.begin_node(True)
            name = self.consume(Token.IDENTIFIER, "Expected generic name.")
            if self.match(Token.COLON):
                inherits = self.consume(Token.IDENTIFIER, "Expected inherited type after ':'.")
            else:
                inherits = None
            span = self.end_node()
            out.append(Generic(span, name, inherits))
            if self.match(Token.COMMA):
                continue
            break
        self.consume(Token.LEFT_ANGLE, "Expected '>' or ',' after generic.")

        return out

    def annotation(self) -> Annotation:
        self.begin_node(True)

        if self.match(Token.AT):
            name = self.consume(Token.IDENTIFIER, "Expected identifier after '@' in annotation.")
            if self.match(Token.LEFT_PAREN):
                args = self.arguments()
                self.consume(Token.RIGHT_PAREN, "Expected ')' after parameters.")
            else:
                args = Arguments(Span(self.position, self.position), [], {})
        else:
            name = self.consume(Token.IDENTIFIER, "Expected annotation identifier.")
            args = Arguments(Span(self.position, self.position), [], {})

        span = self.end_node()
        return Annotation(span, name, args)

    def find_decltype(self) -> DeclType:
        while True:
            if self.match(Token.AT):
                self.annotation()
            elif self.match(Token.IDENTIFIER):
                continue
            elif self.match(Token.LEFT_ANGLE) or self.match(Token.EQUAL):
                return self.DeclType.VAR
            elif self.match(Token.FN_KW):
                return self.DeclType.FN
            elif self.match(Token.CLASS_KW):
                return self.DeclType.CLASS
            raise ParseError(self.peek(), "Unexpected keyword in declaration.")

    def process_imports(self) -> None:
        while self.match_kw("import"):
            self.begin_node()
            path = [self.consume(Token.IDENTIFIER, "Expected identifier after 'import' keyword.", "ERR00000")]
            while self.match(Token.DOT):
                path.append(self.consume(Token.IDENTIFIER, "Expected identifier after '.' in 'import' statement.", "ERR00002"))
            self.consume(Token.SEMICOLON, "Expected '.' or ';' after identifier in 'import' keyword.", "ERR00001")
            span = self.end_node()
            self.imports.append(Import(span, path))
