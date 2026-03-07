use inkwell::values::AnyValue;

pub trait Value<'ctx> {
    type Repr: AnyValue<'ctx>;
    fn get(&self) -> impl AnyValue<'ctx>;
    fn from(val: Self::Repr) -> Self;
}
