use std::{error::Error, thread::Builder};

use inkwell::{OptimizationLevel, context::Context, module::Module, targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine}, types::FunctionType};

struct LLVMContext<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder,
    machine: TargetMachine,
}

impl<'ctx> LLVMContext<'ctx> {

}

fn main() -> Result<(), Box<dyn Error>> {
    Target::initialize_native(&InitializationConfig::default()).expect("Failed to initialize native target.");

    let context = Context::create();
    let module = context.create_module("module");
    let builder = context.create_builder();

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).expect("Unknown target.");
    let machine = target.create_target_machine(
        &triple,
        "generic",
        "",
        OptimizationLevel::None,
        RelocMode::Default,
        CodeModel::Default,
    ).unwrap();

    let int_type = context.i32_type();
    let main_type = int_type.fn_type(&[], false);
    let main_val = module.add_function("main", main_type, None);
    let entry = context.append_basic_block(main_val, "entry");
    builder.position_at_end(entry);

    builder.build_return(None).unwrap();
    main_val.verify(false);

    return Ok(());
}
