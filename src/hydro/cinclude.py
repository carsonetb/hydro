from importlib.resources import as_file, files
import subprocess 
import llvmlite.binding as llvm

from hydro.main import create_logger


logger = create_logger("CInclude")


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
    

if __name__ == "__main__":
    logger.info("Ran independently, running load_hashmap.")
    load_hashmap()