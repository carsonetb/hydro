use chumsky::span::Spanned;
use inkwell::{
    types::{AnyTypeEnum, BasicTypeEnum, VoidType},
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, Metatype, MetatypeBuilder, TypeID},
    value::{Field, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug)]
pub struct Unit {}

impl<'ctx> Value<'ctx> for Unit {
    fn member(
        &self,
        _ctx: &LanguageContext<'ctx>,
        _name: Spanned<String>,
        _into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        panic!()
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Unit")
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        panic!()
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        panic!()
    }
}

impl<'ctx> ValueStatic<'ctx> for Unit {
    fn build_metatype(
        llvm_ctx: &'ctx inkwell::context::Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 0);
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Unit,
            TypeID::from_base("Unit"),
            None,
            AnyTypeEnum::PointerType(ctx.types.ptr),
            false,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}
