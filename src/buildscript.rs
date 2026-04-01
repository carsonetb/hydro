use chumsky::{
    Parser,
    error::Rich,
    extra,
    prelude::{choice, just, none_of},
    span::{Spanned, WrappingSpan},
    text::keyword,
};

#[derive(Debug, Clone)]
struct BuildScript {
    pub source: Spanned<String>,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone)]
struct Target {
    pub target: Spanned<String>,
    pub commands: Vec<BuildCmd>,
}

#[derive(Debug, Clone)]
enum BuildCmd {
    Artifact(Spanned<String>),
    Run {
        cmd: Spanned<String>,
        inside: Option<Spanned<String>>,
    },
    LinkDir(Vec<Spanned<String>>),
    Link(Vec<Spanned<String>>),
}

pub trait GenericParser<'src, R>: Parser<'src, &'src str, R, extra::Err<Rich<'src, char>>> {}
impl<'src, R, P: Parser<'src, &'src str, R, extra::Err<Rich<'src, char>>>> GenericParser<'src, R>
    for P
{
}

pub fn build_cmd<'bs>() -> impl GenericParser<'bs, BuildCmd> {
    let string = none_of('\'')
        .ignored()
        .repeated()
        .to_slice()
        .padded_by(just('\''))
        .spanned()
        .map(|string: Spanned<&str>| string.span.make_wrapped(string.inner.to_string()));
    let artifact = keyword("artifact")
        .padded()
        .ignore_then(string)
        .map(|path| BuildCmd::Artifact(path))
        .boxed();
    choice((artifact))
}
