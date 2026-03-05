from pathlib import Path
import sys
from hydro.loggers import create_logger
from hydro.scanner import Scanner
from hydro.parser.parser import Parser
from hydro.parser.rules import BUILTIN_RULES
from hydro.compiler.compiler import Compiler


logger = create_logger("Main")


def main() -> None:
    logger.info("---------- BEGIN LOGS -----------")

    assert len(sys.argv) == 2

    file = Path(sys.argv[1])
    scanner = Scanner(file)
    lexemes = scanner.scan_source()
    parser = Parser(file.parent,file, lexemes, BUILTIN_RULES, {})
    program = parser.parse()
    compiler = Compiler(program, Path("build/"))
    compiler.gen_program()


if __name__ == "__main__":
    main()
