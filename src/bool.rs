use chumsky::span::Spanned;
use inkwell::{
    IntPredicate,
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicTypeEnum},
    values::{AnyValue, BasicValueEnum, FunctionValue, IntValue},
};

use crate::{
    callable::Function,
    codegen::CompileError,
    context::LanguageContext,
    int::Int,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueStatic},
};

#[derive(Debug, Clone)]
pub struct Bool<'ctx> {
    pub val: IntValue<'ctx>,
}

impl<'ctx> Bool<'ctx> {
    pub fn new(value: IntValue<'ctx>) -> Self {
        Self { val: value }
    }

    pub fn build_binop(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        op_builder: impl Fn(IntValue<'ctx>, IntValue<'ctx>) -> IntValue<'ctx>,
        op_name: &str,
    ) -> FunctionValue<'ctx> {
        let add_llvm_type = ctx
            .types
            .bool
            .fn_type(&[BasicMetadataTypeEnum::IntType(ctx.types.bool); 2], false);
        let add_llvm_fn =
            ctx.module
                .add_function(format!("Bool.{op_name}").as_str(), add_llvm_type, None);
        let entry = llvm_ctx.append_basic_block(add_llvm_fn, "entry");
        let old_block = ctx.builder.get_insert_block().unwrap();
        ctx.builder.position_at_end(entry);

        let left = add_llvm_fn.get_nth_param(0).unwrap().into_int_value();
        let right = add_llvm_fn.get_nth_param(1).unwrap().into_int_value();
        left.set_name("lhs");
        right.set_name("rhs");
        let result = op_builder(left, right);
        ctx.builder.build_return(Some(&result)).unwrap();
        ctx.builder.position_at_end(old_block);

        add_llvm_fn
    }

    fn cmp_type() -> TypeID {
        let typeid = TypeID::from_base("Bool");
        TypeID::new(
            "Function",
            vec![TypeID::new("Tuple", vec![typeid.clone(); 2]), typeid],
        )
    }
}

impl<'ctx> Value<'ctx> for Bool<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        let typeid = TypeID::from_base("Bool");

        macro_rules! op_fun_wrapper {
            ($op_name:expr, $fn_name:expr) => {
                Ok(ValueEnum::Function(Function::new(
                    ctx,
                    ctx.module
                        .get_function($fn_name)
                        .unwrap()
                        .as_global_value()
                        .as_pointer_value(),
                    Self::cmp_type(),
                    $op_name,
                )))
            };
        }

        match &name.inner[..] {
            "==" => op_fun_wrapper!("==", "Bool.=="),
            "!=" => op_fun_wrapper!("!=", "Bool.!="),
            "||" => op_fun_wrapper!("||", "Bool.||"),
            "&&" => op_fun_wrapper!("&&", "Int.&&"),
            _ => Err(CompileError::new(
                name.span,
                &format!("Type `Bool` has no `{}` operator.", name.inner),
            )),
        }
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Bool")
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::IntValue(self.val)
    }

    fn construct_ptr(
        &self,
        ctx: &LanguageContext<'ctx>,
        into_name: &str,
    ) -> inkwell::values::PointerValue<'ctx> {
        let mem = ctx
            .builder
            .build_alloca(ctx.types.bool, &format!("{into_name}_ptr"))
            .unwrap();
        ctx.builder.build_store(mem, self.val);
        mem
    }
}

impl<'ctx> ValueStatic<'ctx> for Bool<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx inkwell::context::Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 0);

        macro_rules! build_binop {
            ($op_name_str:expr, $function_name:ident) => {
                Self::build_binop(
                    llvm_ctx,
                    ctx,
                    |left, right| {
                        ctx.builder
                            .$function_name(left, right, "result")
                            .unwrap()
                            .as_any_value_enum()
                            .into_int_value()
                    },
                    $op_name_str,
                )
            };
        }

        macro_rules! build_cmpop {
            ($op_name_str:expr, $predicate:expr) => {
                Self::build_binop(
                    llvm_ctx,
                    ctx,
                    |left, right| {
                        ctx.builder
                            .build_int_compare($predicate, left, right, "product")
                            .unwrap()
                    },
                    $op_name_str,
                )
            };
        }

        let typeid = TypeID::from_base("Bool");
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Bool,
            typeid.clone(),
            None,
            AnyTypeEnum::IntType(ctx.types.bool),
            false,
        );

        let eqa_llvm_fn = build_cmpop!("==", IntPredicate::EQ);
        let neq_llvm_fn = build_cmpop!("!=", IntPredicate::NE);
        let or_llvm_fn = build_binop!("||", build_or);
        let and_llvm_fn = build_binop!("&&", build_and);
        let eqa_fn = Function::from_function(llvm_ctx, ctx, eqa_llvm_fn, Self::cmp_type());
        let neq_fn = Function::from_function(llvm_ctx, ctx, neq_llvm_fn, Self::cmp_type());
        let or_fn = Function::from_function(llvm_ctx, ctx, or_llvm_fn, Self::cmp_type());
        let and_fn = Function::from_function(llvm_ctx, ctx, and_llvm_fn, Self::cmp_type());
        builder.add_static("==", ValueEnum::Function(eqa_fn));
        builder.add_static("!=", ValueEnum::Function(neq_fn));
        builder.add_static("||", ValueEnum::Function(or_fn));
        builder.add_static("&&", ValueEnum::Function(and_fn));

        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Bool<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: &str,
    ) -> Self {
        Bool::new(val.into_int_value())
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        Bool::from_val(ctx, other.get_value(), other.get_type(ctx), name)
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: inkwell::values::PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        Self::new(
            ctx.builder
                .build_load(ctx.types.bool, ptr, into_name)
                .unwrap()
                .into_int_value(),
        )
    }
}

impl<'ctx> Literal<'ctx> for Bool<'ctx> {
    type LiteralType = bool;
    type Repr = IntValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, literal: Self::LiteralType, name: &str) -> Self {
        Bool::new(ctx.bool(literal))
    }

    fn raw(&self, ctx: &LanguageContext<'ctx>, name: &str) -> Self::Repr {
        self.val
    }
}
