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
class Templated:
    base: Token 
    templates: list[Templated]


@dataclass
class Statement(ABC):
    spans: Span 


@dataclass
class Expression(Statement):
    pass 


@dataclass
class Scope(Expression):
    stmts: list[Statement]


@dataclass
class CustomStatement(Statement):
    name: str
    internal: bool
    expressions: list[Expression | Lexeme]
    following: Sequence[Statement] = []


@dataclass 
class Declaration(ABC):
    pass


@dataclass 
class Program:
    imports: list
    declarations: list