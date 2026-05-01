use std::{
    any::Any,
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::span::{SimpleSpan, Spanned, WrappingSpan};
use inkwell::{
    basic_block::BasicBlock,
    context::Context,
    module::Linkage,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType},
    values::{BasicValue, BasicValueEnum, FunctionValue, PointerValue},
};
use strum_macros::{EnumIs, EnumTryAs};

use crate::{
    bool::Bool,
    buildscript::{LinkInfo, run_buildscript},
    callable::{Callable, Function},
    classes::{Class, ClassInfo, ClassMember},
    context::LanguageContext,
    ffi::compile_ffi,
    float::Float,
    int::Int,
    parser::{
        Atom, Break, Continue, Decl, Eval, Expr, For, FunctionDecl, If, Match, ParseLiteral,
        ParserType, Primary, Program, Return, Set, Stmt, Var, While,
    },
    string::Str,
    types::{BasicBuiltin, ClassBuilder, Metatype, MetatypeBuilder, TypeID},
    unit::Unit,
    value::{Copyable, Field, Literal, Value, ValueEnum, ValueRef, any_to_basic},
    vector::Vector,
};

#[derive(Debug, Clone)]
pub struct CompileError(Vec<(SimpleSpan, String)>);

impl CompileError {
    pub fn new(span: SimpleSpan, reason: &str) -> Self {
        Self(vec![(span, reason.to_string())])
    }

    pub fn with_notes(msg_span: SimpleSpan, msg: &str, note_span: SimpleSpan, note: &str) -> Self {
        Self(vec![
            (msg_span, msg.to_string()),
            (note_span, note.to_string()),
        ])
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

#[derive(Debug, Clone, EnumIs, EnumTryAs)]
pub enum StmtEval<'ctx> {
    None,
    Break(Option<Spanned<String>>),
    Continue(Option<Spanned<String>>),
    Eval(Option<Spanned<String>>, Spanned<ValueEnum<'ctx>>),
    Return(Spanned<ValueEnum<'ctx>>),
}

pub fn gen_literal<'ctx>(
    ctx: &LanguageContext<'ctx>,
    literal: &ParseLiteral,
    name: &str,
) -> ValueEnum<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => ValueEnum::Int(Int::from_literal(ctx, *int, name)),
        ParseLiteral::Float(float) => ValueEnum::Float(Float::from_literal(ctx, *float, name)),
        ParseLiteral::Bool(bool) => ValueEnum::Bool(Bool::from_literal(ctx, *bool, name)),
        ParseLiteral::String(str) => ValueEnum::String(Str::from_literal(ctx, str.clone(), name)),
    }
}

struct FunctionInfo<'ctx> {
    function: FunctionValue<'ctx>,
    returns: TypeID,
    returns_spanned: Option<ParserType>,
}

fn gen_atom<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    atom: &Atom,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match atom {
        Atom::Literal(literal) => Ok(gen_literal(ctx, literal, into_name)),
        Atom::Grouping(expr) => gen_expr(ctx, info, expr, into_name),
        Atom::Var(name) => ctx.get_field(name.clone()).map(|val| val.value.clone()),
        Atom::Array(exprs) => {
            if exprs.is_empty() {
                return Err(CompileError::new(
                    exprs.span,
                    "Cannot (currently) infer type of an empty array.",
                ));
            }
            let mut values = Vec::<ValueEnum<'ctx>>::new();
            for (i, expr) in exprs.iter().enumerate() {
                values.push(gen_expr(
                    ctx,
                    info,
                    expr,
                    &format!("{}_elem{}", into_name, i),
                )?);
            }
            let vec_type = TypeID::new("Vector", vec![values[0].get_type()]);
            ctx.get_with_gen_ext(vec_type.clone());
            let vec = Vector::new(ctx, vec_type.clone(), into_name);
            for (i, val) in values.iter().enumerate() {
                if val.get_type() != vec_type.generics[0] {
                    return Err(CompileError::new(
                        exprs.span,
                        "Not all elements in the array have the same type.",
                    ));
                }
                vec.push(ctx, val, &format!("{}_elem{}", into_name, i));
            }
            Ok(ValueEnum::Vector(vec))
        }
        Atom::Block(stmts) => match gen_stmts(ctx, stmts, info, into_name)? {
            StmtEval::Eval { 0: name, 1: value } => {
                if let Some(name) = name {
                    return Err(CompileError::new(
                        name.span,
                        "No higher order statement to jump to (this is a block).",
                    ));
                };
                Ok(value.inner)
            }
            _ => Err(CompileError::new(
                stmts.last().unwrap().span,
                "Must evaluate with `eval` keyword.",
            )),
        },
        Atom::Tuple(values) => todo!(),
    }
}

