use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{
    IterParser, Parser,
    error::Rich,
    extra,
    prelude::{choice, just},
    span::{Spanned, WrappingSpan},
    text::{ascii::ident, keyword},
};
use inkwell::{
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

struct ForeignFunction {
    name: Spanned<String>,
    params: Vec<(Spanned<String>, Spanned<String>)>,
    returns: Option<Spanned<String>>,
    bound: Spanned<String>,
}

enum FFIMember {
    Var {
        name: Spanned<String>,
        typ: Spanned<String>,
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

fn foreign_function<'src>()
-> impl Parser<'src, &'src str, ForeignFunction, extra::Err<Rich<'src, char>>> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    keyword("fn")
        .padded()
        .ignore_then(id)
        .then(
            id.then_ignore(just(':').padded())
                .then(id)
                .separated_by(just(",").padded())
                .collect::<Vec<_>>()
                .delimited_by(just('(').padded(), just(')').padded())
                .or_not(),
        )
        .then(just("->").padded().ignore_then(id).or_not())
        .then_ignore(just("=").padded())
        .then(id)
        .then_ignore(just(";").padded())
        .map(
            // whyyy
            |(((name, params), returns), bound): (
                (
                    (
                        Spanned<String>,
                        Option<Vec<(Spanned<String>, Spanned<String>)>>,
                    ),
                    Option<Spanned<String>>,
                ),
                Spanned<String>,
            )| {
                ForeignFunction {
                    name: name.span.make_wrapped(name.inner.to_string()),
                    params: if (params.is_some()) {
                        params.unwrap()
                    } else {
                        vec![]
                    },
                    returns,
                    bound,
                }
            },
        )
}

fn ffi_member<'src>() -> impl Parser<'src, &'src str, FFIMember, extra::Err<Rich<'src, char>>> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    let var = id
        .then_ignore(just(':').padded())
        .then(id)
        .then_ignore(just(';').padded())
        .map(|(name, typ)| FFIMember::Var { name, typ })
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
    for decl in decls {
        match decl {
            FFIDecl::Class { name, members } => {
                let class_struct = ctx
                    .context
                    .opaque_struct_type(&format!("User__{}", name.inner));

                let mut body = Vec::<BasicTypeEnum<'ctx>>::new();
                let mut class_members = BTreeMap::<String, ClassMember>::new();
                let mut index = 0;
                for member in &members {
                    match member {
                        FFIMember::Var { name, typ } => {
                            let typ = TypeID::from_base(&typ.inner);
                            class_members
                                .insert(name.inner.clone(), ClassMember::new(typ.clone(), index));
                            body.push(ctx.get_storage(typ));
                        }
                        FFIMember::Function(foreign_function) => todo!(),
                    };
                    index += 1;
                }
                class_struct.set_body(&body, false);

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
                bound,
            }) => {
                let mut param_types = Vec::<BasicMetadataTypeEnum>::new();
                for (_, typ) in &params {
                    param_types.push(
                        any_to_basic(ctx.get(TypeID::from_base(&typ)).storage_type)
                            .unwrap()
                            .into(),
                    );
                }
                let llvm_function_type = if returns.is_some() {
                    any_to_basic(
                        ctx.get(TypeID::from_base(&returns.clone().unwrap().inner))
                            .storage_type,
                    )
                    .unwrap()
                    .fn_type(&param_types, false)
                } else {
                    ctx.types.void.fn_type(&param_types, false)
                };
                let llvm_function = ctx.module.add_function(
                    &format!("User__{}", name.inner),
                    llvm_function_type,
                    None,
                );
                let function_type = TypeID::new(
                    "Function",
                    vec![
                        TypeID::new(
                            "Tuple",
                            params
                                .iter()
                                .map(|(_, typ)| TypeID::from_base(typ))
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

                let linked_function =
                    ctx.module
                        .add_function(&bound.inner, llvm_function_type, None);

                let old_block = ctx.begin_function(llvm_function);
                let mut param_vals: Vec<BasicMetadataValueEnum> = vec![];
                for ((name, typ), val) in params.iter().zip(llvm_function.get_params()) {
                    if typ.inner == "String" {
                        let val = ctx.build_call_returns(
                            ctx.module.get_function("String__to_cstr").unwrap(),
                            &[val.into()],
                            "to_cstr",
                        );
                        param_vals.push(val.into());
                    } else {
                        param_vals.push(val.into());
                    }
                }
                let ret = ctx
                    .builder
                    .build_call(linked_function, &param_vals, "ret")
                    .unwrap();
                ret.set_tail_call(true);
                if returns.is_some() {
                    ctx.builder
                        .build_return(Some(&ret.try_as_basic_value().unwrap_basic()));
                } else {
                    ctx.builder.build_return(None);
                }

                ctx.builder.position_at_end(old_block);
            }
        }
    }

    Ok(())
}
