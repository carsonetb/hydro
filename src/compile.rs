use std::{
    fs::{File, create_dir, exists},
    io::Write,
    process::Command,
};

use inkwell::{execution_engine::JitFunction, targets::FileType};

use crate::context::LanguageContext;

pub fn execute_jit(ctx: &LanguageContext) -> i32 {
    let execution_engine = ctx
        .module
        .create_jit_execution_engine(inkwell::OptimizationLevel::None)
        .unwrap();

    unsafe {
        let func: JitFunction<unsafe extern "C" fn() -> i32> =
            execution_engine.get_function("main").unwrap();
        func.call()
    }
}

pub fn compile(ctx: &LanguageContext) {
    if !exists("bin").unwrap() {
        create_dir("bin").unwrap();
    }
    let mut file = File::create("bin/out.o").unwrap();

    let buffer = ctx
        .machine
        .write_to_memory_buffer(&ctx.module, FileType::Object)
        .unwrap();
    file.write_all(buffer.as_slice()).unwrap();

    Command::new("clang")
        .args([
            "bin/out.o",
            "src/clib/runtime.c",
            "-no-pie",
            "-o",
            "bin/out",
            "-lc",
            "-lgc",
            "-lm",
        ])
        .status()
        .unwrap();
    Command::new("./bin/out").status().unwrap();
}
