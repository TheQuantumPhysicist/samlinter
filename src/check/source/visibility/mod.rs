use super::start;
use crate::finding::{Hit, Rule};
use ra_ap_syntax::{AstNode, SyntaxNode, ast};

pub fn check(root: &SyntaxNode) -> impl Iterator<Item = Hit> {
    root.descendants()
        .filter_map(ast::Visibility::cast)
        .filter(bypasses_hierarchy)
        .map(|visibility| {
            Hit::new(
                start(visibility.syntax()),
                Rule::RestrictedVisibility,
                format!(
                    "`{}` bypasses the module hierarchy",
                    visibility.syntax().text()
                ),
            )
        })
}

/// `pub(crate)`, `pub(super)` and `pub(in path)`; `pub(self)` is plain privacy.
fn bypasses_hierarchy(visibility: &ast::Visibility) -> bool {
    visibility
        .visibility_inner()
        .and_then(|inner| inner.path())
        .is_some_and(|path| path.syntax().text() != "self")
}

#[cfg(test)]
mod tests;
