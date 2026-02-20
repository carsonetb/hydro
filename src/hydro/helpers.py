from typing import Any, Callable
from llvmlite import ir
import llvmlite.binding as llvm

BOOL = ir.IntType(1)
CHAR = ir.IntType(8)
INT = ir.IntType(32)
LONG = ir.IntType(64)
FLOAT = ir.FloatType()
DOUBLE = ir.DoubleType()
VOID = ir.VoidType()
POINTER = ir.IntType(8).as_pointer()
NULL: ir.Constant = POINTER(None)


def cmp_function(module: ir.Module, class_name: str, cmpop: str, lhs: ir.Type, rhs: ir.Type) -> tuple[ir.FunctionType, ir.Function]:
    cmp_type = ir.FunctionType(BOOL, [lhs, rhs])
    cmp = ir.Function(module, cmp_type, f"{class_name}__{cmpop}")
    block = cmp.append_basic_block("entry")
    builder = ir.IRBuilder(block)
    left, right = cmp.args
    builder.ret(builder.icmp_signed(cmpop, left, right))
    return (cmp_type, cmp)


def arith_function(module: ir.Module, class_name: str, name: str, generator: Callable[[ir.IRBuilder, ir.Value, ir.Value], ir.Value], lhs: ir.Type, rhs: ir.Type, ret: ir.Type) -> tuple[ir.FunctionType, ir.Function]:
    arith_type = ir.FunctionType(ret, [lhs, rhs])
    arith = ir.Function(module, arith_type, f"{class_name}__{name}")
    block = arith.append_basic_block("entry")
    builder = ir.IRBuilder(block)
    left, right = arith.args 
    result = generator(builder, left, right)
    builder.ret(result)
    return (arith_type, arith)


def get_type_size(ir_type: ir.Type) -> int:
    """
    This is also definetely not related to ``array``, and should
    be moved elsewhere.
    """

    ir_str = str(ir_type)

    type_sizes = {
        "i1": 1,
        "i8": 1,
        "i16": 2,
        "i32": 4,
        "i64": 8,
        "float": 4,
        "double": 8,
    }

    if ir_str in type_sizes:
        return type_sizes[ir_str]

    if ir_str.endswith("*"):
        return 8

    if isinstance(ir_type, ir.LiteralStructType):
        return sum(get_type_size(elem) for elem in ir_type.elements)

    if isinstance(ir_type, ir.IdentifiedStructType):
        return sum(get_type_size(elem) for elem in ir_type.elements)  # type: ignore

    if isinstance(ir_type, ir.ArrayType):
        return ir_type.count * get_type_size(ir_type.element)

    raise ValueError(f"Unknown type size for '{ir_str}'")
