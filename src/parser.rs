use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{extra::Err, prelude::*, span::WrappingSpan, text::ascii::ident};

use crate::types::TypeID;

#[derive(Debug, Clone)]
pub enum ErrorKind {
    UnexpectedText(String),
    Unknown,
}

#[derive(Debug)]
pub struct Program {
    pub decls: Vec<Decl>,
}

#[derive(Debug)]
pub enum Decl {
    Function {
        name: Spanned<String>,
        generics: Vec<GenericParam>,
        params: Vec<(Spanned<String>, ParserType)>,
        returns: Option<ParserType>,
        body: Spanned<Vec<Spanned<Stmt>>>,
    },
}

#[derive(Debug)]
pub enum Stmt {
    Error(ErrorKind),
    Return(Option<Spanned<Box<Expr>>>),
    VarDecl {
        name: Spanned<String>,
        typ: Option<ParserType>,
        value: Spanned<Box<Expr>>,
    },
    VarSet {
        name: Spanned<String>,
        value: Spanned<Box<Expr>>,
    },
    Expr(Spanned<Box<Expr>>),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Unary(Spanned<String>, Box<Expr>),
    Binary(Spanned<Box<Expr>>, Spanned<String>, Spanned<Box<Expr>>),
    Primary(Spanned<Box<Primary>>),
}

#[derive(Debug, Clone)]
pub enum Primary {
    Atom(Spanned<Box<Atom>>),
    Call {
        on: Spanned<Box<Primary>>,
        generics: Vec<ParserType>,
        args: Vec<Spanned<Expr>>,
    },
    Member {
        on: Spanned<Box<Primary>>,
        name: Spanned<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Atom {
    Literal(Spanned<ParseLiteral>),
    Grouping(Spanned<Expr>),
    Var(Spanned<String>),
}

#[derive(Debug, Clone)]
pub enum ParseLiteral {
    Error(ErrorKind),
    Int(u32),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone)]
pub struct ParserType {
    pub span: SimpleSpan,
    pub base: Spanned<String>,
    pub generics: Vec<ParserType>,
}

#[derive(Debug)]
pub struct GenericParam {
    pub span: SimpleSpan,
    pub alias: Spanned<String>,
    pub inherits: Option<Spanned<TypeID>>,
}

impl ParserType {
    pub fn to_typeid(&self) -> TypeID {
        TypeID::new(
            self.base.inner.clone(),
            self.generics.iter().map(|g| g.to_typeid()).collect(),
        )
    }

    pub fn new(span: SimpleSpan, base: Spanned<String>, generics: Vec<ParserType>) -> Self {
        Self {
            span,
            base,
            generics,
        }
    }

    pub fn from_base(base: Spanned<String>) -> Self {
        Self {
            span: base.span,
            base,
            generics: Vec::<ParserType>::new(),
        }
    }
}

pub trait GenericParser<'src, R>: Parser<'src, &'src str, R, extra::Err<Rich<'src, char>>> {}
impl<'src, R, P: Parser<'src, &'src str, R, extra::Err<Rich<'src, char>>>> GenericParser<'src, R>
    for P
{
}

pub fn type_parser<'src>() -> impl GenericParser<'src, ParserType> {
    recursive(|typ| {
        let ident = text::ascii::ident()
            .spanned()
            .map(|name: Spanned<&str>| name.span.make_wrapped(name.inner.to_string()));

        let args: Boxed<'_, '_, &str, Spanned<Vec<ParserType>>, Err<Rich<'src, char>>> = typ
            .separated_by(just(',').padded())
            .collect::<Vec<_>>()
            .delimited_by(just('<'), just('>'))
            .spanned()
            .boxed();

        ident
            .then(args.or_not())
            .map(|(base, generics)| match generics {
                Some(generics) => {
                    ParserType::new(base.span.union(generics.span), base, generics.inner)
                }
                None => ParserType::from_base(base),
            })
            .labelled("type")
    })
}

pub fn literal<'src>() -> impl GenericParser<'src, Spanned<ParseLiteral>> {
    let int = text::int(10)
        .from_str()
        .map(|r| match r {
            Ok(int) => ParseLiteral::Int(int),
            Err(_) => ParseLiteral::Error(ErrorKind::Unknown),
        })
        .spanned()
        .labelled("number");
    let bool = just("true")
        .map(|_| ParseLiteral::Bool(true))
        .or(just("false").map(|_| ParseLiteral::Bool(false)))
        .spanned()
        .labelled("boolean");
    let string = none_of('"')
        .ignored()
        .repeated()
        .to_slice()
        .padded_by(just('"'))
        .map(|string: &str| ParseLiteral::String(string.to_string()))
        .spanned();
    choice((int, bool, string)).padded()
}

pub fn atom<'src>(
    expr: impl GenericParser<'src, Spanned<Expr>>,
) -> impl Parser<'src, &'src str, Spanned<Box<Atom>>, extra::Err<Rich<'src, char>>> {
    literal()
        .map(|l| l.span.make_wrapped(Box::new(Atom::Literal(l))))
        .or(expr
            .delimited_by(just("("), just(")"))
            .spanned()
            .map(|e| e.span.make_wrapped(Box::new(Atom::Grouping(e.inner)))))
        .or(text::ascii::ident()
            .spanned()
            .padded()
            .map(|keyword: Spanned<&str>| {
                keyword.span.make_wrapped(Box::new(Atom::Var(
                    keyword.span.make_wrapped(keyword.inner.to_string()),
                )))
            }))
}

