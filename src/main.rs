mod callable;
mod codegen;
mod compile;
mod context;
mod errors;
mod int;
mod parser;
mod scope;
mod types;
mod unit;
mod value;

use std::{error::Error, path::Path};

use inkwell::{
    context::Context,
    targets::{InitializationConfig, Target},
};

use crate::{codegen::do_codegen, compile::execute_jit, context::LanguageContext, parser::parse};

fn main() -> Result<(), Box<dyn Error>> {
    let program =
        parse(Path::new("examples/test.hydro").to_path_buf()).expect("Failed to parse program.");

    Target::initialize_native(&InitializationConfig::default())
        .expect("Failed to initialize native machine target!");

    let llvm_ctx = Context::create();
    let mut ctx = LanguageContext::new(&llvm_ctx);

    let main_type = ctx.types.int.fn_type(&[], false);
    let main_val = ctx.module.add_function("main", main_type, None);
    let entry = llvm_ctx.append_basic_block(main_val, "entry");
    ctx.builder.position_at_end(entry);

    ctx.init_metatypes(&llvm_ctx);

    do_codegen(&mut ctx, program);

    ctx.builder.build_return(Some(&ctx.int(0))).unwrap();

    main_val.verify(false);
    ctx.module.verify().unwrap();
    ctx.module.print_to_stderr();

    execute_jit(&ctx);

    Ok(())
}
