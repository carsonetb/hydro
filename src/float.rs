use std::ops::Range;

use crate::{
    callable::{Function, MemberFunction},
    codegen::CompileError,
    context::{LLVMTypes, LanguageContext},
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueRef, ValueStatic},
};
use chumsky::span::{SimpleSpan, Span, Spanned, WrappingSpan};
use inkwell::{
    FloatPredicate, IntPredicate,
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType},
    values::{
        AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue,
        IntValue, PointerValue, StructValue,
    },
};

#[derive(Clone, Debug)]
pub struct Float<'ctx> {
    pub val: FloatValue<'ctx>,
}

impl<'ctx> Float<'ctx> {
    pub fn new(value: FloatValue<'ctx>) -> Self {
        Self { val: value }
    }

    pub fn build_binop(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        op_builder: impl Fn(FloatValue<'ctx>, FloatValue<'ctx>) -> BasicValueEnum<'ctx>,
        op_name: &str,
        boolean: bool,
    ) -> FunctionValue<'ctx> {
        let ret: BasicTypeEnum = if boolean {
            ctx.types.bool.into()
        } else {
            ctx.types.float.into()
        };
        let add_llvm_type = ret.fn_type(&[ctx.types.float.into(); 2], false);
        let add_llvm_fn =
            ctx.module
                .add_function(format!("Float.{op_name}").as_str(), add_llvm_type, None);
        let entry = llvm_ctx.append_basic_block(add_llvm_fn, "entry");
        let old_block = ctx.builder.get_insert_block().unwrap();
        ctx.builder.position_at_end(entry);

        let left = add_llvm_fn.get_nth_param(0).unwrap().into_float_value();
        let right = add_llvm_fn.get_nth_param(1).unwrap().into_float_value();
        left.set_name("lhs");
        right.set_name("rhs");
        ctx.builder
            .build_return(Some(&op_builder(left, right)))
            .unwrap();
        ctx.builder.position_at_end(old_block);

        add_llvm_fn
    }
}

impl<'ctx> Value<'ctx> for Float<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        let bin_type = TypeID::new(
            "Function",
            vec![
                TypeID::new("Tuple", vec![TypeID::from_base("Float"); 2]),
                TypeID::from_base("Float"),
            ],
        );
        let mut cmp_type = bin_type.clone();
        cmp_type.generics[1] = TypeID::from_base("Bool");

        let float_type = TypeID::from_base("Float");
        let to_string_type = TypeID::new(
            "MemberFunction",
            vec![
                float_type.clone(),
                TypeID::new("Tuple", vec![]),
                TypeID::from_base("String"),
            ],
        );

        macro_rules! op_fun_wrapper {
            ($op_name:expr, $fn_name:expr, $ty:expr) => {
                Ok(ValueEnum::Function(Function::new(
                    ctx,
                    ctx.module
                        .get_function($fn_name)
                        .unwrap()
                        .as_global_value()
                        .as_pointer_value(),
                    $ty,
                    $op_name,
                )))
            };
        }

        match &name.inner[..] {
            "+" => op_fun_wrapper!("+", "Float.+", bin_type),
            "-" => op_fun_wrapper!("-", "Float.-", bin_type),
            "*" => op_fun_wrapper!("*", "Float.*", bin_type),
            "/" => op_fun_wrapper!("/", "Float./", bin_type),
            "%" => op_fun_wrapper!("%", "Float.%", bin_type),
            ">" => op_fun_wrapper!(">", "Float.>", cmp_type),
            "<" => op_fun_wrapper!("<", "Float.<", cmp_type),
            "<=" => op_fun_wrapper!("<=", "Float.<=", cmp_type),
            ">=" => op_fun_wrapper!(">=", "Float.>=", cmp_type),
            "==" => op_fun_wrapper!("==", "Float.==", cmp_type),
            "!=" => op_fun_wrapper!("!=", "Float.!=", cmp_type),
            "to_string" => Ok(ValueEnum::MemberFunction(MemberFunction::wrap_function(
                ctx,
                to_string_type,
                "Float__to_string",
                self.val.as_basic_value_enum(),
                into,
            ))),
            _ => Err(CompileError::new(
                name.span,
                &format!("Type `Float` has no `{}` member.", name.inner),
            )),
        }
    }

    fn member_ref(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueRef<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            "Cannot get this member as a reference.",
        ))
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Float")
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        self.val.into()
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        let mem = ctx
            .builder
            .build_alloca(ctx.types.float, &format!("{into_name}_ptr"))
            .unwrap();
        ctx.builder.build_store(mem, self.val);
        mem
    }
}

