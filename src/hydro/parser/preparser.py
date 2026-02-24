from pathlib import Path

from loguru import logger

from hydro.scanner import Lexeme
from hydro.parser.interface import ParserBase
from hydro.parser.rules import Rule


class PreParser(ParserBase):
    def __init__(self, srcdir: Path, file: Path, tokens: list[Lexeme], rules: list[Rule]) -> None:
        super().__init__(srcdir, file, tokens)

        self.rules = rules

        logger.debug(f"Started pre-parser in '{self.file}' with rules: {self.rules}")

    def rule_pass(self) -> list[Rule]:
        logger.debug(f"Pre-parser performing rule pass in '{self.file}'")
        return self.rules
