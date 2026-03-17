use std::{
    any::Any,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::span::{SimpleSpan, Spanned, WrappingSpan};
use inkwell::{
    context::Context,
    module::Linkage,
    types::{AnyType, BasicType, FunctionType},
};

use crate::{
    bool::Bool,
    callable::{Callable, Function},
    context::LanguageContext,
    int::Int,
    parser::{Atom, Decl, Expr, ParseLiteral, Primary, Program, Stmt},
    types::{Metatype, TypeID},
    value::{Field, Literal, Value, ValueEnum},
};

#[derive(Debug, Clone)]
pub struct CompileError(Vec<(SimpleSpan, String)>);

impl CompileError {
    pub fn new(span: SimpleSpan, reason: String) -> Self {
        Self(vec![(span, reason)])
    }

    pub fn with_notes(
        msg_span: SimpleSpan,
        msg: String,
        note_span: SimpleSpan,
        note: String,
    ) -> Self {
        Self(vec![(msg_span, msg), (note_span, note)])
    }

    pub fn message_span(&self) -> SimpleSpan {
        self.0[0].0
    }

    pub fn message(&self) -> String {
        self.0[0].1.clone()
    }

    pub fn notes(&self) -> Vec<(SimpleSpan, String)> {
        self.0[1..].to_vec()
    }
}

pub fn gen_literal<'ctx>(ctx: &LanguageContext<'ctx>, literal: &ParseLiteral) -> ValueEnum<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => ValueEnum::Int(Int::from_literal(ctx, *int, "int".to_string())),
        ParseLiteral::Bool(bool) => {
            ValueEnum::Bool(Bool::from_literal(ctx, *bool, "bool".to_string()))
        }
    }
}

pub fn gen_atom<'ctx>(
    ctx: &LanguageContext<'ctx>,
    atom: &Atom,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match atom {
        Atom::Literal(literal) => Ok(gen_literal(ctx, literal)),
        Atom::Grouping(expr) => gen_expr(ctx, expr),
    }
}

pub fn gen_primary<'ctx>(
    ctx: &LanguageContext<'ctx>,
    prim: &Primary,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => gen_atom(ctx, atom),
    }
}

pub fn gen_expr<'ctx>(
    ctx: &LanguageContext<'ctx>,
    expr: &Expr,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match expr {
        Expr::Unary(op, right) => todo!(),
        Expr::Binary(left, op, right) => {
            let left_val = gen_expr(ctx, left)?;
            let right_val = gen_expr(ctx, right)?;
            let left_type = left_val.get_type(ctx);
            let right_type = right_val.get_type(ctx);
            if left_type != right_type {
                return Err(CompileError(vec![
                    (
                        op.span,
                        format!("Cannot use operator `{}` on different types.", op.inner),
                    ),
                    (
                        left.span,
                        format!("Left operator is of type `{}`.", left_val.get_type(ctx)),
                    ),
                    (
                        right.span,
                        format!("Right operator is of type `{}`.", right_val.get_type(ctx)),
                    ),
                ]));
            }
            let op_fn = left_val
                .member(ctx, op.clone(), op.inner.clone())?
                .try_as_function()
                .unwrap();
            op_fn.verify(vec![left_type, right_type]);
            Ok(op_fn.call(ctx, vec![left_val, right_val], "binary".to_string()))
        }
        Expr::Primary(primary) => gen_primary(ctx, primary),
    }
}

pub fn gen_stmt(ctx: &mut LanguageContext, stmt: &Stmt) -> Result<(), CompileError> {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::VarDecl { name, typ, value } => {
            let eval = gen_expr(ctx, value.as_ref());
            if eval.is_err() {
                return Err(eval.unwrap_err());
            }
            let eval = eval.unwrap();
            let eval_type = eval.get_type(ctx).name();
            if typ.is_some() && eval_type != typ.as_ref().unwrap().inner.to_typeid().name() {
                return Err(CompileError::with_notes(
                    value.span,
                    format!(
                        "Expression evaluates to type `{}`, which is not expected.",
                        eval_type
                    ),
                    typ.as_ref().unwrap().span,
                    format!(
                        "An expected type was specified here. Compilation will continue as if this was `{}`",
                        eval_type
                    ),
                ));
            }
            let field = Field::new(eval, name.inner.clone());
            ctx.add_field(name.inner.clone(), field);
            Ok(())
        }
        Stmt::VarSet { name, value } => {
            let expr = gen_expr(ctx, value.as_ref())?;
            let field = ctx.get_field(name.clone())?;
            field.release(ctx);
            ctx.get_field_mut(name.clone())?.value = expr;
            Ok(())
        }
        Stmt::Expr(expr) => match gen_expr(ctx, expr.as_ref()) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        },
    }
}

