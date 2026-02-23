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


def else_generator(parser: ParserBase) -> CustomStatement:
    parser.begin_node()
    body = parser.body()
    span = parser.end_node()
    return CustomStatement(
        span,
        "else",
        True,
        [body],
    )


def if_generator(parser: ParserBase, is_elif: bool = False) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.LEFT_PAREN, "Expected '('.")
    condition = parser.expression()
    parser.consume(Token.RIGHT_PAREN, "Expected ')' after condition.")
    body = parser.body()
    following: list[CustomStatement] = []
    if parser.match_kw("elif"):
        following.append(if_generator(parser))
    elif parser.match_kw("else"):
        following.append(else_generator(parser))
    span = parser.end_node()
    return CustomStatement(
        span, 
        "if" if not is_elif else "elif", 
        True, 
        [condition, body],
        following,
    )


def while_generator(parser: ParserBase) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.LEFT_PAREN, "Expected '('.")
    condition = parser.expression()
    parser.consume(Token.RIGHT_PAREN, "Expected ')' after condition.")
    body = parser.body()
    span = parser.end_node()
    return CustomStatement(
        span,
        "while",
        True,
        [condition, body]
    )


def for_generator(parser: ParserBase) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.LEFT_PAREN, "Expected '('")
    typ = parser.consume(Token.IDENTIFIER, "Expected variable type.")
    name = parser.consume(Token.IDENTIFIER, "Expected variable name.")
    parser.consume_kw("in", "Expected 'in'.")
    loopee = parser.expression()
    return CustomStatement(
        
    )


def return_generator(parser: ParserBase) -> CustomStatement:
    parser.begin_node()
    expression = parser.expression()
    span = parser.end_node()
    return CustomStatement(span, "return", True, [expression])