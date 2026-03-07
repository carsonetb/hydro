use inkwell::values::{AnyValue, PointerValue};

use crate::{context::LanguageContext};

pub trait Value<'ctx> {
    fn member(&self, name: String) -> Option<impl Value<'ctx>>;
}

pub trait Literal<'ctx> {
    type Repr: AnyValue<'ctx>;
    fn raw(&self, ctx: LanguageContext<'ctx>) -> Self::Repr;
}
