use std::{collections::HashMap, fmt::Display};

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
pub struct Metatype<'ctx> {
    pub base: BasicType,
    pub class_name: String,
    pub id: TypeID,
    pub generics: Vec<TypeID>,
    members: HashMap<String, Field<'ctx>>,
    static_ptr: PointerValue<'ctx>,
    pub static_struct: StructType<'ctx>,
    pub obj_struct: StructType<'ctx>,
}

impl<'ctx> Metatype<'ctx> {}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&Field<'ctx>> {
        let member = self.members.get(&name);
        match member {
            None => None,
            Some(member) => Some(member),
        }
    }

    fn get_type(&self, ctx: &LanguageContext<'ctx>) -> TypeID {
        TypeID::from_base("Type".to_string())
    }

    fn get_ptr(&self) -> PointerValue<'ctx> {
        self.static_ptr
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
            BasicType::Type,
            TypeID::from_base("Type".to_string()),
            ctx.types.type_struct,
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
    val: ValuePtr<'ctx>,
}

pub struct MetatypeBuilder<'ctx> {
    id: TypeID,
    base: BasicType,
    name: String,
    obj_struct: StructType<'ctx>,
    static_values: Vec<BuilderStaticRepr<'ctx>>,
}

impl<'ctx> MetatypeBuilder<'ctx> {
    pub fn new(
        ctx: &mut LanguageContext<'ctx>,
        base: BasicType,
        id: TypeID,
        obj_struct: StructType<'ctx>,
    ) -> Self {
        ctx.reserve_metatype(id.clone());
        Self {
            id: id.clone(),
            base,
            name: id.base,
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
        ctx: &mut LanguageContext<'ctx>,
        generics: Vec<TypeID>,
    ) {
        let name = self.name.as_str();
        let static_struct = llvm_ctx.opaque_struct_type(format!("{name}__static").as_str());
        let static_ptr = ctx
            .builder
            .build_alloca(static_struct, format!("{name}__MTOBJ").as_str())
            .unwrap();
        let mut members = HashMap::<String, Field<'ctx>>::new();
        let internals: Vec<BasicTypeEnum<'ctx>> =
            vec![BasicTypeEnum::PointerType(ctx.types.ptr); self.static_values.len()];
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
            let value_ptr = val.val.get_ptr();
            ctx.builder.build_store(field_ptr, value_ptr).unwrap();

            members.insert(
                val.name.clone(),
                Field::new(field_ptr, val.name.clone(), val.val.get_type(ctx)),
            );

            i += 1;
        }

        let out = Metatype::<'ctx> {
            base: self.base.clone(),
            class_name: name.to_string(),
            id: self.id.clone(),
            generics,
            members,
            static_ptr,
            static_struct,
            obj_struct: self.obj_struct,
        };

        ctx.metatypes.insert(self.id.clone(), Some(out));
    }
}
