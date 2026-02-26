from __future__ import annotations
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from hydro.loggers import create_logger
from hydro.parser.nodes import ClassDecl, Declaration, Program, Type


logger = create_logger("Analyzer")


@dataclass
class TypeInfo:
    node: ClassDecl
    typ: Type


class ClassRepr:
    node: ClassDecl
    subclasses: list[ClassRepr]


class Analyzer:
    def __init__(self, program: Program) -> None:
        logger.debug(f"Starting in {program.path}")

        self.declarations: list[Declaration] = self.consolidate(program)
        self.classes: dict[str, ClassRepr] = {}
        self.errors = False
    
    def find_classes(self) -> None:
        for decl in self.declarations:
            if not isinstance(decl, ClassDecl):
                continue 

    def consolidate(self, program: Program, processed: list[Path] = []) -> list[Declaration]:
        logger.debug(f"Merging {program.path}.")

        out: list[Declaration] = []
        for sub in program.imports:
            if not sub.path in processed:
                processed.append(sub.path)
                out += self.consolidate(sub, processed)
        
        for decl in program.declarations:
            out.append(decl)
        
        return out
    

