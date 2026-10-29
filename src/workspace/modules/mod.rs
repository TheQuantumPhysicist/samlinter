use super::Reach;
use crate::finding::Position;
use crate::syntax::{is_test_gated, is_test_module, path_value, requires_test};
use anyhow::Result;
use ra_ap_syntax::ast::{self, HasAttrs, HasModuleItem, HasName};
use ra_ap_syntax::{AstNode, Edition, TextSize};
use std::borrow::Cow;
use std::ffi::OsStr;
use std::iter;
use std::path::{Path, PathBuf};

/// How a file's own `mod name;` declarations resolve, per the Rust reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    /// A target root, a `mod.rs` file or a file loaded through `#[path]`: children sit beside it.
    ModRs,
    /// Any other module file, `a.rs`: children sit in the directory named after it, `a/`.
    NonModRs,
}

pub struct Candidate {
    pub path: PathBuf,
    pub kind: FileKind,
    /// `Reach::NotLibrary` when only `cfg_attr` paths whose cfg requires `test` lead here.
    pub reach: Reach,
}

/// What the lookup `candidates` is given finds at an inline module's written directory.
pub enum Directory {
    /// An existing directory, by its normalized path.
    At(PathBuf),
    /// - The lookup gives no answer there, so the directory is entered as written.
    /// - Each candidate file below it then gets that same answer when looked up.
    AsWritten,
    /// No directory there.
    Absent,
}

/// One out-of-line `mod name;` item.
#[derive(Debug)]
pub struct Declaration {
    /// The line of the `mod` keyword.
    pub line: usize,
    /// The inline `mod m { .. }` blocks around it, outermost first.
    pub inline: Vec<Module>,
    pub module: Module,
    /// `Reach::NotLibrary` when it, an inline module around it or its file requires `test`
    /// (`syntax::is_test_module`).
    pub reach: Reach,
}

/// A `mod` item's name and the `path` attributes that can place it.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    paths: Paths,
}

/// - The `path` attributes the compiler may apply to one module; cfg is not evaluated.
/// - The compiler applies the first `path` attribute left after `cfg_attr` expansion, so an
///   unconditional one ends the list: no later one can apply.
#[derive(Clone, Debug)]
enum Paths {
    /// No unconditional `path` yet, so a later attribute may still add one.
    Open {
        /// `#[cfg_attr(.., path = "..")]` values so far, each one more possible location.
        conditional: Vec<Conditional>,
    },
    /// Ended by the first unconditional `path`.
    Fixed {
        /// The `#[path = ".."]` value, which replaces the default location.
        fixed: String,
        /// `#[cfg_attr(.., path = "..")]` values before it, each one more possible location.
        conditional: Vec<Conditional>,
    },
}

/// A `#[cfg_attr(.., path = "..")]` value.
#[derive(Clone, Debug)]
struct Conditional {
    path: String,
    /// `Reach::NotLibrary` when the cfg, or one around it, requires `test`.
    reach: Reach,
}

/// One place a module may be.
#[derive(Clone, Copy)]
enum Location<'a> {
    /// Named after the module.
    Default,
    /// A `path` attribute's value, `Reach::NotLibrary` under a `cfg_attr` requiring `test`.
    Path { path: &'a str, reach: Reach },
}

/// How a `mod` item gives its contents.
enum Body {
    /// `mod name;`: in a file of its own.
    OutOfLine,
    /// `mod name { .. }`
    Inline(ast::ItemList),
}

impl Body {
    /// `None` for a `mod` with neither `;` nor an item list.
    fn of(module: &ast::Module) -> Option<Self> {
        module
            .item_list()
            .map(Self::Inline)
            .or_else(|| module.semicolon_token().map(|_| Self::OutOfLine))
    }

    /// The inner `#![..]` attributes; only an inline module's item list holds them.
    fn inner_attrs(&self) -> Vec<ast::Attr> {
        match self {
            Self::OutOfLine => Vec::new(),
            Self::Inline(list) => list.attrs().collect(),
        }
    }
}

