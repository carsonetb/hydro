from dataclasses import dataclass
from pathlib import Path
import llvmlite.binding as llvm
from llvmlite.ir import Function, FunctionType, IRBuilder, Module

import hydro.builders as builders
from hydro.builders import current_module, runtime, builder_stack
from hydro.helpers import INT
from hydro.lang_types import BoolType, ObjectType
from hydro.loggers import create_logger
from hydro.parser.nodes import Program
from hydro.runtime import Runtime


logger = create_logger("Compiler")
errors = create_logger("Compiler", False)


Scope = dict[str, ObjectType]


class Compiler:
    def __init__(self, program: Program, build_dir: Path) -> None:
        logger.debug(f"Starting compiler for {program.path}. Building into {build_dir}")

        self.program = program
        self.build_dir = build_dir
        self.scopes: list[Scope] = [{}]
        self.headers: dict[str, type[ObjectType]] = {
            "Bool": BoolType,
        }

        llvm.initialize_native_target()
        llvm.initialize_native_asmprinter()
        self.target = llvm.Target.from_default_triple().create_target_machine(reloc="pic")
        logger.debug("LLVM Targets initialized.")

        builders.current_module = Module(program.path.stem)
        builders.runtime = Runtime(builders.current_module)
        logger.debug("Runtime initialized.")

        self.scope["Bool"] = BoolType.create_metatype()
        logger.debug("Builtin types initialized.")

        main_ty = FunctionType(INT, [])
        self.main = Function(current_module, main_ty, "main")
    
    @property 
    def scope(self) -> Scope:
        return self.scopes[-1]
    
    @property 
    def module_name(self) -> str:
        return self.program.path.stem
    
    def gen_program(self) -> None:
        logger.info(f"Compiling {self.program.path}")

        for imp in self.program.imports:
            # TODO: Imports
            pass

        
    
    def pop_scope(self) -> Scope:
        pass
        