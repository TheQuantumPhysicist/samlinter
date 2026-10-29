use crate::finding::{Hit, Rule};
use crate::syntax::path_value;
use ra_ap_syntax::{AstNode, SyntaxNode, ast};

/// - Every `path = ..` in an attribute, outer or inner, also inside `cfg_attr`s.
/// - At the `path` key, so two in one `cfg_attr` stay two findings.
/// - Macro bodies are token trees, so an attribute there is not seen.
pub fn check(root: &SyntaxNode) -> impl Iterator<Item = Hit> {
    root.descendants()
        .filter_map(ast::Attr::cast)
        .filter_map(|attr| attr.meta())
        .flat_map(|meta| path_keys(&meta))
        .map(|pair| {
            Hit::new(
                usize::from(pair.syntax().text_range().start()),
                Rule::PathAttr,
                "`path` attribute moves the module away from the file its module path names",
            )
        })
}

/// Each `path = ..` in `meta`, through nested `cfg_attr`s.
fn path_keys(meta: &ast::Meta) -> Vec<ast::KeyValueMeta> {
    match meta {
        ast::Meta::KeyValueMeta(pair) => {
            path_value(pair).map(|_| pair.clone()).into_iter().collect()
        }
        ast::Meta::CfgAttrMeta(cfg_attr) => cfg_attr
            .metas()
            .flat_map(|inner| path_keys(&inner))
            .collect(),
        ast::Meta::CfgMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::TokenTreeMeta(_)
        | ast::Meta::UnsafeMeta(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
