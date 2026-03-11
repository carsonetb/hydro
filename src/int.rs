use crate::{
    callable::Function,
    context::{LLVMTypes, LanguageContext},
    types::{BasicType, Metatype, MetatypeBuilder, TypeID},
    value::{Copyable, Field, Literal, Value, ValuePtr, ValueStatic},
};
use inkwell::{
    context::Context,
    types::StructType,
    values::{IntValue, PointerValue},
};

#[derive(Clone)]
pub struct Int<'ctx> {
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Int<'ctx> {
    pub fn new(ctx: &LanguageContext<'ctx>, value: IntValue<'ctx>, name: String) -> Self {
        let ptr = ctx
            .builder
            .build_alloca(ctx.types.int_struct, &format!("{name}_ptr"))
            .unwrap();
        let value_ptr = ctx
            .builder
            .build_struct_gep(ctx.types.int_struct, ptr, 0, &format!("{name}_value_ptr"))
            .unwrap();
        ctx.builder.build_store(value_ptr, value).unwrap();
        Self { ptr }
    }

    pub fn init_body(types: &LLVMTypes<'ctx>, empty: StructType<'ctx>) {
        empty.set_body(&[types.int_enum()], false);
    }
}

impl<'ctx> Value<'ctx> for Int<'ctx> {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        Option::<&Field<'ctx>>::None
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Int".to_string())
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.ptr
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
            BasicType::Int,
            TypeID::from_base("Int".to_string()),
            ctx.types.int_struct,
        );

        let add_llvm_type = ctx.function(2);
        let add_llvm_fn = ctx.module.add_function("Int__+", add_llvm_type, None);
        let entry = llvm_ctx.append_basic_block(add_llvm_fn, "entry");
        let old_block = ctx.builder.get_insert_block().unwrap();
        ctx.builder.position_at_end(entry);

        let params: Vec<Int<'ctx>> = add_llvm_fn
            .get_params()
            .iter()
            .map(|p| {
                Int::from_ptr(
                    ctx,
                    p.into_pointer_value(),
                    TypeID::from_base("Int".to_string()),
                    "arg".to_string(),
                    "arg_storage".to_string(),
                )
            })
            .collect();
        let result = ctx
            .builder
            .build_int_add(params[0].raw(ctx), params[1].raw(ctx), "add_result")
            .unwrap();
        let as_int = Int::new(ctx, result, "result_ptr".to_string());
        ctx.builder.build_return(Some(&as_int.get_ptr())).unwrap();
        ctx.builder.position_at_end(old_block);

        let add_type = TypeID::new(
            "Function".to_string(),
            vec![
                TypeID::new(
                    "Tuple".to_string(),
                    vec![TypeID::from_base("Int".to_string()); 2],
                ),
                TypeID::from_base("Int".to_string()),
            ],
        );
        let add_fn = Function::from_function(llvm_ctx, ctx, add_llvm_fn, add_type);
        builder.add_static("+".to_string(), ValuePtr::PFunction(add_fn));

        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Int<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        _ptr_type: TypeID,
        this_name: String,
        other_name: String,
    ) -> Self {
        let value_ptr = ctx
            .builder
            .build_struct_gep(
                ctx.types.int_struct,
                ptr,
                0,
                &format!("{this_name}_raw_ptr"),
            )
            .unwrap();
        let value = ctx
            .builder
            .build_load(ctx.types.int, value_ptr, &format!("{this_name}_raw"))
            .unwrap()
            .into_int_value();
        Int::new(ctx, value, other_name)
    }

    fn from(
        ctx: &LanguageContext<'ctx>,
        other: Self,
        this_name: String,
        other_name: String,
    ) -> Self {
        Int::from_ptr(ctx, other.ptr, other.get_type(ctx), this_name, other_name)
    }
}

impl<'ctx> Literal<'ctx> for Int<'ctx> {
    type LiteralType = u32;
    type Repr = IntValue<'ctx>;

    fn from_literal(ctx: &LanguageContext<'ctx>, value: Self::LiteralType, name: String) -> Self {
        let ir_int = ctx.int(value as u64);
        Int::new(ctx, ir_int, name)
    }

    fn raw(&self, ctx: &LanguageContext<'ctx>) -> Self::Repr {
        let value_ptr = unsafe {
            ctx.builder
                .build_gep(
                    ctx.types.int_struct,
                    self.ptr,
                    &[ctx.int(0), ctx.int(0)],
                    "value_ptr",
                )
                .unwrap()
        };
        ctx.builder
            .build_load(ctx.types.int, value_ptr, "int_raw")
            .unwrap()
            .into_int_value()
    }
}
