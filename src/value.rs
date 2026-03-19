use chumsky::span::Spanned;
use enum_dispatch::enum_dispatch;
use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicType, BasicTypeEnum},
    values::{AnyValue, BasicValueEnum, PointerValue},
};
use strum_macros::EnumTryAs;

use crate::{
    bool::Bool,
    callable::Function,
    codegen::CompileError,
    context::LanguageContext,
    int::Int,
    string::Str,
    tuple::Tuple,
    types::{BasicBuiltin, Metatype, TypeID},
    unit::Unit,
};

#[derive(Debug)]
pub struct Field<'ctx> {
    name: String,
    invalid: bool,
    pub is_return: bool,
    pub value: ValueEnum<'ctx>,
}

impl<'ctx> Field<'ctx> {
    pub fn new(value: ValueEnum<'ctx>, name: String) -> Self {
        Self {
            name,
            invalid: false,
            is_return: false,
            value,
        }
    }

    pub fn reference(&self, ctx: &LanguageContext<'ctx>) {
        if self.invalid {
            panic!("Cannot reference an invalidated field!")
        } else {
            // TODO: Retain current value.
        }
    }

    pub fn release(&self, ctx: &LanguageContext<'ctx>) {
        assert_eq!(self.is_return, false);
        // TODO: Release current value.
    }
}

pub fn any_to_basic<'ctx>(any: AnyTypeEnum<'ctx>) -> Option<BasicTypeEnum<'ctx>> {
    match any {
        AnyTypeEnum::ArrayType(t) => Some(t.as_basic_type_enum()),
        AnyTypeEnum::FloatType(t) => Some(t.as_basic_type_enum()),
        AnyTypeEnum::IntType(t) => Some(t.as_basic_type_enum()),
        AnyTypeEnum::PointerType(t) => Some(t.as_basic_type_enum()),
        AnyTypeEnum::StructType(t) => Some(t.as_basic_type_enum()),
        AnyTypeEnum::VectorType(t) => Some(t.as_basic_type_enum()),
        _ => None, // VoidType, FunctionType, LabelType, MetadataType, etc.
    }
}

#[enum_dispatch]
#[derive(Debug, EnumTryAs, Clone)]
pub enum ValueEnum<'ctx> {
    Unit(Unit),
    Bool(Bool<'ctx>),
    Int(Int<'ctx>),
    String(Str<'ctx>),
    Tuple(Tuple<'ctx>),
    Function(Function<'ctx>),
}

impl<'ctx> ValueEnum<'ctx> {
    pub fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        typ: TypeID,
        name: String,
    ) -> Self {
        match ctx.get(typ.clone()).base {
            BasicBuiltin::Unit => panic!(),
            BasicBuiltin::Type => panic!(),
            BasicBuiltin::Bool => Self::Bool(Bool::from_val(ctx, val, typ, name)),
            BasicBuiltin::Int => Self::Int(Int::from_val(ctx, val, typ, name)),
            BasicBuiltin::Function => Self::Function(Function::from_val(ctx, val, typ, name)),
            BasicBuiltin::Tuple => Self::Tuple(Tuple::from_val(ctx, val, typ, name)),
            BasicBuiltin::String => Self::String(Str::from_val(ctx, val, typ, name)),
        }
    }
}

#[enum_dispatch(ValueEnum)]
pub trait Value<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: String,
    ) -> Result<ValueEnum<'ctx>, CompileError>;
    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID;
    fn get_value(&self) -> BasicValueEnum<'ctx>;
}

pub trait ValueStatic<'ctx>: Value<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    );
}

pub trait Copyable<'ctx>: Value<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: String,
    ) -> Self;

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self;
}

pub trait Literal<'ctx> {
    type LiteralType;
    type Repr: AnyValue<'ctx>;
    fn from_literal(ctx: &LanguageContext<'ctx>, literal: Self::LiteralType, name: String) -> Self;
    fn raw(&self, ctx: &LanguageContext<'ctx>, name: String) -> Self::Repr;
}
