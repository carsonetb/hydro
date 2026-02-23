from __future__ import annotations
from dataclasses import dataclass
import typing
from parser.interface import ParserBase
from parser.nodes import CustomStatement, Span
from scanner import Position, Token


Generator = typing.Callable[[ParserBase], CustomStatement]


@dataclass 
class Rule:
    name: str 
    generator: Generator
    following: list[Rule]


def if_generator(parser: ParserBase) -> CustomStatement:
    out = CustomStatement(Span(Position("", 0, 0,), Position("", 0, 0)), "if", True, [])
    return out