use inkwell::values::{AnyValue, PointerValue};

use crate::{context::LanguageContext};

pub struct Field {

}

pub trait Value<'ctx> {
    fn member<T: Copyable<'ctx>>(&self, ctx: LanguageContext<'ctx>, name: String) -> Option<T>;
}

pub trait Copyable<'ctx>: Value<'ctx> {
    fn from_ptr(ctx: LanguageContext<'ctx>, ptr: PointerValue<'ctx>) -> Self;
    fn from(ctx: LanguageContext<'ctx>, other: Self) -> Self;
}

pub trait Literal<'ctx> {
    type LiteralType;
    type Repr: AnyValue<'ctx>;
    fn from_literal(ctx: LanguageContext<'ctx>, literal: Self::LiteralType, name: String) -> Self;
    fn raw(&self, ctx: LanguageContext<'ctx>) -> Self::Repr;
}
