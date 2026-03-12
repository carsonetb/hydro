use chumsky::span::SimpleSpan;

use crate::{
    callable::Callable,
    context::LanguageContext,
    int::Int,
    parser::{Atom, Expr, ParseLiteral, Primary, Program, Stmt},
    value::{Field, Literal, Value, ValueEnum},
};

#[derive(Debug, Clone)]
pub struct CompileError {
    span: SimpleSpan,
    reason: String,
}

impl CompileError {
    fn new(span: SimpleSpan, reason: String) -> Self {
        Self { span, reason }
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
            let value = gen_expr(ctx, value.as_ref());
            if value.is_err() {
                return Err(value.unwrap_err());
            }
            let value = value.unwrap();
            if value.get_type(ctx).name() != typ.inner.clone() {
                return Err(CompileError::new(span, reason));
            }
            let field = Field::new(value, name.inner.clone());
            ctx.add_field(name.inner.clone(), field);
            Ok(())
        }
        Stmt::VarSet { name, value } => {
            let expr = gen_expr(ctx, value.as_ref());
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

pub fn do_codegen(ctx: &mut LanguageContext, program: Program) {
    ctx.push_scope();

    for stmt in program.stmts.iter() {
        gen_stmt(ctx, stmt);
    }

    ctx.pop_scope();
}
