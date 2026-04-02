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
    context::Context,
    module::Linkage,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType},
    values::{BasicValue, BasicValueEnum, FunctionValue, PointerValue},
};

use crate::{
    bool::Bool,
    buildscript::run_buildscript,
    callable::{Callable, Function},
    classes::{Class, ClassInfo, ClassMember},
    context::LanguageContext,
    int::Int,
    parser::{Atom, Decl, Expr, ParseLiteral, ParserType, Primary, Program, Stmt},
    string::Str,
    types::{BasicBuiltin, Metatype, MetatypeBuilder, TypeID},
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

pub fn gen_literal<'ctx>(
    ctx: &LanguageContext<'ctx>,
    literal: &ParseLiteral,
    name: &str,
) -> ValueEnum<'ctx> {
    match literal {
        ParseLiteral::Error(_) => panic!(),
        ParseLiteral::Int(int) => ValueEnum::Int(Int::from_literal(ctx, *int, name)),
        ParseLiteral::Bool(bool) => ValueEnum::Bool(Bool::from_literal(ctx, *bool, name)),
        ParseLiteral::String(str) => ValueEnum::String(Str::from_literal(ctx, str.clone(), name)),
    }
}

pub fn gen_atom<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    atom: &Atom,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match atom {
        Atom::Literal(literal) => Ok(gen_literal(ctx, literal, into_name)),
        Atom::Grouping(expr) => gen_expr(ctx, expr, into_name),
        Atom::Var(name) => ctx.get_field(name.clone()).map(|val| val.value.clone()),
        Atom::Array(exprs) => {
            if exprs.len() == 0 {
                return Err(CompileError::new(
                    exprs.span,
                    "Cannot (currently) infer type of an empty array.",
                ));
            }
            let mut values = Vec::<ValueEnum<'ctx>>::new();
            for (i, expr) in exprs.iter().enumerate() {
                values.push(gen_expr(ctx, expr, &format!("{}_elem{}", into_name, i))?);
            }
            let vec_type = TypeID::new("Vector", vec![values[0].get_type(ctx)]);
            ctx.get_with_gen_ext(vec_type.clone());
            let vec = Vector::new(ctx, vec_type.clone(), into_name);
            for (i, val) in values.iter().enumerate() {
                if val.get_type(ctx) != vec_type.generics[0] {
                    return Err(CompileError::new(
                        exprs.span,
                        "Not all elements in the array have the same type.",
                    ));
                }
                vec.push(ctx, val, &format!("{}_elem{}", into_name, i));
            }
            Ok(ValueEnum::Vector(vec))
        }
    }
}

pub fn gen_primary<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    prim: &Primary,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => gen_atom(ctx, atom, into_name),
        Primary::Call { on, generics, args } => {
            let on_eval = gen_primary(ctx, on, &format!("{}_callee", into_name))?;
            let on_typ = on_eval.get_type(ctx);
            let mut args_eval = Vec::<ValueEnum<'ctx>>::new();
            for (i, arg) in args.iter().enumerate() {
                args_eval.push(gen_expr(
                    ctx,
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
            let on = gen_primary(ctx, on, &format!("{}_on", into_name))?;
            on.member(ctx, name.clone(), into_name)
        }
        Primary::Slice { on, expr } => {
            let on_eval = gen_primary(ctx, on, &format!("{}_callee", into_name))?;
            let on_typ = on_eval.get_type(ctx);
            let slice_eval = gen_expr(ctx, expr, &format!("{into_name}_slice"))?;
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

pub fn gen_primary_ref<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    prim: &Primary,
    into_name: &str,
) -> Result<ValueRef<'ctx>, CompileError> {
    match prim {
        Primary::Atom(atom) => Err(CompileError::with_notes(
            atom.span,
            "Cannot get a reference to an Atom.",
            atom.span,
            "You are probably trying to set to a non-field value (e.g. 1 = 2, or [1, 2] = 5), which is invalid.",
        )),
        Primary::Call { on, generics, args } => Err(CompileError::with_notes(
            on.span,
            "Cannot get a reference to the field returned by a function call.",
            on.span,
            "This could technically be valid if the function returned a field, but that would have to be explicit which is not supported.",
        )),
        Primary::Member { on, name } => {
            let on = gen_primary(ctx, on, &format!("{into_name}_on"))?;
            on.member_ref(ctx, name.clone(), into_name)
        }
        Primary::Slice { on, expr } => {
            Err(CompileError::new(on.span, "Cannot yet set to a slice."))
        }
    }
}

pub fn gen_expr<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    expr: &Expr,
    into_name: &str,
) -> Result<ValueEnum<'ctx>, CompileError> {
    match expr {
        Expr::Unary(op, right) => todo!(),
        Expr::Binary(left, op, right) => {
            let left_val = gen_expr(ctx, left, &format!("{}_left", into_name))?;
            let right_val = gen_expr(ctx, right, &format!("{}_right", into_name))?;
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
                .member(ctx, op.clone(), &op.inner)?
                .try_as_function()
                .unwrap();
            op_fn.verify(vec![left_type, right_type]);
            Ok(op_fn
                .call(ctx, vec![left_val, right_val], &into_name)
                .map_err(|err| CompileError::new(op.span, &err)))?
        }
        Expr::Primary(primary) => gen_primary(ctx, primary, &into_name),
    }
}

