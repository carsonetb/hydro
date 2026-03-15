use crate::{
    callable::Function,
    codegen::CompileError,
    context::{LLVMTypes, LanguageContext},
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueStatic},
};
use chumsky::span::Spanned;
use inkwell::{
    IntPredicate,
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicTypeEnum, StructType},
    values::{
        AnyValue, BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, StructValue,
    },
};

#[derive(Clone, Debug)]
pub struct Int<'ctx> {
    pub val: IntValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    pub fn new(value: IntValue<'ctx>) -> Self {
        Self { val: value }
    }

    pub fn build_binop(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        op_builder: impl Fn(IntValue<'ctx>, IntValue<'ctx>) -> IntValue<'ctx>,
        op_name: String,
        boolean: bool,
    ) -> FunctionValue<'ctx> {
        let ret = if boolean {
            ctx.types.bool
        } else {
            ctx.types.int
        };
        let add_llvm_type = ret.fn_type(&[BasicMetadataTypeEnum::IntType(ctx.types.int); 2], false);
        let add_llvm_fn =
            ctx.module
                .add_function(format!("Int.{op_name}").as_str(), add_llvm_type, None);
        let entry = llvm_ctx.append_basic_block(add_llvm_fn, "entry");
        let old_block = ctx.builder.get_insert_block().unwrap();
        ctx.builder.position_at_end(entry);

        let left = add_llvm_fn.get_nth_param(0).unwrap().into_int_value();
        let right = add_llvm_fn.get_nth_param(1).unwrap().into_int_value();
        left.set_name("lhs");
        right.set_name("rhs");
        let result = op_builder(left, right);
        let as_int = Int::new(result);
        ctx.builder.build_return(Some(&as_int.get_value())).unwrap();
        ctx.builder.position_at_end(old_block);

        add_llvm_fn
    }
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: String,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        // TODO: Some functions use compare function not this.
        let bin_type = TypeID::new(
            "Function".to_string(),
            vec![
                TypeID::new(
                    "Tuple".to_string(),
                    vec![TypeID::from_base("Int".to_string()); 2],
                ),
                TypeID::from_base("Int".to_string()),
            ],
        );

        macro_rules! op_fun_wrapper {
            ($op_name:expr, $fn_name:expr) => {
                Ok(ValueEnum::Function(Function::new(
                    ctx,
                    ctx.module
                        .get_function($fn_name)
                        .unwrap()
                        .as_global_value()
                        .as_pointer_value(),
                    bin_type,
                    $op_name.to_string(),
                )))
            };
        }

        match &name.inner[..] {
            "+" => op_fun_wrapper!("+", "Int.+"),
            "-" => op_fun_wrapper!("-", "Int.-"),
            "*" => op_fun_wrapper!("*", "Int.*"),
            "/" => op_fun_wrapper!("/", "Int./"),
            "%" => op_fun_wrapper!("%", "Int.%"),
            "<<" => op_fun_wrapper!("<<", "Int.<<"),
            ">>" => op_fun_wrapper!(">>", "Int.>>"),
            "&" => op_fun_wrapper!("&", "Int.&"),
            "^" => op_fun_wrapper!("^", "Int.^"),
            "|" => op_fun_wrapper!("|", "Int.|"),
            ">" => op_fun_wrapper!(">", "Int.>"),
            "<" => op_fun_wrapper!("<", "Int.<"),
            "<=" => op_fun_wrapper!("<=", "Int.<="),
            ">=" => op_fun_wrapper!(">=", "Int.>="),
            "==" => op_fun_wrapper!("==", "Int.=="),
            "!=" => op_fun_wrapper!("!=", "Int.!="),
            _ => Err(CompileError::new(
                name.span,
                format!("Type `Int` has no `{}` operator.", name.inner),
            )),
        }
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Int".to_string())
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::IntValue(self.val)
    }
}