fn gen_primary<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    prim: &Primary,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => gen_atom(ctx, info, atom, into_name),
        Primary::Call { on, generics, args } => {
            let on_eval = gen_primary(ctx, info, on, &format!("{}_callee", into_name))?;
            let on_typ = on_eval.get_type();
            let mut args_eval = Vec::<ValueEnum<'ctx>>::new();
            for (i, arg) in args.iter().enumerate() {
                args_eval.push(gen_expr(
                    ctx,
                    info,
                    arg,
                    &format!("{}_callee_arg{}", into_name, i),
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
            } else if on_eval.clone().try_as_type().is_some() {
                Ok(on_eval.try_as_type().unwrap().initializer.unwrap().call(ctx, args_eval, into_name))
            } else {
                Err(CompileError::new(
                    on.span,
                    &format!("Expression does not evaluate to a Function type, instead it is of type `{}`", on_typ),
                ))
            })?
            .map_err(|err| CompileError::new(on.span, &err))?)
        }
        Primary::Member { on, name } => {
            let on = gen_primary(ctx, info, on, &format!("{}_on", into_name))?;
            on.member(ctx, name.clone(), into_name)
        }
        Primary::Slice { on, expr } => {
            let on_eval = gen_primary(ctx, info, on, &format!("{}_callee", into_name))?;
            let on_typ = on_eval.get_type();
            let slice_eval = gen_expr(ctx, info, expr, &format!("{into_name}_slice"))?;
            let slice_fn = on_eval
                .member(
                    ctx,
                    expr.span.make_wrapped("[]".to_string()),
                    &format!("{into_name}_slicer"),
                )?
                .try_as_member_function()
                .unwrap();
            Ok(slice_fn
                .call(ctx, vec![slice_eval], into_name)
                .map_err(|err| CompileError::new(on.span, &err))?)
        }
    }
}

fn gen_primary_ref<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    prim: &Primary,
    into_name: &str,
) -> Result<ValueRef<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => match atom.inner.as_ref() {
            _ => Err(CompileError::with_notes(
                atom.span,
                "Cannot get a reference to this value.",
                atom.span,
                "You are probably trying to set to a non-field value (e.g. 1 = 2, [1, 2] = 5, {eval x;} = 5) which is invalid.",
            )),
            Atom::Var(ident) => todo!("This is not a reference, it's resetting the field."),
        },
        Primary::Call { on, generics, args } => Err(CompileError::with_notes(
            on.span,
            "Cannot get a reference to the field returned by a function call.",
            on.span,
            "This could technically be valid if the function returned a field, but that would have to be explicit which is not supported.",
        )),
        Primary::Member { on, name } => {
            let on = gen_primary(ctx, info, on, &format!("{into_name}_on"))?;
            on.member_ref(ctx, name.clone(), into_name)
        }
        Primary::Slice { on, expr } => {
            Err(CompileError::new(on.span, "Cannot yet set to a slice."))
        }
    }
}

