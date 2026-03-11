use enum_dispatch::enum_dispatch;
use inkwell::{
    context::Context,
    values::{AnyValue, PointerValue},
};

use crate::{
    callable::Function,
    context::LanguageContext,
    int::Int,
    tuple::Tuple,
    types::{BasicType, Metatype, TypeID},
    unit::Unit,
};

#[derive(Debug, Clone)]
pub struct Field<'ctx> {
    name: String,
    typ: TypeID,
    invalid: bool,
    field_ptr: PointerValue<'ctx>,
}

impl<'ctx> Field<'ctx> {
    pub fn new(field_ptr: PointerValue<'ctx>, name: String, typ: TypeID) -> Self {
        Self {
            name,
            typ,
            invalid: false,
            field_ptr,
        }
    }

    pub fn from_value(ctx: &LanguageContext<'ctx>, from: ValuePtr<'ctx>, name: String) -> Self {
        let field_ptr = ctx
            .builder
            .build_alloca(ctx.types.ptr, &format!("FIELD__{name}"))
            .unwrap();
        ctx.builder.build_store(field_ptr, from.get_ptr()).unwrap();
        Self::new(field_ptr, name, from.get_type(ctx))
    }

    pub fn store(&self, ctx: &LanguageContext<'ctx>, from: ValuePtr<'ctx>) {
        ctx.builder
            .build_store(self.field_ptr, from.get_ptr())
            .unwrap();
    }

    pub fn load<T: Copyable<'ctx>>(
        &self,
        ctx: &LanguageContext<'ctx>,
        into_name: String,
    ) -> Option<T> {
        if self.invalid {
            None
        } else {
            let name = self.name.clone();
            let value_ptr = ctx
                .builder
                .build_load(ctx.types.ptr, self.field_ptr, &format!("{name}_ptr"))
                .unwrap()
                .into_pointer_value();
            Some(T::from_ptr(
                ctx,
                value_ptr,
                self.typ.clone(),
                self.name.clone(),
                into_name,
            ))
        }
    }

    pub fn exit_scope(&mut self) {
        self.invalid = true
        // TODO: When there are refcounted values, release it.
    }
}

#[enum_dispatch]
pub enum ValuePtr<'ctx> {
    PUnit(Unit),
    PInt(Int<'ctx>),
    PTuple(Tuple<'ctx>),
    PFunction(Function<'ctx>),
}

impl<'ctx> ValuePtr<'ctx> {
    pub fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        this_name: String,
        other_name: String,
    ) -> Self {
        match ctx.get(typ.clone()).base {
            BasicType::Unit => panic!(),
            BasicType::Type => panic!(),
            BasicType::Int => Self::PInt(Int::from_ptr(ctx, ptr, typ, this_name, other_name)),
            BasicType::Function => {
                Self::PFunction(Function::from_ptr(ctx, ptr, typ, this_name, other_name))
            }
            BasicType::Tuple => Self::PTuple(Tuple::from_ptr(ctx, ptr, typ, this_name, other_name)),
        }
    }
}

#[enum_dispatch(ValuePtr)]
pub trait Value<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&Field<'ctx>>;
    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID;
    fn get_ptr(&self) -> PointerValue<'ctx>;
}

pub trait ValueStatic<'ctx>: Value<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    );
}

pub trait Copyable<'ctx>: Value<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        ptr_type: TypeID,
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
