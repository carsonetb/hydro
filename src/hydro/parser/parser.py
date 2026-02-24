from enum import Enum, auto
from pathlib import Path

from hydro.parser.interface import ParseError
from hydro.parser.nodes import Annotation, ClassDecl, Declaration, DefaultParam, Function, Generic, Import, Param, Parameters, Program, Span, Type, VarDecl
from hydro.parser.rules import FullParser, Rule
from hydro.scanner import Lexeme
from hydro.tokens import Token


class Parser(FullParser):
    class DeclType(Enum):
        VAR = auto()
        FN = auto()
        CLASS = auto()

    def __init__(self, srcdir: Path, file: Path, tokens: list[Lexeme], rules: list[Rule]) -> None:
        super().__init__(srcdir, file, tokens)

        self.rules = rules
        self.imports: list[Import] = []
    
    def parse(self) -> Program:
        self.process_imports()

        declarations: list[Declaration] = []
        while not self.match(Token.EOF):
            declarations.append(self.declaration())
        
        return Program([], declarations)
    
    def declaration(self) -> Declaration:
        decltype = self.find_decltype()
        if decltype == self.DeclType.VAR:
            return self.var_decl()
        if decltype == self.DeclType.FN:
            return self.function_decl()
        if decltype == self.DeclType.CLASS:
            return self.class_decl()
    
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

    def function_decl(self) -> Function:
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
        return Function(span, annotations, name, generics, params, returns, body)

    def class_decl(self) -> ClassDecl:
        self.begin_node(True)
        
        annotations: list[Annotation] = []
        while not self.match(Token.CLASS_KW):
            annotations.append(self.annotation())
        
        name = self.consume(Token.IDENTIFIER, "Expected identifier after 'class' keyword.")
        generics = self.generics_def()

        inherits: list[Type] = []
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

    def typ(self) -> Type:
        self.begin_node(True)
        name = self.consume(Token.IDENTIFIER, "Expected type name.")
        generics: list[Type] = []
        if self.match(Token.LEFT_ANGLE):
            generics = [self.typ()]
            while self.match(Token.COMMA):
                generics.append(self.typ())
            self.consume(Token.RIGHT_ANGLE, "Expected '>' or ',' after generics.")
        span = self.end_node()
        return Type(span, name, generics)

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
            self.consume(Token.COMMA, "Expected ',' or ')' after argument.")
            self.begin_node(True)
            typ = self.typ()
            name = self.consume(Token.IDENTIFIER, "Expected name of parameter after its type.")
            self.consume(Token.EQUAL, "Function parameters may not have positional arguments after keyword arguments (ones with defaults) (or '=' is missing).")
            default = self.expression()
            span = self.end_node()
            defaults.append(DefaultParam(span, typ, name, default))
        
        span = self.end_node()
        return Parameters(span, positionals, defaults)

    def generics_def(self) -> list[Generic]:
        pass

    def annotation(self) -> Annotation:
        pass

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