impl<'ctx> ValueStatic<'ctx> for Float<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
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
                            .into()
                    },
                    $op_name_str,
                    false,
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
                            .build_float_compare($predicate, left, right, "result")
                            .unwrap()
                            .into()
                    },
                    $op_name_str,
                    true,
                )
            };
        }

        let typeid = TypeID::from_base("Float");
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Float,
            typeid.clone(),
            None,
            ctx.types.float.into(),
            false,
        );

        let bin_type = TypeID::new(
            "Function",
            vec![
                TypeID::new("Tuple", vec![typeid.clone(); 2]),
                typeid.clone(),
            ],
        );
        let cmp_type = TypeID::new(
            "Function",
            vec![
                TypeID::new("Tuple", vec![typeid.clone(); 2]),
                TypeID::from_base("Bool"),
            ],
        );
        let add_llvm_fn = build_binop!("+", build_float_add);
        let sub_llvm_fn = build_binop!("-", build_float_sub);
        let mul_llvm_fn = build_binop!("*", build_float_mul);
        let div_llvm_fn = build_binop!("/", build_float_div);
        let mod_llvm_fn = build_binop!("%", build_float_rem);
        let les_llvm_fn = build_cmpop!("<", FloatPredicate::OLT);
        let leq_llvm_fn = build_cmpop!("<=", FloatPredicate::OLE);
        let gre_llvm_fn = build_cmpop!(">", FloatPredicate::OGT);
        let geq_llvm_fn = build_cmpop!(">=", FloatPredicate::OGE);
        let eqa_llvm_fn = build_cmpop!("==", FloatPredicate::OEQ);
        let neq_llvm_fn = build_cmpop!("!=", FloatPredicate::ONE);
        let add_fn = Function::from_function(llvm_ctx, ctx, add_llvm_fn, bin_type.clone());
        let sub_fn = Function::from_function(llvm_ctx, ctx, sub_llvm_fn, bin_type.clone());
        let mul_fn = Function::from_function(llvm_ctx, ctx, mul_llvm_fn, bin_type.clone());
        let div_fn = Function::from_function(llvm_ctx, ctx, div_llvm_fn, bin_type.clone());
        let mod_fn = Function::from_function(llvm_ctx, ctx, mod_llvm_fn, bin_type.clone());
        let les_fn = Function::from_function(llvm_ctx, ctx, les_llvm_fn, cmp_type.clone());
        let leq_fn = Function::from_function(llvm_ctx, ctx, leq_llvm_fn, cmp_type.clone());
        let gre_fn = Function::from_function(llvm_ctx, ctx, gre_llvm_fn, cmp_type.clone());
        let geq_fn = Function::from_function(llvm_ctx, ctx, geq_llvm_fn, cmp_type.clone());
        let eqa_fn = Function::from_function(llvm_ctx, ctx, eqa_llvm_fn, cmp_type.clone());
        let neq_fn = Function::from_function(llvm_ctx, ctx, neq_llvm_fn, cmp_type.clone());
        builder.add_static("+", ValueEnum::Function(add_fn));
        builder.add_static("-", ValueEnum::Function(sub_fn));
        builder.add_static("*", ValueEnum::Function(mul_fn));
        builder.add_static("/", ValueEnum::Function(div_fn));
        builder.add_static("%", ValueEnum::Function(mod_fn));
        builder.add_static("<", ValueEnum::Function(les_fn));
        builder.add_static("<=", ValueEnum::Function(leq_fn));
        builder.add_static(">", ValueEnum::Function(gre_fn));
        builder.add_static(">=", ValueEnum::Function(geq_fn));
        builder.add_static("==", ValueEnum::Function(eqa_fn));
        builder.add_static("!=", ValueEnum::Function(neq_fn));

        builder.build(llvm_ctx, ctx, generics);

        let to_string_type = TypeID::new(
            "MemberFunction",
            vec![
                TypeID::from_base("Float"),
                TypeID::new("Tuple", vec![]),
                TypeID::from_base("String"),
            ],
        );
        ctx.get_with_gen_ext(to_string_type.clone());
    }
}

impl<'ctx> Copyable<'ctx> for Float<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        _ptr_type: TypeID,
        name: &str,
    ) -> Self {
        Self::new(val.into_float_value())
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        Self::from_val(ctx, other.get_value(), other.get_type(ctx), name)
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        Self::new(
            ctx.builder
                .build_load(ctx.types.int, ptr, into_name)
                .unwrap()
                .into_float_value(),
        )
    }
}

impl<'ctx> Literal<'ctx> for Float<'ctx> {
    type LiteralType = f32;
    type Repr = FloatValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, value: Self::LiteralType, _name: &str) -> Self {
        Self::new(ctx.types.float.const_float(value as f64))
    }

    fn raw(&self, _ctx: &LanguageContext<'ctx>, _name: &str) -> Self::Repr {
        self.val
    }
}
