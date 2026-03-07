use std::marker::PhantomData;

use inkwell::{
    context::Context,
    values::{AnyValue, PointerValue},
};

use crate::{context::LanguageContext, int::Int, types::Metatype};

pub struct Field<'ctx, T: Copyable<'ctx>> {
    name: String,
    invalid: bool,
    field_ptr: PointerValue<'ctx>,
    _marker: PhantomData<T>,
}

impl<'ctx, T: Copyable<'ctx>> Field<'ctx, T> {
    pub fn new(field_ptr: PointerValue<'ctx>, name: String) -> Self {
        Self {
            name,
            invalid: false,
            field_ptr,
            _marker: PhantomData,
        }
    }

    pub fn from_value(ctx: &LanguageContext<'ctx>, from: ValuePtr<'ctx>, name: String) -> Self {
        let field_ptr = ctx
            .builder
            .build_alloca(ctx.types.ptr, &format!("{name}_field_ptr"))
            .unwrap();
        ctx.builder.build_store(field_ptr, from.get_ptr()).unwrap();
        Self::new(field_ptr, name)
    }

    pub fn load(&self, ctx: &LanguageContext<'ctx>, into_name: String) -> Option<T> {
        if self.invalid {
            None
        } else {
            let name = self.name.clone();
            let value_ptr = ctx
                .builder
                .build_load(ctx.types.ptr, self.field_ptr, &format!("{name}_ptr"))
                .unwrap()
                .into_pointer_value();
            Some(T::from_ptr(ctx, value_ptr, self.name.clone(), into_name))
        }
    }

    pub fn exit_scope(&mut self) {
        self.invalid = true
        // TODO: When there are refcounted values, release it.
    }
}

pub enum ValuePtr<'ctx> {
    PInt(Int<'ctx>),
}

impl<'ctx> ValuePtr<'ctx> {
    pub fn get_ptr(&self) -> PointerValue<'ctx> {
        match self {
            ValuePtr::PInt(int) => int.ptr,
        }
    }
}

pub enum ValueField<'ctx> {
    RInt(Field<'ctx, Int<'ctx>>),
}

impl<'ctx> ValueField<'ctx> {
    pub fn from_value(ctx: &LanguageContext<'ctx>, from: ValuePtr<'ctx>, name: String) -> Self {
        match from {
            ValuePtr::PInt(_) => Self::RInt(Field::<Int>::from_value(ctx, from, name)),
        }
    }

    pub fn exit_scope(&mut self) {
        match self {
            ValueField::RInt(int) => int.exit_scope(),
        }
    }

    pub fn get_as_int(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<Int<'ctx>> {
        match self {
            ValueField::RInt(int) => Some(
                int.load(ctx, name)
                    .expect("Cannot get as int because field is invalidated!"),
            ),
            _ => None,
        }
    }
}

pub trait Value<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&ValueField<'ctx>>;
    fn build_metatype(llvm_ctx: &'ctx Context, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx>;
}

pub trait Copyable<'ctx>: Value<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        this_name: String,
        other_name: String,
    ) -> Self;
    fn from(
        ctx: &LanguageContext<'ctx>,
        other: Self,
        this_name: String,
        other_name: String,
    ) -> Self;
}

pub trait RefCounted<'ctx> {
    fn retain(&self, ctx: &LanguageContext<'ctx>);
    fn release(&self, ctx: &LanguageContext<'ctx>);
}

pub trait Literal<'ctx> {
    type LiteralType;
    type Repr: AnyValue<'ctx>;
    fn from_literal(ctx: &LanguageContext<'ctx>, literal: Self::LiteralType, name: String) -> Self;
    fn raw(&self, ctx: &LanguageContext<'ctx>) -> Self::Repr;
}
