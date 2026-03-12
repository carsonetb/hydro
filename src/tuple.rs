use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicTypeEnum},
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Field, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug)]
pub struct Tuple<'ctx> {
    metatype: TypeID,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Tuple<'ctx> {}

impl<'ctx> Value<'ctx> for Tuple<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String, into: String) -> ValueEnum<'ctx> {
        let as_int = name
            .parse::<usize>()
            .expect("Cannot pass a non-number for a tuple member.");
        let member_ptr = ctx
            .builder
            .build_struct_gep(
                ctx.get_struct(self.metatype.clone()),
                self.ptr,
                as_int as u32,
                format!("{into}_field").as_str(),
            )
            .unwrap();
        let member_val = ctx
            .builder
            .build_load(
                ctx.get_storage(self.metatype.generics[as_int].clone()),
                member_ptr,
                format!("{into}_val").as_str(),
            )
            .unwrap();
        ValueEnum::from_val(
            ctx,
            member_val,
            self.metatype.generics[as_int].clone(),
            into,
        )
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.ptr)
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
        let body: Vec<BasicTypeEnum<'ctx>> = generics
            .iter()
            .map(|g| ctx.get_storage(g.clone()))
            .collect();
        obj_struct.set_body(&body, false);

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Function,
            TypeID::from_base("Function".to_string()),
            obj_struct,
            BasicTypeEnum::PointerType(ctx.types.ptr),
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
        name: String,
    ) -> Self {
        todo!()
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self {
        todo!()
    }
}
