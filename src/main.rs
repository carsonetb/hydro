mod context;
mod typing;

use std::{error::Error, thread::Builder};

use inkwell::{
    OptimizationLevel,
    context::Context,
    module::Module,
    targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine},
    types::FunctionType,
};

use crate::context::LanguageContext;

fn main() -> Result<(), Box<dyn Error>> {
    let context = LanguageContext::new();

    let int_type = context.i32_type();
    let main_type = int_type.fn_type(&[], false);
    let main_val = module.add_function("main", main_type, None);
    let entry = context.append_basic_block(main_val, "entry");
    builder.position_at_end(entry);

    builder.build_return(None).unwrap();
    main_val.verify(false);

    return Ok(());
}
