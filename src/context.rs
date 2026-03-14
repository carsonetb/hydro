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
    codegen::CompileError,
    int::Int,
    tuple::Tuple,
    types::{Metatype, TypeID},
    value::{Field, ValueStatic},
};

pub type ScopeItem<'ctx> = HashMap<String, Field<'ctx>>;
pub type Scope<'ctx> = Vec<ScopeItem<'ctx>>;

pub struct LanguageContext<'ctx> {
    pub metatypes: HashMap<TypeID, Option<Metatype<'ctx>>>,
    pub types: LLVMTypes<'ctx>,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub machine: TargetMachine,
    pub scope: Scope<'ctx>,
    pub errors: Vec<CompileError>,
    generic_gens: HashMap<String, fn(&'ctx Context, &mut LanguageContext<'ctx>, Vec<TypeID>)>,
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
            metatypes: HashMap::<TypeID, Option<Metatype<'ctx>>>::new(),
            types: LLVMTypes::new(context),
            builder,
            module,
            machine,
            scope: Scope::new(),
            errors: Vec::<CompileError>::new(),
            generic_gens: HashMap::<
                String,
                fn(&'ctx Context, &mut LanguageContext<'ctx>, Vec<TypeID>),
            >::new(),
        }
    }

    pub fn error(&mut self, err: CompileError) {
        self.errors.push(err);
    }

    pub fn init_metatypes(&mut self, context: &'ctx Context) {
        self.generic_gens
            .insert("Function".to_string(), Function::build_metatype);
        self.generic_gens
            .insert("Tuple".to_string(), Tuple::build_metatype);
        Int::build_metatype(context, self, Vec::<TypeID>::new());
        Metatype::build_metatype(context, self, Vec::<TypeID>::new());
    }

    pub fn reserve_metatype(&mut self, name: TypeID) {
        self.metatypes.insert(name, None);
    }

    pub fn validate_id(&self, id: TypeID) {
        self.metatypes
            .get(&id)
            .expect(format!("Could not validate that type {id} exists!").as_str());
    }

    pub fn get_with_gen(&mut self, llvm_ctx: &'ctx Context, id: TypeID) -> Metatype<'ctx> {
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

    pub fn get(&self, id: TypeID) -> Metatype<'ctx> {
        self.maybe_get(id.clone())
            .expect(format!("Cannot find type {id} or it is not fully initialized.").as_str())
    }

    pub fn maybe_get(&self, id: TypeID) -> Option<Metatype<'ctx>> {
        self.metatypes.get(&id).cloned().flatten()
    }

    pub fn int(&self, value: u64) -> IntValue<'ctx> {
        self.types.int.const_int(value, false)
    }

    pub fn get_struct_with_gen(&mut self, llvm_ctx: &'ctx Context, id: TypeID) -> StructType<'ctx> {
        self.get_with_gen(llvm_ctx, id).obj_struct.unwrap()
    }

    pub fn get_struct(&self, id: TypeID) -> StructType<'ctx> {
        self.get(id).obj_struct.unwrap()
    }

    pub fn get_storage(&self, id: TypeID) -> BasicTypeEnum<'ctx> {
        self.get(id).storage_type
    }

    pub fn get_storage_with_gen(
        &mut self,
        llvm_ctx: &'ctx Context,
        id: TypeID,
    ) -> BasicTypeEnum<'ctx> {
        self.get_with_gen(llvm_ctx, id).storage_type
    }

    pub fn is_refcounted(&self, id: TypeID) -> bool {
        self.get(id).is_refcounted
    }

    pub fn add_field(&mut self, name: String, field: Field<'ctx>) {
        let current = self.current_scope_mut();
        current.insert(name, field);
    }

    pub fn push_scope(&mut self) {
        self.scope.push(ScopeItem::new());
    }

    pub fn pop_scope(&mut self) {
        let mut scope = self.scope.pop().unwrap();
        for (_, field) in scope.iter_mut() {
            if !field.is_return {
                field.release(self);
            }
        }
    }

    pub fn current_scope(&self) -> &ScopeItem<'ctx> {
        self.scope
            .last()
            .expect("Cannot get current scope because no scopes have been pushed to the stack.")
    }

    pub fn current_scope_mut(&mut self) -> &mut ScopeItem<'ctx> {
        self.scope
            .last_mut()
            .expect("Cannot get current scope because no scopes have been pushed to stack.")
    }

    pub fn get_field(&self, name: String) -> &Field<'ctx> {
        for scope in self.scope.iter().rev() {
            if scope.contains_key(&name.clone()) {
                return scope.get(&name.clone()).unwrap();
            }
        }
        panic!("No field named {name} in current scope.")
    }

    pub fn get_field_mut(&mut self, name: String) -> &mut Field<'ctx> {
        for scope in self.scope.iter_mut().rev() {
            if scope.contains_key(&name.clone()) {
                return scope.get_mut(&name.clone()).unwrap();
            }
        }
        panic!("No field named {name} in current scope.")
    }
}

pub struct LLVMTypes<'ctx> {
    pub type_struct: StructType<'ctx>,
    pub bool: IntType<'ctx>,
    pub char: IntType<'ctx>,
    pub short: IntType<'ctx>,
    pub int: IntType<'ctx>,
    pub long: IntType<'ctx>,
    pub big: IntType<'ctx>,
    pub float: FloatType<'ctx>,
    pub double: FloatType<'ctx>,
    pub ptr: PointerType<'ctx>,
    pub void: VoidType<'ctx>,
}

impl<'ctx> LLVMTypes<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let type_struct = context.opaque_struct_type("Type");

        let out = Self {
            type_struct,
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

        out
    }

    pub fn int_enum(&self) -> BasicTypeEnum<'ctx> {
        BasicTypeEnum::IntType(self.int)
    }
}