fn gen_expr<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    expr: &Expr,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match expr {
        Expr::Unary(op, right) => todo!(),
        Expr::Binary(left, op, right) => {
            let left_val = gen_expr(ctx, info, left, &format!("{}_left", into_name))?;
            let right_val = gen_expr(ctx, info, right, &format!("{}_right", into_name))?;
            let left_type = left_val.get_type();
            let right_type = right_val.get_type();
            if left_type != right_type {
                return Err(CompileError(vec![
                    (
                        op.span,
                        format!("Cannot use operator `{}` on different types.", op.inner),
                    ),
                    (
                        left.span,
                        format!("Left operator is of type `{}`.", left_val.get_type()),
                    ),
                    (
                        right.span,
                        format!("Right operator is of type `{}`.", right_val.get_type()),
                    ),
                ]));
            }
            let op_fn = left_val
                .member(ctx, op.clone(), &op.inner)?
                .try_as_function()
                .unwrap();
            op_fn.verify(vec![left_type, right_type]);
            Ok(op_fn
                .call(ctx, vec![left_val, right_val], into_name)
                .map_err(|err| CompileError::new(op.span, &err)))?
        }
        Expr::Primary(primary) => gen_primary(ctx, info, primary, into_name),
        Expr::Var(var) => todo!(),
        Expr::Set(set) => todo!(),
        Expr::If(if_stmt) => {
            let if_eval = gen_if(ctx, info, if_stmt, into_name)?;
            if if_eval.is_eval() {
                Ok(if_eval.try_as_eval().unwrap().1.inner)
            } else {
                Err(CompileError::new(
                    if_stmt.condition.span,
                    "If stmt must evaluate on all code paths.",
                ))
            }
        }
        Expr::For(_) => todo!(),
        Expr::Match(_) => todo!(),
        Expr::While(_) => todo!(),
        Expr::Combo(exprs) => todo!(),
    }
}

fn gen_match<'ctx>(
    ctx: &LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    stmt: &Match,
    into_name: &str,
) -> Result<StmtEval<'ctx>, CompileError> {
    todo!()
}

fn lift_eval<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    mem: &mut Option<PointerValue<'ctx>>,
    prev_block: BasicBlock<'ctx>,
    current_block: BasicBlock<'ctx>,
    continued_block: BasicBlock<'ctx>,
    value: &ValueEnum<'ctx>,
) -> Option<PointerValue<'ctx>> {
    if value.is_unit() {
        return None;
    }
    match prev_block.get_first_instruction() {
        Some(inst) => ctx.builder.position_before(&inst),
        None => ctx.builder.position_at_end(prev_block),
    }
    let out = if mem.is_none() {
        Some(
            ctx.builder
                .build_alloca(ctx.get_storage(value.get_type()), "if_eval_ptr")
                .unwrap(),
        )
    } else {
        *mem
    };
    ctx.builder.position_at_end(current_block);
    ctx.builder.build_store(out.unwrap(), value.get_value());
    out
}