pub fn primary<'src>(
    expr: impl GenericParser<'src, Spanned<Expr>> + Clone + 'src,
) -> impl Parser<'src, &'src str, Spanned<Box<Primary>>, extra::Err<Rich<'src, char>>> {
    recursive(|primary| {
        let atom = atom(expr.clone())
            .map(|atom| atom.span.make_wrapped(Box::new(Primary::Atom(atom))))
            .boxed();

        atom.foldl(
            just(".").ignore_then(ident().spanned()).repeated(),
            |on, ident| {
                on.span
                    .union(ident.span)
                    .make_wrapped(Box::new(Primary::Member {
                        on,
                        name: ident.span.make_wrapped(ident.inner.to_string()),
                    }))
            },
        )
        .foldl(
            type_parser()
                .separated_by(just(",").padded())
                .collect::<Vec<_>>()
                .delimited_by(just("<"), just(">"))
                .or_not()
                .then(
                    expr.clone()
                        .separated_by(just(","))
                        .collect::<Vec<_>>()
                        .delimited_by(just("("), just(")")),
                )
                .spanned()
                .repeated(),
            |on, stuff| {
                on.span
                    .union(stuff.span)
                    .make_wrapped(Box::new(Primary::Call {
                        on,
                        generics: match stuff.inner.0 {
                            Some(generics) => generics,
                            None => vec![],
                        },
                        args: stuff.inner.1,
                    }))
            },
        )
        .boxed()
    })
}

pub fn expr<'src>() -> impl Parser<'src, &'src str, Spanned<Expr>, extra::Err<Rich<'src, char>>> {
    macro_rules! op {
        ($c:expr) => {
            just($c).spanned().padded()
        };
    }

    recursive(|expr| {
        macro_rules! binary_op {
            ($prev_rule:expr, $ops:expr) => {
                $prev_rule
                    .clone()
                    .foldl($ops.then($prev_rule).repeated(), |lhs, (op, rhs)| {
                        lhs.span
                            .union(op.span)
                            .union(rhs.span)
                            .make_wrapped(Expr::Binary(
                                lhs.span.make_wrapped(Box::new(lhs.inner)),
                                op.span.make_wrapped(op.to_string()),
                                rhs.span.make_wrapped(Box::new(rhs.inner)),
                            ))
                    })
                    .boxed()
            };
        }

        let primary_as_expr = primary(expr)
            .map(|p| p.span.make_wrapped(Expr::Primary(p)))
            .boxed();
        let power = primary_as_expr
            .clone()
            .foldl(
                op!("**").then(primary_as_expr).repeated(),
                |lhs, (op, rhs)| {
                    lhs.span
                        .union(op.span)
                        .union(rhs.span)
                        .make_wrapped(Expr::Binary(
                            lhs.span.make_wrapped(Box::new(lhs.inner)),
                            op.span.make_wrapped(op.to_string()),
                            rhs.span.make_wrapped(Box::new(rhs.inner)),
                        ))
                },
            )
            .boxed();

        let unary = op!("-")
            .repeated()
            .foldr(power.map(|primary| primary), |op, rhs| {
                op.span.union(rhs.span).make_wrapped(Expr::Unary(
                    op.span.make_wrapped(op.inner.to_string()),
                    Box::new(rhs.inner),
                ))
            })
            .boxed();

        let factor = binary_op!(unary, choice((op!("*"), op!("/"), op!("%"))));
        let sum = binary_op!(factor, choice((op!("+"), op!("-"))));
        let shift = binary_op!(sum, choice((op!("<<"), op!(">>"))));
        let bitwise_and = binary_op!(shift, op!("&"));
        let bitwise_xor = binary_op!(bitwise_and, op!("^"));
        let bitwise_or = binary_op!(bitwise_xor, op!("|"));
        let comparison = binary_op!(
            bitwise_or,
            choice((op!("<="), op!(">="), op!("<"), op!(">")))
        );
        let equality = binary_op!(comparison, choice((op!("=="), op!("!="))));
        let conjunction = binary_op!(equality, op!("&&"));
        let disjunction = binary_op!(conjunction, op!("||"));

        disjunction
    })
}

