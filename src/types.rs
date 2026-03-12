use std::{collections::HashMap, fmt::Display};

use inkwell::{
    context::Context,
    types::{AnyTypeEnum, BasicTypeEnum, StructType},
    values::{BasicValueEnum, PointerValue},
};

use crate::{
    context::LanguageContext,
    value::{Field, Value, ValueEnum, ValueStatic},
};

#[derive(Clone, Debug, PartialEq)]
pub enum BasicBuiltin {
    Unit,
    Type,
    Int,
    Tuple,
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeID {
    pub base: String,
    pub generics: Vec<TypeID>,
}

impl TypeID {
    pub fn new(base: String, generics: Vec<TypeID>) -> Self {
        TypeID { base, generics }
    }

    pub fn from_base(base: String) -> Self {
        TypeID {
            base,
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
pub struct Member {
    pub typ: TypeID,
    pub index: u32,
}

#[derive(Debug, Clone)]
pub struct Metatype<'ctx> {
    pub base: BasicBuiltin,
    pub class_name: String,
    pub id: TypeID,
    pub generics: Vec<TypeID>,
    members: HashMap<String, Member>,
    static_ptr: PointerValue<'ctx>,
    pub static_struct: StructType<'ctx>,
    pub obj_struct: StructType<'ctx>,
    pub storage_type: BasicTypeEnum<'ctx>,
    pub is_refcounted: bool,
}

impl<'ctx> Metatype<'ctx> {}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String, into: String) -> ValueEnum<'ctx> {
        let member = self
            .members
            .get(&name)
            .expect(format!("Cannot get member of name {name} from type {}", self.id).as_str());
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
        ValueEnum::from_val(ctx, member_val, member.typ.clone(), into)
    }

    fn get_type(&self, _ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Type".to_string())
    }

    fn get_value(&self) -> BasicValueEnum<'ctx> {
        BasicValueEnum::PointerValue(self.static_ptr)
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
            TypeID::from_base("Type".to_string()),
            ctx.types.type_struct,
            BasicTypeEnum::PointerType(ctx.types.ptr),
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
    obj_struct: StructType<'ctx>,
    storage_type: BasicTypeEnum<'ctx>,
    static_values: Vec<BuilderStaticRepr<'ctx>>,
    is_refcounted: bool,
}

impl<'ctx> MetatypeBuilder<'ctx> {
    pub fn new(
        ctx: &mut LanguageContext<'ctx>,
        base: BasicBuiltin,
        id: TypeID,
        obj_struct: StructType<'ctx>,
        storage_type: BasicTypeEnum<'ctx>,
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
            is_refcounted,
        }
    }

    pub fn add_static(&mut self, name: String, val: ValueEnum<'ctx>) {
        self.static_values.push(BuilderStaticRepr { name, val });
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
        static_ptr.set_initializer(&static_struct.const_zero());
        let static_ptr = static_ptr.as_pointer_value();
        let mut members = HashMap::<String, Member>::new();
        let internals: Vec<BasicTypeEnum<'ctx>> = self
            .static_values
            .iter()
            .map(|v| ctx.get_storage(v.val.get_type(ctx)))
            .collect();
        static_struct.set_body(&internals, false);
        let mut i = 0;
        while !self.static_values.is_empty() {
            let val = self.static_values.pop().unwrap();
            let field_ptr = ctx
                .builder
                .build_struct_gep(
                    static_struct,
                    static_ptr,
                    i,
                    format!("STATIC__{}_field", val.name).as_str(),
                )
                .unwrap();
            let value_ptr = val.val.get_value();
            ctx.builder.build_store(field_ptr, value_ptr).unwrap();

            members.insert(
                val.name.clone(),
                Member {
                    typ: val.val.get_type(ctx),
                    index: i,
                },
            );

            i += 1;
        }

        let out = Metatype::<'ctx> {
            base: self.base.clone(),
            class_name: name.to_string(),
            id: self.id.clone(),
            members,
            generics,
            static_ptr,
            static_struct,
            storage_type: self.storage_type,
            obj_struct: self.obj_struct,
            is_refcounted: self.is_refcounted,
        };

        ctx.metatypes.insert(self.id.clone(), Some(out));
    }
}