fn gen_if<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    If {
        name,
        condition,
        then,
        elifs,
        else_block,
    }: &If,
    into_name: &str,
) -> Result<StmtEval<'ctx>, CompileError> {
    let truthy_block = ctx.context.append_basic_block(info.function, "truthy");
    let falsey_block = ctx.context.append_basic_block(info.function, "falsey");
    let continued_block = ctx.context.append_basic_block(info.function, "continue");

    let cond_eval = gen_expr(ctx, info, condition, "while_condition")?;
    if cond_eval.get_type() != TypeID::from_base("Bool") {
        return Err(CompileError::new(
            condition.span,
            &format!(
                "Expression must evaluate to type `Bool` in an `if` statement, instead it evaluates to type `{}`.",
                cond_eval.get_type()
            ),
        ));
    }
    ctx.builder
        .build_conditional_branch(
            cond_eval.get_value().into_int_value(),
            truthy_block,
            falsey_block,
        )
        .unwrap();

    let prev_block = ctx.builder.get_insert_block().unwrap();
    ctx.builder.position_at_end(truthy_block);
    let mut if_returns = gen_stmts(ctx, then, info, into_name)?;

    let mut eval_mem: Option<PointerValue<'ctx>> = None;

    match &if_returns {
        StmtEval::Eval(_, Spanned { inner: value, span }) => {
            eval_mem = lift_eval(
                ctx,
                &mut eval_mem,
                prev_block,
                truthy_block,
                continued_block,
                value,
            );
            ctx.builder
                .build_unconditional_branch(continued_block)
                .unwrap();
        }
        StmtEval::Return(spanned) => (),
        _ => {
            ctx.builder
                .build_unconditional_branch(continued_block)
                .unwrap();
        }
    }

    ctx.builder.position_at_end(falsey_block);

    let mut elifs = elifs.clone();

    if !elifs.is_empty() {
        let (name, cond, then) = elifs.remove(0);
        let elif_returns = gen_if(
            ctx,
            info,
            &If {
                condition: cond,
                then,
                elifs: elifs.clone(),
                else_block: else_block.clone(),
                name,
            },
            into_name,
        )?;
        if_returns = if matches!(if_returns, elif_returns) {
            elif_returns
        } else {
            StmtEval::None
        };
        if if_returns.is_eval() {
            eval_mem = lift_eval(
                ctx,
                &mut eval_mem,
                prev_block,
                ctx.builder.get_insert_block().unwrap(),
                continued_block,
                &if_returns.clone().try_as_eval().unwrap().1.inner,
            )
        };
    } else if else_block.is_some() {
        let else_returns = gen_stmts(ctx, else_block.as_ref().unwrap(), info, into_name)?;
        if_returns = if matches!(if_returns, else_returns) {
            else_returns
        } else {
            StmtEval::None
        };
        if if_returns.is_eval() {
            eval_mem = lift_eval(
                ctx,
                &mut eval_mem,
                prev_block,
                falsey_block,
                continued_block,
                &if_returns.clone().try_as_eval().unwrap().1.inner,
            )
        };
    } else {
        if_returns = StmtEval::None
    }

    if !if_returns.is_return() {
        ctx.builder.build_unconditional_branch(continued_block);
        ctx.builder.position_at_end(continued_block);
    }

    if if_returns.is_eval() {
        let (name, value) = if_returns.try_as_eval().unwrap();
        let loaded = ctx
            .builder
            .build_load(
                ctx.get_storage(value.inner.get_type()),
                eval_mem.unwrap(),
                into_name,
            )
            .unwrap();
        let span = value.span;
        let loaded_wrapped = ValueEnum::from_val(ctx, loaded, value.get_type(), into_name);
        if_returns = StmtEval::Eval(name, value.span.make_wrapped(loaded_wrapped));
    }

    Ok(if_returns)
}

fn gen_var_decl<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    name: &Spanned<String>,
    typ: &Option<ParserType>,
    value: &Spanned<Box<Expr>>,
) -> Result<StmtEval<'ctx>, CompileError> {
    let eval = gen_expr(ctx, info, value.as_ref(), &name.inner)?;
    let eval_type = eval.get_type().name();
    if typ.is_some() && eval_type != typ.as_ref().unwrap().to_typeid().name() {
        return Err(CompileError::with_notes(
            value.span,
            &format!(
                "Expression evaluates to type `{}`, which is not expected.",
                eval_type
            ),
            typ.as_ref().unwrap().span,
            &format!(
                "An expected type was specified here. Compilation will continue as if this was `{}`",
                eval_type
            ),
        ));
    }
    let field = Field::new(eval, &name.inner);
    ctx.add_field(&name.inner, field);
    Ok(StmtEval::None)
}