pub fn var_decl<'src>() -> impl Parser<'src, &'src str, Stmt, extra::Err<Rich<'src, char>>> {
    let ident = text::ascii::ident().spanned();
    text::ascii::keyword("var")
        .labelled("var decl")
        .padded()
        .ignore_then(ident)
        .then(just(':').padded().ignore_then(type_parser()).or_not())
        .then_ignore(just("=").padded())
        .then(expr())
        .then_ignore(just(";"))
        .map(|((name, typ), value)| Stmt::VarDecl {
            name: name.span.make_wrapped(name.to_string()),
            typ: typ,
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed()
}

pub fn stmt<'src>() -> impl Parser<'src, &'src str, Stmt, extra::Err<Rich<'src, char>>> {
    let set = text::ascii::ident()
        .labelled("var set")
        .padded()
        .spanned()
        .then_ignore(just("="))
        .then(expr())
        .then_ignore(just(";").padded())
        .map(|(name, value)| Stmt::VarSet {
            name: name.span.make_wrapped(name.to_string()),
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed();

    let ret = text::keyword("return")
        .ignore_then(expr().or_not())
        .then_ignore(just(";").padded())
        .map(|expr| match expr {
            Some(expr) => Stmt::Return(Some(expr.span.make_wrapped(Box::new(expr.inner)))),
            None => Stmt::Return(None),
        });

    let just_expr = expr()
        .then_ignore(just(";").padded())
        .map(|expression| Stmt::Expr(expression.span.make_wrapped(Box::new(expression.inner))));

    let recovery = via_parser(
        none_of(";}")
            .repeated()
            .at_least(1)
            .then_ignore(just(";").padded())
            .map(|_| Stmt::Error(ErrorKind::Unknown)),
    );

    choice((var_decl(), ret, set, just_expr))
        .recover_with(recovery)
        .labelled("statement")
        .boxed()
}

pub fn block<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Stmt>>, extra::Err<Rich<'src, char>>> {
    stmt()
        .spanned()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just("{").padded(), just("}").padded())
}

pub fn decl<'src>() -> impl Parser<'src, &'src str, Decl, extra::Err<Rich<'src, char>>> {
    let decl = text::keyword("fn")
        .padded()
        .ignore_then(text::ident().spanned().padded()) // TODO: Function generic params.
        .then(
            text::ident()
                .spanned()
                .padded()
                .then_ignore(just(":").padded())
                .then(type_parser())
                .map(|(name, typ)| (name.span.make_wrapped(name.inner.to_string()), typ))
                .separated_by(just(",").padded())
                .collect::<Vec<_>>()
                .delimited_by(just("("), just(")"))
                .or_not(),
        )
        .then(just("->").padded().ignore_then(type_parser()).or_not())
        .then(block().spanned())
        .map(|(((name, params), returns), body)| Decl::Function {
            name: name.span.make_wrapped(name.inner.to_string()),
            generics: vec![],
            params: if params.is_some() {
                params.unwrap()
            } else {
                vec![]
            },
            returns,
            body,
        })
        .boxed();
    decl
}

pub fn program<'src>() -> impl Parser<'src, &'src str, Program, extra::Err<Rich<'src, char>>> {
    decl()
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(end().padded())
        .map(|decls| Program { decls })
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
