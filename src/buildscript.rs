use std::{
    collections::HashMap,
    env::{self, consts::OS},
    fs::File,
    io::Read,
    path::PathBuf,
    process::Command,
};

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::{
    IterParser, Parser,
    error::Rich,
    extra,
    prelude::{choice, just, none_of},
    span::{Spanned, WrappingSpan},
    text::{ident, keyword, newline},
};
use git2::Repository;

use crate::{codegen::CompileError, context::LanguageContext};

pub struct LinkInfo {
    pub linkdirs: Vec<PathBuf>,
    pub links: Vec<String>,
}

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

fn string<'src>() -> impl Parser<'src, &'src str, Spanned<String>, extra::Err<Rich<'src, char>>> {
    choice((
        none_of('\'')
            .ignored()
            .repeated()
            .to_slice()
            .padded_by(just('\'')),
        none_of('"')
            .ignored()
            .repeated()
            .to_slice()
            .padded_by(just('"')),
    ))
    .spanned()
    .map(|string: Spanned<&str>| string.span.make_wrapped(string.inner.to_string()))
}

fn build_cmd<'src>() -> impl Parser<'src, &'src str, BuildCmd, extra::Err<Rich<'src, char>>> {
    let artifact = keyword("artifact")
        .padded()
        .ignore_then(string())
        .then_ignore(newline())
        .map(|path| BuildCmd::Artifact(path))
        .boxed();
    let run = keyword("run")
        .padded()
        .ignore_then(string())
        .then(keyword("in").padded().ignore_then(string()).or_not())
        .then_ignore(newline())
        .map(|(cmd, inside)| BuildCmd::Run { cmd, inside });
    let linkdir = keyword("linkdir")
        .padded()
        .ignore_then(string().separated_by(just(",")).collect())
        .then_ignore(newline())
        .map(|dirs| BuildCmd::LinkDir(dirs));
    let link = keyword("link")
        .padded()
        .ignore_then(string().separated_by(just(",").padded()).collect())
        .then_ignore(newline())
        .map(|dirs| BuildCmd::Link(dirs));
    choice((artifact, run, linkdir, link)).boxed()
}

fn target<'src>() -> impl Parser<'src, &'src str, Target, extra::Err<Rich<'src, char>>> {
    keyword("target")
        .padded()
        .ignore_then(ident().spanned().padded())
        .then(
            build_cmd()
                .repeated()
                .collect()
                .delimited_by(just("{").padded(), just("}").padded()),
        )
        .map(|(target, commands)| Target {
            target: target.span.make_wrapped(target.to_string()),
            commands: commands,
        })
}

fn script<'src>() -> impl Parser<'src, &'src str, BuildScript, extra::Err<Rich<'src, char>>> {
    keyword("source")
        .padded()
        .ignore_then(string())
        .then_ignore(newline())
        .then(target().repeated().collect())
        .map(|(source, targets)| BuildScript { source, targets })
}

fn check_paths_exist(paths: &Vec<PathBuf>) -> bool {
    let mut all_exists = true;
    for path in paths {
        if !path.exists() {
            all_exists = false;
            break;
        }
    }
    all_exists
}

pub fn run_buildscript<'ctx>(
    ctx: &mut LanguageContext<'ctx>,
    path: &Spanned<PathBuf>,
    build: &PathBuf,
) -> Result<LinkInfo, CompileError> {
    let filename = path
        .clone()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut file = File::open(&path.inner).unwrap();
    let mut src = "".to_string();
    file.read_to_string(&mut src);
    let (ast, errors) = script().parse(&src).into_output_errors();

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
        return Err(CompileError::new(
            path.span,
            "There was a parse error in the buildscript file.",
        ));
    }

    println!("Running buildscript at `{}`", path.to_str().unwrap());

    let ast = ast.unwrap();
    let clone_dir = build.join(ast.source.inner.split("/").last().unwrap());
    if !clone_dir.exists() {
        println!(
            "Cloning '{}' into `{}`",
            ast.source.inner,
            clone_dir.to_str().unwrap()
        );
        Command::new("git")
            .args(["clone", &ast.source.inner, &clone_dir.to_str().unwrap()])
            .status()
            .unwrap();
    }
    let repo = Repository::open(&clone_dir).unwrap();

    let mut targets = HashMap::<String, Target>::new();
    for target in ast.targets {
        targets.insert(target.target.inner.clone(), target);
    }

    if !targets.contains_key(OS) {
        return Err(CompileError::new(
            path.span,
            &format!("Buildscript doesn't support platform {OS}"),
        ));
    }

    let target = &targets[OS];
    let mut artifacts = Vec::<PathBuf>::new();
    let mut commands = Vec::<Box<dyn FnOnce()>>::new();
    let mut linkdirs = Vec::<PathBuf>::new();
    let mut links = Vec::<String>::new();

    for command in target.commands.clone() {
        match command {
            BuildCmd::Artifact(artifact) => artifacts.push(clone_dir.join(artifact.inner)),
            BuildCmd::Run { cmd, inside } => {
                let mut command = Command::new("sh");
                if inside.is_some() {
                    command.current_dir(clone_dir.join(inside.unwrap().inner));
                } else {
                    command.current_dir(&clone_dir);
                }
                commands.push(Box::new(
                    (move || {
                        command
                            .args(["-c", cmd.inner.as_str()])
                            .status()
                            .expect(&format!("Failed to run build command: {:?}", command));
                    }),
                ));
            }
            BuildCmd::LinkDir(dirs) => {
                for dir in dirs {
                    linkdirs.push(clone_dir.join(dir.inner));
                }
            }
            BuildCmd::Link(linknames) => {
                for link in linknames {
                    links.push(link.inner);
                }
            }
        };
    }

    if !check_paths_exist(&artifacts) {
        println!("Running buildscript for target {}", OS);
        for command in commands {
            command();
        }
    }

    if !check_paths_exist(&artifacts) {
        return Err(CompileError::new(
            path.span,
            "Build script probably failed to run, expected artifacts are nonexistant.",
        ));
    }

    println!("Finished running buildscript.");

    Ok(LinkInfo { linkdirs, links })
}
