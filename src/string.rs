use chumsky::span::Spanned;
use inkwell::{
    types::{AnyType, BasicType},
    values::{BasicValue, BasicValueEnum, IntValue, PointerValue, StructValue},
};

use crate::{
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug)]
pub struct Str<'ctx> {
    pub val: StructValue<'ctx>,
}

impl<'ctx> Str<'ctx> {
    pub fn new(
        ctx: &LanguageContext<'ctx>,
        size: IntValue<'ctx>,
        value: PointerValue<'ctx>,
    ) -> Self {
        let obj_struct = ctx.context.get_struct_type("String").unwrap();
        let obj = obj_struct.const_named_struct(&vec![
            size.as_basic_value_enum(),
            value.as_basic_value_enum(),
        ]);
        Self { val: obj }
    }
}

impl<'ctx> Value<'ctx> for Str<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: String,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            format!("`String` has no member {}", name.inner),
        ))
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("String".to_string())
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

        let typeid = TypeID::from_base("String".to_string());
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::String,
            typeid.clone(),
            Some(obj_struct),
            obj_struct.as_any_type_enum(),
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
        name: String,
    ) -> Self {
        let obj_struct = val.into_struct_value();
        let size = ctx
            .builder
            .build_extract_value(obj_struct, 0, &name)
            .unwrap()
            .into_int_value();
        let ptr = ctx
            .builder
            .build_extract_value(obj_struct, 1, &name)
            .unwrap()
            .into_pointer_value();
        let dest = ctx
            .builder
            .build_malloc(obj_struct.get_type(), &name)
            .unwrap();
        ctx.builder.build_memcpy(dest, 1, ptr, 1, size).unwrap();
        Self::new(ctx, size, dest)
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self {
        Self::from_val(
            ctx,
            other.val.as_basic_value_enum(),
            TypeID::from_base("String".to_string()),
            name,
        )
    }
}

impl<'ctx> Literal<'ctx> for Str<'ctx> {
    type LiteralType = String;
    type Repr = StructValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, literal: Self::LiteralType, name: String) -> Self {
        let const_string = ctx.context.const_string(literal.as_bytes(), true);
        let size = ctx.int(name.len() as u64 + 1);
        let array = ctx
            .builder
            .build_array_malloc(const_string.get_type(), size, &name)
            .unwrap();
        ctx.builder.build_store(array, const_string);
        Self::new(ctx, size, array)
    }

    fn raw(&self, ctx: &LanguageContext<'ctx>, name: String) -> Self::Repr {
        self.val
    }
}
