#![allow(unused)]

mod bool;
mod callable;
mod codegen;
mod compile;
mod context;
mod int;
mod parser;
mod string;
mod tuple;
mod types;
mod unit;
mod value;
mod vector;

use std::{error::Error, path::Path, process::exit};

use inkwell::{
    context::Context,
    targets::{InitializationConfig, Target},
};

use crate::{
    codegen::do_codegen,
    compile::{compile, execute_jit},
    context::LanguageContext,
    parser::parse,
};

fn main() -> Result<(), Box<dyn Error>> {
    let path = Path::new("examples/test.hy").to_path_buf();
    let program = match parse(path.clone()) {
        Some(program) => program,
        None => exit(0),
    };

    Target::initialize_native(&InitializationConfig::default())
        .expect("Failed to initialize native machine target!");

    let llvm_ctx = Context::create();
    let mut ctx = LanguageContext::new(&llvm_ctx);

    let main_type = ctx.types.int.fn_type(&[], false);
    let main_val = ctx.module.add_function("lang_main", main_type, None);
    let entry = llvm_ctx.append_basic_block(main_val, "entry");
    ctx.builder.position_at_end(entry);

    do_codegen(&llvm_ctx, &mut ctx, path, program).unwrap();

    ctx.builder.build_return(Some(&ctx.int(0))).unwrap();

    main_val.verify(true);
    ctx.module.print_to_stderr();
    ctx.module.verify().unwrap();

    compile(&ctx);

    Ok(())
}