impl<'ctx> ValueStatic<'ctx> for Int<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 0);

        macro_rules! build_binop {
            ($op_name_str:expr, $function_name:ident) => {
                Int::build_binop(
                    llvm_ctx,
                    ctx,
                    |left, right| {
                        ctx.builder
                            .$function_name(left, right, "result")
                            .unwrap()
                            .as_any_value_enum()
                            .into_int_value()
                    },
                    $op_name_str.to_string(),
                    false,
                )
            };
        }

        macro_rules! build_cmpop {
            ($op_name_str:expr, $predicate:expr) => {
                Int::build_binop(
                    llvm_ctx,
                    ctx,
                    |left, right| {
                        ctx.builder
                            .build_int_compare($predicate, left, right, "result")
                            .unwrap()
                            .as_any_value_enum()
                            .into_int_value()
                    },
                    $op_name_str.to_string(),
                    true,
                )
            };
        }

        let typeid = TypeID::from_base("Int".to_string());
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Int,
            typeid.clone(),
            None,
            BasicTypeEnum::IntType(ctx.types.int),
            false,
        );

        let bin_type = TypeID::new(
            "Function".to_string(),
            vec![
                TypeID::new("Tuple".to_string(), vec![typeid.clone(); 2]),
                typeid.clone(),
            ],
        );
        let cmp_type = TypeID::new(
            "Function".to_string(),
            vec![
                TypeID::new("Tuple".to_string(), vec![typeid.clone(); 2]),
                TypeID::from_base("Bool".to_string()),
            ],
        );
        let add_llvm_fn = build_binop!("+", build_int_add);
        let sub_llvm_fn = build_binop!("-", build_int_sub);
        let mul_llvm_fn = build_binop!("*", build_int_mul);
        let div_llvm_fn = build_binop!("/", build_int_signed_div);
        let mod_llvm_fn = build_binop!("%", build_int_signed_rem);
        let lsh_llvm_fn = build_binop!("<<", build_left_shift);
        let rsh_llvm_fn = Int::build_binop(
            llvm_ctx,
            ctx,
            |left, right| {
                ctx.builder
                    .build_right_shift(left, right, false, "product")
                    .unwrap()
                    .as_any_value_enum()
                    .into_int_value()
            },
            ">>".to_string(),
            false,
        );
        let bwa_llvm_fn = build_binop!("&", build_and);
        let bxo_llvm_fn = build_binop!("^", build_xor);
        let bwo_llvm_fn = build_binop!("|", build_or);
        let les_llvm_fn = build_cmpop!("<", IntPredicate::SLT);
        let leq_llvm_fn = build_cmpop!("<=", IntPredicate::SLE);
        let gre_llvm_fn = build_cmpop!(">", IntPredicate::SGT);
        let geq_llvm_fn = build_cmpop!(">=", IntPredicate::SGE);
        let eqa_llvm_fn = build_cmpop!("==", IntPredicate::EQ);
        let neq_llvm_fn = build_cmpop!("!=", IntPredicate::NE);
        let add_fn = Function::from_function(llvm_ctx, ctx, add_llvm_fn, bin_type.clone());
        let sub_fn = Function::from_function(llvm_ctx, ctx, sub_llvm_fn, bin_type.clone());
        let mul_fn = Function::from_function(llvm_ctx, ctx, mul_llvm_fn, bin_type.clone());
        let div_fn = Function::from_function(llvm_ctx, ctx, div_llvm_fn, bin_type.clone());
        let mod_fn = Function::from_function(llvm_ctx, ctx, mod_llvm_fn, bin_type.clone());
        let lsh_fn = Function::from_function(llvm_ctx, ctx, lsh_llvm_fn, bin_type.clone());
        let rsh_fn = Function::from_function(llvm_ctx, ctx, rsh_llvm_fn, bin_type.clone());
        let bwa_fn = Function::from_function(llvm_ctx, ctx, bwa_llvm_fn, bin_type.clone());
        let bxo_fn = Function::from_function(llvm_ctx, ctx, bxo_llvm_fn, bin_type.clone());
        let bwo_fn = Function::from_function(llvm_ctx, ctx, bwo_llvm_fn, bin_type.clone());
        let les_fn = Function::from_function(llvm_ctx, ctx, les_llvm_fn, cmp_type.clone());
        let leq_fn = Function::from_function(llvm_ctx, ctx, leq_llvm_fn, cmp_type.clone());
        let gre_fn = Function::from_function(llvm_ctx, ctx, gre_llvm_fn, cmp_type.clone());
        let geq_fn = Function::from_function(llvm_ctx, ctx, geq_llvm_fn, cmp_type.clone());
        let eqa_fn = Function::from_function(llvm_ctx, ctx, eqa_llvm_fn, cmp_type.clone());
        let neq_fn = Function::from_function(llvm_ctx, ctx, neq_llvm_fn, cmp_type.clone());
        builder.add_static("+".to_string(), ValueEnum::Function(add_fn));
        builder.add_static("-".to_string(), ValueEnum::Function(sub_fn));
        builder.add_static("*".to_string(), ValueEnum::Function(mul_fn));
        builder.add_static("/".to_string(), ValueEnum::Function(div_fn));
        builder.add_static("%".to_string(), ValueEnum::Function(mod_fn));
        builder.add_static("<<".to_string(), ValueEnum::Function(lsh_fn));
        builder.add_static(">>".to_string(), ValueEnum::Function(rsh_fn));
        builder.add_static("&".to_string(), ValueEnum::Function(bwa_fn));
        builder.add_static("^".to_string(), ValueEnum::Function(bxo_fn));
        builder.add_static("|".to_string(), ValueEnum::Function(bwo_fn));
        builder.add_static("<".to_string(), ValueEnum::Function(les_fn));
        builder.add_static("<=".to_string(), ValueEnum::Function(leq_fn));
        builder.add_static(">".to_string(), ValueEnum::Function(gre_fn));
        builder.add_static(">=".to_string(), ValueEnum::Function(geq_fn));
        builder.add_static("==".to_string(), ValueEnum::Function(eqa_fn));
        builder.add_static("!=".to_string(), ValueEnum::Function(neq_fn));

        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Int<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        _ptr_type: TypeID,
        name: String,
    ) -> Self {
        Int::new(val.into_int_value())
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self {
        Int::from_val(ctx, other.get_value(), other.get_type(ctx), name)
    }
}

impl<'ctx> Literal<'ctx> for Int<'ctx> {
    type LiteralType = u32;
    type Repr = IntValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, value: Self::LiteralType, _name: String) -> Self {
        Int::new(ctx.int(value as u64))
    }

    fn raw(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Self::Repr {
        self.val
    }
}
