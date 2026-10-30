use super::Reach;
use crate::finding::Position;
use crate::syntax::{
    ModuleBody, is_test_gated, is_test_module, mod_keyword, module_body, module_items, path_value,
    requires_test, unraw,
};
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

impl FileKind {
    /// A file two routes reach keeps the shape any route gives it: a `NonModRs` route makes it
    /// `NonModRs`, so the file is linted by the shape that declares submodules.
    pub const fn either(self, other: Self) -> Self {
        match (self, other) {
            (Self::ModRs, Self::ModRs) => Self::ModRs,
            (Self::ModRs, Self::NonModRs) | (Self::NonModRs, Self::ModRs | Self::NonModRs) => {
                Self::NonModRs
            }
        }
    }
}

/// Whether a file's code is test code, whole-file, as the walk reaches it: the route passes a
/// test-gated module declaration or a `cfg_attr` whose cfg requires `test`, each decided by
/// `syntax::cfg`, or the file is an integration-test target's root. The file-level
/// counterpart of `is_test_gated`, which covers a file's own attributes and test functions.
/// Carried by `SourceFile`, so the source check can stay silent in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestCode {
    Test,
    NotTest,
}

impl TestCode {
    /// The file a route of testness `self` reaches through a declaring route of testness
    /// `gate`: a test module's own submodules are test code too, so one test leg is enough.
    pub const fn through(self, gate: Self) -> Self {
        match (self, gate) {
            (Self::Test, Self::NotTest | Self::Test) | (Self::NotTest, Self::Test) => Self::Test,
            (Self::NotTest, Self::NotTest) => Self::NotTest,
        }
    }

    /// - A file two routes reach is test code only when every route reaches it as test: one
    ///   route that lints it keeps it linted, since that route's build compiles the file
    ///   whole. Conservative in the silence direction, and the same either way round.
    pub const fn either(self, other: Self) -> Self {
        match (self, other) {
            (Self::Test, Self::Test) => Self::Test,
            (Self::Test, Self::NotTest) | (Self::NotTest, Self::Test | Self::NotTest) => {
                Self::NotTest
            }
        }
    }
}

/// What one route says about the file it reaches: whether its code can end up in the
/// package's library, and whether this route compiles it as test code, whole-file. The two
/// travel together: the target sets both, each declaring route folds them at once, and every
/// merge takes both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteCode {
    pub reach: Reach,
    pub test: TestCode,
}

impl RouteCode {
    /// What the file a route of `self` reaches through a declaring route of testness `test`
    /// is: a test gate keeps the code out of the library, and one test leg makes it test code.
    pub const fn through(self, test: TestCode) -> Self {
        Self {
            reach: self.reach.through(gate_reach(test)),
            test: self.test.through(test),
        }
    }

    /// A file two routes reach: library code when either route is, test code when every route
    /// is.
    pub const fn either(self, other: Self) -> Self {
        Self {
            reach: self.reach.either(other.reach),
            test: self.test.either(other.test),
        }
    }
}

pub struct Candidate {
    pub path: PathBuf,
    pub kind: FileKind,
    /// `TestCode::Test` when only `cfg_attr` paths whose cfg requires `test` lead here.
    pub test: TestCode,
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
    /// `Test` when the declaration, an inline module around it or its file requires `test`
    /// (`syntax::is_test_module`): the code it names is test-only, so it stays out of the
    /// library and is test code whole-file.
    pub test: TestCode,
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
    /// `Test` when the cfg, or one around it, requires `test`.
    test: TestCode,
}