/// - The inline module a walk is inside, linked to the ones around it.
/// - Borrowed from the stack frames of the walk, so memory grows linearly with nesting depth.
struct Enclosing<'a> {
    module: Module,
    /// `None` at the outermost inline module.
    outer: Option<&'a Enclosing<'a>>,
}

/// A `path` attribute whose value is not a valid string literal.
#[derive(Debug, PartialEq, Eq)]
pub struct BadPath {
    /// The line of the attribute.
    pub line: usize,
}

impl Paths {
    /// The location with no cfg set, then the `cfg_attr` path values.
    fn split(&self) -> (Location<'_>, &[Conditional]) {
        match self {
            Self::Open { conditional } => (Location::Default, conditional),
            Self::Fixed { fixed, conditional } => (
                Location::Path {
                    path: fixed,
                    reach: Reach::Library,
                },
                conditional,
            ),
        }
    }

    /// Every location some cfg setting gives the module.
    fn locations(&self) -> impl Iterator<Item = Location<'_>> {
        let (unconditional, conditional) = self.split();
        iter::once(unconditional).chain(conditional.iter().map(Conditional::location))
    }

    /// Every path value in attribute order, each `through` the reach of the cfg around them.
    fn into_values(self, reach: Reach) -> Vec<Conditional> {
        let values = match self {
            Self::Open { conditional } => conditional,
            Self::Fixed { fixed, conditional } => [
                conditional,
                vec![Conditional {
                    path: fixed,
                    reach: Reach::Library,
                }],
            ]
            .concat(),
        };
        values
            .into_iter()
            .map(|value| Conditional {
                reach: reach.through(value.reach),
                ..value
            })
            .collect()
    }

    /// `self` followed by the attribute `meta` on `line`; once fixed, nothing more applies.
    fn then(self, meta: &ast::Meta, line: usize) -> Result<Self, BadPath> {
        match self {
            Self::Open { conditional } => open_then(conditional, meta, line),
            Self::Fixed {
                fixed: _,
                conditional: _,
            } => Ok(self),
        }
    }
}

impl Conditional {
    fn location(&self) -> Location<'_> {
        Location::Path {
            path: &self.path,
            reach: self.reach,
        }
    }
}

/// The open paths `conditional` followed by the attribute `meta` on `line`.
fn open_then(
    conditional: Vec<Conditional>,
    meta: &ast::Meta,
    line: usize,
) -> Result<Paths, BadPath> {
    match meta {
        ast::Meta::KeyValueMeta(pair) => match path_value(pair) {
            Some(value) => Ok(Paths::Fixed {
                fixed: string_value(&value).ok_or(BadPath { line })?,
                conditional,
            }),
            None => Ok(Paths::Open { conditional }),
        },
        ast::Meta::CfgAttrMeta(cfg_attr) => {
            // The same rule holds inside the `cfg_attr`, and its cfg makes each path conditional.
            let inner = paths(cfg_attr.metas().map(|meta| (meta, line)))?;
            let reach = gate_reach(
                cfg_attr
                    .cfg_predicate()
                    .is_some_and(|predicate| requires_test(&predicate)),
            );
            Ok(Paths::Open {
                conditional: [conditional, inner.into_values(reach)].concat(),
            })
        }
        ast::Meta::CfgMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::TokenTreeMeta(_)
        | ast::Meta::UnsafeMeta(_) => Ok(Paths::Open { conditional }),
    }
}

/// - Every out-of-line `mod` declaration in module item lists, inline modules included.
/// - Modules inside function bodies and macro bodies are not seen.
pub fn declarations(text: &str, edition: Edition) -> Result<Vec<Declaration>, BadPath> {
    let (tree, _) = crate::syntax::parse(text, edition);
    in_items(
        text,
        tree.items(),
        None,
        gate_reach(is_test_gated(tree.syntax())),
    )
}

