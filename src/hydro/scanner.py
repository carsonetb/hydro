from tokens import Token, Position, Lexeme
from loguru import logger


def alphanumeric(char: str) -> bool:
    return char.isnumeric() or char.isalpha() or char == "_"


class Scanner:
    """
    The scanner is responsible for taking a raw input string (a
    program), and turning it into a sequence of Lexemes.
    """

    KEYWORDS = {
        "class": Token.CLASS_KW,
        "fn": Token.FN_KW,
    }

    ESCAPES = {
        "\\": "\\",
        "n": "\n",
        "t": "\t",
        "'": "'",
        "\"": "\"",
    }

    def __init__(self, file: str, source: str) -> None:
        """
        :param file: The path to the module relative to project root.
        :param source: Raw contents of the file.
        """        

        self.file = file
        self.source = source
        self.start = 0
        self.current = 0
        self.line = 1
        self.col = 1 
        self.lexemes: list[Lexeme] = []

        logger.debug(f"Parser created in {self.file}.")
    
    @property 
    def position(self) -> Position:
        return Position(self.file, self.line, self.col)

    @property 
    def raw(self) -> str:
        return self.source[self.start:self.current]
    
    @property 
    def at_end(self) -> bool:
        return self.current >= len(self.source)
    
    def scan_source(self) -> list[Lexeme]:
        """
        Scans the whole source file and converts it to a list of 
        Lexemes.

        This function is the only one that should be called publicly.
        """

        while not self.at_end:
            self.start = self.current
            self.scan_lexeme()
        
        logger.debug(f"Finished parsing {self.file}.")

        return self.lexemes
    
    def scan_lexeme(self) -> None:
        if self.at_end:
            self.add_lexeme(Token.EOF)
            return

        char = self.advance()

        if char.isalpha():
            self.identifier()
            return 
        if char.isnumeric():
            self.number()
            return

        match char:
            case " ": self.col += 1
            case "\r": pass
            case "\t": self.col += 4
            case "\n":
                self.line += 1
                self.col = 1
            case "!": self.add_lexeme(Token.BANG_EQUAL if self.match("=") else Token.BANG)
            case "@": self.add_lexeme(Token.AT_EQUAL if self.match("=") else Token.AT)
            case "%": self.add_lexeme(Token.MODULO_EQUAL if self.match("=") else Token.MODULO)
            case "^": self.add_lexeme(Token.CARET_EQUAL if self.match("=") else Token.CARET)
            case "&": 
                if self.match("&"):
                    if self.match("="):
                        self.add_lexeme(Token.AND_AND_EQUAL)
                    else:
                        self.add_lexeme(Token.AND_AND)
                elif self.match("="):
                    self.add_lexeme(Token.AND_EQUAL)
                else:
                    self.add_lexeme(Token.AND)
            case "*":
                if self.match("*"):
                    if self.match("="):
                        self.add_lexeme(Token.STAR_STAR_EQUAL)
                    else:
                        self.add_lexeme(Token.STAR_STAR)
                elif self.match("="):
                    self.add_lexeme(Token.STAR_EQUAL)
                else:
                    self.add_lexeme(Token.STAR)
            case "(": self.add_lexeme(Token.LEFT_PAREN)
            case ")": self.add_lexeme(Token.RIGHT_PAREN)
            case "-": self.add_lexeme(Token.MINUS_EQUAL if self.match("=") else Token.MINUS)
            case "+": self.add_lexeme(Token.PLUS_EQUAL if self.match("=") else Token.PLUS)
            case "=": self.add_lexeme(Token.EQUAL_EQUAL if self.match("=") else Token.EQUAL)
            case "[": self.add_lexeme(Token.LEFT_BRACKET)
            case "]": self.add_lexeme(Token.RIGHT_BRACKET)
            case "{": self.add_lexeme(Token.LEFT_CURLY)
            case "}": self.add_lexeme(Token.RIGHT_CURLY)
            case "|": 
                if self.match("|"):
                    if self.match("="):
                        self.add_lexeme(Token.PIPE_PIPE_EQUAL)
                    else:
                        self.add_lexeme(Token.PIPE_PIPE)
                elif self.match("="):
                    self.add_lexeme(Token.PIPE_EQUAL)
                else:
                    self.add_lexeme(Token.PIPE)
            case ":": self.add_lexeme(Token.COLON)
            case ";": self.add_lexeme(Token.SEMICOLON)
            case "<": 
                if self.match("<"):
                    if self.match("="):
                        self.add_lexeme(Token.LEFT_ANGLE_ANGLE_EQUAL)
                    else:
                        self.add_lexeme(Token.LEFT_ANGLE_ANGLE)
                else:
                    self.add_lexeme(Token.LEFT_ANGLE)
            case ">": 
                if self.match(">"):
                    if self.match("="):
                        self.add_lexeme(Token.RIGHT_ANGLE_ANGLE_EQUAL)
                    else:
                        self.add_lexeme(Token.RIGHT_ANGLE_ANGLE)
                else:
                    self.add_lexeme(Token.RIGHT_ANGLE)
            case ",": self.add_lexeme(Token.COMMA)
            case ".": self.add_lexeme(Token.DOT)
            case "/":
                if self.match("="):
                    self.add_lexeme(Token.SLASH_EQUAL)
                elif self.match("/"):
                    self.comment()
                elif self.match("*"):
                    self.comment_multiline()
                else:
                    self.add_lexeme(Token.SLASH)
            case "\"":
                self.string()
            case "'":
                self.char()
    
    def add_lexeme(self, token: Token, literal: int | float | str | bool | None = None) -> Lexeme:
        out = Lexeme(self.position, token, self.raw, literal)
        self.lexemes.append(out)
        self.col += len(out.raw)
        return out
    
    def identifier(self) -> None:
        while alphanumeric(self.peek()):
            self.advance()

        if self.raw == "true":
            self.add_lexeme(Token.BOOL, True)
        if self.raw == "false":
            self.add_lexeme(Token.BOOL, False)
        
        if self.raw in self.KEYWORDS:
            self.add_lexeme(self.KEYWORDS[self.raw])
        else:
            self.add_lexeme(Token.IDENTIFIER)
    
    def number(self) -> None:
        while self.peek().isnumeric():
            self.advance()
        
        if not self.peek() == ".":
            self.add_lexeme(Token.INT, int(self.raw))
            return 
        
        self.advance()

        while self.peek().isnumeric():
            self.advance()
        
        self.add_lexeme(Token.FLOAT, float(self.raw))
    
    def char(self) -> None:
        if self.match("\\"):
            esc = self.advance()
            if esc in self.ESCAPES:
                char = self.ESCAPES[esc]
            else:
                self.error(f"Unsupported escape sequence '\\{esc}' (using '{esc}' as character).", False)
                char = esc 
        else:
            char = self.advance()
            if char == "'":
                self.error("No character after \"'\". Did you mean to use \"\\'\" instead?")
                return 
        
        if not self.match("'"):
            self.error("Expected \"'\" after character. Maybe you meant to use a string, with double quotes?")
            return 
        
        self.add_lexeme(Token.CHARACTER, char)
    
    def string(self) -> None:
        string = ""
        while not self.at_end and self.peek() != '"':
            if self.match("\\"):
                if self.peek() in self.ESCAPES:
                    string += self.ESCAPES[self.peek()]
                    self.advance()
                    continue
                else:
                    self.error(f"Unsupported escape sequence '\\{self.peek()}'", False)
            
            string += self.advance()
        
        if self.at_end:
            self.error("Unterminated string.")
            return 
        
        self.advance()
        self.add_lexeme(Token.STRING, string)
    
    def comment(self) -> None:
        while self.peek() != "\n" and not self.at_end:
            self.advance()
    
    def comment_multiline(self) -> None:
        while not (self.peek() == "*" and self.peek(1) == "/") and not self.at_end:
            self.advance()

    def advance(self) -> str:
        out = self.source[self.current]
        self.current += 1
        return out
    
    def peek(self, amount: int = 0) -> str:
        if self.current + amount >= len(self.source):
            return "\0"
        return self.source[self.current + amount]
    
    def match(self, char: str, amount: int = 0) -> bool:
        if self.peek(amount) == char:
            self.advance()
            return True 
        return False
    
    def error(self, msg: str, err_tok: bool = True) -> None:
        if err_tok:
            lexeme = self.add_lexeme(Token.ERROR)
            logger.error(f"[Scanner] [{lexeme.pos}] [{lexeme}] {msg}")
        else:
            logger.error(f"[Scanner] [{self.position}] {msg}")