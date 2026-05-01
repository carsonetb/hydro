use chumsky::span::Spanned;
use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicTypeEnum},
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Field, Value, ValueEnum, ValueRef, ValueStatic, any_to_basic},
};

#[derive(Clone, Debug)]
pub struct Tuple<'ctx> {
    metatype: TypeID,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Tuple<'ctx> {}

impl<'ctx> Value<'ctx> for Tuple<'ctx> {
    fn member(
        &self,
        ctx: &mut LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        let as_int = name.parse::<usize>().map_err(|_| {
            CompileError::new(
                name.span,
                &format!(
                    "`{}` has no member `{}`. All Tuple member must be a number.",
                    self.get_type(),
                    name.inner
                ),
            )
        })?;
        let member_ptr = ctx
            .builder
            .build_struct_gep(
                ctx.get_struct(self.metatype.clone()),
                self.ptr,
                as_int as u32,
                format!("{into}_field").as_str(),
            )
            .map_err(|_| {
                CompileError::new(
                    name.span,
                    &format!(
                        "`{}` does not have `{}` items.",
                        self.get_type(),
                        as_int + 1
                    ),
                )
            })?;
        let member_val = ctx
            .builder
            .build_load(
                ctx.get_storage(self.metatype.generics[as_int].clone()),
                member_ptr,
                format!("{into}_val").as_str(),
            )
            .unwrap();
        Ok(ValueEnum::from_val(
            ctx,
            member_val,
            self.metatype.generics[as_int].clone(),
            into,
        ))
    }

    fn member_ref(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueRef<'ctx>, CompileError> {
        todo!()
    }

    fn get_type(&self) -> TypeID {
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.ptr)
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        todo!()
    }
}

impl<'ctx> ValueStatic<'ctx> for Tuple<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        let type_name = TypeID::new("Tuple", generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(type_name.name().as_str());
        let body: Vec<BasicTypeEnum<'ctx>> = generics
            .iter()
            .map(|g| ctx.get_storage(g.clone()))
            .collect();
        obj_struct.set_body(&body, false);

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Function,
            TypeID::from_base("Function"),
            Some(obj_struct),
            AnyTypeEnum::PointerType(ctx.types.ptr),
            false,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Tuple<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_typ: TypeID,
        name: &str,
    ) -> Self {
        todo!()
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        todo!()
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        todo!()
    }
}
