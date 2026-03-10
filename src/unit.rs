use inkwell::values::PointerValue;

use crate::{
    context::LanguageContext,
    types::{BasicType, Metatype, MetatypeBuilder, TypeId},
    value::{Field, Value, ValueStatic},
};

#[derive(Clone)]
pub struct Unit {}

impl<'ctx> Value<'ctx> for Unit {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        panic!()
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeId {
        TypeId("Unit".to_string())
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        panic!()
    }
}

impl<'ctx> ValueStatic<'ctx> for Unit {
    fn build_metatype(
        llvm_ctx: &'ctx inkwell::context::Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeId>,
    ) {
        assert_eq!(generics.len(), 0);
        let obj_struct = llvm_ctx.opaque_struct_type("Unit");
        obj_struct.set_body(&[], false);
        let mut builder =
            MetatypeBuilder::new(ctx, BasicType::Unit, "Unit".to_string(), obj_struct);
        builder.build(llvm_ctx, ctx, generics);
    }
}
