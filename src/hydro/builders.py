from llvmlite import ir
from runtime import Runtime

current_module: ir.Module = ir.Module()
runtime: Runtime = Runtime(current_module)
builder_stack: list[ir.IRBuilder] = []
