use inkwell::{types::{BasicTypeEnum, StructType}, values::{IntValue, PointerValue}};
use crate::{context::{LLVMTypes, LanguageContext}, errors::{CompileError, Result}, value::{Literal, Value}};

struct Int<'ctx> {
    ptr: PointerValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    fn new(ctx: LanguageContext<'ctx>, value: u32, name: String) -> Self {
        let ptr = ctx.builder.build_alloca(ctx.types.int_struct, &format!("{name}_ptr")).unwrap();
        let value_ptr = ctx.builder.build_struct_gep(ctx.types.int_struct, ptr, 0, &format!("{name}_value_ptr")).unwrap();
        ctx.builder.build_store(value_ptr, ctx.int(value as u64)).unwrap();
        Self {
            ptr
        }
    }

    fn struct_body(types: LLVMTypes<'ctx>, empty: StructType<'ctx>) {
        empty.set_body(&[types.int_enum()], false);
    }
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    fn member(&self, name: String) -> Option<impl Value<'ctx>> {
        Option::<Self>::None
    }
}

impl<'ctx> Literal<'ctx> for Int<'ctx> {
    type Repr = IntValue<'ctx>;
    fn raw(&self, ctx: LanguageContext<'ctx>) -> Self::Repr {
        let value_ptr = unsafe {
            ctx.builder.build_gep(
                ctx.types.int_struct,
                self.ptr,
                &[ctx.int(0), ctx.int(0)],
                "value_ptr"
            ).unwrap()
        };
        ctx.builder.build_load(ctx.types.int, value_ptr, "int_raw").unwrap().into_int_value()
    }
}
