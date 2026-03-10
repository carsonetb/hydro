use inkwell::{
    context::Context,
    types::{BasicMetadataTypeEnum, BasicTypeEnum},
    values::{BasicMetadataValueEnum, FunctionValue, PointerValue},
};

use crate::{
    context::LanguageContext,
    types::{BasicType, Metatype, MetatypeBuilder},
    unit::Unit,
    value::{Copyable, Field, Value, ValuePtr, ValueStatic},
};

pub trait Callable<'ctx> {
    fn verify(&self, args: Vec<Metatype<'ctx>>) -> bool;
    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValuePtr<'ctx>>,
        into_name: String,
    ) -> ValuePtr<'ctx>;
    fn args(&self) -> Vec<Metatype<'ctx>>;
    fn returns(&self) -> Metatype<'ctx>;
}

#[derive(Clone)]
pub struct Function<'ctx> {
    name: String,
    metatype: Metatype<'ctx>,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Function<'ctx> {
    pub fn new(
        ctx: &LanguageContext<'ctx>,
        fn_ptr: PointerValue<'ctx>,
        typ: Metatype<'ctx>,
        name: String,
    ) -> Self {
        let ptr = ctx
            .builder
            .build_alloca(typ.obj_struct, &format!("{name}_ptr"))
            .unwrap();
        let value_ptr = ctx
            .builder
            .build_struct_gep(typ.obj_struct, ptr, 0, &format!("{name}_value_ptr"))
            .unwrap();
        ctx.builder.build_store(value_ptr, fn_ptr).unwrap();
        Self {
            name,
            metatype: typ,
            ptr,
        }
    }

    pub fn from_function(
        ctx: &LanguageContext<'ctx>,
        fn_val: FunctionValue<'ctx>,
        typ: Metatype<'ctx>,
    ) -> Self {
        Self::new(
            ctx,
            fn_val.as_global_value().as_pointer_value(),
            typ,
            fn_val.get_name().to_str().unwrap().to_owned(),
        )
    }
}

impl<'ctx> Callable<'ctx> for Function<'ctx> {
    fn verify(&self, args: Vec<Metatype<'ctx>>) -> bool {
        self.args() == args
    }

    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValuePtr<'ctx>>,
        into_name: String,
    ) -> ValuePtr<'ctx> {
        assert!(self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()));

        let arg_ptrs: Vec<BasicMetadataValueEnum> = args
            .iter()
            .map(|arg| BasicMetadataValueEnum::PointerValue(arg.get_ptr()))
            .collect();
        let params = vec![BasicMetadataTypeEnum::PointerType(ctx.types.ptr); self.args().len()];
        let fn_type = ctx.types.ptr.fn_type(&params, false);
        let result = ctx
            .builder
            .build_indirect_call(fn_type, self.ptr, &arg_ptrs, &into_name)
            .unwrap()
            .try_as_basic_value();

        if result.is_instruction() {
            ValuePtr::from_ptr(
                ctx,
                result
                    .expect_basic("Function return type is not a value?")
                    .into_pointer_value(),
                self.returns(),
                format!("{}_returns", self.name),
                into_name,
            )
        } else {
            assert_eq!(self.returns().base, BasicType::Unit);
            ValuePtr::PUnit(Unit {})
        }
    }

    fn args(&self) -> Vec<Metatype<'ctx>> {
        todo!()
    }

    fn returns(&self) -> Metatype<'ctx> {
        todo!()
    }
}

impl<'ctx> Value<'ctx> for Function<'ctx> {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        Option::<&Field<'ctx>>::None
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        self.metatype.clone()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.ptr
    }
}

impl<'ctx> ValueStatic<'ctx> for Function<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        generics: Vec<Metatype<'ctx>>,
    ) -> Metatype<'ctx> {
        assert_eq!(generics.len(), 0);
        assert_eq!(generics[0].class_name, "Tuple");

        let type_name = Metatype::gen_name("Function".to_string(), &generics);
        let obj_struct = llvm_ctx.opaque_struct_type(&type_name);
        obj_struct.set_body(&[BasicTypeEnum::PointerType(ctx.types.ptr)], false);

        let mut builder =
            MetatypeBuilder::new(BasicType::Function, "Function".to_string(), obj_struct);
        builder.build(llvm_ctx, ctx, generics)
    }
}

impl<'ctx> Copyable<'ctx> for Function<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        ptr_type: Metatype<'ctx>,
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
            .into_pointer_value();
        Self::new(ctx, value, ptr_type, other_name)
    }

    fn from(
        ctx: &LanguageContext<'ctx>,
        other: Self,
        this_name: String,
        other_name: String,
    ) -> Self {
        Self::from_ptr(ctx, other.ptr, other.get_type(ctx), this_name, other_name)
    }
}
