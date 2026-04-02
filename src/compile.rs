use std::{
    fs::{File, create_dir, exists},
    io::Write,
    process::Command,
};

use inkwell::{execution_engine::JitFunction, targets::FileType};

use crate::{buildscript::LinkInfo, context::LanguageContext};

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

pub fn compile(ctx: &LanguageContext, link_info: LinkInfo) {
    if !exists("bin").unwrap() {
        create_dir("bin").unwrap();
    }
    let mut file = File::create("bin/out.o").unwrap();

    let buffer = ctx
        .machine
        .write_to_memory_buffer(&ctx.module, FileType::Object)
        .unwrap();
    file.write_all(buffer.as_slice()).unwrap();

    let mut compile = Command::new("clang");
    compile.args([
        "bin/out.o",
        "src/clib/runtime.c",
        "-no-pie",
        "-o",
        "bin/out",
        "-lc",
        "-lgc",
    ]);

    for linkdir in link_info.linkdirs {
        compile.arg(format!("-L{}", linkdir.to_str().unwrap()));
    }

    for link in link_info.links {
        compile.arg(format!("-l{}", link));
    }

    println!("Linking with {:?}", compile);

    compile.status().unwrap();

    Command::new("./bin/out").status().unwrap();
}
