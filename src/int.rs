use inkwell::values::{AnyValue, IntValue};
use crate::value::Value;

pub struct Int<'ctx> {
    pub value: IntValue<'ctx>
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    type Repr = IntValue<'ctx>;

    fn get(&self) -> impl AnyValue<'ctx> {
        return self.value;
    }

    fn from(val: IntValue<'ctx>) -> Self {
        Int {
            value: val
        }
    }
}
