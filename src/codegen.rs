use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::span::SimpleSpan;

use crate::{
    callable::Callable,
    context::LanguageContext,
    int::Int,
    parser::{Atom, Expr, ParseLiteral, Primary, Program, Stmt},
    value::{Field, Literal, Value, ValueEnum},
};

#[derive(Debug, Clone)]
pub struct CompileError(Vec<(SimpleSpan, String)>);

impl CompileError {
    fn new(span: SimpleSpan, reason: String) -> Self {
        Self(vec![(span, reason)])
    }

    fn with_notes(msg_span: SimpleSpan, msg: String, note_span: SimpleSpan, note: String) -> Self {
        Self(vec![(msg_span, msg), (note_span, note)])
    }

    fn message_span(&self) -> SimpleSpan {
        self.0[0].0
    }

    fn message(&self) -> String {
        self.0[0].1.clone()
    }

    fn notes(&self) -> Vec<(SimpleSpan, String)> {
        self.0[1..].to_vec()
    }
}

pub fn gen_literal<'ctx>(ctx: &LanguageContext<'ctx>, literal: &ParseLiteral) -> ValueEnum<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => {
            ValueEnum::Int(Int::from_literal(ctx, int.clone(), "int".to_string()))
        }
    }
}

pub fn gen_atom<'ctx>(ctx: &LanguageContext<'ctx>, atom: &Atom) -> ValueEnum<'ctx> {
    match atom {
        Atom::Literal(literal) => gen_literal(ctx, literal),
    }
}

pub fn gen_primary<'ctx>(ctx: &LanguageContext<'ctx>, prim: &Primary) -> ValueEnum<'ctx> {
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
            let left = gen_expr(ctx, left).unwrap();
            let right = gen_expr(ctx, right).unwrap();
            let left_type = left.get_type(ctx);
            let right_type = right.get_type(ctx);
            if left_type != right_type {
                return Err(CompileError::new(
                    op.span,
                    format!("Cannot use operator '{}' on different types!", op.inner),
                ));
            }
            let op_fn = ctx
                .get(left_type.clone())
                .member(ctx, op.inner.clone(), op.inner.clone())
                .try_as_function()
                .unwrap();
            op_fn.verify(vec![left_type, right_type]);
            Ok(op_fn.call(ctx, vec![left, right], "binary".to_string()))
        }
        Expr::Primary(primary) => Ok(gen_primary(ctx, primary)),
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
            let field = ctx.get_field(name.inner.clone());
            field.release(ctx);
            ctx.get_field_mut(name.inner.clone()).value = expr;
            Ok(())
        }
        Stmt::Expr(expr) => match gen_expr(ctx, expr.as_ref()) {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        },
    }
}

pub fn do_codegen(ctx: &mut LanguageContext, path: PathBuf, program: Program) {
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

    for stmt in program.stmts.iter() {
        match gen_stmt(ctx, stmt) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }

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
