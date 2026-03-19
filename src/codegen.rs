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
    values::BasicValue,
};

use crate::{
    bool::Bool,
    callable::{Callable, Function},
    context::LanguageContext,
    int::Int,
    parser::{Atom, Decl, Expr, ParseLiteral, Primary, Program, Stmt},
    string::Str,
    types::{Metatype, TypeID},
    unit::Unit,
    value::{Field, Literal, Value, ValueEnum, any_to_basic},
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

pub fn gen_literal<'ctx>(
    ctx: &LanguageContext<'ctx>,
    literal: &ParseLiteral,
    name: String,
) -> ValueEnum<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => ValueEnum::Int(Int::from_literal(ctx, *int, name)),
        ParseLiteral::Bool(bool) => ValueEnum::Bool(Bool::from_literal(ctx, *bool, name)),
        ParseLiteral::String(str) => ValueEnum::String(Str::from_literal(ctx, str.clone(), name)),
    }
}

pub fn gen_atom<'ctx>(
    ctx: &LanguageContext<'ctx>,
    atom: &Atom,
    into_name: String,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match atom {
        Atom::Literal(literal) => Ok(gen_literal(ctx, literal, into_name)),
        Atom::Grouping(expr) => gen_expr(ctx, expr, into_name),
        Atom::Var(name) => ctx.get_field(name.clone()).map(|val| val.value.clone()),
    }
}

pub fn gen_primary<'ctx>(
    ctx: &LanguageContext<'ctx>,
    prim: &Primary,
    into_name: String,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => gen_atom(ctx, atom, into_name),
        Primary::Call { on, generics, args } => {
            let on_eval = gen_primary(ctx, on, format!("{}_callee", into_name))?;
            let on_typ = on_eval.get_type(ctx);
            let mut args_eval = Vec::<ValueEnum<'ctx>>::new();
            for (i, arg) in args.iter().enumerate() {
                args_eval.push(gen_expr(
                    ctx,
                    arg,
                    format!("{}_callee_arg{}", into_name, i),
                )?);
            }
            Ok((if on_eval.clone().try_as_function().is_some() {
                Ok(on_eval
                    .try_as_function()
                    .unwrap()
                    .call(ctx, args_eval, into_name))
            } else if on_eval.clone().try_as_member_function().is_some() {
                Ok(on_eval
                    .try_as_member_function()
                    .unwrap()
                    .call(ctx, args_eval, into_name))
            } else {
                Err(CompileError::new(
                    on.span,
                    format!("Expression does not evaluate to a Function type, instead it is of type `{}`", on_typ),
                ))
            })?
            .map_err(|err| CompileError::new(on.span, err))?)
        }
        Primary::Member { on, name } => {
            let on = gen_primary(ctx, on, format!("{}_on", into_name)).unwrap();
            on.member(ctx, name.clone(), into_name)
        }
    }
}

pub fn gen_expr<'ctx>(
    ctx: &LanguageContext<'ctx>,
    expr: &Expr,
    into_name: String,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match expr {
        Expr::Unary(op, right) => todo!(),
        Expr::Binary(left, op, right) => {
            let left_val = gen_expr(ctx, left, format!("{}_left", into_name))?;
            let right_val = gen_expr(ctx, right, format!("{}_right", into_name))?;
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
            Ok(op_fn
                .call(ctx, vec![left_val, right_val], into_name)
                .map_err(|err| CompileError::new(op.span, err)))?
        }
        Expr::Primary(primary) => gen_primary(ctx, primary, into_name),
    }
}

pub fn gen_stmt<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    stmt: &Stmt,
) -> Result<Option<ValueEnum<'ctx>>, CompileError> {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::VarDecl { name, typ, value } => {
            let eval = gen_expr(ctx, value.as_ref(), name.inner.clone());
            if eval.is_err() {
                return Err(eval.unwrap_err());
            }
            let eval = eval.unwrap();
            let eval_type = eval.get_type(ctx).name();
            if typ.is_some() && eval_type != typ.as_ref().unwrap().to_typeid().name() {
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
            Ok(None)
        }
        Stmt::VarSet { name, value } => {
            let expr = gen_expr(ctx, value.as_ref(), name.inner.clone())?;
            let field = ctx.get_field(name.clone())?;
            field.release(ctx);
            ctx.get_field_mut(name.clone())?.value = expr;
            Ok(None)
        }
        Stmt::Expr(expr) => match gen_expr(ctx, expr.as_ref(), "UNUSED".to_string()) {
            Ok(_) => Ok(None),
            Err(err) => Err(err),
        },
        Stmt::Return(expr) => match expr {
            Some(expr) => gen_expr(ctx, expr, "RETURN".to_string()).map(|val| Some(val)),
            None => Ok(Some(ValueEnum::Unit(Unit {}))),
        },
    }
}