fn gen_stmt<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    info: &FunctionInfo<'ctx>,
    stmt: &Stmt,
    into_name: &str,
) -> Result<StmtEval<'ctx>, CompileError> {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::Var(Var {
            annotations,
            name,
            typ,
            value,
        }) => gen_var_decl(ctx, info, name, typ, value),
        Stmt::Set(Set { on, value }) => {
            let expr = gen_expr(ctx, info, value.as_ref(), "UNUSED_setas")?;
            let field = gen_primary_ref(ctx, info, on, "UNUSED_setto")?;
            if field.typ != expr.get_type() {
                return Err(CompileError::with_notes(
                    value.span,
                    &format!(
                        "Type of expression, `{}`, is different from the type of the field.",
                        expr.get_type()
                    ),
                    on.span,
                    &format!("The type of the field is `{}`", field.typ),
                ));
            };
            ctx.builder.build_store(field.ptr, expr.get_value());

            Ok(StmtEval::None)
        }
        Stmt::Expr(expr) => match gen_expr(ctx, info, expr.as_ref(), "UNUSED") {
            Ok(_) => Ok(StmtEval::None),
            Err(err) => Err(err),
        },
        Stmt::While(While {
            condition,
            inner,
            name,
        }) => {
            let cond_block = ctx.context.append_basic_block(info.function, "while_cond");
            let loop_block = ctx.context.append_basic_block(info.function, "while_loop");
            let continued_block = ctx.context.append_basic_block(info.function, "continue");

            ctx.builder.build_unconditional_branch(cond_block);
            ctx.builder.position_at_end(cond_block);

            let cond_eval = gen_expr(ctx, info, condition, "while_condition")?;
            if cond_eval.get_type() != TypeID::from_base("Bool") {
                return Err(CompileError::new(
                    condition.span,
                    &format!(
                        "Expression must evaluate to type `Bool` in a `while` loop, instead it evaluates to type `{}`.",
                        cond_eval.get_type()
                    ),
                ));
            }
            ctx.builder
                .build_conditional_branch(
                    cond_eval.get_value().into_int_value(),
                    loop_block,
                    continued_block,
                )
                .unwrap();
            ctx.builder.position_at_end(loop_block);

            let returns = gen_stmts(ctx, inner, info, into_name)?;
            if returns.is_none() {
                ctx.builder.build_unconditional_branch(cond_block);
            } // TODO: Warning here

            ctx.builder.position_at_end(continued_block);
            Ok(returns)
        }
        Stmt::For(For {
            looper,
            loopee,
            block,
            name,
        }) => {
            let loopee_eval = gen_expr(ctx, info, loopee, "loopee")?;
            let as_vec = loopee_eval.clone().try_as_vector();
            if as_vec.is_none() {
                return Err(CompileError::new(
                    loopee.span,
                    &format!(
                        "Loopee of a `for` loop must be of type `Vec`, instead it is of type `{}`",
                        loopee_eval.get_type()
                    ),
                ));
            }
            let as_vec = as_vec.unwrap();
            let len = as_vec.len(ctx, "loopee_length");
            let ind = ctx
                .builder
                .build_alloca(ctx.types.int, "looper_ind_ptr")
                .unwrap();
            ctx.builder.build_store(ind, ctx.int(0));

            let cond_block = ctx.context.append_basic_block(info.function, "for_cond");
            let loop_block = ctx.context.append_basic_block(info.function, "for_loop");
            let continue_block = ctx.context.append_basic_block(info.function, "continue");

            ctx.builder.build_unconditional_branch(cond_block);
            ctx.builder.position_at_end(cond_block);

            let ind_val = ctx
                .builder
                .build_load(ctx.types.int, ind, "loopee_ind")
                .unwrap()
                .into_int_value();
            ctx.builder.build_conditional_branch(
                ctx.builder
                    .build_int_compare(inkwell::IntPredicate::SLT, ind_val, len, "should_loop")
                    .unwrap(),
                loop_block,
                continue_block,
            );

            ctx.builder.position_at_end(loop_block);

            let vec_item = as_vec.get(ctx, &ind_val, &looper.inner);
            ctx.add_field(&looper.inner, Field::new(vec_item, &looper.inner));
            let returns = gen_stmts(ctx, block, info, into_name)?;
            if returns.is_none() {
                ctx.builder.build_store(
                    ind,
                    ctx.builder
                        .build_int_add(ind_val, ctx.int(1), "looper_index")
                        .unwrap(),
                );
                ctx.builder.build_unconditional_branch(cond_block);
            } // TODO: Warning here

            ctx.builder.position_at_end(continue_block);

            Ok(returns)
        }
        Stmt::If(stmt) => gen_if(ctx, info, stmt, into_name),
        Stmt::Match(stmt) => gen_match(ctx, info, stmt, into_name),
        Stmt::Return(Return { 0: expr }) => match &expr.inner {
            Some(expr) => Ok(StmtEval::Return(expr.span.make_wrapped(gen_expr(
                ctx,
                info,
                &expr.inner,
                "RETURN",
            )?))),
            None => Ok(StmtEval::Return(
                expr.span.make_wrapped(ValueEnum::Unit(Unit {})),
            )),
        },
        Stmt::Eval(Eval { from: name, val }) => Ok(StmtEval::Eval(
            name.clone(),
            match &val.inner {
                Some(val) => val
                    .span
                    .make_wrapped(gen_expr(ctx, info, &val.inner, into_name)?),
                None => val.span.make_wrapped(ValueEnum::Unit(Unit {})),
            },
        )),
        Stmt::Break(Break(name)) => Ok(StmtEval::Break(name.clone())),
        Stmt::Continue(Continue(name)) => Ok(StmtEval::Continue(name.clone())),
    }
}

