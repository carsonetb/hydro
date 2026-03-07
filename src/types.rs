use std::collections::HashMap;

use inkwell::{
    context::Context,
    types::{BasicTypeEnum, StructType},
    values::PointerValue,
};

use crate::{
    context::LanguageContext,
    value::{Field, Value, ValueField, ValuePtr},
};

pub struct Metatype<'ctx> {
    members: HashMap<String, ValueField<'ctx>>,
    static_ptr: PointerValue<'ctx>,
    pub static_struct: StructType<'ctx>,
    pub obj_struct: StructType<'ctx>,
}

impl<'ctx> Metatype<'ctx> {}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member(&self, ctx: &LanguageContext<'ctx>, name: String) -> Option<&ValueField<'ctx>> {
        let member = self.members.get(&name);
        match member {
            None => None,
            Some(member) => Some(member),
        }
    }

    fn build_metatype(llvm_ctx: &'ctx Context, ctx: &LanguageContext<'ctx>) -> Metatype<'ctx> {
        let mut builder = MetatypeBuilder::new("Type".to_string(), ctx.types.type_struct);
        builder.build(llvm_ctx, ctx)
    }
}

struct BuilderStaticRepr<'ctx> {
    name: String,
    val: ValuePtr<'ctx>,
}

pub struct MetatypeBuilder<'ctx> {
    name: String,
    obj_struct: StructType<'ctx>,
    static_values: Vec<BuilderStaticRepr<'ctx>>,
}

impl<'ctx> MetatypeBuilder<'ctx> {
    pub fn new(name: String, obj_struct: StructType<'ctx>) -> Self {
        Self {
            name: name.clone(),
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
    ) -> Metatype<'ctx> {
        let name = self.name.as_str();
        let static_struct = llvm_ctx.opaque_struct_type(format!("{name}__static").as_str());
        let static_ptr = ctx.builder.build_alloca(static_struct, name).unwrap();
        let mut members = HashMap::<String, ValueField<'ctx>>::new();
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
                match val.val {
                    ValuePtr::PInt(_int) => {
                        ValueField::RInt(Field::new(field_ptr, val.name.clone()))
                    }
                },
            );

            i += 1;
        }
        static_struct.set_body(&internals, false);

        Metatype::<'ctx> {
            members,
            static_ptr,
            static_struct,
            obj_struct: self.obj_struct,
        }
    }
}
