mod cfg;

pub use cfg::{
    cfg_attributes, has_test_attribute, is_bare_cfg_atom, is_test_gated, is_test_module,
    requires_test,
};
use ra_ap_syntax::{
    AstNode, Edition, SyntaxError, SyntaxNode, SyntaxToken, WalkEvent,
    ast::{self, HasAttrs},
};

/// - Tree depth up to which `ra_ap_syntax`'s own drop thread frees a tree safely.
/// - That thread has a default 2 MiB stack and frees recursively; it overflowed near 10000
///   levels (5000 nested inline modules), so this keeps a tenfold margin.
const DROP_THREAD_DEPTH: usize = 1000;

/// How deep a tree is, against what the parser's drop thread frees safely.
#[derive(Debug, PartialEq, Eq)]
enum Depth {
    Shallow,
    Deep,
}

impl Depth {
    /// Walked iteratively, so the measure itself does not recurse.
    fn of(root: &SyntaxNode) -> Self {
        let (_, deepest) = root
            .preorder()
            .fold((0_usize, 0_usize), |(depth, deepest), event| match event {
                WalkEvent::Enter(_) => {
                    let entered = depth.saturating_add(1);
                    (entered, deepest.max(entered))
                }
                WalkEvent::Leave(_) => (depth.saturating_sub(1), deepest),
            });
        if deepest > DROP_THREAD_DEPTH {
            Self::Deep
        } else {
            Self::Shallow
        }
    }
}

/// - `text` parsed as a Rust file: its tree and its syntax errors.
/// - `ra_ap_syntax` hands a dropped parse to its own thread, whose 2 MiB stack overflows freeing
///   a deep tree and aborts the process.
/// - So a deep tree's parse is never dropped: it stays in memory until samlinter exits. A
///   shallow one, every ordinary file, is dropped and freed as usual.
pub fn parse(text: &str, edition: Edition) -> (ast::SourceFile, Vec<SyntaxError>) {
    let parse = ast::SourceFile::parse(text, edition);
    let parsed = (parse.tree(), parse.errors());
    match Depth::of(parsed.0.syntax()) {
        Depth::Shallow => drop(parse),
        Depth::Deep => std::mem::forget(parse),
    }
    parsed
}

/// - The text of a raw identifier unquoted: one `r#` prefix taken off.
/// - Shared by the module walk, `path_attr` and `module_shape`, which name things by it.
pub fn unraw(text: &str) -> &str {
    text.strip_prefix("r#").unwrap_or(text)
}

/// - The value of `path = <expr>`, the one definition of a `path` attribute, with a bare
///   `path` key or its raw-identifier spelling `r#path`, which rustc accepts alike.
/// - Shared by the module walk, which follows it, and `path_attr`, which reports it.
pub fn path_value(pair: &ast::KeyValueMeta) -> Option<ast::Expr> {
    pair.path()
        .and_then(|path| path.as_single_name_ref())
        .filter(|name| unraw(name.text()) == "path")
        .and(pair.expr())
}

/// - The segment names of a path joined with `::`, so a message stays one line whatever
///   whitespace or comments the path holds.
/// - Generic arguments and a `<T as Trait>` qualifier are left out.
/// - Shared by `subscriber` and `banned`, which name the symbol a finding sits at.
pub fn shown_path(path: &ast::Path) -> String {
    path.segments()
        .filter_map(|segment| segment.name_ref())
        .map(|name| name.text().to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// - The `mod` items among `items`, the one module-item filter.
/// - Shared by the module walk, which follows them, and `module_shape`, which reports them.
/// - Exhaustive on purpose: a new `ast::Item` variant forces a decision here.
pub fn module_items(items: impl Iterator<Item = ast::Item>) -> impl Iterator<Item = ast::Module> {
    items.filter_map(|item| match item {
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
}

/// How a `mod` item gives its contents.
pub enum ModuleBody {
    /// `mod name;`: in a file of its own.
    OutOfLine,
    /// `mod name { .. }`
    Inline(ast::ItemList),
}

impl ModuleBody {
    /// The inner `#![..]` attributes; only an inline module's item list holds them.
    pub fn inner_attrs(&self) -> Vec<ast::Attr> {
        match self {
            Self::OutOfLine => Vec::new(),
            Self::Inline(list) => list.attrs().collect(),
        }
    }
}

/// - How `module` gives its contents: inline with its item list, or out of line after a `;`.
/// - `None` for a `mod` with neither `;` nor an item list; the parser reports it as a syntax
///   error, so no caller sees it on a file that parsed clean.
/// - Shared by the module walk, which follows it, and `module_shape`, which reports it.
pub fn module_body(module: &ast::Module) -> Option<ModuleBody> {
    module
        .item_list()
        .map(ModuleBody::Inline)
        .or_else(|| module.semicolon_token().map(|_| ModuleBody::OutOfLine))
}

/// - The `mod` keyword token of `module`.
/// - Invariant: the parser starts every module item at its `mod` keyword. A missing keyword is
///   a parser bug, not bad input, so there is nothing to report.
/// - Shared by the module walk, which positions the declaration by it, and `module_shape`,
///   which reports the finding at it.
pub fn mod_keyword(module: &ast::Module) -> SyntaxToken {
    module
        .mod_token()
        .expect("the parser builds every module item from its `mod` keyword")
}

#[cfg(test)]
mod tests;