/// - `Reach::NotLibrary` for a test-gated file or module.
/// - The reach a declaration gives a library route; `Reach::through` combines them.
const fn gate_reach(test_gated: bool) -> Reach {
    if test_gated {
        Reach::NotLibrary
    } else {
        Reach::Library
    }
}

/// `reach` is that of the items' module: their file, then each inline module around them.
fn in_items(
    text: &str,
    items: impl Iterator<Item = ast::Item>,
    enclosing: Option<&Enclosing>,
    reach: Reach,
) -> Result<Vec<Declaration>, BadPath> {
    let nested = items
        .filter_map(|item| match item {
            ast::Item::Module(module) => Some(module),
            ast::Item::AsmExpr(_)
            | ast::Item::Const(_)
            | ast::Item::Enum(_)
            | ast::Item::ExternBlock(_)
            | ast::Item::ExternCrate(_)
            | ast::Item::Fn(_)
            | ast::Item::Impl(_)
            | ast::Item::MacroCall(_)
            | ast::Item::MacroDef(_)
            | ast::Item::MacroRules(_)
            | ast::Item::Static(_)
            | ast::Item::Struct(_)
            | ast::Item::Trait(_)
            | ast::Item::TypeAlias(_)
            | ast::Item::Union(_)
            | ast::Item::Use(_) => None,
        })
        .map(|module| in_module(text, &module, enclosing, reach))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(nested.into_iter().flatten().collect())
}

fn in_module(
    text: &str,
    module: &ast::Module,
    enclosing: Option<&Enclosing>,
    reach: Reach,
) -> Result<Vec<Declaration>, BadPath> {
    match (module.name(), Body::of(module)) {
        (Some(name), Some(body)) => in_named_module(text, module, &name, body, enclosing, reach),
        // A `mod` without a name, or with neither `;` nor an item list, is a syntax error, which
        // the source check reports.
        (None, None | Some(_)) | (Some(_), None) => Ok(Vec::new()),
    }
}

/// `in_module` for a `mod` item with the name `name` and the contents `body`.
fn in_named_module(
    text: &str,
    module: &ast::Module,
    name: &ast::Name,
    body: Body,
    enclosing: Option<&Enclosing>,
    reach: Reach,
) -> Result<Vec<Declaration>, BadPath> {
    // - Invariant: the parser starts every module item at its `mod` keyword.
    // - A missing keyword is a parser bug, not bad input, so there is nothing to report.
    let keyword = module
        .mod_token()
        .expect("the parser builds every module item from its `mod` keyword");
    // rustc reads the outer attributes, then an inline module's inner `#![..]` ones.
    let attributes = module.attrs().chain(body.inner_attrs()).filter_map(|attr| {
        let line = line(text, attr.syntax().text_range().start());
        attr.meta().map(|meta| (meta, line))
    });
    let this = Module {
        name: name.text().trim_start_matches("r#").to_owned(),
        paths: paths(attributes)?,
    };
    let reach = reach.through(gate_reach(is_test_module(module)));

    match body {
        Body::OutOfLine => Ok(vec![Declaration {
            line: line(text, keyword.text_range().start()),
            inline: outermost_first(enclosing),
            module: this,
            reach,
        }]),
        Body::Inline(list) => in_items(
            text,
            list.items(),
            Some(&Enclosing {
                module: this,
                outer: enclosing,
            }),
            reach,
        ),
    }
}

fn outermost_first(enclosing: Option<&Enclosing>) -> Vec<Module> {
    let innermost_first: Vec<&Module> = iter::successors(enclosing, |link| link.outer)
        .map(|link| &link.module)
        .collect();
    innermost_first.into_iter().rev().cloned().collect()
}

fn line(text: &str, offset: TextSize) -> usize {
    Position::of(text, usize::from(offset)).line
}

