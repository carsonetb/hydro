use crate::{
    callable::{Callable, Function},
    context::LanguageContext,
    int::Int,
    parser::{Atom, Expr, ParseLiteral, Primary, Program, Stmt},
    value::{Field, Literal, Value, ValuePtr},
};

pub fn gen_literal<'ctx>(ctx: &LanguageContext<'ctx>, literal: &ParseLiteral) -> ValuePtr<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => {
            ValuePtr::PInt(Int::from_literal(ctx, int.clone(), "int".to_string()))
        }
    }
}

pub fn gen_atom<'ctx>(ctx: &LanguageContext<'ctx>, atom: &Atom) -> ValuePtr<'ctx> {
    match atom {
        Atom::Literal(literal) => gen_literal(ctx, literal),
    }
}

pub fn gen_primary<'ctx>(ctx: &LanguageContext<'ctx>, prim: &Primary) -> ValuePtr<'ctx> {
    match prim {
        Primary::Atom(atom) => gen_atom(ctx, atom),
    }
}

pub fn gen_expr<'ctx>(ctx: &LanguageContext<'ctx>, expr: &Expr) -> ValuePtr<'ctx> {
    match expr {
        Expr::Unary(op, right) => todo!(),
        Expr::Binary(left, op, right) => {
            let left = gen_expr(ctx, left);
            let right = gen_expr(ctx, right);
            let left_type = left.get_type(ctx);
            let right_type = right.get_type(ctx);
            assert_eq!(left_type, right_type);
            let op_fn = ctx
                .get(left_type.clone())
                .member(ctx, op.clone())
                .unwrap()
                .load::<Function<'ctx>>(ctx, "binary_fn".to_string())
                .unwrap();
            op_fn.verify(vec![left_type, right_type]);
            op_fn.call(ctx, vec![left, right], "binary".to_string())
        }
        Expr::Primary(primary) => gen_primary(ctx, primary),
    }
}

pub fn gen_stmt(ctx: &mut LanguageContext, stmt: &Stmt) {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::VarDecl { name, typ, value } => {
            let value = gen_expr(ctx, value.as_ref());
            assert_eq!(value.get_type(ctx).name(), typ.clone());
            let field = Field::from_value(ctx, value, name.clone());
            ctx.scope.add_field(name.clone(), field);
        }
        Stmt::VarSet { name, value } => {
            let field = ctx
                .scope
                .get_field(name.clone())
                .expect("Need error handling for this.");
            field.store(ctx, gen_expr(ctx, value.as_ref()));
        }
        Stmt::Expr(expr) => {
            gen_expr(ctx, expr.as_ref());
        }
    }
}

pub fn do_codegen(ctx: &mut LanguageContext, program: Program) {
    ctx.scope.push_scope();

    for stmt in program.stmts.iter() {
        gen_stmt(ctx, stmt);
    }

    ctx.scope.pop_scope();
}
