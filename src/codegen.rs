use crate::{
    context::LanguageContext,
    parser::{Expr, Program, Stmt},
    value::{ValueField, ValuePtr},
};

pub fn gen_expr<'ctx>(ctx: &mut LanguageContext, expr: &Expr) -> ValuePtr<'ctx> {
    todo!()
}

pub fn gen_stmt(ctx: &mut LanguageContext, stmt: &Stmt) {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::VarDecl { name, typ, value } => {
            let value = gen_expr(ctx, value.as_ref());
            assert_eq!(value.get_type(ctx).name, typ.clone());
            let field = ValueField::from_value(ctx, value, name.clone());
            ctx.scope.add_field(name.clone(), field);
        }
        Stmt::VarSet { name, value } => todo!(),
        Stmt::Expr(expr) => todo!(),
    }
}

pub fn do_codegen(ctx: &mut LanguageContext, program: Program) {
    for stmt in program.stmts.iter() {
        gen_stmt(ctx, stmt);
    }
}
