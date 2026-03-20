use std::collections::HashMap;

use chumsky::span::{Spanned, WrappingSpan};
use inkwell::{
    AddressSpace, OptimizationLevel,
    builder::Builder,
    context::Context,
    module::Module,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
    types::{
        AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FloatType, FunctionType,
        IntType, PointerType, StructType, VoidType,
    },
    values::{BasicMetadataValueEnum, IntValue},
};

use crate::{
    bool::Bool,
    callable::{Function, MemberFunction},
    codegen::CompileError,
    int::Int,
    string::Str,
    tuple::Tuple,
    types::{Metatype, TypeID},
    unit::Unit,
    value::{Field, ValueEnum, ValueStatic, any_to_basic},
};

pub type ScopeItem<'ctx> = HashMap<String, Field<'ctx>>;
pub type Scope<'ctx> = Vec<ScopeItem<'ctx>>;

pub struct LanguageContext<'ctx> {
    pub context: &'ctx Context,
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
            context,
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
        Str::build_metatype(context, self, vec![]);
        let string_metatype = self.get(TypeID::from_base("String".to_string()));
        let print_llvm_type = self.types.void.fn_type(
            &[BasicMetadataTypeEnum::PointerType(
                string_metatype.storage_type.into_pointer_type(),
            )],
            false,
        );
        let print_llvm_fn = self.module.add_function("print", print_llvm_type, None);
        let entry = context.append_basic_block(print_llvm_fn, "entry");
        let old_block = self.builder.get_insert_block().unwrap();
        self.builder.position_at_end(entry);
        let ptr_ptr = self
            .builder
            .build_struct_gep(
                string_metatype.obj_struct.unwrap(),
                print_llvm_fn
                    .get_first_param()
                    .unwrap()
                    .into_pointer_value(),
                1,
                "strptr",
            )
            .unwrap();
        let ptr = self
            .builder
            .build_load(self.types.ptr, ptr_ptr, "strptr")
            .unwrap()
            .into_pointer_value();
        let printf_fn = self.module.add_function("printf", print_llvm_type, None);
        self.builder.build_call(
            printf_fn,
            &[BasicMetadataValueEnum::PointerValue(ptr)],
            "unused",
        );
        self.builder.build_return(None);
        self.builder.position_at_end(old_block);

        let print_type = TypeID::new(
            "Function".to_string(),
            vec![
                TypeID::new(
                    "Tuple".to_string(),
                    vec![TypeID::from_base("String".to_string())],
                ),
                TypeID::from_base("Unit".to_string()),
            ],
        );
        let print = Function::from_function(context, self, print_llvm_fn, print_type);
        self.add_field(
            "print".to_string(),
            Field::new(ValueEnum::Function(print), "print".to_string()),
        );

        self.generic_gens
            .insert("Function".to_string(), Function::build_metatype);
        self.generic_gens
            .insert("MemberFunction".to_string(), MemberFunction::build_metatype);
        self.generic_gens
            .insert("Tuple".to_string(), Tuple::build_metatype);
        Unit::build_metatype(context, self, vec![]);
        Bool::build_metatype(context, self, vec![]);
        Int::build_metatype(context, self, vec![]);
        Metatype::build_metatype(context, self, vec![]);
    }

    pub fn reserve_metatype(&mut self, name: TypeID) {
        self.metatypes.insert(name, None);
    }

    pub fn validate_id(&self, id: TypeID) {
        self.metatypes
            .get(&id)
            .expect(format!("Could not validate that type {id} exists!").as_str());
    }

    pub fn get_with_gen(
        &mut self,
        llvm_ctx: &'ctx Context,
        id: Spanned<TypeID>,
    ) -> Result<&Metatype<'ctx>, CompileError> {
        if self.metatypes.contains_key(&id.clone()) {
            self.get_err(id)
        } else {
            self.generic_gens.get(&id.base).ok_or_else(|| {
                CompileError::new(
                    id.span,
                    "Could not find type in the current scope.".to_string(),
                )
            })?(llvm_ctx, self, id.generics.clone());
            self.get_err(id)
        }
    }

    pub fn get_with_gen_ext(&mut self, id: TypeID) -> &Metatype<'ctx> {
        if self.metatypes.contains_key(&id.clone()) {
            self.get(id)
        } else {
            self.generic_gens.get(&id.base).unwrap()(self.context, self, id.generics.clone());
            self.get(id)
        }
    }

    pub fn get(&self, id: TypeID) -> &Metatype<'ctx> {
        self.maybe_get(id.clone())
            .expect(format!("Cannot find type {id} or it is not fully initialized.").as_str())
    }

    pub fn get_err(&self, id: Spanned<TypeID>) -> Result<&Metatype<'ctx>, CompileError> {
        let out = self.maybe_get(id.inner.clone());
        if out.is_some() {
            Ok(out.unwrap())
        } else {
            Err(CompileError::new(
                id.span,
                "Could not find type in the current scope.".to_string(),
            ))
        }
    }

    pub fn maybe_get(&self, id: TypeID) -> Option<&Metatype<'ctx>> {
        self.metatypes
            .get(&id)
            .expect(format!("Could not find type {id}").as_str())
            .as_ref()
    }

    pub fn string(&self) -> &Metatype<'ctx> {
        self.get(TypeID::from_base("String".to_string()))
    }

    pub fn int(&self, value: u64) -> IntValue<'ctx> {
        self.types.int.const_int(value, false)
    }

    pub fn bool(&self, value: bool) -> IntValue<'ctx> {
        self.types.bool.const_int(value as u64, false)
    }

    pub fn get_struct_with_gen(
        &mut self,
        llvm_ctx: &'ctx Context,
        id: Spanned<TypeID>,
    ) -> Result<StructType<'ctx>, CompileError> {
        Ok(self.get_with_gen(llvm_ctx, id)?.obj_struct.unwrap())
    }

    pub fn get_struct(&self, id: TypeID) -> StructType<'ctx> {
        self.get(id).obj_struct.unwrap()
    }

    pub fn get_storage(&self, id: TypeID) -> BasicTypeEnum<'ctx> {
        any_to_basic(self.get(id).storage_type).unwrap()
    }

    pub fn get_storage_with_gen(
        &mut self,
        llvm_ctx: &'ctx Context,
        id: Spanned<TypeID>,
    ) -> Result<AnyTypeEnum<'ctx>, CompileError> {
        Ok(self.get_with_gen(llvm_ctx, id)?.storage_type)
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

    pub fn get_field(&self, name: Spanned<String>) -> Result<&Field<'ctx>, CompileError> {
        self.get_field_nospan(name.inner.clone()).ok_or_else(|| {
            CompileError::new(
                name.span,
                format!("No field named {} in current scope.", name.inner),
            )
        })
    }

    pub fn get_field_nospan(&self, name: String) -> Option<&Field<'ctx>> {
        for scope in self.scope.iter().rev() {
            if scope.contains_key(&name.clone()) {
                return Some(scope.get(&name.clone()).unwrap());
            }
        }
        None
    }

    pub fn get_field_mut(
        &mut self,
        name: Spanned<String>,
    ) -> Result<&mut Field<'ctx>, CompileError> {
        for scope in self.scope.iter_mut().rev() {
            if scope.contains_key(&name.inner.clone()) {
                return Ok(scope.get_mut(&name.inner.clone()).unwrap());
            }
        }
        Err(CompileError::new(
            name.span,
            format!("No field named {} in current scope.", name.inner),
        ))
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
