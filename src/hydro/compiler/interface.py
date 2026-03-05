from abc import ABC, abstractmethod
from dataclasses import dataclass
from pathlib import Path

from hydro.parser.nodes import Declaration, Expression, Program, Span, Statement
from hydro.tokens import Lexeme
from src.hydro.lang_types import BaseMetatype, ObjectType
from src.hydro.loggers import create_logger


logger = create_logger("Compiler")
errors = create_logger("Compiler", False)


@dataclass
class Header:
    node: Declaration
    generics: list[BaseMetatype]
    params: list[BaseMetatype]
    inside: BaseMetatype | None


Scope = dict[Lexeme, ObjectType | Header]


class CompileError(RuntimeError):
    def __init__(self, lexeme: Lexeme | Span, msg: str, code: str = "-1") -> None:
        super().__init__()
        if isinstance(lexeme, Lexeme):
            errors.error(f"[{lexeme.pos}] [{lexeme}] {f"[{code}]" if code != "-1" else ""} {msg}")
        self.lexeme = lexeme
        self.msg = msg
        self.code = code


class CompilerBase(ABC):
    def __init__(self, program: Program, build_dir: Path) -> None:
        logger.debug(f"Starting compiler for {program.path}. Building into {build_dir}")

        self.program = program
        self.build_dir = build_dir

    @property
    def module_name(self) -> str:
        return self.program.path.stem

    @abstractmethod
    def gen_program(self) -> None:
        pass

    @abstractmethod
    def declaration(self, decl: Declaration, inside: BaseMetatype | None) -> None:
        pass

    @abstractmethod
    def statement(self, stmt: Statement) -> ObjectType:
        pass

    @abstractmethod
    def expression(self, expr: Expression, into_name: str = "unnamed_expression") -> ObjectType:
        pass

    @abstractmethod
    def block(self, stmts: list[Statement]) -> ObjectType | None:
        pass
