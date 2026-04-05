use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{
    extra::Err,
    prelude::*,
    span::WrappingSpan,
    text::{ascii::ident, keyword, newline},
};

use crate::types::TypeID;

#[derive(Debug, Clone)]
pub enum ErrorKind {
    UnexpectedText(String),
    Unknown,
}

#[derive(Debug)]
pub struct Program {
    pub imports: Vec<Spanned<Import>>,
    pub decls: Vec<Decl>,
}

#[derive(Debug)]
pub struct Import {
    pub path: Vec<Spanned<String>>,
}

#[derive(Debug)]
pub enum Decl {
    Var(Var),
    Function {
        name: Spanned<String>,
        generics: Vec<GenericParam>,
        params: Vec<(Spanned<String>, ParserType)>,
        returns: Option<ParserType>,
        body: Spanned<Vec<Spanned<Stmt>>>,
    },
    Class {
        name: Spanned<String>,
        params: Vec<(Spanned<String>, ParserType)>,
        decls: Vec<Decl>,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Error(ErrorKind),
    Expr(Spanned<Box<Expr>>),
    Var(Var),
    Set(Set),
    Break(Break),
    Continue(Continue),
    Eval(Eval),
    Return(Return),
    If(If),
    For(For),
    Match(Match),
    While(While),
}

#[derive(Debug, Clone)]
pub struct Var {
    pub name: Spanned<String>,
    pub typ: Option<ParserType>,
    pub value: Spanned<Box<Expr>>,
}

#[derive(Debug, Clone)]
pub struct Set {
    pub on: Spanned<Box<Primary>>,
    pub value: Spanned<Box<Expr>>,
}

#[derive(Debug, Clone)]
pub struct Break(pub Option<Spanned<String>>);

#[derive(Debug, Clone)]
pub struct Continue(pub Option<Spanned<String>>);

#[derive(Debug, Clone)]
pub struct Eval {
    pub from: Option<Spanned<String>>,
    pub val: Option<Spanned<Expr>>,
}

#[derive(Debug, Clone)]
pub struct Return(pub Option<Spanned<Expr>>);

#[derive(Debug, Clone)]
pub struct If {
    pub name: Option<Spanned<String>>,
    pub condition: Spanned<Expr>,
    pub then: Vec<Spanned<Stmt>>,
    pub elifs: Vec<(Option<Spanned<String>>, Spanned<Expr>, Vec<Spanned<Stmt>>)>,
    pub else_block: Option<Vec<Spanned<Stmt>>>,
}

#[derive(Debug, Clone)]
pub struct For {
    pub name: Option<Spanned<String>>,
    pub looper: Spanned<String>,
    pub loopee: Spanned<Expr>,
    pub block: Vec<Spanned<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub name: Option<Spanned<String>>,
    pub what: Spanned<Expr>,
    pub options: Vec<(Spanned<Box<Primary>>, Spanned<Expr>)>,
}

#[derive(Debug, Clone)]
pub struct While {
    pub name: Option<Spanned<String>>,
    pub condition: Spanned<Expr>,
    pub inner: Vec<Spanned<Stmt>>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Unary(Spanned<String>, Box<Expr>),
    Binary(Spanned<Box<Expr>>, Spanned<String>, Spanned<Box<Expr>>),
    Primary(Spanned<Box<Primary>>),
    Var(Var),
    Set(Set),
    If(Box<If>),
    For(Box<For>),
    Match(Box<Match>),
    While(Box<While>),
    Combo(Vec<Expr>),
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
    Slice {
        on: Spanned<Box<Primary>>,
        expr: Spanned<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum Atom {
    Literal(Spanned<ParseLiteral>),
    Block(Vec<Spanned<Stmt>>),
    Grouping(Spanned<Expr>),
    Array(Spanned<Vec<Spanned<Expr>>>),
    Tuple(Spanned<Vec<Spanned<Expr>>>),
    Var(Spanned<String>),
}

#[derive(Debug, Clone)]
pub enum ParseLiteral {
    Error(ErrorKind),
    Int(u32),
    Float(f32),
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
            &self.base.inner.clone(),
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

pub fn comment<'src>() -> impl GenericParser<'src, ()> {
    let single = just("//")
        .padded()
        .ignore_then(any().and_is(newline().not()).repeated())
        .then(newline().or(end()))
        .ignored();
    let multi = just("/*")
        .then(any().and_is(just("*/").not()).repeated())
        .then_ignore(just("*/"))
        .ignored();
    single.or(multi).repeated().boxed()
}

pub fn type_parser<'src>() -> impl GenericParser<'src, ParserType> + Clone {
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

pub fn stmt_name<'src>() -> impl GenericParser<'src, Option<Spanned<String>>> + Clone {
    just("'")
        .ignore_then(ident().map(|s: &str| s.to_string()).spanned())
        .or_not()
}

pub fn literal<'src>() -> impl GenericParser<'src, Spanned<ParseLiteral>> + Clone {
    let int = text::int(10)
        .from_str()
        .map(|r| match r {
            Ok(int) => ParseLiteral::Int(int),
            Err(_) => ParseLiteral::Error(ErrorKind::Unknown),
        })
        .spanned()
        .labelled("number");
    let float = text::int(10)
        .then(just('.').padded().ignore_then(text::int(10)))
        .map(|(int, frac)| match format!("{}.{}", int, frac).parse() {
            Ok(float) => ParseLiteral::Float(float),
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
    choice((float, int, bool, string)).padded()
}

pub fn atom<'src>(
    expr: impl GenericParser<'src, Spanned<Expr>> + Clone,
) -> impl Parser<'src, &'src str, Spanned<Box<Atom>>, extra::Err<Rich<'src, char>>> + Clone {
    let literal = literal().map(|l| l.span.make_wrapped(Box::new(Atom::Literal(l))));

    let grouping = expr
        .clone()
        .delimited_by(just("("), just(")"))
        .spanned()
        .map(|e| e.span.make_wrapped(Box::new(Atom::Grouping(e.inner))));

    let array = expr
        .separated_by(just(","))
        .collect::<Vec<_>>()
        .delimited_by(just("["), just("]"))
        .spanned()
        .map(|es| es.span.make_wrapped(Box::new(Atom::Array(es))));

    let var = text::ascii::ident()
        .spanned()
        .padded()
        .map(|keyword: Spanned<&str>| {
            keyword.span.make_wrapped(Box::new(Atom::Var(
                keyword.span.make_wrapped(keyword.inner.to_string()),
            )))
        });

    choice((literal, grouping, array, var))
}

enum Suffix {
    Member(Spanned<String>),
    Call {
        span: SimpleSpan,
        generics: Option<Vec<ParserType>>,
        args: Vec<Spanned<Expr>>,
    },
    Slice(Spanned<Expr>),
}

pub fn primary<'src>(
    expr: impl GenericParser<'src, Spanned<Expr>> + Clone + 'src,
) -> impl Parser<'src, &'src str, Spanned<Box<Primary>>, extra::Err<Rich<'src, char>>> + Clone {
    recursive(|primary| {
        let atom = atom(expr.clone())
            .map(|atom| atom.span.make_wrapped(Box::new(Primary::Atom(atom))))
            .boxed();

        let member_suffix = just(".")
            .ignore_then(ident().spanned())
            .map(|i: Spanned<&str>| Suffix::Member(i.span.make_wrapped(i.inner.to_string())));

        let call_suffix = type_parser()
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
            .map(|stuff| Suffix::Call {
                span: stuff.span,
                generics: stuff.inner.0,
                args: stuff.inner.1,
            });

        let slice_suffix = expr
            .clone()
            .delimited_by(just("["), just("]"))
            .map(Suffix::Slice)
            .boxed();

        atom.foldl(
            choice((member_suffix, call_suffix, slice_suffix)).repeated(),
            |on, suffix| match suffix {
                Suffix::Member(ident) => {
                    on.span
                        .union(ident.span)
                        .make_wrapped(Box::new(Primary::Member {
                            on,
                            name: ident.span.make_wrapped(ident.inner.to_string()),
                        }))
                }
                Suffix::Call {
                    span,
                    generics,
                    args,
                } => on.span.union(span).make_wrapped(Box::new(Primary::Call {
                    on,
                    generics: generics.unwrap_or_default(),
                    args,
                })),
                Suffix::Slice(expr) => on
                    .span
                    .union(expr.span)
                    .make_wrapped(Box::new(Primary::Slice { on, expr })),
            },
        )
        .boxed()
    })
}

pub fn justexpr<'src>()
-> impl Parser<'src, &'src str, Spanned<Expr>, extra::Err<Rich<'src, char>>> + Clone {
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

pub fn expr<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, Spanned<Expr>> + Clone {
    choice((
        justexpr(),
        var(block.clone()).map(Expr::Var).spanned(),
        set(block.clone()).map(Expr::Set).spanned(),
        if_parser(block.clone())
            .map(|x| Expr::If(Box::new(x)))
            .spanned(),
        for_parser(block.clone())
            .map(|x| Expr::For(Box::new(x)))
            .spanned(),
        match_parser(block.clone())
            .map(|x| Expr::Match(Box::new(x)))
            .spanned(),
        while_parser(block.clone())
            .map(|x| Expr::While(Box::new(x)))
            .spanned(),
    ))
    .boxed() // istgtspmo rst s vbcd
}

pub fn var<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, Var> + Clone {
    let ident = text::ascii::ident().spanned();
    text::ascii::keyword("var")
        .labelled("var decl")
        .padded()
        .ignore_then(ident)
        .then(just(':').padded().ignore_then(type_parser()).or_not())
        .then_ignore(just("=").padded())
        .then(expr(block))
        .then_ignore(just(";"))
        .map(|((name, typ), value)| Var {
            name: name.span.make_wrapped(name.to_string()),
            typ: typ,
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed()
}

pub fn set<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, Set> + Clone {
    primary(expr(block.clone()))
        .labelled("var set")
        .padded()
        .spanned()
        .then_ignore(just("="))
        .then(expr(block))
        .map(|(on, value)| Set {
            on: on.span.make_wrapped(on.inner.inner),
            value: value.span.make_wrapped(Box::new(value.inner)),
        })
        .boxed()
}

pub fn if_parser<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, If> + Clone {
    text::keyword("if")
        .padded()
        .ignore_then(stmt_name())
        .then(expr(block.clone()))
        .then(block.clone())
        .then(
            keyword("elif")
                .ignore_then(stmt_name())
                .then(expr(block.clone()))
                .then(block.clone())
                .map(|((name, condition), body)| (name, condition, body))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then(keyword("else").ignore_then(block.clone()).or_not())
        .map(|((((name, condition), then), elifs), else_block)| If {
            name,
            condition,
            then,
            elifs,
            else_block,
        })
}

pub fn for_parser<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, For> + Clone {
    keyword("for")
        .padded()
        .ignore_then(stmt_name())
        .then(ident().spanned().padded())
        .then_ignore(keyword("in"))
        .then(expr(block.clone()))
        .then(block.clone())
        .map(|(((name, looper), loopee), block)| For {
            name,
            looper: looper.span.make_wrapped(looper.inner.to_string()),
            loopee,
            block,
        })
}

pub fn match_parser<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, Match> + Clone {
    keyword("match")
        .padded()
        .ignore_then(stmt_name())
        .then(expr(block.clone()))
        .then(
            primary(expr(block.clone()))
                .then_ignore(just("->").padded())
                .then(expr(block.clone()))
                .then_ignore(just(";").padded())
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just("{").padded(), just("}").padded()),
        )
        .map(|((name, what), options)| Match {
            name,
            what,
            options,
        })
}

pub fn while_parser<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl GenericParser<'src, While> + Clone {
    text::keyword("while")
        .padded()
        .ignore_then(stmt_name())
        .then(expr(block.clone()))
        .then(block.clone())
        .map(|((name, condition), block)| While {
            name,
            condition,
            inner: block,
        })
}

pub fn stmt<'src>(
    block: impl GenericParser<'src, Vec<Spanned<Stmt>>> + Clone + 'src,
) -> impl Parser<'src, &'src str, Stmt, extra::Err<Rich<'src, char>>> + Clone {
    let break_stmt = keyword("break")
        .padded()
        .ignore_then(stmt_name())
        .then_ignore(just(";").padded())
        .map(|name| Stmt::Break(Break(name)));

    let continue_stmt = keyword("continue")
        .padded()
        .ignore_then(stmt_name())
        .then_ignore(just(";").padded())
        .map(|name| Stmt::Continue(Continue(name)));

    let eval = text::keyword("eval")
        .padded()
        .ignore_then(stmt_name())
        .then(expr(block.clone()).or_not())
        .then_ignore(just(";").padded())
        .map(|(from, val)| Stmt::Eval(Eval { from, val }));

    let ret = text::keyword("return")
        .padded()
        .ignore_then(expr(block.clone()).or_not())
        .then_ignore(just(";").padded())
        .map(|expr| match expr {
            Some(expr) => Stmt::Return(Return(Some(expr.span.make_wrapped(expr.inner)))),
            None => Stmt::Return(Return(None)),
        });

    let var_stmt = var(block.clone()).map(Stmt::Var);
    let set_stmt = set(block.clone()).map(Stmt::Set);
    let if_stmt = if_parser(block.clone()).map(Stmt::If);
    let for_stmt = for_parser(block.clone()).map(Stmt::For);
    let match_stmt = match_parser(block.clone()).map(Stmt::Match);
    let while_stmt = while_parser(block.clone()).map(Stmt::While);

    let just_expr = expr(block.clone())
        .then_ignore(just(";").padded())
        .map(|expression| Stmt::Expr(expression.span.make_wrapped(Box::new(expression.inner))));

    let recovery = via_parser(
        none_of(";}")
            .repeated()
            .at_least(1)
            .then_ignore(just(";").padded())
            .map(|_| Stmt::Error(ErrorKind::Unknown)),
    );

    choice((
        just_expr,
        var_stmt,
        set_stmt,
        break_stmt,
        continue_stmt,
        eval,
        ret,
    ))
    .then_ignore(just(";").padded())
    .or(choice((if_stmt, for_stmt, match_stmt, while_stmt)))
    .recover_with(recovery)
    .then_ignore(comment())
    .labelled("statement")
    .boxed()
}

pub fn block<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Stmt>>, extra::Err<Rich<'src, char>>> + Clone {
    recursive(|block| {
        stmt(block)
            .spanned()
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just("{").padded(), just("}").padded())
    })
}

