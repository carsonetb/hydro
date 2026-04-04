use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, write},
    io::Read,
    path::PathBuf,
    process::Command,
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use askama::Template;
use chumsky::{
    IterParser, Parser,
    error::Rich,
    extra,
    prelude::{choice, just},
    span::{Spanned, WrappingSpan},
    text::{ascii::ident, keyword},
};
use inkwell::{
    module::Module,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum},
    values::{AnyValue, BasicMetadataValueEnum},
};

use crate::{
    callable::Function,
    classes::{ClassInfo, ClassMember},
    codegen::CompileError,
    context::LanguageContext,
    types::{BasicBuiltin, MetatypeBuilder, TypeID},
    value::{Field, ValueEnum, any_to_basic},
};

struct FFIArg {
    name: Spanned<String>,
    typ: Spanned<String>,
    raw: bool,
}

struct ForeignFunction {
    name: Spanned<String>,
    params: Vec<FFIArg>,
    returns: Option<Spanned<String>>,
    returns_raw: Option<bool>,
    bound: Spanned<String>,
}

enum FFIMember {
    Var {
        name: Spanned<String>,
        typ: Spanned<String>,
        raw: bool,
    },
    Function(ForeignFunction),
}

enum FFIDecl {
    Class {
        name: Spanned<String>,
        members: Vec<FFIMember>,
    },
    Function(ForeignFunction),
}

struct ClassFieldTemplate {
    c_type: String,
    name: String,
}

struct ClassDefTemplate {
    name: String,
    fields: Vec<ClassFieldTemplate>,
}

struct FuncArgTemplate {
    name: String,
    c_type: String,
    bound_type: String,
    fat_string: bool,
}

struct FuncDefTemplate {
    c_target: String,
    args: Vec<FuncArgTemplate>,
    returns: String,
    bound_returns: String,
    retstr: bool,
    ret_struct: String,
    toptr: bool,
}

#[derive(Template)]
#[template(path = "bridge.c.template", escape = "none")]
struct BridgeTemplate {
    classes: Vec<ClassDefTemplate>,
    funcs: Vec<FuncDefTemplate>,
}

impl BridgeTemplate {
    fn new() -> BridgeTemplate {
        BridgeTemplate {
            classes: vec![],
            funcs: vec![],
        }
    }
}

fn foreign_function<'src>()
-> impl Parser<'src, &'src str, ForeignFunction, extra::Err<Rich<'src, char>>> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    keyword("fn")
        .padded()
        .ignore_then(id)
        .then(
            id.then_ignore(just(':').padded())
                .then(keyword("raw").padded().or_not().map(|x| x.is_some()))
                .then(id)
                .map(|((name, raw), typ)| FFIArg { name, typ, raw })
                .separated_by(just(",").padded())
                .collect::<Vec<_>>()
                .delimited_by(just('(').padded(), just(')').padded())
                .or_not(),
        )
        .then(
            just("->")
                .padded()
                .ignore_then(keyword("raw").padded().or_not().map(|x| x.is_some()))
                .then(id)
                .or_not(),
        )
        .then_ignore(just("=").padded())
        .then(id)
        .then_ignore(just(";").padded())
        .map(
            // whyyy
            |(((name, params), returns), bound): (
                (
                    (Spanned<String>, Option<Vec<FFIArg>>),
                    Option<(bool, Spanned<String>)>,
                ),
                Spanned<String>,
            )| ForeignFunction {
                name: name.span.make_wrapped(name.inner.to_string()),
                params: if (params.is_some()) {
                    params.unwrap()
                } else {
                    vec![]
                },
                returns: returns.clone().map(|r| r.1),
                returns_raw: returns.map(|r| r.0),
                bound,
            },
        )
}

fn ffi_member<'src>() -> impl Parser<'src, &'src str, FFIMember, extra::Err<Rich<'src, char>>> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    let var = id
        .then_ignore(just(':').padded())
        .then(keyword("raw").padded().or_not().map(|x| x.is_some()))
        .then(id)
        .then_ignore(just(';').padded())
        .map(|((name, raw), typ)| FFIMember::Var { name, typ, raw })
        .boxed();
    let function = foreign_function().map(|f| FFIMember::Function(f));
    choice((var, function))
}

