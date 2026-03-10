use inkwell::{
    context::Context,
    types::{BasicMetadataTypeEnum, BasicTypeEnum},
    values::{BasicMetadataValueEnum, FunctionValue, PointerValue},
};

use crate::{
    context::LanguageContext,
    types::{BasicType, Metatype, MetatypeBuilder, TypeId},
    unit::Unit,
    value::{Copyable, Field, Value, ValuePtr, ValueStatic},
};

pub trait Callable<'ctx> {
    fn verify(&self, args: Vec<TypeId>) -> bool;
    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValuePtr<'ctx>>,
        into_name: String,
    ) -> ValuePtr<'ctx>;
    fn args(&self) -> Vec<TypeId>;
    fn args_meta(&self, ctx: &LanguageContext<'ctx>) -> Vec<Metatype<'ctx>> {
        self.args().iter().map(|a| ctx.get(a.clone())).collect()
    }
    fn returns(&self) -> TypeId;
    fn returns_meta(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get(self.returns())
    }
}

#[derive(Clone)]
pub struct Function<'ctx> {
    name: String,
    metatype: TypeId,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Function<'ctx> {
    pub fn new(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        fn_ptr: PointerValue<'ctx>,
        typ: TypeId,
        name: String,
    ) -> Self {
        let typ_struct = ctx.get_struct_with_gen(llvm_ctx, typ.clone());
        let ptr = ctx
            .builder
            .build_alloca(typ_struct, &format!("{name}_ptr"))
            .unwrap();
        let value_ptr = ctx
            .builder
            .build_struct_gep(typ_struct, ptr, 0, &format!("{name}_value_ptr"))
            .unwrap();
        ctx.builder.build_store(value_ptr, fn_ptr).unwrap();
        Self {
            name,
            metatype: typ,
            ptr,
        }
    }

    pub fn from_function(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        fn_val: FunctionValue<'ctx>,
        typ: TypeId,
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
    fn verify(&self, args: Vec<TypeId>) -> bool {
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
            assert_eq!(self.returns_meta(ctx).base, BasicType::Unit);
            ValuePtr::PUnit(Unit {})
        }
    }

    fn args(&self) -> Vec<TypeId> {
        todo!()
    }

    fn returns(&self) -> TypeId {
        todo!()
    }
}

impl<'ctx> Value<'ctx> for Function<'ctx> {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        Option::<&Field<'ctx>>::None
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeId {
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
        generics: Vec<TypeId>,
    ) {
        assert_eq!(generics.len(), 2);
        assert_eq!(generics[0].base, "Tuple");

        let type_name = TypeId::new("Function".to_string(), generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(&type_name.name().as_str());
        obj_struct.set_body(&[BasicTypeEnum::PointerType(ctx.types.ptr)], false);

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicType::Function,
            TypeId::from_base("Function".to_string()),
            obj_struct,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Function<'ctx> {
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        ptr_type: TypeId,
        this_name: String,
        other_name: String,
    ) -> Self {
        todo!();
        // let value_ptr = ctx
        //     .builder
        //     .build_struct_gep(
        //         ctx.types.int_struct,
        //         ptr,
        //         0,
        //         &format!("{this_name}_raw_ptr"),
        //     )
        //     .unwrap();
        // let value = ctx
        //     .builder
        //     .build_load(ctx.types.int, value_ptr, &format!("{this_name}_raw"))
        //     .unwrap()
        //     .into_pointer_value();
        // Self::new(ctx, value, ptr_type, other_name)
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