/// One place a module may be.
#[derive(Clone, Copy)]
enum Location<'a> {
    /// Named after the module.
    Default,
    /// A `path` attribute's value, `Test` under a `cfg_attr` whose cfg requires `test`.
    Path { path: &'a str, test: TestCode },
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
                    test: TestCode::NotTest,
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

    /// Every path value in attribute order, each `through` the testness of the cfg around them.
    fn into_values(self, test: TestCode) -> Vec<Conditional> {
        let values = match self {
            Self::Open { conditional } => conditional,
            Self::Fixed { fixed, conditional } => [
                conditional,
                vec![Conditional {
                    path: fixed,
                    test: TestCode::NotTest,
                }],
            ]
            .concat(),
        };
        values
            .into_iter()
            .map(|value| Conditional {
                test: test.through(value.test),
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
            test: self.test,
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
            let test = gate_test(
                cfg_attr
                    .cfg_predicate()
                    .is_some_and(|predicate| requires_test(&predicate)),
            );
            Ok(Paths::Open {
                conditional: [conditional, inner.into_values(test)].concat(),
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
        crate::syntax::module_items(tree.items()),
        None,
        gate_test(is_test_gated(tree.syntax())),
    )
}

/// - `TestCode::Test` for a test-gated file or module.
/// - The testness a declaration gives a route; `TestCode::through` combines them.
const fn gate_test(test_gated: bool) -> TestCode {
    if test_gated {
        TestCode::Test
    } else {
        TestCode::NotTest
    }
}

/// The library reach of a declaring route's testness: a test gate keeps the code out of the
/// library.
const fn gate_reach(test: TestCode) -> Reach {
    match test {
        TestCode::Test => Reach::NotLibrary,
        TestCode::NotTest => Reach::Library,
    }
}

/// `testness` is that of the items' module: their file, then each inline module around them.
fn in_items(
    text: &str,
    modules: impl Iterator<Item = ast::Module>,
    enclosing: Option<&Enclosing>,
    testness: TestCode,
) -> Result<Vec<Declaration>, BadPath> {
    let nested = modules
        .map(|module| in_module(text, &module, enclosing, testness))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(nested.into_iter().flatten().collect())
}

fn in_module(
    text: &str,
    module: &ast::Module,
    enclosing: Option<&Enclosing>,
    testness: TestCode,
) -> Result<Vec<Declaration>, BadPath> {
    match (module.name(), module_body(module)) {
        (Some(name), Some(body)) => in_named_module(text, module, &name, body, enclosing, testness),
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
    body: ModuleBody,
    enclosing: Option<&Enclosing>,
    testness: TestCode,
) -> Result<Vec<Declaration>, BadPath> {
    let keyword = mod_keyword(module);
    // rustc reads the outer attributes, then an inline module's inner `#![..]` ones.
    let attributes = module.attrs().chain(body.inner_attrs()).filter_map(|attr| {
        let line = line(text, attr.syntax().text_range().start());
        attr.meta().map(|meta| (meta, line))
    });
    let this = Module {
        name: unraw(name.text()).to_owned(),
        paths: paths(attributes)?,
    };
    let testness = testness.through(gate_test(is_test_module(module)));

    match body {
        ModuleBody::OutOfLine => Ok(vec![Declaration {
            line: line(text, keyword.text_range().start()),
            inline: outermost_first(enclosing),
            module: this,
            test: testness,
        }]),
        ModuleBody::Inline(list) => in_items(
            text,
            module_items(list.items()),
            Some(&Enclosing {
                module: this,
                outer: enclosing,
            }),
            testness,
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
    /// `Test` when only `cfg_attr` directories whose cfg requires `test` lead here.
    test: TestCode,
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
            test: TestCode::NotTest,
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
            let test = self.test.through(value.test);
            match directory(&written) {
                Ok(Directory::At(path)) => Some(Ok(Self::at(path, test))),
                Ok(Directory::AsWritten) => Some(Ok(Self::at(written, test))),
                Ok(Directory::Absent) => None,
                Err(error) => Some(Err(error)),
            }
        });
        iter::once(Ok(Self::at(first, self.test)))
            .chain(others)
            .collect()
    }

    /// The directory of the inline module `module` at `location`.
    fn dir(&self, module: &Module, location: Location<'_>) -> PathBuf {
        match location {
            Location::Default => self.default.join(&module.name),
            Location::Path { path, test: _ } => self.path.join(path),
        }
    }

    /// Inside an inline module both bases are its directory.
    fn at(dir: PathBuf, test: TestCode) -> Self {
        Self {
            default: dir.clone(),
            path: dir,
            test,
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
                        test: self.test,
                    },
                    Candidate {
                        path: self.default.join(&module.name).join("mod.rs"),
                        kind: FileKind::ModRs,
                        test: self.test,
                    },
                ],
                Location::Path { path, test } => vec![Candidate {
                    path: self.path.join(path),
                    kind: FileKind::ModRs,
                    test: self.test.through(test),
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
/// - A directory several locations lead to is test code only when every location is.
fn first_of_each(bases: Vec<Base>) -> Vec<Base> {
    bases
        .into_iter()
        .fold(Vec::new(), |mut kept: Vec<Base>, base| {
            match kept.iter_mut().find(|seen| seen.path == base.path) {
                Some(seen) => seen.test = seen.test.either(base.test),
                None => kept.push(base),
            }
            kept
        })
}

#[cfg(test)]
mod tests;