/// The `path` attributes among `metas`, each with the line of the attribute holding it.
fn paths(metas: impl IntoIterator<Item = (ast::Meta, usize)>) -> Result<Paths, BadPath> {
    metas.into_iter().try_fold(
        Paths::Open {
            conditional: Vec::new(),
        },
        |paths, (meta, line)| paths.then(&meta, line),
    )
}

/// `None` for anything but a valid string literal.
fn string_value(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Literal(literal) => match literal.kind() {
            ast::LiteralKind::String(string) => string.value().ok().map(Cow::into_owned),
            ast::LiteralKind::Bool(_)
            | ast::LiteralKind::Byte(_)
            | ast::LiteralKind::ByteString(_)
            | ast::LiteralKind::CString(_)
            | ast::LiteralKind::Char(_)
            | ast::LiteralKind::FloatNumber(_)
            | ast::LiteralKind::IntNumber(_) => None,
        },
        ast::Expr::ArrayExpr(_)
        | ast::Expr::AsmExpr(_)
        | ast::Expr::AwaitExpr(_)
        | ast::Expr::BecomeExpr(_)
        | ast::Expr::BinExpr(_)
        | ast::Expr::BlockExpr(_)
        | ast::Expr::BreakExpr(_)
        | ast::Expr::CallExpr(_)
        | ast::Expr::CastExpr(_)
        | ast::Expr::CfgPredExpr(_)
        | ast::Expr::ClosureExpr(_)
        | ast::Expr::ContinueExpr(_)
        | ast::Expr::FieldExpr(_)
        | ast::Expr::ForExpr(_)
        | ast::Expr::FormatArgsExpr(_)
        | ast::Expr::IfExpr(_)
        | ast::Expr::IncludeBytesExpr(_)
        | ast::Expr::IndexExpr(_)
        | ast::Expr::LetExpr(_)
        | ast::Expr::LoopExpr(_)
        | ast::Expr::MacroExpr(_)
        | ast::Expr::MatchExpr(_)
        | ast::Expr::MethodCallExpr(_)
        | ast::Expr::OffsetOfExpr(_)
        | ast::Expr::ParenExpr(_)
        | ast::Expr::PathExpr(_)
        | ast::Expr::PrefixExpr(_)
        | ast::Expr::RangeExpr(_)
        | ast::Expr::RecordExpr(_)
        | ast::Expr::RefExpr(_)
        | ast::Expr::ReturnExpr(_)
        | ast::Expr::TryExpr(_)
        | ast::Expr::TupleExpr(_)
        | ast::Expr::UnderscoreExpr(_)
        | ast::Expr::WhileExpr(_)
        | ast::Expr::YeetExpr(_)
        | ast::Expr::YieldExpr(_) => None,
    }
}

/// The directories one file's `mod` declarations resolve against, as the compiler tracks them.
struct Base {
    /// Holds the default files `<name>.rs` and `<name>/mod.rs`.
    default: PathBuf,
    /// `path` values are relative to it: the file's own directory, which differs from `default`
    /// only for a non-mod-rs file outside every inline module.
    path: PathBuf,
    /// `Reach::NotLibrary` when only `cfg_attr` directories requiring `test` lead here.
    reach: Reach,
}

impl Base {
    /// The bases of the file `name` of `kind` in the directory `dir`.
    fn of(dir: &Path, name: &OsStr, kind: FileKind) -> Self {
        let default = match kind {
            FileKind::ModRs => dir.to_path_buf(),
            FileKind::NonModRs => dir.join(Path::new(name).with_extension("")),
        };
        Self {
            default,
            path: dir.to_path_buf(),
            reach: Reach::Library,
        }
    }

