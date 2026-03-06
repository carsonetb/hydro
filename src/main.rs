use std::error::Error;

use inkwell::{OptimizationLevel, context::Context, targets::{InitializationConfig, Target}, types::FunctionType};

fn main() -> Result<(), Box<dyn Error>> {
    Target::initialize_native(&InitializationConfig::default()).expect("Failed to initialize native target.");

    let context = Context::create();
    let module = context.create_module("module");
    let builder = context.create_builder();
    let execution_engine = module.create_execution_engine()?;

    let int_type = context.i32_type();
    let main_type = int_type.fn_type(&[], false);
    let main_val = module.add_function("main", main_type, None);
    let entry = context.append_basic_block(main_val, "entry");
    builder.position_at_end(entry);

    builder.build_return(None).unwrap();
    main_val.verify(false);

    return Ok(());
}
