use crate::{
    callable::Function,
    context::{LLVMTypes, LanguageContext},
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Copyable, Literal, Value, ValueEnum, ValueStatic},
};
use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicTypeEnum, StructType},
    values::{BasicValueEnum, FunctionValue, IntValue},
};

#[derive(Clone, Debug)]
pub struct Int<'ctx> {
    pub val: IntValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    pub fn new(value: IntValue<'ctx>) -> Self {
        Self { val: value }
    }

    pub fn init_body(types: &LLVMTypes<'ctx>, empty: StructType<'ctx>) {
        empty.set_body(&[types.int_enum()], false);
    }

    fn build_binop(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        op_builder: impl Fn(IntValue<'ctx>, IntValue<'ctx>) -> IntValue<'ctx>,
        op_name: String,
    ) -> FunctionValue<'ctx> {
        let add_llvm_type = ctx
            .types
            .int
            .fn_type(&[BasicMetadataTypeEnum::IntType(ctx.types.int); 2], false);
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
        _ctx: &LanguageContext<'ctx>,
        _name: String,
        _into: String,
    ) -> ValueEnum<'ctx> {
        panic!()
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
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Int,
            TypeID::from_base("Int".to_string()),
            ctx.types.int_struct,
            BasicTypeEnum::IntType(ctx.types.int),
            false,
        );

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
        let add_llvm_fn = Int::build_binop(
            llvm_ctx,
            ctx,
            |left, right| ctx.builder.build_int_add(left, right, "sum").unwrap(),
            "+".to_string(),
        );
        let sub_llvm_fn = Int::build_binop(
            llvm_ctx,
            ctx,
            |left, right| ctx.builder.build_int_sub(left, right, "diff").unwrap(),
            "-".to_string(),
        );
        let mul_llvm_fn = Int::build_binop(
            llvm_ctx,
            ctx,
            |left, right| ctx.builder.build_int_mul(left, right, "product").unwrap(),
            "*".to_string(),
        );
        let div_llvm_fn = Int::build_binop(
            llvm_ctx,
            ctx,
            |left, right| {
                ctx.builder
                    .build_int_signed_div(left, right, "quotient")
                    .unwrap()
            },
            "/".to_string(),
        );
        let add_fn = Function::from_function(llvm_ctx, ctx, add_llvm_fn, bin_type.clone());
        let sub_fn = Function::from_function(llvm_ctx, ctx, sub_llvm_fn, bin_type.clone());
        let mul_fn = Function::from_function(llvm_ctx, ctx, mul_llvm_fn, bin_type.clone());
        let div_fn = Function::from_function(llvm_ctx, ctx, div_llvm_fn, bin_type.clone());
        builder.add_static("+".to_string(), ValueEnum::Function(add_fn));
        builder.add_static("-".to_string(), ValueEnum::Function(sub_fn));
        builder.add_static("*".to_string(), ValueEnum::Function(mul_fn));
        builder.add_static("/".to_string(), ValueEnum::Function(div_fn));

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
        let ir_int = ctx.int(value as u64);
        Int::new(ir_int)
    }

    fn raw(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Self::Repr {
        self.val
    }
}