pub fn gen_decl<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    decl: &Decl,
) -> Result<(), CompileError> {
    match decl {
        Decl::Function {
            name,
            generics,
            params,
            returns,
            body,
        } => {
            let scope = ctx.current_scope();
            if scope.contains_key(&name.inner) {
                return Err(CompileError::new(
                    name.span,
                    "A function with this name already exists.".to_string(),
                ));
            }
            for (name, typ) in params {
                ctx.get_with_gen(llvm_ctx, typ.span.make_wrapped(typ.inner.to_typeid()))?;
            }
            let param_types = params
                .iter()
                .map(|(_, t)| ctx.get(t.to_typeid()).storage_type.into())
                .collect::<Vec<_>>();
            let llvm_function_type = if returns.is_some() {
                let returns = returns.clone().unwrap();
                let returns = ctx
                    .get_with_gen(
                        llvm_ctx,
                        returns.span.make_wrapped(returns.inner.to_typeid()),
                    )?
                    .storage_type;
                returns.fn_type(&param_types, false)
            } else {
                llvm_ctx.void_type().fn_type(&param_types, false)
            };
            let llvm_function = ctx.module.add_function(&name, llvm_function_type, None);
            let function_type = TypeID::new(
                "Function".to_string(),
                vec![
                    TypeID::new(
                        "Tuple".to_string(),
                        params
                            .iter()
                            .map(|(_, typ)| typ.inner.to_typeid())
                            .collect(),
                    ),
                    TypeID::from_base("Unit".to_string()),
                ],
            );
            let function = Function::from_function(llvm_ctx, ctx, llvm_function, function_type);
            ctx.add_field(
                name.inner.clone(),
                Field::new(ValueEnum::Function(function), name.inner.clone()),
            );

            let prev = ctx.builder.get_insert_block().unwrap();
            let entry = llvm_ctx.append_basic_block(llvm_function, "entry");
            ctx.builder.position_at_end(entry);
            ctx.push_scope();

            for ((name, typ), value) in params.iter().zip(llvm_function.get_params()) {
                let value = ValueEnum::from_val(ctx, value, typ.to_typeid(), name.inner.clone());
                ctx.add_field(name.inner.clone(), Field::new(value, name.inner.clone()));
            }

            for stmt in body {
                gen_stmt(ctx, stmt)?
            }

            ctx.builder.build_return(None);
            ctx.pop_scope();
            ctx.builder.position_at_end(prev);

            Ok(())
        }
    }
}

pub fn do_codegen<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    path: PathBuf,
    program: Program,
) {
    let filename = path
        .clone()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut file = File::open(path).expect("Cannot read path!");
    let mut src = "".to_string();
    file.read_to_string(&mut src).unwrap();

    ctx.push_scope();

    for decl in program.decls.iter() {
        match gen_decl(llvm_ctx, ctx, decl) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }

    let main = ctx.get_field_nospan("main".to_string());
    match main {
        Some(main) => {
            main.value
                .clone()
                .try_as_function()
                .unwrap()
                .call(ctx, vec![], "res".to_string())
        }
        None => panic!("Requires a main() function."),
    };

    ctx.pop_scope();

    for err in ctx.errors.clone() {
        let mut report = Report::build(
            ReportKind::Error,
            (filename.clone(), err.message_span().into_range()),
        )
        .with_message("Compiler Error")
        .with_label(
            Label::new((filename.clone(), err.message_span().into_range()))
                .with_message(err.message().to_string())
                .with_color(Color::Red),
        );

        for (span, note) in err.notes() {
            report = report.with_label(
                Label::new((filename.clone(), span.into_range()))
                    .with_message(note)
                    .with_color(Color::Blue),
            )
        }

        report
            .finish()
            .eprint((filename.clone(), Source::from(&src)))
            .unwrap();
    }
}
