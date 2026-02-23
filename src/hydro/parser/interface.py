from abc import ABC, abstractmethod
from loguru import logger
from nodes import Expression, Program
from scanner import Lexeme, Token


class ParseError(RuntimeError):
    def __init__(self, lexeme: Lexeme, msg: str) -> None:
        super().__init__()
        logger.error(f"[Parser] [{lexeme.pos}] [{lexeme}] {msg}")
        self.lexeme = lexeme 
        self.msg = msg


class ParserBase(ABC):
    def __init__(self, tokens: list[Lexeme]) -> None:
        self.tokens = tokens 
        self.current = 0
    
    @property 
    def previous(self) -> Lexeme:
        return self.tokens[self.current - 1]

    @abstractmethod
    def parse(self) -> Program:
        pass 

    @abstractmethod
    def expression(self) -> Expression:
        pass

    @abstractmethod 
    def scope(self) -> Expression:
        pass

    # -- Helpers --

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
    
    def match(self, token: Token, amount: int = 1) -> bool:
        if self.peek(amount).token == token:
            self.advance()
            return True 
        return False 
    
    def consume(self, token: Token, err: str) -> Lexeme:
        if not self.match(token):
            raise ParseError(self.peek(), err)
        return self.previous
    
    def check(self, token: Token, ahead: int = 1) -> bool:
        return self.peek(ahead).token == token