pub fn gen_if<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    stmt: &Stmt,
    function: FunctionValue<'ctx>,
    returns: TypeID,
    returns_spanned: Option<ParserType>,
) -> Result<Option<ValueEnum<'ctx>>, CompileError> {
    match stmt {
        Stmt::If {
            condition,
            then,
            elifs,
            else_block,
        } => {
            let truthy_block = ctx.context.append_basic_block(function, "truthy");
            let falsey_block = ctx.context.append_basic_block(function, "falsey");
            let continued_block = ctx.context.append_basic_block(function, "continue");

            let cond_eval = gen_expr(ctx, condition, "while_condition")?;
            if cond_eval.get_type(ctx) != TypeID::from_base("Bool") {
                return Err(CompileError::new(
                    condition.span,
                    &format!(
                        "Expression must evaluate to type `Bool` in an `if` statement, instead it evaluates to type `{}`.",
                        cond_eval.get_type(ctx)
                    ),
                ));
            }
            ctx.builder.build_conditional_branch(
                cond_eval.get_value().into_int_value(),
                truthy_block,
                falsey_block,
            );

            ctx.builder.position_at_end(truthy_block);
            let mut if_returns = gen_stmts(ctx, then, function, &returns, returns_spanned.clone())?;
            if if_returns.is_none() {
                ctx.builder.build_unconditional_branch(continued_block);
            }

            ctx.builder.position_at_end(falsey_block);

            let mut elifs = elifs.clone();

            if elifs.len() > 0 {
                let (cond, then) = elifs.remove(0);
                let elif_returns = gen_if(
                    ctx,
                    &Stmt::If {
                        condition: cond,
                        then,
                        elifs: elifs.clone(),
                        else_block: else_block.clone(),
                    },
                    function,
                    returns,
                    returns_spanned,
                )?;
                if_returns = if if_returns.is_some() {
                    elif_returns
                } else {
                    None
                };
            } else if else_block.is_some() {
                let else_returns = gen_stmts(
                    ctx,
                    else_block.as_ref().unwrap(),
                    function,
                    &returns,
                    returns_spanned,
                )?;
                if_returns = if if_returns.is_some() {
                    else_returns
                } else {
                    None
                }
            }

            if if_returns.is_none() {
                ctx.builder.build_unconditional_branch(continued_block);
                ctx.builder.position_at_end(continued_block);
            }

            Ok(if_returns)
        }
        _ => panic!(),
    }
}

