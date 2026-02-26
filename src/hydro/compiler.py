from dataclasses import dataclass
import llvmlite.binding as llvm
from llvmlite.ir import Module

import hydro.builders as builders
from hydro.loggers import create_logger
from hydro.parser.nodes import Program
from hydro.runtime import Runtime


logger = create_logger("Compiler")
errors = create_logger("Compiler", False)


# TODO: A Scope class thing


class Compiler:
    def __init__(self, program: Program) -> None:
        self.program = program

        logger.debug("Starting compiler.")

        llvm.initialize_native_target()
        llvm.initialize_native_asmprinter()
        self.target = llvm.Target.from_default_triple().create_target_machine(reloc="pic")
        logger.debug("LLVM Targets initialized.")

        builders.current_module = Module(program.path.stem)
        builders.runtime = Runtime(builders.current_module)
        logger.debug("Runtime initialized.")

        