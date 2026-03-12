use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{prelude::*, span::WrappingSpan};

#[derive(Debug)]
pub enum ErrorKind {
    UnexpectedText(String),
    Unknown,
}

#[derive(Debug)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    Error(ErrorKind),
    VarDecl {
        name: Spanned<String>,
        typ: Spanned<String>,
        value: Spanned<Box<Expr>>,
    },
    VarSet {
        name: Spanned<String>,
        value: Spanned<Box<Expr>>,
    },
    Expr(Spanned<Box<Expr>>),
}

#[derive(Debug)]
pub enum Expr {
    Unary(Spanned<String>, Box<Expr>),
    Binary(Box<Expr>, Spanned<String>, Box<Expr>),
    Primary(Spanned<Box<Primary>>),
}

#[derive(Debug)]
pub enum Primary {
    Atom(Spanned<Box<Atom>>),
}

#[derive(Debug)]
pub enum Atom {
    Literal(Spanned<ParseLiteral>),
}

#[derive(Debug)]
pub enum ParseLiteral {
    Error(ErrorKind),
    Int(u32),
}

pub fn literal<'src>()
-> impl Parser<'src, &'src str, Spanned<ParseLiteral>, extra::Err<Rich<'src, char>>> {
    text::int(10)
        .padded()
        .from_str()
        .map(|r| match r {
            Ok(int) => ParseLiteral::Int(int),
            Err(_) => ParseLiteral::Error(ErrorKind::Unknown),
        })
        .spanned()
        .boxed()
}

pub fn atom<'src>() -> impl Parser<'src, &'src str, Spanned<Box<Atom>>, extra::Err<Rich<'src, char>>>
{
    literal().map(|l| Box::new(Atom::Literal(l))).spanned()
}

pub fn primary<'src>()
-> impl Parser<'src, &'src str, Spanned<Box<Primary>>, extra::Err<Rich<'src, char>>> {
    atom().map(|atom| Box::new(Primary::Atom(atom))).spanned()
}

pub fn expr<'src>() -> impl Parser<'src, &'src str, Spanned<Expr>, extra::Err<Rich<'src, char>>> {
    macro_rules! op {
        ($c:expr) => {
            just($c).padded().spanned()
        };
    }

    macro_rules! binary_op {
        ($prev_rule:expr, $ops:expr) => {
            $prev_rule
                .clone()
                .foldl($ops.then($prev_rule).repeated(), |lhs, (op, rhs)| {
                    lhs.span
                        .union(op.span)
                        .union(rhs.span)
                        .make_wrapped(Expr::Binary(
                            Box::new(lhs.inner),
                            op.span.make_wrapped(op.to_string()),
                            Box::new(rhs.inner),
                        ))
                })
                .boxed()
        };
    }

    let unary = op!("-")
        .repeated()
        .foldr(
            primary().map(|primary| Expr::Primary(primary)),
            |op, rhs| Expr::Unary(op.span.make_wrapped(op.inner.to_string()), Box::new(rhs)),
        )
        .spanned()
        .boxed();

    // TODO: Spanned<Expr> for rhs and lhs.
    let factor = binary_op!(unary, choice((op!("*"), op!("/"))));
    let sum = binary_op!(factor, choice((op!("+"), op!("-"))));

    sum
}

pub fn var_decl<'src>() -> impl Parser<'src, &'src str, Stmt, extra::Err<Rich<'src, char>>> {
    let ident = text::ascii::ident().padded().spanned();
    text::ascii::keyword("var")
        .padded()
        .ignore_then(ident)
        .then_ignore(just(":"))
        .then(ident)
        .then_ignore(just("="))
        .then(expr())
        .then_ignore(just(";"))
        .map(|((name, typ), value)| Stmt::VarDecl {
            name: name.span.make_wrapped(name.to_string()),
            typ: typ.span.make_wrapped(typ.to_string()),
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed()
}

pub fn stmt<'src>() -> impl Parser<'src, &'src str, Stmt, extra::Err<Rich<'src, char>>> {
    let ident = text::ascii::ident().padded().spanned();
    let set = ident
        .then_ignore(just("="))
        .then(expr())
        .then_ignore(just(";"))
        .map(|(name, value)| Stmt::VarSet {
            name: name.span.make_wrapped(name.to_string()),
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed();

    let recovery = via_parser(
        any()
            .and_is(just(";").not())
            .repeated()
            .then_ignore(just(";"))
            .map(|_| Stmt::Error(ErrorKind::Unknown)),
    );

    var_decl()
        .or(set)
        .or(expr()
            .then_ignore(just(";"))
            .map(|expression| Stmt::Expr(expression.span.make_wrapped(Box::new(expression.inner)))))
        .recover_with(recovery)
        .boxed()
}

pub fn block<'src>() -> impl Parser<'src, &'src str, Vec<Stmt>, extra::Err<Rich<'src, char>>> {
    stmt()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just("{").padded(), just("}").padded())
}

pub fn program<'src>() -> impl Parser<'src, &'src str, Program, extra::Err<Rich<'src, char>>> {
    stmt()
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(end().padded())
        .map(|stmts| Program { stmts })
}

pub fn parse<'src>(path: PathBuf) -> Option<Program> {
    let filename = path
        .clone()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut file = File::open(path).expect("Cannot read path!");
    let mut src = "".to_string();
    file.read_to_string(&mut src).unwrap();
    let (ast, errors) = program().parse(&src).into_output_errors();

    if errors.len() > 0 {
        for err in errors {
            Report::build(
                ReportKind::Error,
                (filename.clone(), err.span().into_range()),
            )
            .with_message("Syntax Error")
            .with_label(
                Label::new((filename.clone(), err.span().into_range()))
                    .with_message(err.reason().to_string())
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.clone(), Source::from(&src)))
            .unwrap();
        }
        None
    } else {
        Some(ast.unwrap())
    }
}