pub fn params<'src>()
-> impl Parser<'src, &'src str, Vec<(Spanned<String>, ParserType)>, extra::Err<Rich<'src, char>>> + Clone
{
    text::ident()
        .spanned()
        .padded()
        .then_ignore(just(":").padded())
        .then(type_parser())
        .map(|(name, typ)| (name.span.make_wrapped(name.inner.to_string()), typ))
        .separated_by(just(",").padded())
        .collect::<Vec<_>>()
        .delimited_by(just("(").padded(), just(")").padded())
}

pub fn decl<'src>() -> impl Parser<'src, &'src str, Decl, extra::Err<Rich<'src, char>>> + Clone {
    recursive(|decl| {
        let var = var(block()).map(Decl::Var);
        let function = text::keyword("fn")
            .padded()
            .ignore_then(text::ident().spanned().padded()) // TODO: Function generic params.
            .then(params().or_not())
            .then(just("->").padded().ignore_then(type_parser()).or_not())
            .then(block().spanned())
            .map(|(((name, params), returns), body)| Decl::Function {
                name: name.span.make_wrapped(name.inner.to_string()),
                generics: vec![],
                params: match params {
                    Some(params) => params,
                    None => vec![],
                },
                returns,
                body,
            })
            .boxed();
        let class = keyword("class")
            .padded()
            .ignore_then(ident().spanned())
            .then(params().or_not())
            .then(
                decl.repeated()
                    .collect::<Vec<_>>()
                    .delimited_by(just("{").padded(), just("}").padded()),
            )
            .map(|((name, params), decls)| Decl::Class {
                name: name.span.make_wrapped(name.inner.to_string()),
                params: match params {
                    Some(params) => params,
                    None => vec![],
                },
                decls,
            });
        choice((var, function, class))
            .then_ignore(comment())
            .boxed()
    })
}

pub fn import<'src>() -> impl Parser<'src, &'src str, Spanned<Import>, extra::Err<Rich<'src, char>>>
{
    keyword("import")
        .padded()
        .ignore_then(
            ident()
                .map(|s: &str| s.to_string())
                .spanned()
                .separated_by(just('.').padded())
                .collect(),
        )
        .then_ignore(just(';').padded())
        .map(|path| Import { path })
        .spanned()
}

pub fn program<'src>() -> impl Parser<'src, &'src str, Program, extra::Err<Rich<'src, char>>> {
    import()
        .repeated()
        .collect()
        .then(decl().repeated().collect())
        .then_ignore(end().padded())
        .map(|(imports, decls)| Program { imports, decls })
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
