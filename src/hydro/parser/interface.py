from abc import ABC, abstractmethod
from pathlib import Path

from hydro.loggers import create_logger
from hydro.parser.nodes import Expression, Program, Block, Span
from hydro.scanner import Lexeme, Position, Token


logger = create_logger("Parser", False)


class ParseError(RuntimeError):
    def __init__(self, lexeme: Lexeme | Span, msg: str, code: str = "-1") -> None:
        super().__init__()
        if isinstance(lexeme, Lexeme):
            logger.error(f"[{lexeme.pos}] [{lexeme}] {f"[{code}]" if code != "-1" else ""} {msg}")
        self.lexeme = lexeme
        self.msg = msg
        self.code = code


class ParserBase(ABC):
    def __init__(self, srcdir: Path, file: Path, tokens: list[Lexeme]) -> None:
        self.srcdir = srcdir
        self.file = file
        self.tokens = tokens
        self.current = 0
        self._stack: list[Position] = []

    @property
    def previous(self) -> Lexeme:
        return self.tokens[self.current - 1]

    @property
    def position(self) -> Position:
        return self.previous.pos

    # -- Helpers --

    def begin_node(self, next_tok=True) -> None:
        self._stack.append(self.position if not next_tok else self.peek().pos)

    def end_node(self) -> Span:
        start = self._stack.pop()
        return Span(start, self.position)

    def outside(self, ind: int) -> bool:
        return ind >= len(self.tokens)

    def peek(self, amount: int = 1) -> Lexeme:
        ind = self.current + amount - 1
        if self.outside(ind):
            return self.tokens[-1]
        return self.tokens[ind]

    def advance(self) -> Lexeme:
        ret = self.tokens[self.current]
        self.current += 1
        return ret

    def match_kw(self, kw: str, amount: int = 1) -> bool:
        peek = self.peek(amount)
        if peek.token != Token.IDENTIFIER:
            return False
        if peek.raw != kw:
            return False
        self.advance()
        return True

    def match(self, what: Token | list[Token], amount: int = 1) -> bool:
        if isinstance(what, Token):
            if self.peek(amount).token == what:
                self.advance()
                return True
            return False
        else:
            if self.peek(amount).token in what:
                self.advance()
                return True
            return False

    def consume_kw(self, kw: str, err: str, code: str = "-1") -> Lexeme:
        if not self.match_kw(kw):
            raise ParseError(self.peek(), err, code)
        return self.previous

    def consume(self, token: Token, err: str, code: str = "-1") -> Lexeme:
        if not self.match(token):
            raise ParseError(self.peek(), err, code)
        return self.previous

    def check(self, token: Token, ahead: int = 1) -> bool:
        return self.peek(ahead).token == token


class FullParser(ParserBase):
    def __init__(self, srcdir: Path, file: Path, tokens: list[Lexeme]) -> None:
        super().__init__(srcdir, file, tokens)

    @abstractmethod
    def parse(self) -> Program:
        pass

    @abstractmethod
    def expression(self) -> Expression:
        pass

    @abstractmethod
    def body(self) -> Block:
        pass
