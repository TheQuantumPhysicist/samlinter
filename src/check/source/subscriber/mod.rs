use crate::config::Config;
use crate::finding::{Hit, Rule};
use crate::syntax::{has_test_attribute, is_test_gated};
use crate::workspace::{Package, PackageKind, Section};
use ra_ap_syntax::{AstNode, SyntaxNode, WalkEvent, ast};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Method names that install a subscriber, as in `registry().with(layer).init()`.
const METHODS: [&str; 2] = ["init", "try_init"];

/// Last path segments of functions that install one, as in `fmt::init()`.
const FUNCTIONS: [&str; 4] = ["init", "try_init", "set_global_default", "set_default"];

/// Whether a file's calls that install a log subscriber are findings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installs {
    Allowed,
    Flagged,
}

/// The source files whose subscriber installs are findings, by path.
pub struct Gate<'a> {
    flagged: BTreeSet<&'a Path>,
}

impl<'a> Gate<'a> {
    /// - The library code of every package with no binary target that lists a configured
    ///   subscriber crate as a regular dependency, whichever package owns each file.
    /// - The dependency only gates the check: building layers for a binary to install is fine.
    pub fn of(packages: impl IntoIterator<Item = &'a Package>, config: &Config) -> Self {
        let flagged = packages
            .into_iter()
            .filter_map(
                |package| match (&package.kind, depends_on_a_subscriber(package, config)) {
                    (PackageKind::Library, true) => Some(&package.library_code),
                    (PackageKind::Library, false) | (PackageKind::Binary, true | false) => None,
                },
            )
            .flatten()
            .map(PathBuf::as_path)
            .collect();
        Self { flagged }
    }

    /// What the gate makes of the file at the absolute, normalized `path`.
    pub fn installs(&self, path: &Path) -> Installs {
        if self.flagged.contains(path) {
            Installs::Flagged
        } else {
            Installs::Allowed
        }
    }
}

/// Dev and build dependencies never reach the library's users.
fn depends_on_a_subscriber(package: &Package, config: &Config) -> bool {
    package
        .dependencies
        .iter()
        .filter(|dependency| match dependency.section {
            Section::Normal => true,
            Section::Development | Section::Build => false,
        })
        .any(|dependency| {
            config
                .subscriber_crates
                .iter()
                .any(|subscriber| same_crate_name(subscriber.as_str(), &dependency.crate_name))
        })
}

/// crates.io treats `-` and `_` in a crate name as one character, so both name one crate.
fn same_crate_name(first: &str, second: &str) -> bool {
    let unified = |character| if character == '_' { '-' } else { character };
    first.chars().map(unified).eq(second.chars().map(unified))
}

/// - `library_subscriber`: each call that installs a subscriber, at its method or function name.
/// - Matched by name, as there is no type information; macro bodies are not seen.
/// - Test code may install one: under a cfg that requires `test`, or in a test function.
pub fn check(root: &SyntaxNode, installs: Installs) -> Vec<Hit> {
    match installs {
        Installs::Allowed => Vec::new(),
        Installs::Flagged => outside_test_code(root),
    }
}

/// What the walk does at one node.
enum Visit {
    /// Test-gated or a test function: skipped with everything inside.
    TestCode,
    Call(ast::CallableExpr),
    Other,
}

impl Visit {
    /// Reads only `node`'s own attributes, so each node costs as many steps as it has.
    fn of(node: SyntaxNode) -> Self {
        if is_test_gated(&node) || has_test_attribute(&node) {
            Self::TestCode
        } else {
            ast::CallableExpr::cast(node).map_or(Self::Other, Self::Call)
        }
    }
}

/// One preorder walk, linear in the tree: a test subtree is skipped whole, not checked per call.
fn outside_test_code(root: &SyntaxNode) -> Vec<Hit> {
    let mut walk = root.preorder();
    let mut hits = Vec::new();
    while let Some(event) = walk.next() {
        match event {
            WalkEvent::Enter(node) => match Visit::of(node) {
                Visit::TestCode => walk.skip_subtree(),
                Visit::Call(call) => hits.extend(install(&call)),
                Visit::Other => {}
            },
            WalkEvent::Leave(_) => {}
        }
    }
    hits
}

/// The finding for `call` when it installs a subscriber.
fn install(call: &ast::CallableExpr) -> Option<Hit> {
    let (name, shown) = match call {
        ast::CallableExpr::MethodCall(call) => {
            let name = call.name_ref().filter(|name| named(name, &METHODS))?;
            let shown = format!(".{}()", name.text());
            (name, shown)
        }
        ast::CallableExpr::Call(call) => {
            let path = ast::PathExpr::cast(call.expr()?.syntax().clone())?.path()?;
            let name = path
                .segment()?
                .name_ref()
                .filter(|name| named(name, &FUNCTIONS))?;
            (name, format!("{}()", shown_path(&path)))
        }
    };
    Some(Hit::new(
        usize::from(name.syntax().text_range().start()),
        Rule::LibrarySubscriber,
        format!("library installs a log subscriber: `{shown}`"),
    ))
}

fn named(name: &ast::NameRef, names: &[&str]) -> bool {
    names.iter().any(|install| name.text() == *install)
}

/// - The segment names joined with `::`, so the message stays one line whatever whitespace or
///   comments the path holds.
/// - Generic arguments and a `<T as Trait>` qualifier are left out.
fn shown_path(path: &ast::Path) -> String {
    path.segments()
        .filter_map(|segment| segment.name_ref())
        .map(|name| name.text().to_string())
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests;