    /// - The directories of the inline module `module`: its location with no cfg set, then each
    ///   `cfg_attr` location that `directory` finds.
    /// - An existing one at the normalized path `directory` gives, a missing one as written.
    fn enter(
        &self,
        module: &Module,
        directory: &impl Fn(&Path) -> Result<Directory>,
    ) -> Result<Vec<Self>> {
        let (unconditional, conditional) = module.paths.split();
        // - Kept even when missing: rustc looks there with no cfg set.
        // - So a missing-module error always names it.
        let written = self.dir(module, unconditional);
        let first = match directory(&written)? {
            Directory::At(path) => path,
            Directory::AsWritten | Directory::Absent => written,
        };
        // - A `cfg_attr` `path` directory that does not exist holds no file.
        // - Entering it anyway doubles the bases at every nesting level.
        let others = conditional.iter().filter_map(|value| {
            let written = self.dir(module, value.location());
            let reach = self.reach.through(value.reach);
            match directory(&written) {
                Ok(Directory::At(path)) => Some(Ok(Self::at(path, reach))),
                Ok(Directory::AsWritten) => Some(Ok(Self::at(written, reach))),
                Ok(Directory::Absent) => None,
                Err(error) => Some(Err(error)),
            }
        });
        iter::once(Ok(Self::at(first, self.reach)))
            .chain(others)
            .collect()
    }

    /// The directory of the inline module `module` at `location`.
    fn dir(&self, module: &Module, location: Location<'_>) -> PathBuf {
        match location {
            Location::Default => self.default.join(&module.name),
            Location::Path { path, reach: _ } => self.path.join(path),
        }
    }

    /// Inside an inline module both bases are its directory.
    fn at(dir: PathBuf, reach: Reach) -> Self {
        Self {
            default: dir.clone(),
            path: dir,
            reach,
        }
    }

    /// The files the out-of-line module `module` may be; a file loaded through a `path` value
    /// resolves its own children like a `mod.rs` file.
    fn files(&self, module: &Module) -> Vec<Candidate> {
        module
            .paths
            .locations()
            .flat_map(|location| match location {
                Location::Default => vec![
                    Candidate {
                        path: self.default.join(format!("{}.rs", module.name)),
                        kind: FileKind::NonModRs,
                        reach: self.reach,
                    },
                    Candidate {
                        path: self.default.join(&module.name).join("mod.rs"),
                        kind: FileKind::ModRs,
                        reach: self.reach,
                    },
                ],
                Location::Path { path, reach } => vec![Candidate {
                    path: self.path.join(path),
                    kind: FileKind::ModRs,
                    reach: self.reach.through(reach),
                }],
            })
            .collect()
    }
}

/// - The files `declaration`, found in the file `name` of `kind` in the directory `dir`, may be:
///   every combination of the locations of the inline modules around it, then of its own.
/// - `directory` tells what is at an inline module's written directory; it walks the written
///   path, so it also meets any symlink on the way.
/// - An inline module's `cfg_attr` location is entered only when it exists, and each
///   directory once however it is spelled, so the work is bounded by the existing directories.
pub fn candidates(
    dir: &Path,
    name: &OsStr,
    kind: FileKind,
    declaration: &Declaration,
    directory: impl Fn(&Path) -> Result<Directory>,
) -> Result<Vec<Candidate>> {
    let bases = declaration.inline.iter().try_fold(
        vec![Base::of(dir, name, kind)],
        |bases, module| -> Result<Vec<Base>> {
            let entered = bases
                .iter()
                .map(|base| base.enter(module, &directory))
                .collect::<Result<Vec<_>>>()?;
            Ok(first_of_each(entered.into_iter().flatten().collect()))
        },
    )?;
    Ok(bases
        .iter()
        .flat_map(|base| base.files(&declaration.module))
        .collect())
}

/// - `bases` in order, each directory once: `enter` gave every spelling of an existing one the
///   same path.
/// - A directory several locations lead to is library code when any of them is.
fn first_of_each(bases: Vec<Base>) -> Vec<Base> {
    bases
        .into_iter()
        .fold(Vec::new(), |mut kept: Vec<Base>, base| {
            match kept.iter_mut().find(|seen| seen.path == base.path) {
                Some(seen) => seen.reach = seen.reach.either(base.reach),
                None => kept.push(base),
            }
            kept
        })
}

#[cfg(test)]
mod tests;
