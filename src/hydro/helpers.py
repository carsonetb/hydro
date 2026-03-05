from typing import Callable
from llvmlite import ir
import llvmlite.binding as llvm
from llvmlite.ir.types import IntType

BOOL: IntType = ir.IntType(1)
CHAR: IntType = ir.IntType(8)
INT: IntType = ir.IntType(32)
LONG: IntType = ir.IntType(64)
FLOAT = ir.FloatType()
DOUBLE = ir.DoubleType()
VOID = ir.VoidType()
POINTER = ir.PointerType()
NULL: ir.Constant = POINTER(None)


def rcallocate(typ: ir.Type) -> ir.Value:
    from hydro.runtime import runtime, builder_stack

    builder = builder_stack[-1]
    struct_mem = builder.call(runtime.rc_alloc_func, [INT(get_type_size(typ))], "struct_mem")
    return builder.bitcast(struct_mem, typ, "struct_ptr") # type: ignore

def cmp_function(
    module: ir.Module, class_name: str, cmpop: str, lhs: ir.Type, rhs: ir.Type
) -> ir.Function:
    cmp_type = ir.FunctionType(BOOL, [lhs, rhs])
    cmp = ir.Function(module, cmp_type, f"{class_name}__{cmpop}")
    block = cmp.append_basic_block("entry")
    builder = ir.IRBuilder(block)
    left, right = cmp.args
    builder.ret(builder.icmp_signed(cmpop, left, right))
    return cmp


def arith_function(
    module: ir.Module,
    class_name: str,
    name: str,
    generator: Callable[[ir.IRBuilder, ir.Value, ir.Value], ir.Value],
    lhs: ir.Type,
    rhs: ir.Type,
    ret: ir.Type,
) -> ir.Function:
    arith_type = ir.FunctionType(ret, [lhs, rhs])
    arith = ir.Function(module, arith_type, f"{class_name}__{name}")
    block = arith.append_basic_block("entry")
    builder = ir.IRBuilder(block)
    left, right = arith.args
    result = generator(builder, left, right)
    builder.ret(result)
    return arith


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

def functions_into_struct(builder: ir.IRBuilder, functions: list[tuple[str, ir.Function]], struct_ptr: ir.Value) -> None:
    builder.comment("Convert functions to pointers and store into struct:")

    pointers: list[tuple[str, ir.Value]] = []

    for name, function in functions:
        pointers.append((name, builder.bitcast(function, POINTER, f"{name}_ptr"))) # type: ignore

    field_pointers: list[tuple[str, ir.Value]] = []
    for i, (name, _) in enumerate(functions):
        field_pointers.append((name, builder.gep(struct_ptr, [INT(0), INT(i + 1)], name=f"{name}_field_ptr")))

    for (_, ptr), (_, field) in zip(pointers, field_pointers):
        builder.store(ptr, field)
