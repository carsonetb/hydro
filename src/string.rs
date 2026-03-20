use chumsky::span::Spanned;
use inkwell::{
    types::{AnyType, BasicMetadataTypeEnum, BasicType},
    values::{
        BasicMetadataValueEnum, BasicValue, BasicValueEnum, IntValue, PointerValue, StructValue,
    },
};

use crate::{
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug)]
pub struct Str<'ctx> {
    pub val: PointerValue<'ctx>,
}

impl<'ctx> Str<'ctx> {
    pub fn new(
        ctx: &LanguageContext<'ctx>,
        size: IntValue<'ctx>,
        value: PointerValue<'ctx>,
        into_name: &str,
    ) -> Self {
        let obj_struct = ctx.context.get_struct_type("String").unwrap();
        let mem = ctx.builder.build_malloc(obj_struct, into_name).unwrap();
        let dest_size = ctx
            .builder
            .build_struct_gep(obj_struct, mem, 0, "out_size_ptr")
            .unwrap();
        let dest_raw_ptr = ctx
            .builder
            .build_struct_gep(obj_struct, mem, 1, "out_raw_ptr_ptr")
            .unwrap();
        ctx.builder.build_store(dest_size, ctx.int(48));
        ctx.builder.build_store(dest_raw_ptr, value);
        Self { val: mem }
    }
}

impl<'ctx> Value<'ctx> for Str<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            &format!("`String` has no member {}", name.inner),
        ))
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("String")
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        self.val.as_basic_value_enum()
    }
}

impl<'ctx> ValueStatic<'ctx> for Str<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx inkwell::context::Context,
        ctx: &mut crate::context::LanguageContext<'ctx>,
        generics: Vec<crate::types::TypeID>,
    ) {
        assert_eq!(generics.len(), 0);

        let obj_struct = llvm_ctx.opaque_struct_type("String");
        obj_struct.set_body(
            &vec![
                ctx.types.int.as_basic_type_enum(),
                ctx.types.ptr.as_basic_type_enum(),
            ],
            false,
        );

        let typeid = TypeID::from_base("String");
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::String,
            typeid.clone(),
            Some(obj_struct),
            ctx.types.ptr.as_any_type_enum(),
            false,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Str<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: &str,
    ) -> Self {
        let copy_type = ctx
            .types
            .ptr
            .fn_type(&[BasicMetadataTypeEnum::PointerType(ctx.types.ptr)], false);
        let copy = ctx.module.get_function("String__copy").unwrap();
        let copied = ctx
            .build_call_returns(
                copy,
                &[BasicMetadataValueEnum::PointerValue(
                    val.into_pointer_value(),
                )],
                name,
            )
            .into_pointer_value();
        Self { val: copied }
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        Self::from_val(
            ctx,
            other.val.as_basic_value_enum(),
            TypeID::from_base("String"),
            name,
        )
    }
}

impl<'ctx> Literal<'ctx> for Str<'ctx> {
    type LiteralType = String;
    type Repr = PointerValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, literal: Self::LiteralType, name: &str) -> Self {
        let const_string = ctx.context.const_string(literal.as_bytes(), true);
        let size = ctx.int(name.len() as u64 + 1);
        let array = ctx
            .builder
            .build_array_malloc(const_string.get_type(), size, &name)
            .unwrap();
        ctx.builder.build_store(array, const_string);
        Self::new(ctx, size, array, name)
    }

    fn raw(&self, ctx: &LanguageContext<'ctx>, name: &str) -> Self::Repr {
        self.val
    }
}
