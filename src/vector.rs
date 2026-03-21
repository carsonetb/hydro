use std::fmt::format;

use chumsky::span::Spanned;
use inkwell::{
    context::Context,
    values::{BasicValue, BasicValueEnum, PointerValue},
};

use crate::{
    callable::MemberFunction,
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug)]
pub struct Vector<'ctx> {
    metatype: TypeID,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Vector<'ctx> {
    pub fn new(ctx: &LanguageContext<'ctx>, typ: TypeID, name: &str) -> Self {
        assert!(typ.generics.len() == 1);
        let new_fn = ctx.module.get_function("Vector__new").unwrap();
        let ptr = ctx
            .build_call_returns(
                new_fn,
                &[
                    ctx.get(typ.generics[0].clone())
                        .storage_type
                        .size_of()
                        .unwrap()
                        .into(),
                    ctx.types.long.const_int(8, false).into(),
                ],
                name,
            )
            .into_pointer_value();
        Self { metatype: typ, ptr }
    }

    pub fn push(&self, ctx: &LanguageContext<'ctx>, val: &ValueEnum<'ctx>, val_name: &str) {
        let push_fn = ctx.module.get_function("Vector__push").unwrap();
        let push_int_fn = ctx.module.get_function("Vector__push__Int").unwrap();
        let push_bool_fn = ctx.module.get_function("Vector__push__Bool").unwrap();
        let (push_fn, val_to_push) = match val {
            ValueEnum::Int(int) => (push_int_fn, int.get_value()),
            ValueEnum::Bool(bool) => (push_bool_fn, bool.get_value()),
            _ => (
                push_fn,
                val.construct_ptr(ctx, val_name).as_basic_value_enum(),
            ),
        };
        ctx.builder
            .build_call(push_fn, &[self.ptr.into(), val_to_push.into()], "UNUSED");
    }

    pub fn contains(&self) -> TypeID {
        self.metatype.generics[0].clone()
    }

    fn get_fn_type(me: TypeID) -> TypeID {
        TypeID::new(
            "MemberFunction",
            vec![
                me.clone(),
                TypeID::new("Tuple", vec![TypeID::from_base("Int")]),
                me.generics[0].clone(),
            ],
        )
    }

    fn push_type(me: TypeID) -> TypeID {
        TypeID::new(
            "MemberFunction",
            vec![
                me.clone(),
                TypeID::new("Tuple", vec![me.generics[0].clone()]),
                TypeID::from_base("Unit"),
            ],
        )
    }
}

impl<'ctx> Value<'ctx> for Vector<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        match &name.inner[..] {
            "[]" | "get" => {
                let fn_name = match &self.contains().to_string()[..] {
                    "Int" => "Vector__get__Int",
                    "Bool" => "Vector__get__Bool",
                    _ => "Vector__get",
                };
                Ok(ValueEnum::MemberFunction(MemberFunction::wrap_function(
                    ctx,
                    Self::get_fn_type(self.metatype.clone()),
                    fn_name,
                    self.ptr.into(),
                    into,
                )))
            }
            "push" => {
                let fn_name = match &self.contains().to_string()[..] {
                    "Int" => "Vector__push__Int",
                    "Bool" => "Vector__push__Bool",
                    _ => "Vector__push",
                };
                Ok(ValueEnum::MemberFunction(MemberFunction::wrap_function(
                    ctx,
                    Self::push_type(self.metatype.clone()),
                    fn_name,
                    self.ptr.into(),
                    into,
                )))
            }
            _ => Err(CompileError::new(
                name.span,
                &format!("Type `{}` has no `{}` member.", self.metatype, name.inner),
            )),
        }
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        self.ptr.into()
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        self.ptr
    }
}

impl<'ctx> ValueStatic<'ctx> for Vector<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert!(generics.len() == 1);
        let typeid = TypeID::new("Vector", generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(&typeid.to_string());

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Vector,
            typeid.clone(),
            Some(obj_struct),
            ctx.types.ptr.into(),
            false,
        );
        builder.build(llvm_ctx, ctx, generics);

        ctx.get_with_gen_ext(Self::get_fn_type(typeid.clone()));
        ctx.get_with_gen_ext(Self::push_type(typeid.clone()));

        obj_struct.set_body(
            &[
                ctx.types.long.into(),
                ctx.types.long.into(),
                ctx.types.long.into(),
                ctx.types.ptr.into(),
            ],
            false,
        );
    }
}

impl<'ctx> Copyable<'ctx> for Vector<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: &str,
    ) -> Self {
        Self {
            metatype: val_type,
            ptr: val.into_pointer_value(),
        }
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        other
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        Self::from_val(ctx, ptr.into(), typ, into_name)
    }
}
