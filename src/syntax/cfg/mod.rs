use ra_ap_syntax::ast;
use ra_ap_syntax::{AstNode, SyntaxKind, SyntaxNode};

/// The `#[cfg(..)]` attributes among `attrs`.
pub fn cfg_attributes(
    attrs: impl Iterator<Item = ast::Attr>,
) -> impl Iterator<Item = ast::CfgMeta> {
    attrs.filter_map(|attr| attr.meta()).filter_map(cfg_meta)
}

fn cfg_meta(meta: ast::Meta) -> Option<ast::CfgMeta> {
    match meta {
        ast::Meta::CfgMeta(cfg) => Some(cfg),
        ast::Meta::CfgAttrMeta(_)
        | ast::Meta::KeyValueMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::TokenTreeMeta(_)
        | ast::Meta::UnsafeMeta(_) => None,
    }
}

/// - The attributes `node` holds itself: outer ones of an item or expression, inner ones of a
///   file, item list or block.
/// - They lead its child nodes, doc comments among them, so the scan stops at the first other
///   child instead of reading a whole item or statement list.
fn own_attrs(node: &SyntaxNode) -> impl Iterator<Item = ast::Attr> {
    node.children()
        .take_while(|child| {
            child.kind() == SyntaxKind::ATTR || child.kind() == SyntaxKind::DOC_COMMENT
        })
        .filter_map(ast::Attr::cast)
}

/// Stacked cfg attributes all apply, so one that requires `test` gates `node` to test builds.
pub fn is_test_gated(node: &SyntaxNode) -> bool {
    cfg_attributes(own_attrs(node))
        .filter_map(|cfg| cfg.cfg_predicate())
        .any(|predicate| requires_test(&predicate))
}

/// - A module compiles only in test builds when its outer attributes or, inline, the inner ones
///   of its item list require `test`.
/// - The one test-module definition: layout slots, the module walk and `library_subscriber`.
pub fn is_test_module(module: &ast::Module) -> bool {
    is_test_gated(module.syntax())
        || module
            .item_list()
            .is_some_and(|list| is_test_gated(list.syntax()))
}

/// - A test function: an attribute whose path ends in `test`, such as `#[test]` or
///   `#[tokio::test]`.
/// - Those attributes compile the function only in test builds.
pub fn has_test_attribute(node: &SyntaxNode) -> bool {
    own_attrs(node).any(|attr| {
        attr.path()
            .and_then(|path| path.segment())
            .and_then(|segment| segment.name_ref())
            .is_some_and(|name| name.text() == "test")
    })
}

/// - Whether `predicate` holds only when a bare `test` does.
/// - Every member of `all(..)` must hold; `any(..)` and `not(..)` require no single member.
pub fn requires_test(predicate: &ast::CfgPredicate) -> bool {
    match predicate {
        ast::CfgPredicate::CfgAtom(atom) => is_bare_atom(atom, "test"),
        ast::CfgPredicate::CfgComposite(composite) => match composite.keyword() {
            Some(keyword) if keyword.text() == "all" => composite
                .cfg_predicates()
                .any(|member| requires_test(&member)),
            // `any`, `not`, and keywords rustc rejects.
            Some(_) | None => false,
        },
    }
}

/// `cfg(<name>)` with a bare identifier and no value.
pub fn is_bare_cfg_atom(predicate: &ast::CfgPredicate, name: &str) -> bool {
    match predicate {
        ast::CfgPredicate::CfgAtom(atom) => is_bare_atom(atom, name),
        ast::CfgPredicate::CfgComposite(_) => false,
    }
}

/// The atom `<name>`, without a `= "value"`.
fn is_bare_atom(atom: &ast::CfgAtom, name: &str) -> bool {
    match Atom::of(atom) {
        Atom::Bare(ident) => ident.text() == name,
        Atom::Other => false,
    }
}

/// The shape of a cfg atom that decides whether it names one bare cfg.
enum Atom {
    /// `name`, with no value.
    Bare(ra_ap_syntax::SyntaxToken),
    /// `name = "value"`, or an atom without a name, which rustc rejects.
    Other,
}

impl Atom {
    fn of(atom: &ast::CfgAtom) -> Self {
        match (atom.ident_token(), atom.eq_token()) {
            (Some(ident), None) => Self::Bare(ident),
            (Some(_), Some(_)) | (None, None | Some(_)) => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests;