fn gen_stmts<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    stmts: &[Spanned<Stmt>],
    info: &FunctionInfo<'ctx>,
    into_name: &str,
) -> Result<StmtEval<'ctx>, CompileError> {
    ctx.push_scope();
    for stmt in stmts.iter() {
        let eval = gen_stmt(ctx, info, stmt, into_name)?;
        match eval {
            StmtEval::Return(ref ret) => {
                if ret.get_type() != info.returns {
                    let mut out = CompileError::new(
                        stmt.span,
                        &format!("Incorrect return type, expected `{}`", info.returns),
                    );
                    if info.returns_spanned.is_some() {
                        out.0.push((
                            info.returns_spanned.as_ref().unwrap().span,
                            "Return type specified here.".to_string(),
                        ));
                    }
                    return Err(out);
                }
                let ret_value: Option<&dyn BasicValue<'ctx>> = if !ret.inner.is_unit() {
                    Some(&ret.get_value())
                } else {
                    None
                };
                ctx.builder.build_return(ret_value);
                return Ok(eval);
            }
            StmtEval::None => continue,
            _ => {
                return Ok(eval);
            }
        }
    }
    ctx.pop_scope();
    Ok(StmtEval::None)
}

pub fn gen_decl_pre<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    decl: &Decl,
    inside: Option<TypeID>,
    init_fn: FunctionValue<'ctx>,
) -> Result<(), CompileError> {
    match decl {
        Decl::Function(FunctionDecl {
            annotations,
            name,
            generics,
            params,
            returns,
            body,
        }) => {
            let scope = ctx.current_scope();
            if scope.contains_key(&name.inner) {
                return Err(CompileError::new(
                    name.span,
                    "A function with this name already exists.",
                ));
            }
            for (name, typ) in params {
                ctx.get_with_gen(llvm_ctx, typ.span.make_wrapped(typ.to_typeid()))?;
            }
            let mut param_types = Vec::<BasicMetadataTypeEnum>::new();
            if inside.is_some() {
                param_types.push(ctx.types.ptr.into());
            }
            for (_, typ) in params {
                param_types.push(
                    any_to_basic(ctx.get(typ.to_typeid()).storage_type)
                        .unwrap()
                        .into(),
                );
            }
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
            let fn_name = if inside.is_some() {
                format!("User__{}.{}", inside.clone().unwrap(), name.inner)
            } else {
                format!("User__{}", name.inner)
            };
            let llvm_function = ctx.module.add_function(&fn_name, llvm_function_type, None);
            let mut generics = if inside.is_some() {
                vec![inside.clone().unwrap()]
            } else {
                vec![]
            };
            let params_unbound = params
                .iter()
                .map(|(_, typ)| typ.to_typeid())
                .collect::<Vec<_>>();
            generics.extend(params_unbound.clone());
            let params_typeid = TypeID::new("Tuple", generics);
            let returns_typeid = if returns.is_some() {
                returns.as_ref().unwrap().to_typeid()
            } else {
                TypeID::from_base("Unit")
            };
            let function_type =
                TypeID::new("Function", vec![params_typeid, returns_typeid.clone()]);
            let function = Function::from_function(llvm_ctx, ctx, llvm_function, function_type);
            ctx.add_field(name, Field::new(ValueEnum::Function(function), name));

            Ok(())
        }
        Decl::Class {
            annotations,
            name,
            params,
            decls,
        } => Ok(()),
        Decl::Var(Var {
            annotations,
            name,
            typ,
            value,
        }) => gen_var_decl(
            ctx,
            &FunctionInfo {
                function: init_fn,
                returns: TypeID::from_base("Unit"),
                returns_spanned: None,
            },
            name,
            typ,
            value,
        )
        .map(|x| ()),
    }
}

