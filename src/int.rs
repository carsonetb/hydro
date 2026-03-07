use inkwell::{types::{BasicTypeEnum, StructType}, values::{IntValue, PointerValue}};
use crate::{context::{LLVMTypes, LanguageContext}, errors::{CompileError, Result}, value::{Copyable, Literal, Value}};

struct Int<'ctx> {
    ptr: PointerValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    fn new(ctx: LanguageContext<'ctx>, value: IntValue<'ctx>, name: String) -> Self {
        let ptr = ctx.builder.build_alloca(ctx.types.int_struct, &format!("{name}_ptr")).unwrap();
        let value_ptr = ctx.builder.build_struct_gep(ctx.types.int_struct, ptr, 0, &format!("{name}_value_ptr")).unwrap();
        ctx.builder.build_store(value_ptr, value).unwrap();
        Self {
            ptr
        }
    }

    fn init_body(types: LLVMTypes<'ctx>, empty: StructType<'ctx>) {
        empty.set_body(&[types.int_enum()], false);
    }
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    fn member<T: Copyable<'ctx>>(&self, ctx: LanguageContext<'ctx>, name: String) -> Option<T> {
        Option::<T>::None
    }
}

impl<'ctx> Copyable<'ctx> for Int<'ctx> {
    fn from_ptr(ctx: LanguageContext<'ctx>, ptr: PointerValue<'ctx>) -> Self {
        let value_ptr = ctx.builder.build_struct_gep(ctx.types.int_struct, ptr, 0, "from_ptr").unwrap();
        let value = ctx.builder.build_load(ctx.types.int, value_ptr, "from_raw").unwrap().into_int_value();
        Int::new(ctx, value, "new_struct".to_string())
    }

    fn from(ctx: LanguageContext<'ctx>, other: Self) -> Self {
        Int::from_ptr(ctx, other.ptr)
    }
}

impl<'ctx> Literal<'ctx> for Int<'ctx> {
    type LiteralType = u32;
    type Repr = IntValue<'ctx>;

    fn from_literal(ctx: LanguageContext<'ctx>, value: Self::LiteralType, name: String) -> Self {
        let ir_int = ctx.int(value as u64);
        Int::new(ctx, ir_int, name)
    }

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
