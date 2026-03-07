use crate::{
    context::{LLVMTypes, LanguageContext},
    types::{Metatype, MetatypeBuilder},
    value::{Copyable, Literal, Value, ValueField},
};
use inkwell::{
    context::Context,
    types::StructType,
    values::{IntValue, PointerValue},
};

pub struct Int<'ctx> {
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    pub fn new(ctx: &LanguageContext<'ctx>, value: IntValue<'ctx>, name: String) -> Self {
        let ptr = ctx
            .builder
            .build_alloca(ctx.types.int_struct, &format!("{name}_ptr"))
            .unwrap();
        let value_ptr = ctx
            .builder
            .build_struct_gep(ctx.types.int_struct, ptr, 0, &format!("{name}_value_ptr"))
            .unwrap();
        ctx.builder.build_store(value_ptr, value).unwrap();
        Self { ptr }
    }

    pub fn init_body(types: &LLVMTypes<'ctx>, empty: StructType<'ctx>) {
        empty.set_body(&[types.int_enum()], false);
    }
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&ValueField<'ctx>> {
        Option::<&ValueField<'ctx>>::None
    }

    fn build_metatype(llvm_ctx: &'ctx Context, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        let mut builder = MetatypeBuilder::new("Int".to_string(), ctx.types.int_struct);
        builder.build(llvm_ctx, ctx)
    }
}

impl<'ctx> Copyable<'ctx> for Int<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        this_name: String,
        other_name: String,
    ) -> Self {
        let value_ptr = ctx
            .builder
            .build_struct_gep(
                ctx.types.int_struct,
                ptr,
                0,
                &format!("{this_name}_raw_ptr"),
            )
            .unwrap();
        let value = ctx
            .builder
            .build_load(ctx.types.int, value_ptr, &format!("{this_name}_raw"))
            .unwrap()
            .into_int_value();
        Int::new(ctx, value, other_name)
    }

    fn from(
        ctx: &LanguageContext<'ctx>,
        other: Self,
        this_name: String,
        other_name: String,
    ) -> Self {
        Int::from_ptr(ctx, other.ptr, this_name, other_name)
    }
}

impl<'ctx> Literal<'ctx> for Int<'ctx> {
    type LiteralType = u32;
    type Repr = IntValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, value: Self::LiteralType, name: String) -> Self {
        let ir_int = ctx.int(value as u64);
        Int::new(ctx, ir_int, name)
    }

    fn raw(&self, ctx: &LanguageContext<'ctx>) -> Self::Repr {
        let value_ptr = unsafe {
            ctx.builder
                .build_gep(
                    ctx.types.int_struct,
                    self.ptr,
                    &[ctx.int(0), ctx.int(0)],
                    "value_ptr",
                )
                .unwrap()
        };
        ctx.builder
            .build_load(ctx.types.int, value_ptr, "int_raw")
            .unwrap()
            .into_int_value()
    }
}