fn ffi_decl<'src>() -> impl Parser<'src, &'src str, FFIDecl, extra::Err<Rich<'src, char>>> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    let class = keyword("class")
        .padded()
        .ignore_then(id)
        .then(
            ffi_member()
                .repeated()
                .collect()
                .delimited_by(just("{").padded(), just("}").padded()),
        )
        .map(|(name, members)| FFIDecl::Class { name, members });
    let function = foreign_function().map(|f| FFIDecl::Function(f));
    choice((function, class))
}

fn program<'src>() -> impl Parser<'src, &'src str, Vec<FFIDecl>, extra::Err<Rich<'src, char>>> {
    ffi_decl().repeated().collect()
}

fn map_to_ctype(typ: String) -> String {
    let type_map = HashMap::from([
        ("Int", "int"),
        ("Float", "float"),
        ("Bool", "char"),
        ("String", "String*"),
    ]);
    if type_map.contains_key(&typ.as_str()) {
        type_map.get(&typ.as_str()).unwrap().to_string()
    } else {
        format!("{}*", typ)
    }
}

fn map_to_bound(typ: String, raw: bool) -> String {
    let type_map = HashMap::from([
        ("Int", "int"),
        ("Float", "float"),
        ("Bool", "char"),
        ("String", "const char*"),
    ]);
    if type_map.contains_key(&typ.as_str()) {
        type_map.get(&typ.as_str()).unwrap().to_string()
    } else if raw {
        typ
    } else {
        format!("{}*", typ)
    }
}

