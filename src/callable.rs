use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType},
    values::{BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue},
};

use crate::{
    context::LanguageContext,
    types::{BasicBuiltin, Metatype, MetatypeBuilder, TypeID},
    unit::Unit,
    value::{Copyable, Field, Value, ValueEnum, ValueStatic},
};

pub trait Callable<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool;
    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: String,
    ) -> ValueEnum<'ctx>;
    fn args(&self) -> Vec<TypeID>;
    fn args_meta(&self, ctx: &LanguageContext<'ctx>) -> Vec<Metatype<'ctx>> {
        self.args().iter().map(|a| ctx.get(a.clone())).collect()
    }
    fn returns(&self) -> TypeID;
    fn returns_meta(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get(self.returns())
    }
}

#[derive(Clone, Debug)]
pub struct Function<'ctx> {
    name: String,
    metatype: TypeID,
    pub ptr: PointerValue<'ctx>,
}

impl<'ctx> Function<'ctx> {
    pub fn new(
        ctx: &LanguageContext<'ctx>,
        fn_ptr: PointerValue<'ctx>,
        typ: TypeID,
        name: String,
    ) -> Self {
        Self {
            name,
            metatype: typ,
            ptr: fn_ptr,
        }
    }

    pub fn from_function(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        fn_val: FunctionValue<'ctx>,
        typ: TypeID,
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
    fn verify(&self, args: Vec<TypeID>) -> bool {
        self.args() == args
    }

    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: String,
    ) -> ValueEnum<'ctx> {
        assert!(self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()));

        let arg_ptrs: Vec<BasicMetadataValueEnum<'ctx>> = args
            .into_iter()
            .map(|arg| BasicMetadataValueEnum::try_from(arg.get_value()).unwrap())
            .collect();
        let params: Vec<BasicMetadataTypeEnum<'ctx>> = self
            .args()
            .into_iter()
            .map(|a| BasicMetadataTypeEnum::try_from(ctx.get_storage(a.clone())).unwrap())
            .collect();
        let fn_type = ctx.get_storage(self.returns()).fn_type(&params, false);
        let result = ctx
            .builder
            .build_indirect_call(fn_type, self.ptr, &arg_ptrs, &into_name)
            .unwrap()
            .try_as_basic_value();

        if result.is_basic() {
            ValueEnum::from_val(
                ctx,
                result.expect_basic("Function return type is not a value?"),
                self.returns(),
                format!("{}_returns", self.name),
            )
        } else {
            assert_eq!(self.returns_meta(ctx).base, BasicBuiltin::Unit);
            ValueEnum::Unit(Unit {})
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
    fn member(
        &self,
        _ctx: &LanguageContext<'ctx>,
        _name: String,
        _into: String,
    ) -> ValueEnum<'ctx> {
        panic!()
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.ptr)
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

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Function,
            TypeID::new("Function".to_string(), generics.clone()),
            None,
            BasicTypeEnum::PointerType(ctx.types.ptr),
            false,
        );
        builder.build(llvm_ctx, ctx, generics);
    }
}

impl<'ctx> Copyable<'ctx> for Function<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        ptr: BasicValueEnum<'ctx>,
        ptr_type: TypeID,
        name: String,
    ) -> Self {
        Self::new(ctx, ptr.into_pointer_value(), ptr_type, name)
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self {
        Self::from_val(
            ctx,
            BasicValueEnum::PointerValue(other.ptr),
            other.get_type(ctx),
            name,
        )
    }
}