pub fn gen_decl_pre<'ctx>(
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
                ctx.get_with_gen(llvm_ctx, typ.span.make_wrapped(typ.to_typeid()))?;
            }
            let param_types = params
                .iter()
                .map(|(_, t)| {
                    any_to_basic(ctx.get(t.to_typeid()).storage_type)
                        .unwrap()
                        .into()
                })
                .collect::<Vec<_>>();
            let llvm_function_type = if returns.is_some() {
                let returns = returns.clone().unwrap();
                let returns = any_to_basic(
                    ctx.get_with_gen(llvm_ctx, returns.span.make_wrapped(returns.to_typeid()))?
                        .storage_type,
                )
                .unwrap();
                returns.fn_type(&param_types, false)
            } else {
                llvm_ctx.void_type().fn_type(&param_types, false)
            };
            let llvm_function =
                ctx.module
                    .add_function(&format!("User__{}", name.inner), llvm_function_type, None);
            let function_type = TypeID::new(
                "Function".to_string(),
                vec![
                    TypeID::new(
                        "Tuple".to_string(),
                        params.iter().map(|(_, typ)| typ.to_typeid()).collect(),
                    ),
                    if returns.is_some() {
                        returns.as_ref().unwrap().to_typeid()
                    } else {
                        TypeID::from_base("Unit".to_string())
                    },
                ],
            );
            let function = Function::from_function(llvm_ctx, ctx, llvm_function, function_type);
            ctx.add_field(
                name.inner.clone(),
                Field::new(ValueEnum::Function(function), name.inner.clone()),
            );

            Ok(())
        }
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
            let returns_spanned = returns;
            let returns = match returns {
                Some(returns) => returns.to_typeid(),
                None => TypeID::from_base("Unit".to_string()),
            };

            let name = name.span.make_wrapped(format!("User__{}", name.inner));
            let function = ctx.module.get_function(name.as_str()).unwrap();
            let prev = ctx.builder.get_insert_block().unwrap();
            let entry = llvm_ctx.append_basic_block(function, "entry");
            ctx.builder.position_at_end(entry);
            ctx.push_scope();

            for ((name, typ), value) in params.iter().zip(function.get_params()) {
                value.set_name(&name.inner);
                let value = ValueEnum::from_val(ctx, value, typ.to_typeid(), name.inner.clone());
                ctx.add_field(name.inner.clone(), Field::new(value, name.inner.clone()));
            }

            let mut returned = false;
            for stmt in body.inner.iter() {
                match gen_stmt(ctx, &stmt.inner)? {
                    Some(ret) => {
                        if ret.get_type(ctx) != returns {
                            let mut out = CompileError::new(
                                stmt.span,
                                format!("Incorrect return type, expected `{}`", returns),
                            );
                            if returns_spanned.is_some() {
                                out.0.push((
                                    returns_spanned.as_ref().unwrap().span,
                                    "Return type specified here.".to_string(),
                                ));
                            }
                            return Err(out);
                        }
                        let ret_value: Option<&dyn BasicValue<'ctx>> =
                            if ret.clone().try_as_unit().is_none() {
                                Some(&ret.get_value())
                            } else {
                                None
                            };
                        ctx.builder.build_return(ret_value);
                        returned = true;
                        break;
                    }
                    None => continue,
                }
            }

            if !returned {
                if returns != TypeID::from_base("Unit".to_string()) {
                    return Err(CompileError::with_notes(
                        body.span,
                        "Function does not always return.".to_string(),
                        returns_spanned.as_ref().unwrap().span,
                        "Return type specified here.".to_string(),
                    ));
                }
                ctx.builder.build_return(None);
            }
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

    ctx.init_metatypes(&llvm_ctx);

    for decl in program.decls.iter() {
        match gen_decl_pre(llvm_ctx, ctx, decl) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }

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
