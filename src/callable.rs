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
    value::{Copyable, Field, Value, ValueEnum, ValueRef, ValueStatic, any_to_basic},
};

pub trait Callable<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool;
    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: &str,
    ) -> Result<ValueEnum<'ctx>, String>;
    fn args(&self) -> Vec<TypeID>;
    fn returns(&self) -> TypeID;
    fn call_basic(
        &self,
        ctx: &LanguageContext<'ctx>,
        fn_name: &str,
        fn_ptr: PointerValue<'ctx>,
        args: &Vec<ValueEnum<'ctx>>,
        into_name: &str,
    ) -> Result<ValueEnum<'ctx>, String> {
        let arg_ptrs: Vec<BasicMetadataValueEnum<'ctx>> = args
            .into_iter()
            .map(|arg| BasicMetadataValueEnum::try_from(arg.get_value()).unwrap())
            .collect();
        let params: Vec<BasicMetadataTypeEnum<'ctx>> = args
            .into_iter()
            .map(|a| ctx.get_storage(a.get_type(ctx)).into())
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
                &format!("{}_returns", fn_name),
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
        name: &str,
    ) -> Self {
        assert!(typ.generics.len() == 2);
        assert!(typ.generics[0].base == "Tuple".to_string());
        Self {
            name: name.to_string(),
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
            &fn_val.get_name().to_str().unwrap().to_owned(),
        )
    }

    pub fn to_member_function(
        &self,
        ctx: &LanguageContext<'ctx>,
        bound: BasicValueEnum<'ctx>,
        name: &str,
    ) -> MemberFunction<'ctx> {
        let typ = TypeID::new(
            "MemberFunction",
            vec![
                self.args()[0].clone(),
                TypeID::new("Tuple", self.args()[1..].to_vec()),
                self.returns(),
            ],
        );
        let fn_struct = ctx.get_struct(typ.clone()).get_undef();
        let fn_struct = ctx
            .builder
            .build_insert_value(fn_struct, bound, 0, &format!("{name}_bound"))
            .unwrap();
        let fn_struct = ctx
            .builder
            .build_insert_value(fn_struct, self.ptr, 1, &format!("{name}_fn"))
            .unwrap();
        MemberFunction::new(ctx, fn_struct.into_struct_value(), typ, name)
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
        into_name: &str,
    ) -> Result<ValueEnum<'ctx>, String> {
        if !self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()) {
            return Err("Arguments to this function are incorrect.".to_string()); // TODO: Improve this error.
        }

        self.call_basic(ctx, &self.name, self.ptr, &args, into_name)
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
        _into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            &format!("Function types have no members!"),
        ))
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

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.ptr)
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
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

        let type_name = TypeID::new("Function", generics.clone());

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Function,
            TypeID::new("Function", generics.clone()),
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
        name: &str,
    ) -> Self {
        Self::new(ctx, ptr.into_pointer_value(), ptr_type, name)
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        Self::from_val(
            ctx,
            BasicValueEnum::PointerValue(other.ptr),
            other.get_type(ctx),
            name,
        )
    }

    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        Self::from_val(ctx, ptr.into(), typ, into_name)
    }
}

#[derive(Clone, Debug)]
pub struct MemberFunction<'ctx> {
    name: String,
    metatype: TypeID,
    pub val: StructValue<'ctx>,
}

impl<'ctx> MemberFunction<'ctx> {
    pub fn new(
        ctx: &LanguageContext<'ctx>,
        val: StructValue<'ctx>,
        typ: TypeID,
        name: &str,
    ) -> Self {
        assert!(typ.generics.len() == 3);
        assert!(typ.generics[1].base == "Tuple".to_string());

        Self {
            name: name.to_string(),
            metatype: typ,
            val,
        }
    }

    pub fn wrap_function(
        ctx: &LanguageContext<'ctx>,
        typ: TypeID,
        fn_name: &str,
        val: BasicValueEnum<'ctx>,
        name: &str,
    ) -> Self {
        let fn_struct = ctx.get_struct(typ.clone()).get_undef();
        let fn_struct = ctx
            .builder
            .build_insert_value(fn_struct, val, 0, &format!("{name}_bound"))
            .unwrap();
        let fn_struct = ctx
            .builder
            .build_insert_value(
                fn_struct,
                ctx.module
                    .get_function(fn_name)
                    .unwrap()
                    .as_global_value()
                    .as_pointer_value(),
                1,
                &format!("{name}_fn"),
            )
            .unwrap();
        Self::new(ctx, fn_struct.into_struct_value(), typ, name)
    }

    pub fn get_bound(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> ValueEnum<'ctx> {
        ValueEnum::from_val(
            ctx,
            ctx.builder
                .build_extract_value(self.val, 0, into_name)
                .unwrap()
                .as_basic_value_enum(),
            self.metatype.generics[0].clone(),
            into_name,
        )
    }
}

impl<'ctx> Callable<'ctx> for MemberFunction<'ctx> {
    fn verify(&self, args: Vec<TypeID>) -> bool {
        args == self.args()
    }

    fn call(
        &self,
        ctx: &LanguageContext<'ctx>,
        args: Vec<ValueEnum<'ctx>>,
        into_name: &str,
    ) -> Result<ValueEnum<'ctx>, String> {
        if !self.verify(args.iter().map(|arg| arg.get_type(ctx)).collect()) {
            return Err("Arguments to this function are incorrect.".to_string()); // TODO: Improve this error.
        }

        let mut args_with_bound =
            vec![self.get_bound(ctx, format!("{}_bound", into_name).as_str())];
        args_with_bound.extend(args);

        let fn_ptr = ctx
            .builder
            .build_extract_value(self.val, 1, &format!("{}_callee", into_name))
            .unwrap()
            .into_pointer_value();
        self.call_basic(ctx, &self.name, fn_ptr, &args_with_bound, into_name)
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
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        Err(CompileError::new(
            name.span,
            &format!("Function types have no members!"),
        ))
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
        self.metatype.clone()
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        self.val.as_basic_value_enum()
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        todo!()
    }
}

impl<'ctx> ValueStatic<'ctx> for MemberFunction<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 3);
        assert_eq!(generics[1].base, "Tuple");

        let type_name = TypeID::new("MemberFunction", generics.clone());
        let obj_struct = llvm_ctx.opaque_struct_type(&type_name.to_string());

        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::MemberFunction,
            TypeID::new("MemberFunction", generics.clone()),
            Some(obj_struct),
            AnyTypeEnum::StructType(obj_struct),
            false,
        );
        builder.build(llvm_ctx, ctx, generics.clone());

        obj_struct.set_body(
            &[
                any_to_basic(ctx.get(generics[0].clone()).storage_type).unwrap(),
                ctx.types.ptr.as_basic_type_enum(),
            ],
            false,
        );
    }
}

impl<'ctx> Copyable<'ctx> for MemberFunction<'ctx> {
    fn from_val(
        ctx: &LanguageContext<'ctx>,
        val: BasicValueEnum<'ctx>,
        val_type: TypeID,
        name: &str,
    ) -> Self {
        Self::new(ctx, val.into_struct_value(), val_type, name)
    }

    fn from(ctx: &LanguageContext<'ctx>, other: Self, name: &str) -> Self {
        Self::from_val(
            ctx,
            BasicValueEnum::StructValue(other.val),
            other.get_type(ctx),
            name,
        )
    }
    fn from_ptr(
        ctx: &LanguageContext<'ctx>,
        ptr: PointerValue<'ctx>,
        typ: TypeID,
        into_name: &str,
    ) -> Self {
        todo!()
    }
}
