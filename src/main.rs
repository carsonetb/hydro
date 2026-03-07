mod context;
mod int;
mod value;

use std::{error::Error};

use inkwell::{context::Context, targets::{InitializationConfig, Target}};

use crate::context::LanguageContext;

fn main() -> Result<(), Box<dyn Error>> {
    Target::initialize_native(&InitializationConfig::default()).expect("Failed to initialize native machine target!");

    let llvm_ctx = Context::create();
    let ctx = LanguageContext::new(&llvm_ctx);

    let main_type = ctx.types.int.fn_type(&[], false);
    let main_val = ctx.module.add_function("main", main_type, None);
    let entry = llvm_ctx.append_basic_block(main_val, "entry");
    ctx.builder.position_at_end(entry);
    main_val.verify(false);

    ctx.builder.build_return(None).unwrap();

    Ok(())
}