pub fn gen_decl<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    decl: &Decl,
) -> Result<(), CompileError> {
    match decl {
        Decl::Var(Var {
            annotations,
            name,
            typ,
            value,
        }) => todo!(),
        Decl::Function(FunctionDecl {
            annotations,
            name,
            generics,
            params,
            returns,
            body,
        }) => {
            let returns_spanned = returns;
            let returns = match returns {
                Some(returns) => returns.to_typeid(),
                None => TypeID::from_base("Unit"),
            };

            let name = name.span.make_wrapped(format!("User__{}", name.inner));
            let function = ctx.module.get_function(name.as_str()).unwrap();
            let prev = ctx.builder.get_insert_block().unwrap();
            let entry = llvm_ctx.append_basic_block(function, "entry");
            ctx.builder.position_at_end(entry);
            ctx.push_scope();

            for ((name, typ), value) in params.iter().zip(function.get_params()) {
                value.set_name(&name.inner);
                let value = ValueEnum::from_val(ctx, value, typ.to_typeid(), &name.inner);
                ctx.add_field(&name.inner, Field::new(value, &name.inner));
            }

            let returns_val = gen_stmts(
                ctx,
                &body.inner,
                &FunctionInfo {
                    function,
                    returns: returns.clone(),
                    returns_spanned: returns_spanned.clone(),
                },
                "UNUSED",
            )?;

            if returns_val.is_none() {
                if returns != TypeID::from_base("Unit") {
                    return Err(CompileError::with_notes(
                        body.span,
                        "Function does not always return.",
                        returns_spanned.as_ref().unwrap().span,
                        "Return type specified here.",
                    ));
                }
                ctx.builder.build_return(None);
            }
            ctx.pop_scope();
            ctx.builder.position_at_end(prev);

            Ok(())
        }
        Decl::Class {
            annotations,
            name,
            params,
            decls,
        } => {
            ctx.push_scope();

            let mut builder = ClassBuilder::new(
                ctx,
                name,
                &params
                    .iter()
                    .map(|(name, typ)| (name.clone(), typ.to_typeid()))
                    .collect::<Vec<_>>(),
            );

            let mut functions: Vec<&FunctionDecl> = vec![];
            for decl in decls {
                if let Decl::Function(function_decl) = decl {
                    functions.push(function_decl);
                }
            }

            gen_decls_pre(ctx, decls, Some(TypeID::from_base(name)), builder.init_llvm);

            let scope = ctx.current_scope();
            for ((name, field), decl) in scope.iter().zip(decls) {
                builder.add_member(name, &field.value);
            }

            ctx.pop_scope();

            builder.build(ctx);

            for ((fn_name, fun), decl) in builder.functions.iter().zip(functions) {
                let function_val = ctx
                    .module
                    .get_function(&format!("User__{}.{fn_name}", name.inner))
                    .unwrap();
                let old_block = ctx.begin_function(function_val);
                ctx.push_scope();

                let returns = match &decl.returns {
                    Some(typ) => typ.to_typeid(),
                    None => TypeID::from_base("Unit"),
                };

                let obj = function_val.get_first_param().unwrap().into_pointer_value();
                for (name, member) in &builder.members {
                    let value = ctx.build_ptr_load(
                        builder.class_struct,
                        member.typ.clone(),
                        obj,
                        member.index,
                        name,
                    );
                    let value = ValueEnum::from_val(ctx, value, member.typ.clone(), name);
                    ctx.add_field(name, Field::new(value, name));
                }
                // TODO: add member functions
                for ((name, typ), value) in params.iter().zip(function_val.get_params()).skip(1) {
                    value.set_name(name);
                    let value = ValueEnum::from_val(ctx, value, typ.to_typeid(), name);
                    ctx.add_field(name, Field::new(value, name));
                }

                let returns_val = gen_stmts(
                    ctx,
                    &decl.body.inner,
                    &FunctionInfo {
                        function: function_val,
                        returns: returns.clone(),
                        returns_spanned: decl.returns.clone(),
                    },
                    "UNUSED",
                )?;

                if returns_val.is_none() {
                    if returns != TypeID::from_base("Unit") {
                        return Err(CompileError::with_notes(
                            decl.body.span,
                            "Function does not always return.",
                            decl.returns.as_ref().unwrap().span,
                            "Return type specified here.",
                        ));
                    }
                    ctx.builder.build_return(None);
                }

                ctx.pop_scope();
                ctx.builder.position_at_end(old_block); // TODO: A lot of duplicate code here.
            }

            Ok(())
        }
    }
}

