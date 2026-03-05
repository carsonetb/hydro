from abc import ABC, abstractmethod
from dataclasses import dataclass
from pathlib import Path

from hydro.parser.nodes import Declaration, Expression, Program, Span, Statement
from hydro.tokens import Lexeme
from hydro.loggers import create_logger


logger = create_logger("ICompiler")
errors = create_logger("Compiler", False)


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
