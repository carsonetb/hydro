use inkwell::{context::Context, types::BasicTypeEnum, values::PointerValue};

use crate::{
    context::LanguageContext,
    types::{BasicType, MetatypeBuilder, TypeID},
    value::{Copyable, Field, Value, ValueStatic},
};

#[derive(Clone)]
pub struct Tuple<'ctx> {
    metatype: TypeID,
    items: Vec<Field<'ctx>>,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Tuple<'ctx> {}

impl<'ctx> Value<'ctx> for Tuple<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&Field<'ctx>> {
        let as_int = name
            .parse::<usize>()
            .expect("Cannot pass a non-number for a tuple member.");
        Some(&self.items[as_int])
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.ptr
    }
}

impl<'ctx> ValueStatic<'ctx> for Tuple<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        let type_name = TypeID::new("Tuple".to_string(), generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(&type_name.name().as_str());
        obj_struct.set_body(
            &vec![BasicTypeEnum::PointerType(ctx.types.ptr); generics.len()],
            false,
        );

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicType::Function,
            TypeID::from_base("Function".to_string()),
            obj_struct,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Tuple<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        ptr_type: TypeID,
        this_name: String,
        other_name: String,
    ) -> Self {
        todo!()
    }

    fn from(
        ctx: &LanguageContext<'ctx>,
        other: Self,
        this_name: String,
        other_name: String,
    ) -> Self {
        todo!()
    }
}
