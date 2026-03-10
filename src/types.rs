use std::collections::HashMap;

use enum_dispatch::enum_dispatch;
use inkwell::{
    context::Context,
    types::{BasicTypeEnum, StructType},
    values::PointerValue,
};

use crate::{
    context::LanguageContext,
    value::{Field, Value, ValuePtr, ValueStatic},
};

#[derive(Clone, Debug, PartialEq)]
pub enum BasicType {
    Unit,
    Type,
    Int,
    Function,
}

#[enum_dispatch]
pub enum IndevMetatype<'ctx> {
    Opaque(OpaqueMetatype),
    Defined(Metatype<'ctx>),
}

pub struct OpaqueMetatype {}

impl<'ctx> Value<'ctx> for OpaqueMetatype {
    fn member(&self, _ctx: &LanguageContext<'ctx>, _name: String) -> Option<&Field<'ctx>> {
        panic!("OpaqueMetatype has no members, cannot use members at this stage.")
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get_base_metatype("Type".to_string()).unwrap()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        panic!("OpaqueMetatype has no pointer, cannot use get_ptr at this stage.")
    }
}

#[derive(Debug, Clone)]
pub struct Metatype<'ctx> {
    pub base: BasicType,
    pub class_name: String,
    pub name: String,
    pub generics: Vec<Metatype<'ctx>>,
    members: HashMap<String, Field<'ctx>>,
    static_ptr: PointerValue<'ctx>,
    pub static_struct: StructType<'ctx>,
    pub obj_struct: StructType<'ctx>,
}

impl<'ctx> Metatype<'ctx> {
    pub fn gen_name(name: String, generics: &Vec<Metatype<'ctx>>) -> String {
        let mut generic_name = name.to_owned();

        if !generics.is_empty() {
            let generic_subnames: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
            generic_name.push_str(&format!("<{}>", generic_subnames.join(", ")));
        }

        generic_name
    }
}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&Field<'ctx>> {
        let member = self.members.get(&name);
        match member {
            None => None,
            Some(member) => Some(member),
        }
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        ctx.get_base_metatype("Type".to_string()).unwrap()
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.static_ptr
    }
}

impl<'ctx> ValueStatic<'ctx> for Metatype<'ctx> {
    fn build_metatype(
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        generics: Vec<Metatype<'ctx>>,
    ) -> Metatype<'ctx> {
        assert_eq!(generics.len(), 0);
        let mut builder =
            MetatypeBuilder::new(BasicType::Type, "Type".to_string(), ctx.types.type_struct);
        builder.build(llvm_ctx, ctx, generics)
    }
}

impl<'ctx> PartialEq for Metatype<'ctx> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

struct BuilderStaticRepr<'ctx> {
    name: String,
    val: ValuePtr<'ctx>,
}

pub struct MetatypeBuilder<'ctx> {
    base: BasicType,
    name: String,
    pub indev: IndevMetatype<'ctx>,
    obj_struct: StructType<'ctx>,
    static_values: Vec<BuilderStaticRepr<'ctx>>,
}

impl<'ctx> MetatypeBuilder<'ctx> {
    pub fn new(base: BasicType, name: String, obj_struct: StructType<'ctx>) -> Self {
        Self {
            base,
            name: name.clone(),
            indev: IndevMetatype::Opaque(OpaqueMetatype {}),
            obj_struct,
            static_values: Vec::<BuilderStaticRepr<'ctx>>::new(),
        }
    }

    pub fn add_static(&mut self, name: String, val: ValuePtr<'ctx>) {
        self.static_values.push(BuilderStaticRepr { name, val });
    }

    pub fn build(
        &mut self,
        llvm_ctx: &'ctx Context,
        ctx: &LanguageContext<'ctx>,
        generics: Vec<Metatype<'ctx>>,
    ) -> Metatype<'ctx> {
        let name = self.name.as_str();
        let static_struct = llvm_ctx.opaque_struct_type(format!("{name}__static").as_str());
        let static_ptr = ctx.builder.build_alloca(static_struct, name).unwrap();
        let mut members = HashMap::<String, Field<'ctx>>::new();
        let mut internals = Vec::<BasicTypeEnum<'ctx>>::new();
        let mut i = 0;
        while !self.static_values.is_empty() {
            let val = self.static_values.pop().unwrap();
            internals.push(BasicTypeEnum::PointerType(ctx.types.ptr));
            let field_ptr = ctx
                .builder
                .build_struct_gep(static_struct, static_ptr, i, "this_ptr")
                .unwrap();
            let value_ptr = val.val.get_ptr();
            ctx.builder.build_store(field_ptr, value_ptr).unwrap();

            members.insert(
                val.name.clone(),
                Field::new(field_ptr, val.name.clone(), val.val.get_type(ctx)),
            );

            i += 1;
        }
        static_struct.set_body(&internals, false);

        Metatype::<'ctx> {
            base: self.base.clone(),
            class_name: name.to_string(),
            name: Metatype::gen_name(name.to_string(), &generics),
            generics,
            members,
            static_ptr,
            static_struct,
            obj_struct: self.obj_struct,
        }
    }
}
