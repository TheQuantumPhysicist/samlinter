use crate::finding::Position;
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
}

/// One out-of-line `mod name;` item.
#[derive(Debug)]
pub struct Declaration {
    /// The line of the `mod` keyword.
    pub line: usize,
    /// The inline `mod m { .. }` blocks around it, outermost first.
    pub inline: Vec<Module>,
    pub module: Module,
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
        conditional: Vec<String>,
    },
    /// Ended by the first unconditional `path`.
    Fixed {
        /// The `#[path = ".."]` value, which replaces the default location.
        fixed: String,
        /// `#[cfg_attr(.., path = "..")]` values before it, each one more possible location.
        conditional: Vec<String>,
    },
}

/// One place a module may be.
enum Location<'a> {
    /// Named after the module.
    Default,
    /// A `path` attribute's value.
    Path(&'a str),
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
}

/// A `path` attribute whose value is not a valid string literal.
#[derive(Debug, PartialEq, Eq)]
pub struct BadPath {
    /// The line of the attribute.
    pub line: usize,
}

impl Paths {
    /// Every location some cfg setting gives the module.
    fn locations(&self) -> impl Iterator<Item = Location<'_>> {
        let (first, conditional) = match self {
            Self::Open { conditional } => (Location::Default, conditional),
            Self::Fixed { fixed, conditional } => (Location::Path(fixed), conditional),
        };
        iter::once(first).chain(conditional.iter().map(|path| Location::Path(path)))
    }

    /// Every path value in attribute order.
    fn into_values(self) -> Vec<String> {
        match self {
            Self::Open { conditional } => conditional,
            Self::Fixed { fixed, conditional } => [conditional, vec![fixed]].concat(),
        }
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

/// The open paths `conditional` followed by the attribute `meta` on `line`.
fn open_then(conditional: Vec<String>, meta: &ast::Meta, line: usize) -> Result<Paths, BadPath> {
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
            Ok(Paths::Open {
                conditional: [conditional, inner.into_values()].concat(),
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
    let tree = ast::SourceFile::parse(text, edition).tree();
    in_items(text, tree.items(), &[])
}

fn in_items(
    text: &str,
    items: impl Iterator<Item = ast::Item>,
    inline: &[Module],
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
        .map(|module| in_module(text, &module, inline))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(nested.into_iter().flatten().collect())
}

fn in_module(
    text: &str,
    module: &ast::Module,
    inline: &[Module],
) -> Result<Vec<Declaration>, BadPath> {
    // A `mod` without a name, or with neither `;` nor an item list, is a syntax error, which
    // the source check reports.
    let (Some(name), Some(body)) = (module.name(), Body::of(module)) else {
        return Ok(Vec::new());
    };
    // - Invariant: the parser starts every module item at its `mod` keyword.
    // - A missing keyword is a parser bug, not bad input, so there is nothing to report.
    let keyword = module
        .mod_token()
        .expect("the parser builds every module item from its `mod` keyword");
    // rustc reads the outer attributes, then an inline module's inner `#![..]` ones.
    let inner = module.item_list().into_iter().flat_map(|list| list.attrs());
    let attributes = module.attrs().chain(inner).filter_map(|attr| {
        let line = line(text, attr.syntax().text_range().start());
        attr.meta().map(|meta| (meta, line))
    });
    let this = Module {
        name: name.text().trim_start_matches("r#").to_owned(),
        paths: paths(attributes)?,
    };

    match body {
        Body::OutOfLine => Ok(vec![Declaration {
            line: line(text, keyword.text_range().start()),
            inline: inline.to_vec(),
            module: this,
        }]),
        Body::Inline(list) => in_items(text, list.items(), &[inline, &[this]].concat()),
    }
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

/// The value of `path = <expr>`.
fn path_value(pair: &ast::KeyValueMeta) -> Option<ast::Expr> {
    pair.path()
        .and_then(|path| path.as_single_name_ref())
        .filter(|name| name.text() == "path")
        .and(pair.expr())
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
        }
    }

    /// The bases inside the inline module `module`, one per location: its name below `default`,
    /// or its `path` value below `path`.
    fn enter(&self, module: &Module) -> Vec<Self> {
        module
            .paths
            .locations()
            .map(|location| {
                let dir = match location {
                    Location::Default => self.default.join(&module.name),
                    Location::Path(path) => self.path.join(path),
                };
                Self {
                    default: dir.clone(),
                    path: dir,
                }
            })
            .collect()
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
                    },
                    Candidate {
                        path: self.default.join(&module.name).join("mod.rs"),
                        kind: FileKind::ModRs,
                    },
                ],
                Location::Path(path) => vec![Candidate {
                    path: self.path.join(path),
                    kind: FileKind::ModRs,
                }],
            })
            .collect()
    }
}

/// The files `declaration`, found in the file `name` of `kind` in the directory `dir`, may be:
/// every combination of the locations of the inline modules around it, then of its own.
pub fn candidates(
    dir: &Path,
    name: &OsStr,
    kind: FileKind,
    declaration: &Declaration,
) -> Vec<Candidate> {
    let bases = declaration
        .inline
        .iter()
        .fold(vec![Base::of(dir, name, kind)], |bases, module| {
            bases.iter().flat_map(|base| base.enter(module)).collect()
        });
    bases
        .iter()
        .flat_map(|base| base.files(&declaration.module))
        .collect()
}

#[cfg(test)]
mod tests;
