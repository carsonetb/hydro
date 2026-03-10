use inkwell::values::PointerValue;

use crate::{
    context::LanguageContext,
    types::{BasicType, Metatype, MetatypeBuilder},
    value::{Field, Value, ValueStatic},
};

#[derive(Clone)]
pub struct Unit {}

impl<'ctx> Value<'ctx> for Unit {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        panic!()
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get_base_metatype("Unit".to_string()).unwrap()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        panic!()
    }
}

impl<'ctx> ValueStatic<'ctx> for Unit {
    fn build_metatype(
        llvm_ctx: &'ctx inkwell::context::Context,
        ctx: &LanguageContext<'ctx>,
        generics: Vec<Metatype<'ctx>>,
    ) -> Metatype<'ctx> {
        assert_eq!(generics.len(), 0);
        let obj_struct = llvm_ctx.opaque_struct_type("Unit");
        obj_struct.set_body(&[], false);
        let mut builder = MetatypeBuilder::new(BasicType::Unit, "Unit".to_string(), obj_struct);
        builder.build(llvm_ctx, ctx, generics)
    }
}
