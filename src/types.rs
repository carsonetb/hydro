use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
};

use chumsky::span::{SimpleSpan, Span, Spanned, WrappingSpan};
use inkwell::{
    basic_block::BasicBlock,
    context::Context,
    types::{AnyTypeEnum, BasicTypeEnum, FunctionType, StructType},
    values::{BasicValueEnum, FunctionValue, PointerValue},
};

use crate::{
    callable::{Callable, Function, function_type},
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
        ctx: &mut LanguageContext<'ctx>,
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

    fn get_type(&self) -> TypeID {
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
                        SimpleSpan::new((), 0..0).make_wrapped(v.val.get_type()),
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
                    typ: val.val.get_type(),
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

struct MemberDefault<'ctx> {
    value: BasicValueEnum<'ctx>,
    index: u32,
    name: String,
}

/// ClassBuilder is a wrapper for MetatypeBuilder designed to asset in building
/// specifically user created classes (of base BasicBuiltin::Class).
pub struct ClassBuilder<'ctx> {
    old_block: BasicBlock<'ctx>,

    pub class_struct: StructType<'ctx>,
    pub init_llvm: FunctionValue<'ctx>,
    body: Vec<BasicTypeEnum<'ctx>>,
    static_body: Vec<BasicTypeEnum<'ctx>>,

    initializer: Function<'ctx>,
    builder: MetatypeBuilder<'ctx>,
    pub members: BTreeMap<String, ClassMember>,
    pub functions: BTreeMap<String, Function<'ctx>>,
    default_members: Vec<MemberDefault<'ctx>>,

    member_index: u32,
}

impl<'ctx> ClassBuilder<'ctx> {
    pub fn new(
        ctx: &mut LanguageContext<'ctx>,
        name: &str,
        params: &Vec<(Spanned<String>, TypeID)>,
    ) -> Self {
        let mut builder = MetatypeBuilder::new(
            ctx,
            BasicBuiltin::Class,
            TypeID::from_base(name),
            None,
            ctx.types.ptr.into(),
            false,
        );

        let class_struct = ctx.context.opaque_struct_type(&format!("User__{name}"));

        let init_llvm_type = ctx.types.ptr.fn_type(
            &params
                .iter()
                .map(|(_, typ)| {
                    any_to_basic(ctx.get(typ.clone()).storage_type)
                        .unwrap()
                        .into()
                })
                .collect::<Vec<_>>(),
            false,
        );
        let init_llvm_fn = ctx.add_function(&format!("User__{}.()", name), init_llvm_type);
        let init_type = function_type(
            params.iter().map(|(name, typ)| typ.clone()).collect(),
            TypeID::from_base(name),
        );
        let init_fn = Function::from_function(ctx.context, ctx, init_llvm_fn, init_type);
        builder.add_initializer(init_fn.clone());

        let old_block = ctx.begin_function(init_llvm_fn);

        let mut out = Self {
            old_block,
            class_struct: class_struct,
            init_llvm: init_llvm_fn,
            body: vec![],
            static_body: vec![],

            initializer: init_fn,
            builder,
            members: BTreeMap::new(),
            functions: BTreeMap::new(),
            default_members: vec![],

            member_index: 0,
        };

        for ((name, typ), val) in params.iter().zip(init_llvm_fn.get_params()) {
            out.add_member(name, &ValueEnum::from_val(ctx, val, typ.clone(), name));
        }

        out
    }

    pub fn build(&mut self, ctx: &mut LanguageContext<'ctx>) {
        self.class_struct.set_body(&self.body, false);

        let mem = ctx.build_gc_malloc(self.class_struct.size_of().unwrap(), "out");
        for MemberDefault { value, index, name } in &self.default_members {
            ctx.build_ptr_store(self.class_struct, mem, *value, *index, &name);
        }

        if self.functions.contains_key("init") {
            let function = self.functions["init"].to_member_function(ctx, mem.into(), "init");
            function.call(ctx, vec![], "UNUSED").unwrap();
        };

        ctx.builder.build_return(Some(&mem));
        ctx.builder.position_at_end(self.old_block);

        self.builder.add_class_info(ClassInfo::new(
            self.class_struct,
            self.members.clone(),
            self.functions.clone(),
        ));
        self.builder.build(ctx.context, ctx, vec![]);
    }

    pub fn add_member(&mut self, name: &String, value: &ValueEnum<'ctx>) {
        // TODO: Check if init function and validate.
        match value {
            ValueEnum::Function(function) => {
                self.functions.insert(name.clone(), function.clone());
            }
            _ => {
                let llvm_value = value.get_value();
                self.body.push(llvm_value.get_type());
                self.default_members.push(MemberDefault {
                    value: llvm_value,
                    index: self.member_index,
                    name: name.clone(),
                });
                self.members.insert(
                    name.clone(),
                    ClassMember::new(value.get_type(), self.member_index),
                );
                self.member_index += 1;
            }
        }
    }

    pub fn add_static(&mut self, name: &str, value: &ValueEnum<'ctx>) {
        self.builder.add_static(name, value.clone());
    }
}
