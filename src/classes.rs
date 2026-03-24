use std::collections::{BTreeMap, HashMap};

use chumsky::span::Spanned;
use inkwell::{
    types::StructType,
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    callable::Function,
    codegen::CompileError,
    context::LanguageContext,
    types::TypeID,
    value::{Copyable, Value, ValueEnum, ValueStatic},
};

#[derive(Debug, Clone)]
pub struct ClassMember {
    pub typ: TypeID,
    pub index: u32,
}

impl ClassMember {
    pub fn new(typ: TypeID, index: u32) -> Self {
        ClassMember { typ, index }
    }
}

#[derive(Debug, Clone)]
pub struct ClassInfo<'ctx> {
    class_struct: StructType<'ctx>,
    members: BTreeMap<String, ClassMember>,
    functions: BTreeMap<String, Function<'ctx>>,
}

impl<'ctx> ClassInfo<'ctx> {
    pub fn new(
        class_struct: StructType<'ctx>,
        members: BTreeMap<String, ClassMember>,
        functions: BTreeMap<String, Function<'ctx>>,
    ) -> Self {
        Self {
            class_struct,
            members,
            functions,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Class<'ctx> {
    pub typ: TypeID,
    ptr: PointerValue<'ctx>,
}

impl<'ctx> Class<'ctx> {
    pub fn info(&self, ctx: &LanguageContext<'ctx>) -> ClassInfo<'ctx> {
        ctx.get(self.typ.clone()).class_info.clone().unwrap()
    }
}

impl<'ctx> Value<'ctx> for Class<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        let info = self.info(ctx);
        if info.functions.contains_key(&name.inner) {
            return Ok(ValueEnum::Function(info.functions[&name.inner].clone()));
        }
        let member = info.members.get(&name.inner).ok_or_else(|| {
            CompileError::new(
                name.span,
                &format!("Type `{}` has no member `{}`.", self.typ, name.inner),
            )
        })?;
        let member_val = ctx.build_ptr_load(
            info.class_struct,
            member.typ.clone(),
            self.ptr,
            member.index,
            into,
        );
        Ok(ValueEnum::from_val(
            ctx,
            member_val,
            member.typ.clone(),
            into,
        ))
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        self.typ.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        self.ptr.into()
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        self.ptr
    }
}

impl<'ctx> Copyable<'ctx> for Class<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: &str,
    ) -> Self {
        Self {
            typ: val_type,
            ptr: val.into_pointer_value(),
        }
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        other
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        Self::from_val(ctx, ptr.into(), typ, into_name)
    }
}
