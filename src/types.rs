use std::collections::HashMap;

use inkwell::{types::StructType, values::PointerValue};

use crate::{context::LanguageContext, value::{Copyable, Value}};

struct Metatype<'ctx> {
    member_indices: HashMap<String, u32>,
    static_ptr: PointerValue<'ctx>,
    static_struct: StructType<'ctx>,
    obj_struct: StructType<'ctx>,
}

impl<'ctx> Value<'ctx> for Metatype<'ctx> {
    fn member<T: Copyable<'ctx>>(&self, ctx: LanguageContext<'ctx>, name: String) -> Option<T> {
        let index = self.member_indices.get(&name);
        match index {
            None => None,
            Some(index) => {
                let member_ptr = ctx.builder.build_struct_gep(self.static_struct, self.static_ptr, *index, "type_static").unwrap();
                Some(T::from_ptr(ctx, member_ptr))
            }
        }
    }
}
