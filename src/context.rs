use std::collections::HashMap;

use inkwell::{
    AddressSpace, OptimizationLevel,
    builder::Builder,
    context::Context,
    module::Module,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
    types::{
        BasicMetadataTypeEnum, BasicTypeEnum, FloatType, FunctionType, IntType, PointerType,
        StructType, VoidType,
    },
    values::IntValue,
};

use crate::{
    callable::Function,
    int::Int,
    scope::Scope,
    types::{Metatype, TypeId},
    value::ValueStatic,
};

pub struct LanguageContext<'ctx> {
    pub metatypes: HashMap<TypeId, Option<Metatype<'ctx>>>,
    pub types: LLVMTypes<'ctx>,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub machine: TargetMachine,
    pub scope: Scope<'ctx>,
    generic_gens: HashMap<String, fn(&'ctx Context, &mut LanguageContext<'ctx>, Vec<TypeId>)>,
}

impl<'ctx> LanguageContext<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
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
        let builder = context.create_builder();

        Self {
            metatypes: HashMap::<TypeId, Option<Metatype<'ctx>>>::new(),
            types: LLVMTypes::new(context),
            builder,
            module,
            machine,
            scope: Scope::new(),
            generic_gens: HashMap::<
                String,
                fn(&'ctx Context, &mut LanguageContext<'ctx>, Vec<TypeId>),
            >::new(),
        }
    }

    pub fn init_metatypes(&mut self, context: &'ctx Context) {
        self.generic_gens
            .insert("Function".to_string(), Function::build_metatype);
        Int::build_metatype(context, self, Vec::<TypeId>::new());
        Metatype::build_metatype(context, self, Vec::<TypeId>::new());
    }

    pub fn reserve_metatype(&mut self, name: TypeId) {
        self.metatypes.insert(name, None);
    }

    pub fn validate_id(&self, id: TypeId) {
        self.metatypes
            .get(&id)
            .expect(format!("Could not validate that type {id} exists!").as_str());
    }

    pub fn get_with_gen(&mut self, llvm_ctx: &'ctx Context, id: TypeId) -> Metatype<'ctx> {
        let maybe = self.maybe_get(id.clone());
        if maybe.is_some() {
            maybe.unwrap()
        } else {
            self.generic_gens
                .get(&id.base)
                .expect(format!("Base type {} has no generic builder.", id.base).as_str())(
                llvm_ctx,
                self,
                id.generics.clone(),
            );
            self.get(id)
        }
    }

    pub fn get(&self, id: TypeId) -> Metatype<'ctx> {
        self.maybe_get(id.clone())
            .expect(format!("Cannot find type {id} or it is not fully initialized.").as_str())
    }

    pub fn maybe_get(&self, id: TypeId) -> Option<Metatype<'ctx>> {
        self.metatypes.get(&id).cloned().flatten()
    }

    pub fn int(&self, value: u64) -> IntValue<'ctx> {
        self.types.int.const_int(value, false)
    }

    pub fn ptr(&self) -> PointerType<'ctx> {
        self.types.ptr
    }

    pub fn function(&self, args: u32) -> FunctionType<'ctx> {
        self.ptr().fn_type(
            &vec![BasicMetadataTypeEnum::PointerType(self.ptr()); args as usize],
            false,
        )
    }

    pub fn get_struct_with_gen(&mut self, llvm_ctx: &'ctx Context, id: TypeId) -> StructType<'ctx> {
        self.get_with_gen(llvm_ctx, id).obj_struct
    }

    pub fn get_struct(&self, id: TypeId) -> StructType<'ctx> {
        self.get(id).obj_struct
    }
}

pub struct LLVMTypes<'ctx> {
    pub type_struct: StructType<'ctx>,
    pub bool: IntType<'ctx>,
    pub char: IntType<'ctx>,
    pub short: IntType<'ctx>,
    pub int: IntType<'ctx>,
    pub int_struct: StructType<'ctx>,
    pub long: IntType<'ctx>,
    pub big: IntType<'ctx>,
    pub float: FloatType<'ctx>,
    pub double: FloatType<'ctx>,
    pub ptr: PointerType<'ctx>,
    pub void: VoidType<'ctx>,
}

impl<'ctx> LLVMTypes<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let int_struct = context.opaque_struct_type("Int");

        let type_struct = context.opaque_struct_type("Type");

        let out = Self {
            type_struct,
            int_struct,
            bool: context.bool_type(),
            char: context.i8_type(),
            short: context.i16_type(),
            int: context.i32_type(),
            long: context.i64_type(),
            big: context.i128_type(),
            float: context.f32_type(),
            double: context.f64_type(),
            ptr: context.ptr_type(AddressSpace::from(0u16)),
            void: context.void_type(),
        };

        Int::init_body(&out, int_struct);

        out
    }

    pub fn int_enum(&self) -> BasicTypeEnum<'ctx> {
        BasicTypeEnum::IntType(self.int)
    }
}
