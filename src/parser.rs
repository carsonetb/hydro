use chumsky::prelude::*;

#[derive(Debug)]
pub enum ErrorKind {
    UnexpectedText(String),
    Unknown,
}

#[derive(Debug)]
pub enum Stmt<'src> {
    Error(ErrorKind),
    VarDecl {
        name: &'src str,
        typ: &'src str,
        value: Box<Expr<'src>>,
    },
    VarSet {
        name: &'src str,
        value: Box<Expr<'src>>,
    },
    Expr(Box<Expr<'src>>),
}

#[derive(Debug)]
pub enum Expr<'src> {
    Unary(&'src str, Box<Expr<'src>>),
    Binary(Box<Expr<'src>>, &'src str, Box<Expr<'src>>),
    Primary(Box<Primary>),
}

#[derive(Debug)]
pub enum Primary {
    Atom(Box<Atom>),
}

#[derive(Debug)]
pub enum Atom {
    Literal(Literal),
}

#[derive(Debug)]
pub enum Literal {
    Error(ErrorKind),
    Int(u32),
}

pub fn literal<'src>() -> impl Parser<'src, &'src str, Literal, extra::Err<Rich<'src, char>>> {
    text::int(10).padded().from_str().map(|r| match r {
        Ok(int) => Literal::Int(int),
        Err(_) => Literal::Error(ErrorKind::Unknown),
    })
}

pub fn atom<'src>() -> impl Parser<'src, &'src str, Atom, extra::Err<Rich<'src, char>>> {
    literal().map(Atom::Literal)
}

pub fn primary<'src>() -> impl Parser<'src, &'src str, Primary, extra::Err<Rich<'src, char>>> {
    atom().map(|atom| Primary::Atom(Box::new(atom)))
}

pub fn expr<'src>() -> impl Parser<'src, &'src str, Expr<'src>, extra::Err<Rich<'src, char>>> {
    macro_rules! op {
        ($c:expr) => {
            just($c).padded()
        };
    }

    macro_rules! binary_op {
        ($prev_rule:expr, $ops:expr) => {
            $prev_rule
                .clone()
                .foldl($ops.then($prev_rule).repeated(), |lhs, (op, rhs)| {
                    Expr::Binary(Box::new(lhs), op, Box::new(rhs))
                })
                .boxed()
        };
    }

    let unary = op!("-")
        .repeated()
        .foldr(
            primary().map(|primary| Expr::Primary(Box::new(primary))),
            |op, rhs| Expr::Unary(op, Box::new(rhs)),
        )
        .boxed();

    let factor = binary_op!(unary, choice((op!("*"), op!("/"))));
    let sum = binary_op!(factor, choice((op!("+"), op!("-"))));

    sum
}

pub fn var_decl<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, extra::Err<Rich<'src, char>>> {
    let ident = text::ascii::ident().padded();
    text::ascii::keyword("var")
        .padded()
        .ignore_then(ident)
        .then_ignore(just(":"))
        .then(ident)
        .then_ignore(just("="))
        .then(expr())
        .then_ignore(just(";"))
        .map(|((name, typ), value)| Stmt::VarDecl {
            name,
            typ,
            value: Box::new(value),
        })
        .boxed()
}

pub fn stmt<'src>() -> impl Parser<'src, &'src str, Stmt<'src>, extra::Err<Rich<'src, char>>> {
    let ident = text::ascii::ident().padded();
    let set = ident
        .then_ignore(just("="))
        .then(expr())
        .then_ignore(just(";"))
        .map(|(name, value)| Stmt::VarSet {
            name,
            value: Box::new(value),
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
        .or(expr().map(|expression| Stmt::Expr(Box::new(expression))))
        .recover_with(recovery)
        .boxed()
}

pub fn block<'src>() -> impl Parser<'src, &'src str, Vec<Stmt<'src>>, extra::Err<Rich<'src, char>>>
{
    stmt()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just("{").padded(), just("}").padded())
}

pub fn program<'src>() -> impl Parser<'src, &'src str, Vec<Stmt<'src>>, extra::Err<Rich<'src, char>>>
{
    stmt().repeated().collect::<Vec<_>>().then_ignore(end())
}
