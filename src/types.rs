use std::{collections::HashMap, fmt::Display};

use chumsky::span::{SimpleSpan, Span, Spanned, WrappingSpan};
use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicTypeEnum, FunctionType, StructType},
    values::{BasicValueEnum, FunctionValue, PointerValue},
};

use crate::{
    callable::{Function, function_type},
    classes::{ClassInfo, ClassMember},
    codegen::CompileError,
    context::LanguageContext,
    value::{Field, Value, ValueEnum, ValueRef, ValueStatic, any_to_basic},
};

#[derive(Clone, Debug, PartialEq)]
pub enum BasicBuiltin {
    Unit,
    Type,
    Bool,
    Int,
    Float,
    String,
    Tuple,
    Vector,
    Function,
    MemberFunction,
    Class,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeID {
    pub base: String,
    pub generics: Vec<TypeID>,
}

impl TypeID {
    pub fn new(base: &str, generics: Vec<TypeID>) -> Self {
        TypeID {
            base: base.to_string(),
            generics,
        }
    }

    pub fn from_base(base: &str) -> Self {
        TypeID {
            base: base.to_string(),
            generics: Vec::<TypeID>::new(),
        }
    }

    pub fn name(&self) -> String {
        let mut generic_name = self.base.clone();

        if !self.generics.is_empty() {
            let generic_subnames: Vec<String> = self.generics.iter().map(|g| g.name()).collect();
            generic_name.push_str(&format!("<{}>", generic_subnames.join(", ")));
        }

        generic_name
    }
}

impl Display for TypeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.name().fmt(f)
    }
}

#[derive(Debug, Clone)]
pub struct Member<'ctx> {
    pub typ: TypeID,
    pub index: u32,
    pub value: ValueEnum<'ctx>,
}

#[derive(Debug, Clone)]
pub struct Metatype<'ctx> {
    pub base: BasicBuiltin,
    pub class_name: String,
    pub id: TypeID,
    pub inherits: Vec<TypeID>,
    pub generics: Vec<TypeID>,
    pub members: HashMap<String, Member<'ctx>>,
    static_ptr: PointerValue<'ctx>,
    pub static_struct: StructType<'ctx>,
    pub obj_struct: Option<StructType<'ctx>>,
    pub storage_type: AnyTypeEnum<'ctx>,
    pub is_refcounted: bool,
    pub initializer: Option<Function<'ctx>>,
    pub class_info: Option<ClassInfo<'ctx>>,
}

impl<'ctx> Metatype<'ctx> {}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueEnum<'ctx>, CompileError> {
        let member = self.members.get(&name.inner).ok_or_else(|| {
            CompileError::new(
                name.span,
                &format!(
                    "Cannot get member of name {} from type {}",
                    name.inner, self.id
                ),
            )
        })?;
        let member_ptr = ctx
            .builder
            .build_struct_gep(
                self.static_struct,
                self.static_ptr,
                member.index,
                format!("{into}_field").as_str(),
            )
            .unwrap();
        let member_val = ctx
            .builder
            .build_load(
                ctx.get_storage(member.typ.clone()),
                member_ptr,
                format!("{into}_val").as_str(),
            )
            .unwrap();
        Ok(ValueEnum::from_val(
            ctx,
            member_val,
            member.typ.clone(),
            into,
        ))
    }

    fn member_ref(
        &self,
        ctx: &LanguageContext<'ctx>,
        name: Spanned<String>,
        into: &str,
    ) -> Result<ValueRef<'ctx>, CompileError> {
        let member = self.members.get(&name.inner).ok_or_else(|| {
            CompileError::new(
                name.span,
                &format!(
                    "Cannot get member of name {} from type {}",
                    name.inner, self.id
                ),
            )
        })?;
        let member_ptr = ctx
            .builder
            .build_struct_gep(
                self.static_struct,
                self.static_ptr,
                member.index,
                format!("{into}_field").as_str(),
            )
            .unwrap();
        Ok(ValueRef {
            ptr: member_ptr,
            typ: member.typ.clone(),
        })
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Type")
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.static_ptr)
    }

    fn construct_ptr(&self, ctx: &LanguageContext<'ctx>, into_name: &str) -> PointerValue<'ctx> {
        todo!()
    }
}

impl<'ctx> ValueStatic<'ctx> for Metatype<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        assert_eq!(generics.len(), 0);
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Type,
            TypeID::from_base("Type"),
            Some(ctx.types.type_struct),
            AnyTypeEnum::VoidType(ctx.types.void),
            false,
        );
        builder.build(llvm_ctx, ctx, generics)
    }
}

impl<'ctx> PartialEq for Metatype<'ctx> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

struct BuilderStaticRepr<'ctx> {
    name: String,
    val: ValueEnum<'ctx>,
}

