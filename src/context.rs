use inkwell::{
    context::Context,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
};

pub struct LanguageContext<'ctx> {
    context: Context,
    module: Module<'ctx>,
    builder: Builder,
    machine: TargetMachine,
}

pub impl<'ctx> LanguageContext<'ctx> {
    pub fn new() -> Self {
        let context = Context::create();
        let module = context.create_module("module");

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("Unknown target.");
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::None,
                RelocMode::Default,
                CodeModel::Default,
            )
            .unwrap();

        Self {
            context,
            builder,
            module,
            machine,
        }
    }
}