pub fn gen_var_decl<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    name: &Spanned<String>,
    typ: &Option<ParserType>,
    value: &Spanned<Box<Expr>>,
) -> Result<Option<ValueEnum<'ctx>>, CompileError> {
    let eval = gen_expr(ctx, value.as_ref(), &name.inner);
    if eval.is_err() {
        return Err(eval.unwrap_err());
    }
    let eval = eval.unwrap();
    let eval_type = eval.get_type(ctx).name();
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
    Ok(None)
}

pub fn gen_stmt<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    stmt: &Stmt,
    function: FunctionValue<'ctx>,
    returns: TypeID,
    returns_spanned: Option<ParserType>,
) -> Result<Option<ValueEnum<'ctx>>, CompileError> {
    match stmt {
        Stmt::Error(_) => panic!(),
        Stmt::VarDecl { name, typ, value } => gen_var_decl(ctx, name, typ, value),
        Stmt::VarSet { on, value } => {
            let expr = gen_expr(ctx, value.as_ref(), "UNUSED_setas")?;
            let field = gen_primary_ref(ctx, on, "UNUSED_setto")?;
            if field.typ != expr.get_type(ctx) {
                return Err(CompileError::with_notes(
                    value.span,
                    &format!(
                        "Type of expression, `{}`, is different from the type of the field.",
                        expr.get_type(ctx)
                    ),
                    on.span,
                    &format!("The type of the field is `{}`", field.typ),
                ));
            };
            ctx.builder.build_store(field.ptr, expr.get_value());

            Ok(None)
        }
        Stmt::Expr(expr) => match gen_expr(ctx, expr.as_ref(), "UNUSED") {
            Ok(_) => Ok(None),
            Err(err) => Err(err),
        },
        Stmt::While { condition, inner } => {
            let cond_block = ctx.context.append_basic_block(function, "while_cond");
            let loop_block = ctx.context.append_basic_block(function, "while_loop");
            let continued_block = ctx.context.append_basic_block(function, "continue");

            ctx.builder.build_unconditional_branch(cond_block);
            ctx.builder.position_at_end(cond_block);

            let cond_eval = gen_expr(ctx, condition, "while_condition")?;
            if cond_eval.get_type(ctx) != TypeID::from_base("Bool") {
                return Err(CompileError::new(
                    condition.span,
                    &format!(
                        "Expression must evaluate to type `Bool` in a `while` loop, instead it evaluates to type `{}`.",
                        cond_eval.get_type(ctx)
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

            let returns = gen_stmts(ctx, inner, function, &returns, returns_spanned)?;
            if returns.is_none() {
                ctx.builder.build_unconditional_branch(cond_block);
            } // TODO: Warning here

            ctx.builder.position_at_end(continued_block);
            Ok(returns)
        }
        Stmt::For {
            looper,
            loopee,
            block,
        } => {
            let loopee_eval = gen_expr(ctx, loopee, "loopee")?;
            let as_vec = loopee_eval.clone().try_as_vector();
            if as_vec.is_none() {
                return Err(CompileError::new(
                    loopee.span,
                    &format!(
                        "Loopee of a `for` loop must be of type `Vec`, instead it is of type `{}`",
                        loopee_eval.get_type(ctx)
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

            let cond_block = ctx.context.append_basic_block(function, "for_cond");
            let loop_block = ctx.context.append_basic_block(function, "for_loop");
            let continue_block = ctx.context.append_basic_block(function, "continue");

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
            let returns = gen_stmts(ctx, block, function, &returns, returns_spanned)?;
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
        Stmt::If {
            condition,
            then,
            elifs,
            else_block,
        } => gen_if(ctx, stmt, function, returns, returns_spanned),
        Stmt::Return(expr) => match expr {
            Some(expr) => gen_expr(ctx, expr, "RETURN").map(|val| Some(val)),
            None => Ok(Some(ValueEnum::Unit(Unit {}))),
        },
    }
}

pub fn gen_stmts<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    stmts: &Vec<Spanned<Stmt>>,
    function: FunctionValue<'ctx>,
    returns: &TypeID,
    returns_spanned: Option<ParserType>,
) -> Result<Option<ValueEnum<'ctx>>, CompileError> {
    ctx.push_scope();
    for stmt in stmts.iter() {
        match gen_stmt(
            ctx,
            stmt,
            function,
            returns.clone(),
            returns_spanned.clone(),
        )? {
            Some(ret) => {
                if &ret.get_type(ctx) != returns {
                    let mut out = CompileError::new(
                        stmt.span,
                        &format!("Incorrect return type, expected `{}`", returns),
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
                return Ok(Some(ret));
            }
            None => continue,
        }
    }
    ctx.pop_scope();
    Ok(None)
}

pub fn gen_decl_pre<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    decl: &Decl,
    inside: Option<TypeID>,
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
            ctx.add_field(
                &name.inner,
                Field::new(ValueEnum::Function(function), &name.inner),
            );

            Ok(())
        }
        Decl::Class {
            name,
            params,
            decls,
        } => Ok(()),
        Decl::Var { name, typ, value } => gen_var_decl(ctx, name, typ, value).map(|x| ()),
    }
}

pub fn gen_decl<'ctx>(
    llvm_ctx: &'ctx Context,
    ctx: &mut LanguageContext<'ctx>,
    decl: &Decl,
) -> Result<(), CompileError> {
    match decl {
        Decl::Var { name, typ, value } => Ok(()),
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
                function,
                &returns,
                returns_spanned.clone(),
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
            name,
            params,
            decls,
        } => {
            ctx.push_scope();

            let class_struct = ctx
                .context
                .opaque_struct_type(&format!("User__{}", name.inner));

            let init_llvm_type = ctx.types.ptr.fn_type(
                &params
                    .iter()
                    .map(|(_, typ)| {
                        any_to_basic(ctx.get(typ.to_typeid()).storage_type)
                            .unwrap()
                            .into()
                    })
                    .collect::<Vec<_>>(),
                false,
            );
            let init_llvm_fn =
                ctx.add_function(&format!("User__{}.()", name.inner), init_llvm_type);
            let init_type = TypeID::new(
                "Function",
                vec![
                    TypeID::new(
                        "Tuple",
                        params.iter().map(|(name, typ)| typ.to_typeid()).collect(),
                    ),
                    TypeID::from_base(name),
                ],
            );
            let init_fn = Function::from_function(ctx.context, ctx, init_llvm_fn, init_type);

            let old_block = ctx.begin_function(init_llvm_fn);

            let mut builder = MetatypeBuilder::new(
                ctx,
                BasicBuiltin::Class,
                TypeID::from_base(name),
                None,
                ctx.types.ptr.into(),
                false,
            );
            builder.add_initializer(init_fn);

            gen_decls_pre(ctx, decls, Some(TypeID::from_base(name)));

            let mut body = Vec::<BasicTypeEnum<'ctx>>::new();
            let mut members = BTreeMap::<String, ClassMember>::new();
            let mut functions = BTreeMap::<String, Function<'ctx>>::new();
            let mut functions_decls = BTreeMap::<String, (Function<'ctx>, &Decl)>::new();
            let mut to_store = Vec::<(BasicValueEnum<'ctx>, u32, &String)>::new();

            let mut member_index = 0;
            for ((name, typ), val) in params.iter().zip(init_llvm_fn.get_params()) {
                body.push(val.get_type());
                to_store.push((val, member_index, &name.inner));
                ctx.add_field(
                    name,
                    Field::new(ValueEnum::from_val(ctx, val, typ.to_typeid(), name), name),
                );
                members.insert(
                    name.inner.clone(),
                    ClassMember::new(typ.to_typeid(), member_index),
                );
                member_index += 1;
            }

            let scope = ctx.current_scope();
            for ((name, field), decl) in scope.iter().zip(decls) {
                if let Some(function) = field.value.clone().try_as_function() {
                    functions.insert(name.clone(), function.clone());
                    functions_decls.insert(name.clone(), (function, decl));
                } else {
                    body.push(
                        any_to_basic(ctx.get(field.value.get_type(ctx)).storage_type).unwrap(),
                    );
                    to_store.push((field.value.get_value(), member_index, name));
                    members.insert(
                        name.clone(),
                        ClassMember::new(field.value.get_type(ctx), member_index),
                    );
                    member_index += 1;
                }
            }

            class_struct.set_body(&body, false);

            let malloc = ctx.module.get_function("GC_malloc").unwrap();
            let mem = ctx
                .build_call_returns(malloc, &[class_struct.size_of().unwrap().into()], "out")
                .into_pointer_value();

            for (val, index, name) in to_store {
                ctx.build_ptr_store(class_struct, mem, val, index, name);
            }
            println!("{:?}", members);

            if functions.contains_key("init") {
                let init_type = ctx.types.void.fn_type(&[ctx.types.ptr.into()], false);
                ctx.builder.build_indirect_call(
                    init_type,
                    functions["init"].ptr,
                    &[mem.into()],
                    "UNUSED",
                );
            }

            ctx.pop_scope();
            ctx.builder.build_return(Some(&mem));
            ctx.builder.position_at_end(old_block);

            let class_info = ClassInfo::new(class_struct, members.clone(), functions);
            builder.add_class_info(class_info);
            builder.build(llvm_ctx, ctx, vec![]);

            for (fn_name, (function, decl)) in functions_decls {
                let function_val = ctx
                    .module
                    .get_function(&format!("User__{}.{fn_name}", name.inner))
                    .unwrap();
                let old_block = ctx.begin_function(function_val);
                ctx.push_scope();

                ctx.get_with_gen_ext(TypeID::new(
                    "MemberFunction",
                    vec![
                        function.args()[0].clone(),
                        TypeID::new("Tuple", function.args()[1..].to_vec()),
                        function.returns(),
                    ],
                ));

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
                            None => TypeID::from_base("Unit"),
                        };

                        let obj = function_val.get_first_param().unwrap().into_pointer_value();
                        for (name, member) in members.clone() {
                            let value = ctx.build_ptr_load(
                                class_struct,
                                member.typ.clone(),
                                obj,
                                member.index,
                                &name,
                            );
                            let value = ValueEnum::from_val(ctx, value, member.typ, &name);
                            ctx.add_field(&name, Field::new(value, &name));
                        }
                        // TODO: add member functions
                        for ((name, typ), value) in
                            params.iter().zip(function_val.get_params()).skip(1)
                        {
                            value.set_name(name);
                            let value = ValueEnum::from_val(ctx, value, typ.to_typeid(), name);
                            ctx.add_field(name, Field::new(value, name));
                        }

                        let returns_val = gen_stmts(
                            ctx,
                            &body.inner,
                            function_val,
                            &returns,
                            returns_spanned.clone(),
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
                    }
                    _ => panic!(),
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
    decls: &Vec<Decl>,
    inside: Option<TypeID>,
) {
    for decl in decls {
        match gen_decl_pre(ctx.context, ctx, decl, inside.clone()) {
            Ok(_) => continue,
            Err(err) => ctx.error(err),
        }
    }
}

pub fn gen_decls<'ctx>(ctx: &mut LanguageContext<'ctx>, decls: &Vec<Decl>) {
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
    source: &PathBuf,
    build: &PathBuf,
) -> Result<(), ()> {
    let filename = path
        .clone()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut file = File::open(path).expect("Cannot read path!");
    let mut src = "".to_string();
    file.read_to_string(&mut src).unwrap();

    for import in program.imports {
        let mut path = source.clone();
        for name in import.inner.path {
            path = path.join(name.inner);
        }

        if path.with_extension("hy").exists() {
            // TODO: Importing other files
        } else if path.with_extension("hyi").exists() {
            // TODO: Importing stubs

            let buildscript = path.with_extension("hyb");
            if buildscript.exists() {
                run_buildscript(ctx, &import.span.make_wrapped(buildscript), build);
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

    ctx.push_scope();

    ctx.init_metatypes(&llvm_ctx);

    gen_decls_pre(ctx, &program.decls, None);
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

    if ctx.errors.len() > 0 {
        Err(())
    } else {
        Ok(())
    }
}
