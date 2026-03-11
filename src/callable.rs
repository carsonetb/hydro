use inkwell::{
    context::Context,
    types::{BasicMetadataTypeEnum, BasicTypeEnum, StructType},
    values::{BasicMetadataValueEnum, FunctionValue, PointerValue},
};

use crate::{
    context::LanguageContext,
    types::{BasicType, Metatype, MetatypeBuilder, TypeID},
    unit::Unit,
    value::{Copyable, Field, Value, ValuePtr, ValueStatic},
};

pub trait Callable<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool;
    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValuePtr<'ctx>>,
        into_name: String,
    ) -> ValuePtr<'ctx>;
    fn args(&self) -> Vec<TypeID>;
    fn args_meta(&self, ctx: &LanguageContext<'ctx>) -> Vec<Metatype<'ctx>> {
        self.args().iter().map(|a| ctx.get(a.clone())).collect()
    }
    fn returns(&self) -> TypeID;
    fn returns_meta(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get(self.returns())
    }
}

#[derive(Clone)]
pub struct Function<'ctx> {
    name: String,
    metatype: TypeID,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Function<'ctx> {
    fn new_with_struct(
        ctx: &LanguageContext<'ctx>,
        type_struct: StructType<'ctx>,
        fn_ptr: PointerValue<'ctx>,
        typ: TypeID,
        name: String,
    ) -> Self {
        let ptr = ctx
            .builder
            .build_alloca(type_struct, &format!("FN__{name}_ptr"))
            .unwrap();
        let value_ptr = ctx
            .builder
            .build_struct_gep(type_struct, ptr, 0, &format!("FN__{name}_raw_ptr"))
            .unwrap();
        ctx.builder.build_store(value_ptr, fn_ptr).unwrap();
        Self {
            name,
            metatype: typ,
            ptr,
        }
    }

    pub fn new(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        fn_ptr: PointerValue<'ctx>,
        typ: TypeID,
        name: String,
    ) -> Self {
        let struct_type = ctx.get_struct_with_gen(llvm_ctx, typ.clone());
        Self::new_with_struct(ctx, struct_type, fn_ptr, typ, name)
    }

    pub fn from_function(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        fn_val: FunctionValue<'ctx>,
        typ: TypeID,
    ) -> Self {
        Self::new(
            llvm_ctx,
            ctx,
            fn_val.as_global_value().as_pointer_value(),
            typ,
            fn_val.get_name().to_str().unwrap().to_owned(),
        )
    }
}

impl<'ctx> Callable<'ctx> for Function<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool {
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

        if result.is_basic() {
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
            assert_eq!(self.returns_meta(ctx).base, BasicType::Unit);
            ValuePtr::PUnit(Unit {})
        }
    }

    fn args(&self) -> Vec<TypeID> {
        self.metatype.generics[0].generics.clone()
    }

    fn returns(&self) -> TypeID {
        self.metatype.generics[1].clone()
    }
}

impl<'ctx> Value<'ctx> for Function<'ctx> {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        Option::<&Field<'ctx>>::None
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.ptr
    }
}

impl<'ctx> ValueStatic<'ctx> for Function<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 2);
        assert_eq!(generics[0].base, "Tuple");

        let type_name = TypeID::new("Function".to_string(), generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(&type_name.name().as_str());
        obj_struct.set_body(&[BasicTypeEnum::PointerType(ctx.types.ptr)], false);

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicType::Function,
            TypeID::new("Function".to_string(), generics.clone()),
            obj_struct,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Function<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        ptr_type: TypeID,
        this_name: String,
        other_name: String,
    ) -> Self {
        let fn_struct = ctx.get_struct(ptr_type.clone());
        let fn_ptr_ptr = ctx
            .builder
            .build_struct_gep(fn_struct, ptr, 0, &format!("{this_name}_raw_ptr"))
            .unwrap();
        let fn_ptr = ctx
            .builder
            .build_load(ctx.types.ptr, fn_ptr_ptr, &format!("{this_name}_raw"))
            .unwrap()
            .into_pointer_value();
        Self::new_with_struct(
            ctx,
            ctx.get_struct(ptr_type.clone()),
            fn_ptr,
            ptr_type,
            other_name,
        )
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
