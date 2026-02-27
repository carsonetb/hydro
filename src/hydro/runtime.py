from llvmlite import ir
from helpers import CHAR, LONG, POINTER, INT, VOID
from importlib.resources import as_file, files
import subprocess 
import llvmlite.binding as llvm

from hydro.lang_types import BOOL, current_module
from hydro.main import create_logger


logger = create_logger("Runtime")


def load_hashmap():
    logger.debug("Compiling hashmap implementation to a shared object.")
    resources_dir = files("hydro.resources")
    with as_file(resources_dir) as resources:
        hashmap = resources / "hashmap"
        subprocess.run(
            ["clang", "-fPIC", "-c", "hashmap.c", "-o", "hashmap.o"],
            cwd=hashmap
        )
        subprocess.run(
            ["clang", "-shared", "-o", "hashmap.so", "hashmap.o"],
            cwd=hashmap
        )
        object_file = hashmap / "hashmap.so"
        assert object_file.exists()
        logger.debug(f"Linking shared object: {object_file}")
        llvm.load_library_permanently(str(object_file))
        logger.debug("Hashmap library loaded.")


class Runtime:
    RC_HEADER_SIZE = 4  # bytes

    def __init__(self, module: ir.Module) -> None:
        self.module = module

        # void* malloc(size_t size)
        self.malloc_type = ir.FunctionType(POINTER, [INT])
        self.malloc = ir.Function(self.module, self.malloc_type, name="malloc")

        # void* realloc(void* pointer, size_t size)
        self.realloc_type = ir.FunctionType(POINTER, [POINTER, INT])
        self.realloc = ir.Function(self.module, self.realloc_type, name="realloc")

        # void free(void* pointer)
        self.free_type = ir.FunctionType(VOID, [POINTER])
        self.free = ir.Function(self.module, self.free_type, name="free")

        # int32 printf(char* str, ...)
        self.printf_type = ir.FunctionType(INT, [POINTER], True)
        self.printf_func = ir.Function(self.module, self.printf_type, name="printf")

        # void sprintf(char* str, char* format, ...)
        self.sprintf_type = ir.FunctionType(VOID, [POINTER, POINTER], True)
        self.sprintf_func = ir.Function(self.module, self.sprintf_type, "sprintf")

        # %struct.hashmap = type { ptr, ptr, ptr, i64, i64, i64, i64, ptr, ptr, ptr, ptr, i64, i64, i64, i64, i64, i64, i8, i8, i8, ptr, ptr, ptr }
        self.hashmap_struct = current_module.context.get_identified_type("struct.hashmap")
        self.hashmap_struct.set_body([
            POINTER, POINTER, POINTER,
            LONG, LONG, LONG, LONG, 
            POINTER, POINTER, POINTER, POINTER,
            LONG, LONG, LONG, LONG, LONG, LONG, 
            CHAR, CHAR, CHAR, 
            POINTER, POINTER, POINTER,
        ])

        self.hashmap_new_typ = ir.FunctionType(POINTER, [LONG, LONG, LONG, LONG, LONG, LONG, LONG])
        self.hashmap_new = ir.Function(current_module, self.hashmap_new_typ, "hashmap_new")

        self.hashmap_free_typ = ir.FunctionType(VOID, [POINTER])
        self.hashmap_free = ir.Function(current_module, self.hashmap_free_typ, "hashmap_free")

        self.hashmap_count_typ = ir.FunctionType(LONG, [POINTER])
        self.hashmap_free = ir.Function(current_module, self.hashmap_count_typ, "hashmap_count")

        self.hashmap_set_typ = ir.FunctionType(POINTER, [POINTER, POINTER])
        self.hashmap_set = ir.Function(current_module, self.hashmap_set_typ, "hashmap_set")

        self.hashmap_get_typ = ir.FunctionType(POINTER, [POINTER, POINTER])
        self.hashmap_get = ir.Function(current_module, self.hashmap_get_typ, "hashmap_get")

        self.hashmap_delete_typ = ir.FunctionType(POINTER, [POINTER, POINTER])
        self.hashmap_delete = ir.Function(current_module, self.hashmap_delete_typ, "hashmap_delete")

        self.hashmap_clear_typ = ir.FunctionType(VOID, [POINTER, BOOL])
        self.hashmap_clear = ir.Function(current_module, self.hashmap_clear_typ, "hashmap_clear")

        self.hashmap_iter_typ = ir.FunctionType(BOOL, [POINTER, POINTER, POINTER])
        self.hashmap_iter = ir.Function(current_module, self.hashmap_iter_typ, "hashmap_iter")

        self.hashmap_scan_typ = ir.FunctionType(BOOL, [POINTER, POINTER, POINTER])
        self.hashmap_scan = ir.Function(current_module, self.hashmap_scan_typ, "hashmap_scan")

        # byte* rc_alloc(int32 size)
        self.rc_alloc_type = ir.FunctionType(POINTER, [INT])
        self.rc_alloc_func = ir.Function(module, self.rc_alloc_type, "rc_alloc")
        block = self.rc_alloc_func.append_basic_block("entry")
        builder = ir.IRBuilder(block)

        size = self.rc_alloc_func.args[0]
        total_size = builder.add(size, INT(self.RC_HEADER_SIZE), "total_size")
        mem = builder.call(self.malloc, [total_size], "mem")
        count_ptr = builder.bitcast(mem, INT.as_pointer(), "count_ptr")
        builder.store(INT(1), count_ptr)
        data = builder.gep(mem, [INT(4)], name="data")
        builder.ret(data)

        # void rc_retain(byte* ptr)
        self.rc_retain_type = ir.FunctionType(VOID, [POINTER])
        self.rc_retain_func = ir.Function(module, self.rc_retain_type, "rc_retain")
        block = self.rc_retain_func.append_basic_block("entry")
        builder = ir.IRBuilder(block)

        ptr = self.rc_retain_func.args[0]
        header = builder.gep(ptr, [INT(-self.RC_HEADER_SIZE)], inbounds=False, name="header")
        count_ptr = builder.bitcast(header, INT.as_pointer(), "count_ptr")
        old_count = builder.load(count_ptr, "old_count")
        new_count = builder.add(old_count, INT(1), "new_count")
        builder.store(new_count, count_ptr)
        builder.ret_void()

        # void rc_release(byte* pointer)
        self.destructor_fn_type = ir.FunctionType(VOID, [POINTER])
        self.destructor_ptr_type = self.destructor_fn_type.as_pointer()

        self.rc_release_type = ir.FunctionType(VOID, [POINTER, self.destructor_ptr_type])
        self.rc_release_func = ir.Function(module, self.rc_release_type, name="rc_release")

        entry_block = self.rc_release_func.append_basic_block("entry")
        free_block = self.rc_release_func.append_basic_block("free_block")
        free_call_dtor_block = self.rc_release_func.append_basic_block("free_call_dtor")
        free_do_free_block = self.rc_release_func.append_basic_block("free_do_free")
        end_block = self.rc_release_func.append_basic_block("end")

        builder = ir.IRBuilder(entry_block)

        data_ptr = self.rc_release_func.args[0]
        destructor = self.rc_release_func.args[1]

        header_ptr = builder.gep(data_ptr, [INT(-self.RC_HEADER_SIZE)], inbounds=False, name="header_ptr")
        count_ptr = builder.bitcast(header_ptr, INT.as_pointer(), "count_ptr")
        old_count = builder.load(count_ptr, name="old_count")
        new_count = builder.sub(old_count, INT(1), name="new_count")
        builder.store(new_count, count_ptr)
        is_zero = builder.icmp_unsigned("==", new_count, INT(0), name="is_zero")
        builder.cbranch(is_zero, free_block, end_block)

        # Free block just chooses between calling dtor or just freeing.
        builder.position_at_start(free_block)

        null_dtor = self.destructor_ptr_type(None)  # basically just a nullptr
        has_dtor = builder.icmp_unsigned("!=", destructor, null_dtor, "has_dtor")
        builder.cbranch(has_dtor, free_call_dtor_block, free_do_free_block)

        builder.position_at_start(free_call_dtor_block)

        builder.call(destructor, [data_ptr])
        builder.branch(free_do_free_block)

        builder.position_at_start(free_do_free_block)

        builder.call(self.free, [header_ptr])
        builder.branch(end_block)

        builder.position_at_start(end_block)
        builder.ret_void()

        logger.debug("Initialized runtime.")
