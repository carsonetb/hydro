from __future__ import annotations
from dataclasses import dataclass
import typing
from parser.interface import FullParser, ParserBase
from parser.nodes import CustomStatement, Span
from scanner import Position, Token

Generator = typing.Callable[[FullParser], CustomStatement]


@dataclass
class Rule:
    name: str
    generator: Generator

    def __repr__(self) -> str:
        return self.name

    def __str__(self) -> str:
        return self.name


def else_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    body = parser.body()
    span = parser.end_node()
    return CustomStatement(
        span,
        "else",
        {
            "body": body,
        },
    )


def if_generator(parser: FullParser, is_elif: bool = False) -> CustomStatement:
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
        {
            "condition": condition,
            "body": body,
        },
        following,
    )


def while_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.LEFT_PAREN, "Expected '('.")
    condition = parser.expression()
    parser.consume(Token.RIGHT_PAREN, "Expected ')' after condition.")
    body = parser.body()
    span = parser.end_node()
    return CustomStatement(
        span,
        "while",
        {
            "condition": condition,
            "body": body,
        },
    )


def for_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.LEFT_PAREN, "Expected '('")
    typ = parser.consume(Token.IDENTIFIER, "Expected variable type.")
    name = parser.consume(Token.IDENTIFIER, "Expected variable name.")
    parser.consume_kw("in", "Expected 'in'.")
    loopee = parser.expression()
    parser.consume(Token.RIGHT_PAREN, "Expected ')'")
    body = parser.body()
    span = parser.end_node()
    return CustomStatement(
        span,
        "for",
        {
            "type": typ,
            "name": name,
            "loopee": loopee,
            "body": body,
        },
    )


def return_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    if not parser.match(Token.SEMICOLON):
        expression = parser.expression()
        parser.consume(Token.SEMICOLON, "Expected ';' after 'return' statement.")
    else:
        expression = None
    span = parser.end_node()
    return CustomStatement(
        span,
        "return",
        (
            {
                "expression": expression,
            }
            if expression is not None
            else {}
        ),
    )


def continue_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.SEMICOLON, "Expected ';' after 'continue' statement.")
    span = parser.end_node()
    return CustomStatement(span, "continue", {})


def break_generator(parser: FullParser) -> CustomStatement:
    parser.begin_node()
    parser.consume(Token.SEMICOLON, "Expected ';' after 'break' statement.")
    span = parser.end_node()
    return CustomStatement(span, "break", {})


BUILTIN_RULES = [
    Rule("if", if_generator),
    Rule("while", while_generator),
    Rule("for", for_generator),
    Rule("return", return_generator),
    Rule("continue", continue_generator),
    Rule("break", break_generator),
]