pub fn gen_decls_pre<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    decls: &[Decl],
    inside: Option<TypeID>,
    init_fn: FunctionValue<'ctx>,
) {
    for decl in decls {
        match gen_decl_pre(ctx.context, ctx, decl, inside.clone(), init_fn) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }
}

pub fn gen_decls<'ctx>(ctx: &mut LanguageContext<'ctx>, decls: &[Decl]) {
    for decl in decls.iter() {
        match gen_decl(ctx.context, ctx, decl) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }
}

pub fn do_codegen<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    path: PathBuf,
    program: Program,
    source: &Path,
    build: &Path,
    main: FunctionValue<'ctx>,
) -> Result<LinkInfo, ()> {
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

    ctx.init_metatypes(llvm_ctx);

    let mut link_info = LinkInfo::empty();
    for import in program.imports {
        let mut path = source.to_path_buf();
        for name in import.inner.path {
            path = path.join(name.inner);
        }

        if path.with_extension("hy").exists() {
            // TODO: Importing other files
        } else if path.with_extension("hyi").exists() {
            let stub = path.with_extension("hyi");
            compile_ffi(ctx, &import.span.make_wrapped(stub), build);

            let buildscript = path.with_extension("hyb");
            if buildscript.exists() {
                let maybe_info = run_buildscript(&import.span.make_wrapped(buildscript), build);
                if maybe_info.is_err() {
                    ctx.error(maybe_info.clone().err().unwrap());
                    continue;
                }
                link_info = link_info.merge(maybe_info.unwrap());
            }
        } else {
            ctx.error(CompileError::new(
                import.span,
                &format!(
                    "The path {} could not be found, so it could not be imported.",
                    path.to_str().unwrap()
                ),
            ));
        };
    }

    gen_decls_pre(ctx, &program.decls, None, main);
    gen_decls(ctx, &program.decls);

    let main = ctx.get_field_nospan("main");
    match main {
        Some(main) => main
            .value
            .clone()
            .try_as_function()
            .unwrap()
            .call(ctx, vec![], "res"),
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

    if !ctx.errors.is_empty() {
        Err(())
    } else {
        Ok(link_info)
    }
}
