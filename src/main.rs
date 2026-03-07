mod compile;
mod context;
mod errors;
mod int;
mod scope;
mod types;
mod value;

use std::error::Error;

use inkwell::{
    context::Context,
    targets::{InitializationConfig, Target},
};

use crate::{
    compile::execute_jit,
    context::LanguageContext,
    int::Int,
    value::{Literal, ValueField, ValuePtr},
};

fn main() -> Result<(), Box<dyn Error>> {
    Target::initialize_native(&InitializationConfig::default())
        .expect("Failed to initialize native machine target!");

    let llvm_ctx = Context::create();
    let mut ctx = LanguageContext::new(&llvm_ctx);

    let main_type = ctx.types.int.fn_type(&[], false);
    let main_val = ctx.module.add_function("main", main_type, None);
    let entry = llvm_ctx.append_basic_block(main_val, "entry");
    ctx.builder.position_at_end(entry);

    ctx.init_metatypes(&llvm_ctx);

    let int_value = Int::from_literal(&ctx, 1, "int".to_string());
    let int_field = ValueField::from_value(&ctx, ValuePtr::PInt(int_value), "int".to_string());

    let int_value = int_field
        .get_as_int(&ctx, "int_reloaded".to_string())
        .unwrap();
    let raw = int_value.raw(&ctx);

    ctx.builder.build_return(Some(&raw)).unwrap();

    main_val.verify(true);
    ctx.module.verify().unwrap();
    ctx.module.print_to_stderr();

    execute_jit(&ctx);

    Ok(())
}