pub struct MetatypeBuilder<'ctx> {
    id: TypeID,
    base: BasicBuiltin,
    name: String,
    obj_struct: Option<StructType<'ctx>>,
    storage_type: AnyTypeEnum<'ctx>,
    static_values: Vec<BuilderStaticRepr<'ctx>>,
    is_refcounted: bool,
    inherits: Vec<TypeID>,
    pub initializer: Option<Function<'ctx>>,
    class_info: Option<ClassInfo<'ctx>>,
}

impl<'ctx> MetatypeBuilder<'ctx> {
    pub fn new(
        ctx: &mut LanguageContext<'ctx>,
        base: BasicBuiltin,
        id: TypeID,
        obj_struct: Option<StructType<'ctx>>,
        storage_type: AnyTypeEnum<'ctx>,
        is_refcounted: bool,
    ) -> Self {
        ctx.reserve_metatype(id.clone());
        Self {
            id: id.clone(),
            base,
            name: id.base,
            obj_struct,
            storage_type,
            static_values: Vec::<BuilderStaticRepr<'ctx>>::new(),
            inherits: Vec::<TypeID>::new(),
            is_refcounted,
            initializer: None,
            class_info: None,
        }
    }

    pub fn add_initializer(&mut self, initializer: Function<'ctx>) {
        self.initializer = Some(initializer)
    }

    pub fn add_parent(&mut self, ctx: LanguageContext<'ctx>, id: TypeID) {
        self.inherits.push(id.clone());
        let metatype = ctx.get(id);
        for (name, member) in metatype.members.clone() {
            self.add_static(&name, member.value);
        }
    }

    pub fn add_static(&mut self, name: &str, val: ValueEnum<'ctx>) {
        self.static_values.push(BuilderStaticRepr {
            name: name.to_string(),
            val,
        });
    }

    pub fn add_class_info(&mut self, info: ClassInfo<'ctx>) {
        self.class_info = Some(info);
    }

    pub fn build(
        &mut self,
        llvm_ctx: &'ctx Context,
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        let name = self.id.name();
        let static_struct = llvm_ctx.opaque_struct_type(format!("{name}__static").as_str());
        let static_ptr =
            ctx.module
                .add_global(static_struct, None, format!("Type__{name}").as_str());
        let mut members = HashMap::<String, Member>::new();
        let internals: Vec<BasicTypeEnum<'ctx>> = self
            .static_values
            .iter()
            .map(|v| {
                any_to_basic(
                    ctx.get_storage_with_gen(
                        llvm_ctx,
                        SimpleSpan::new((), 0..0).make_wrapped(v.val.get_type(ctx)),
                    )
                    .unwrap(),
                )
                .unwrap()
            })
            .collect();
        static_struct.set_body(&internals, false);
        let mut i = 0;
        let mut static_values = Vec::<BasicValueEnum<'ctx>>::new();
        while !self.static_values.is_empty() {
            let val = self.static_values.pop().unwrap();
            static_values.push(val.val.get_value());

            members.insert(
                val.name.clone(),
                Member {
                    typ: val.val.get_type(ctx),
                    index: i,
                    value: val.val,
                },
            );

            i += 1;
        }
        static_ptr.set_initializer(&static_struct.const_named_struct(&static_values));
        let static_ptr = static_ptr.as_pointer_value();

        let out = Metatype::<'ctx> {
            base: self.base.clone(),
            class_name: name.to_string(),
            id: self.id.clone(),
            members,
            inherits: self.inherits.clone(),
            generics,
            static_ptr,
            static_struct,
            storage_type: self.storage_type,
            obj_struct: self.obj_struct,
            is_refcounted: self.is_refcounted,
            initializer: self.initializer.clone(),
            class_info: self.class_info.clone(),
        };

        ctx.metatypes.insert(self.id.clone(), Some(out.clone()));
        ctx.add_field(&name, Field::new(ValueEnum::Type(out), &name));
    }
}

/// ClassBuilder is a wrapper for MetatypeBuilder designed to asset in building
/// specifically user created classes (of base BasicBuiltin::Class).
pub struct ClassBuilder<'ctx> {
    class_struct: StructType<'ctx>,
    members: BTreeMap<String, ClassMember>,
    functions: BTreeMap<String, Function<'ctx>>,
    initializer: FunctionValue<'ctx>,
}

impl<'ctx> ClassBuilder<'ctx> {
    pub fn new(
        ctx: &mut LanguageContext<'ctx>,
        name: &str,
        params: &Vec<(String, TypeID)>,
    ) -> Self {
        let init_llvm_type = ctx.types.ptr.fn_type(
            params
                .iter()
                .map(|(_, typ)| any_to_basic(ctx.get(typ.clone()).storage_type))
                .collect(),
            is_var_args,
        );
        let init_llvm_fn = ctx.add_function(&format("User__{}.()", name), init_llvm_type);
        let init_type = function_type(
            params.iter().map(|(name, typ)| typ).collect(),
            TypeID::from_base(name),
        );
        let init_fn = Function::from_function(ctx.context, ctx, init_llvm_fn, init_type);
    }
}
