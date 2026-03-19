use chumsky::span::Spanned;
use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType},
    values::{
        BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue,
        StructValue,
    },
};

use crate::{
    codegen::CompileError,
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
    ) -> Result<ValueEnum<'ctx>, String>;
    fn args(&self) -> Vec<TypeID>;
    fn returns(&self) -> TypeID;
    fn call_basic(
        &self,
        ctx: &LanguageContext<'ctx>,
        fn_name: String,
        fn_ptr: PointerValue<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: String,
    ) -> Result<ValueEnum<'ctx>, String> {
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
            .build_indirect_call(fn_type, fn_ptr, &arg_ptrs, &into_name)
            .unwrap()
            .try_as_basic_value();

        Ok(if self.returns().base != "Unit".to_string() {
            ValueEnum::from_val(
                ctx,
                result.expect_basic("Function return type is not a value?"),
                self.returns(),
                format!("{}_returns", fn_name),
            )
        } else {
            ValueEnum::Unit(Unit {})
        })
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
        assert!(typ.generics.len() == 2);
        assert!(typ.generics[0].base == "Tuple".to_string());
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
    ) -> Result<ValueEnum<'ctx>, String> {
        if !self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()) {
            return Err("Arguments to this function are incorrect.".to_string()); // TODO: Improve this error.
        }

        self.call_basic(ctx, self.name.clone(), self.ptr, args, into_name)
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
        name: Spanned<String>,
        _into: String,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            format!("Function types have no members!"),
        ))
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
            AnyTypeEnum::PointerType(ctx.types.ptr),
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

#[derive(Clone, Debug)]
pub struct MemberFunction<'ctx> {
    name: String,
    metatype: TypeID,
    pub val: StructValue<'ctx>,
}

impl<'ctx> MemberFunction<'ctx> {
    pub fn get_bound(&self, ctx: &LanguageContext<'ctx>) -> ValueEnum<'ctx> {
        todo!()
    }
}

impl<'ctx> Callable<'ctx> for MemberFunction<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool {
        todo!()
    }

    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: String,
    ) -> Result<ValueEnum<'ctx>, String> {
        if !self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()) {
            return Err("Arguments to this function are incorrect.".to_string()); // TODO: Improve this error.
        }

        let mut args_with_bound = vec![self.get_bound(ctx)];
        args_with_bound.extend(args);

        let fn_ptr = ctx
            .builder
            .build_extract_value(self.val, 1, &format!("{}_callee", into_name))
            .unwrap()
            .into_pointer_value();
        self.call_basic(ctx, self.name.clone(), fn_ptr, args_with_bound, into_name)
    }

    fn args(&self) -> Vec<TypeID> {
        self.metatype.generics[1].generics.clone()
    }

    fn returns(&self) -> TypeID {
        self.metatype.generics[2].clone()
    }
}

impl<'ctx> Value<'ctx> for MemberFunction<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: String,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        todo!()
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        todo!()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        todo!()
    }
}

impl<'ctx> ValueStatic<'ctx> for MemberFunction<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        todo!()
    }
}

impl<'ctx> Copyable<'ctx> for MemberFunction<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: String,
    ) -> Self {
        todo!()
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: String) -> Self {
        todo!()
    }
}
