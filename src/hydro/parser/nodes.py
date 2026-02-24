from __future__ import annotations
from abc import ABC
from dataclasses import dataclass
from typing import Sequence
from tokens import Lexeme, Position, Token


@dataclass
class Span:
    start: Position
    end: Position

    def __repr__(self) -> str:
        return f"{self.start}-{self.end}"


@dataclass
class Node(ABC):
    spans: Span


@dataclass 
class Type(Node):
    name: Lexeme 
    generics: list[Type]

    def __str__(self) -> str:
        return f"{self.name}<{",".join(str(generic) for generic in self.generics)}>"


@dataclass 
class Generic(Node):
    name: Lexeme 
    inherits: Lexeme | None


@dataclass
class Arguments(Node):
    pos: list[Expression]
    kwargs: dict[Lexeme, Expression]


@dataclass 
class Annotation(Node):
    name: Lexeme 
    args: Arguments


@dataclass 
class Param(Node):
    typ: Type 
    name: Lexeme 


@dataclass 
class DefaultParam(Node):
    typ: Type 
    name: Lexeme 
    default: Expression 


@dataclass 
class Parameters(Node):
    pos: list[Param]
    defaults: list[DefaultParam]


@dataclass
class Statement(Node):
    pass


@dataclass
class Expression(Statement):
    pass


@dataclass 
class Ternary(Expression):
    switch: Expression 
    truthy: Expression 
    falsey: Expression


@dataclass 
class Unary(Expression):
    op: Lexeme 
    right: Expression


@dataclass 
class Binary(Expression):
    left: Expression
    op: Lexeme 
    right: Expression


@dataclass 
class Primary(Expression):
    pass 


@dataclass 
class Member(Primary):
    on: Primary
    name: Lexeme 


@dataclass 
class Call(Primary):
    on: Primary 
    generics: list[Type]
    args: Arguments 


@dataclass 
class Slice(Primary):
    on: Primary 
    using: Expression 


@dataclass 
class Atom(Primary):
    pass


@dataclass 
class Identifier(Atom):
    text: Lexeme


@dataclass 
class Literal(Atom):
    token: Lexeme
    value: int | float | str | bool 


@dataclass
class Grouping(Atom):
    expr: Expression


@dataclass 
class Tuple(Atom):
    values: list[Expression]


@dataclass 
class Array(Atom):
    values: list[Expression]


@dataclass
class Block(Atom):
    stmts: list[Statement]


@dataclass
class CustomStatement(Statement):
    name: str
    expressions: dict[str, Expression | Lexeme]
    following: Sequence[Statement] = []
    internal: bool = True


@dataclass 
class VarSet(Statement):
    into: Primary
    value: Expression


@dataclass
class Declaration(Node):
    pass


@dataclass 
class Import(Declaration):
    path: list[Lexeme]


@dataclass 
class VarDecl(Declaration, Statement):
    typ: Type 
    name: Lexeme 
    value: Expression


@dataclass 
class Function(Declaration):
    annotations: list[Annotation]
    name: Lexeme 
    generics: list[Generic]
    params: Parameters
    returns: Type | None 
    block: Block | None


@dataclass
class ClassDecl(Declaration):
    annotations: list[Annotation]
    name: Lexeme 
    generics: list[Generic]
    inherits: list[Type]
    params: Parameters 
    members: list[Declaration]


@dataclass
class Program:
    imports: list[Program]
    declarations: list[Declaration]