pub fn compile_ffi<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    path: &Spanned<PathBuf>,
    build: &PathBuf,
) -> Result<(), CompileError> {
    let filename = path
        .clone()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut file = File::open(&path.inner).unwrap();
    let mut src = "".to_string();
    file.read_to_string(&mut src);
    let (ast, errors) = program().parse(&src).into_output_errors();

    if errors.len() > 0 {
        for err in errors {
            Report::build(
                ReportKind::Error,
                (filename.clone(), err.span().into_range()),
            )
            .with_message("Stub Syntax Error")
            .with_label(
                Label::new((filename.clone(), err.span().into_range()))
                    .with_message(err.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.clone(), Source::from(&src)))
            .unwrap();
        }
        return Err(CompileError::new(
            path.span,
            "There was a parse error in the stub file.",
        ));
    };

    let decls = ast.unwrap();

    let mut bridge = BridgeTemplate::new();
    for decl in &decls {
        match decl {
            FFIDecl::Class { name, members } => {
                bridge.classes.push(ClassDefTemplate {
                    name: name.inner.clone(),
                    fields: members
                        .iter()
                        .map(|member| match member {
                            FFIMember::Var { name, typ, raw } => ClassFieldTemplate {
                                c_type: map_to_ctype(typ.inner.clone()),
                                name: name.inner.clone(),
                            },
                            FFIMember::Function(foreign_function) => todo!(),
                        })
                        .collect(),
                });
            }
            FFIDecl::Function(ForeignFunction {
                name,
                params,
                returns,
                returns_raw,
                bound,
            }) => {
                bridge.funcs.push(FuncDefTemplate {
                    c_target: bound.inner.clone(),
                    args: params
                        .iter()
                        .map(|FFIArg { name, typ, raw }| FuncArgTemplate {
                            name: name.inner.clone(),
                            c_type: map_to_ctype(typ.inner.clone()),
                            bound_type: map_to_bound(typ.inner.clone(), *raw),
                            fat_string: typ.inner == "String",
                        })
                        .collect(),
                    returns: if returns.is_some() {
                        map_to_ctype(returns.clone().unwrap().inner)
                    } else {
                        "void".to_string()
                    },
                    bound_returns: if returns.is_some() {
                        map_to_bound(returns.clone().unwrap().inner, returns_raw.unwrap())
                    } else {
                        "void".to_string()
                    },
                    retstr: if returns.is_none() {
                        false
                    } else {
                        returns.clone().unwrap().inner == "String"
                    },
                    ret_struct: if returns.is_some() {
                        returns.clone().unwrap().inner
                    } else {
                        "void".to_string()
                    },
                    toptr: if returns_raw.is_some() {
                        returns_raw.unwrap()
                    } else {
                        false
                    },
                });

                // let mut param_types = Vec::<BasicMetadataTypeEnum>::new();
                // for (FFIArg { name, typ, raw }) in &params {
                //     param_types.push(
                //         any_to_basic(ctx.get(TypeID::from_base(&typ)).storage_type)
                //             .unwrap()
                //             .into(),
                //     );
                // }
                // let llvm_function_type = if returns.is_some() {
                //     any_to_basic(
                //         ctx.get(TypeID::from_base(&returns.clone().unwrap().inner))
                //             .storage_type,
                //     )
                //     .unwrap()
                //     .fn_type(&param_types, false)
                // } else {
                //     ctx.types.void.fn_type(&param_types, false)
                // };
                // let llvm_function = ctx.module.add_function(&bound, llvm_function_type, None);
            }
        }
    }

    let contents = bridge.render().unwrap();

    write(build.join("bridge.c"), contents);

    let status = Command::new("clang")
        .args([
            "-O0",
            "-emit-llvm",
            "-c",
            "bin/bridge.c",
            "-o",
            "-g",
            "bin/bridge.bc",
        ])
        .status()
        .unwrap();

    let module = Module::parse_bitcode_from_path("bin/bridge.bc", ctx.context).unwrap();
    ctx.module.link_in_module(module);

    for decl in decls {
        match decl {
            FFIDecl::Class { name, members } => {
                println!("struct.{}", name.inner);
                let class_struct = ctx
                    .context
                    .get_struct_type(&format!("struct.{}", name.inner))
                    .unwrap();

                let mut class_members = BTreeMap::<String, ClassMember>::new();
                let mut index = 0;
                for member in &members {
                    match member {
                        FFIMember::Var { name, typ, raw } => {
                            class_members.insert(
                                name.inner.clone(),
                                ClassMember::new(TypeID::from_base(&typ.inner), index),
                            );
                        }
                        FFIMember::Function(foreign_function) => todo!(),
                    };
                    index += 1;
                }

                let mut builder = MetatypeBuilder::new(
                    ctx,
                    BasicBuiltin::Class,
                    TypeID::from_base(&name.inner),
                    Some(class_struct),
                    ctx.types.ptr.as_any_type_enum(),
                    false,
                );
                let class_info = ClassInfo::new(class_struct, class_members, BTreeMap::new());
                builder.add_class_info(class_info);
                builder.build(ctx.context, ctx, vec![]);
            }
            FFIDecl::Function(ForeignFunction {
                name,
                params,
                returns,
                returns_raw,
                bound,
            }) => {
                let llvm_function = ctx
                    .module
                    .get_function(&format!("Bridge__{}", bound.inner))
                    .unwrap();
                let function_type = TypeID::new(
                    "Function",
                    vec![
                        TypeID::new(
                            "Tuple",
                            params
                                .iter()
                                .map(|param| TypeID::from_base(&param.typ.inner))
                                .collect(),
                        ),
                        if returns.is_some() {
                            TypeID::from_base(&returns.clone().unwrap())
                        } else {
                            TypeID::from_base("Unit")
                        },
                    ],
                );

                let function =
                    Function::from_function(ctx.context, ctx, llvm_function, function_type);
                ctx.add_field(&name, Field::new(ValueEnum::Function(function), &name));
            }
        }
    }

    // for decl in decls {
    //     match decl {
    //         FFIDecl::Class { name, members } => {
    //             let class_struct = ctx
    //                 .context
    //                 .opaque_struct_type(&format!("User__{}", name.inner));

    //             let mut body = Vec::<BasicTypeEnum<'ctx>>::new();
    //             let mut class_members = BTreeMap::<String, ClassMember>::new();
    //             let mut index = 0;
    //             for member in &members {
    //                 match member {
    //                     FFIMember::Var { name, typ } => {
    //                         let typ = TypeID::from_base(&typ.inner);
    //                         class_members
    //                             .insert(name.inner.clone(), ClassMember::new(typ.clone(), index));
    //                         body.push(ctx.get_storage(typ));
    //                     }
    //                     FFIMember::Function(foreign_function) => todo!(),
    //                 };
    //                 index += 1;
    //             }
    //             class_struct.set_body(&body, false);

    //             let mut builder = MetatypeBuilder::new(
    //                 ctx,
    //                 BasicBuiltin::Class,
    //                 TypeID::from_base(&name.inner),
    //                 Some(class_struct),
    //                 ctx.types.ptr.as_any_type_enum(),
    //                 false,
    //             );
    //             let class_info = ClassInfo::new(class_struct, class_members, BTreeMap::new());
    //             builder.add_class_info(class_info);
    //             builder.build(ctx.context, ctx, vec![]);
    //         }
    //         FFIDecl::Function(ForeignFunction {
    //             name,
    //             params,
    //             returns,
    //             bound,
    //         }) => {
    //             let mut param_types = Vec::<BasicMetadataTypeEnum>::new();
    //             for (FFIArg { name, typ, raw }) in &params {
    //                 param_types.push(
    //                     any_to_basic(ctx.get(TypeID::from_base(&typ)).storage_type)
    //                         .unwrap()
    //                         .into(),
    //                 );
    //             }
    //             let llvm_function_type = if returns.is_some() {
    //                 any_to_basic(
    //                     ctx.get(TypeID::from_base(&returns.clone().unwrap().inner))
    //                         .storage_type,
    //                 )
    //                 .unwrap()
    //                 .fn_type(&param_types, false)
    //             } else {
    //                 ctx.types.void.fn_type(&param_types, false)
    //             };
    //             let llvm_function = ctx.module.add_function(
    //                 &format!("User__{}", name.inner),
    //                 llvm_function_type,
    //                 None,
    //             );
    //             let function_type = TypeID::new(
    //                 "Function",
    //                 vec![
    //                     TypeID::new(
    //                         "Tuple",
    //                         params
    //                             .iter()
    //                             .map(|(_, typ)| TypeID::from_base(typ))
    //                             .collect(),
    //                     ),
    //                     if returns.is_some() {
    //                         TypeID::from_base(&returns.clone().unwrap())
    //                     } else {
    //                         TypeID::from_base("Unit")
    //                     },
    //                 ],
    //             );
    //             let function =
    //                 Function::from_function(ctx.context, ctx, llvm_function, function_type);
    //             ctx.add_field(&name, Field::new(ValueEnum::Function(function), &name));

    //             let linked_function =
    //                 ctx.module
    //                     .add_function(&bound.inner, llvm_function_type, None);

    //             let old_block = ctx.begin_function(llvm_function);
    //             let mut param_vals: Vec<BasicMetadataValueEnum> = vec![];
    //             for ((name, typ), val) in params.iter().zip(llvm_function.get_params()) {
    //                 if typ.inner == "String" {
    //                     let val = ctx.build_call_returns(
    //                         ctx.module.get_function("String__to_cstr").unwrap(),
    //                         &[val.into()],
    //                         "to_cstr",
    //                     );
    //                     param_vals.push(val.into());
    //                 } else {
    //                     param_vals.push(val.into());
    //                 }
    //             }
    //             let ret = ctx
    //                 .builder
    //                 .build_call(linked_function, &param_vals, "ret")
    //                 .unwrap();
    //             ret.set_tail_call(true);
    //             if returns.is_some() {
    //                 ctx.builder
    //                     .build_return(Some(&ret.try_as_basic_value().unwrap_basic()));
    //             } else {
    //                 ctx.builder.build_return(None);
    //             }

    //             ctx.builder.position_at_end(old_block);
    //         }
    //     }
    // }

    Ok(())
}
