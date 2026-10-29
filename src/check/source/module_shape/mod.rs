use crate::finding::{Hit, Rule};
use crate::syntax::{ModuleBody, mod_keyword, module_body, module_items, unraw};
use crate::workspace::FileKind;
use ra_ap_syntax::ast::{self, HasModuleItem, HasName};
use std::path::Path;

/// - One hit per out-of-line `mod` declaration in a file the walk reached as
///   `FileKind::NonModRs`: the file is `something.rs`, and a module with submodules is a
///   directory, `something/mod.rs`.
/// - Fires whether or not the child file exists (a missing child is already a load error),
///   and inside inline modules. A `mod.rs` file (`FileKind::ModRs`) declaring submodules is
///   the required shape; so is every target root, which the walk reaches as `ModRs`.
pub fn check(tree: &ast::SourceFile, kind: FileKind, shown: &Path) -> Vec<Hit> {
    match kind {
        FileKind::ModRs => Vec::new(),
        FileKind::NonModRs => declarations(tree.items())
            .iter()
            .map(|module| hit(module, shown))
            .collect(),
    }
}

/// Every out-of-line `mod` in the module tree of `items`: inline module bodies recursed.
/// Function and macro bodies hold no module items.
fn declarations(items: impl Iterator<Item = ast::Item>) -> Vec<ast::Module> {
    module_items(items)
        .flat_map(|module| match module_body(&module) {
            Some(ModuleBody::Inline(list)) => declarations(list.items()),
            Some(ModuleBody::OutOfLine) => vec![module],
            // - A `mod` with neither `;` nor an item list is a syntax error, so this arm runs
            //   only on files the parser rejects: there is nothing to classify, as in the walk.
            None => Vec::new(),
        })
        .collect()
}

/// The hit for one out-of-line `module`, at its `mod` keyword.
fn hit(module: &ast::Module, shown: &Path) -> Hit {
    let keyword = mod_keyword(module);
    let name = module
        .name()
        .expect("a module item in a file that parsed clean has a name");
    let name = unraw(name.text()).to_owned();
    // - Invariant: output shows a file by a path with a name.
    let shown_file = shown
        .file_name()
        .expect("output shows a file, which has a name");
    let file = shown_file.to_string_lossy();
    let stem = Path::new(shown_file)
        .file_stem()
        .unwrap_or(shown_file)
        .to_string_lossy();
    Hit::new(
        usize::from(keyword.text_range().start()),
        Rule::ModuleShape,
        format!(
            "the submodule `{name}` makes `{stem}` a directory: move `{file}` to `{stem}/mod.rs`"
        ),
    )
}

#[cfg(test)]
mod tests;
