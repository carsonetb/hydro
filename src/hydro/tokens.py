from __future__ import annotations
from dataclasses import dataclass
from enum import Enum, auto


class Token(Enum):
    """
    Represents a type of Lexeme.
    """

    # Single character
    BANG = auto()
    AT = auto()
    MODULO = auto()
    CARET = auto()
    AND = auto()
    STAR = auto()
    LEFT_PAREN = auto()
    RIGHT_PAREN = auto()
    MINUS = auto()
    PLUS = auto()
    EQUAL = auto()
    LEFT_BRACKET = auto()
    RIGHT_BRACKET = auto()
    LEFT_CURLY = auto()
    RIGHT_CURLY = auto()
    PIPE = auto()
    COLON = auto()
    SEMICOLON = auto()
    LEFT_ANGLE = auto()
    RIGHT_ANGLE = auto()
    COMMA = auto()
    DOT = auto()
    SLASH = auto()
    QUESTION = auto()

    # Two-character
    BANG_EQUAL = auto()
    AT_EQUAL = auto()
    MODULO_EQUAL = auto()
    CARET_EQUAL = auto()
    AND_AND = auto()
    AND_EQUAL = auto()
    STAR_STAR = auto()
    STAR_EQUAL = auto()
    MINUS_EQUAL = auto()
    PLUS_EQUAL = auto()
    EQUAL_EQUAL = auto()
    PIPE_EQUAL = auto()
    PIPE_PIPE = auto()
    LEFT_ANGLE_ANGLE = auto()
    RIGHT_ANGLE_ANGLE = auto()
    SLASH_EQUAL = auto()

    # Three-character
    PIPE_PIPE_EQUAL = auto()
    AND_AND_EQUAL = auto()
    STAR_STAR_EQUAL = auto()
    LEFT_ANGLE_ANGLE_EQUAL = auto()
    RIGHT_ANGLE_ANGLE_EQUAL = auto()

    # Literals
    INT = auto()
    FLOAT = auto()
    CHARACTER = auto()
    STRING = auto()
    BOOL = auto()

    # Keywords
    CLASS_KW = auto()
    FN_KW = auto()

    # Other
    IDENTIFIER = auto()
    ERROR = auto()
    EOF = auto()

@dataclass 
class Position:
    """
    A unique position in a file.
    """

    file: str
    line: int 
    col: int 

    def __repr__(self) -> str:
        return f"{self.line}:{self.col}"

@dataclass
class Lexeme:
    """
    The parser takes a source and converts it to a list of Lexemes.
    """

    #: Position of the first character in the Lexeme.
    pos: Position

    #: Token type of the Lexeme.
    token: Token 

    #: Raw string of the entire Lexeme.
    raw: str 

    #: If the Lexeme is a literal, this is its value.
    literal: int | float | str | bool | None = None

    @staticmethod
    def make_id(raw: str) -> Lexeme:
        """
        Creates an external identifier at <external>:-1:-1

        :param raw: The raw text of the identifier.
        """

        return Lexeme(Position("<external>", -1, -1), Token.IDENTIFIER, raw)
    
    def __repr__(self) -> str:
        return self.raw
    
    def __str__(self) -> str:
        return self.raw