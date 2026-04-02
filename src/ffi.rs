use chumsky::{
    IterParser, Parser,
    prelude::{choice, just},
    span::{Spanned, WrappingSpan},
    text::{ascii::ident, keyword},
};

struct ForeignFunction {
    name: Spanned<String>,
    params: Vec<(Spanned<String>, Spanned<String>)>,
    returns: Option<Spanned<String>>,
    bound: Spanned<String>,
}

enum FFIMember {
    Var {
        name: Spanned<String>,
        typ: Spanned<String>,
    },
    Function(ForeignFunction),
}

enum FFIDecl {
    Class {
        name: Spanned<String>,
        members: Vec<FFIMember>,
    },
    Function(ForeignFunction),
}

fn foreign_function<'src>() -> impl Parser<'src, &'src str, ForeignFunction> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    keyword("fn")
        .padded()
        .ignore_then(id)
        .then(
            id.then_ignore(just(':').padded())
                .then(id)
                .separated_by(just(",").padded())
                .collect::<Vec<_>>()
                .delimited_by(just('(').padded(), just(')').padded())
                .or_not(),
        )
        .then(just("->").padded().ignore_then(id).or_not())
        .then_ignore(just("=").padded())
        .then(id)
        .then_ignore(just(";").padded())
        .map(
            // whyyy
            |(((name, params), returns), bound): (
                (
                    (
                        Spanned<String>,
                        Option<Vec<(Spanned<String>, Spanned<String>)>>,
                    ),
                    Option<Spanned<String>>,
                ),
                Spanned<String>,
            )| {
                ForeignFunction {
                    name: name.span.make_wrapped(name.inner.to_string()),
                    params: if (params.is_some()) {
                        params.unwrap()
                    } else {
                        vec![]
                    },
                    returns,
                    bound,
                }
            },
        )
}

fn ffi_member<'src>() -> impl Parser<'src, &'src str, FFIMember> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    let var = id
        .then_ignore(just(':').padded())
        .then(id)
        .then_ignore(just(';').padded())
        .map(|(name, typ)| FFIMember::Var { name, typ })
        .boxed();
    let function = foreign_function().map(|f| FFIMember::Function(f));
    choice((var, function))
}

fn ffi_decl<'src>() -> impl Parser<'src, &'src str, FFIDecl> {
    let id = ident().map(|i: &str| i.to_string()).spanned();
    let class = keyword("class")
        .ignore_then(id)
        .then(
            ffi_member()
                .repeated()
                .collect()
                .delimited_by(just("{").padded(), just("}").padded()),
        )
        .map(|(name, members)| FFIDecl::Class { name, members });
    let function = foreign_function().map(|f| FFIDecl::Function(f));
    choice((function, class))
}
