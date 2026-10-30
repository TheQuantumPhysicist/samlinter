use super::outside_test_code;
use crate::finding::{Hit, Rule};
use crate::workspace::TestCode;
use ra_ap_syntax::{AstNode, NodeOrToken, SyntaxKind, SyntaxNode, ast};

const MESSAGE: &str = "`json!` constructs untyped JSON";

/// - Every `json!` macro invocation, at the macro's name: `json!`, `serde_json::json!`, any
///   leading path, in item or expression position.
/// - Silent on an invocation whose argument is exactly an empty brace group: `json!({})`,
///   inner whitespace allowed. An empty object has no typed-struct replacement, so the
///   rule's guidance cannot apply to it.
/// - Silent inside test code: a file the walk reached as test code whole-file, a node whose
///   own attributes require `test`, and test functions. `json!` is allowed in tests, and an
///   expect attribute on every test's use would be churn.
/// - Macro bodies are token trees, so an invocation inside one is not seen.
pub fn check(root: &SyntaxNode, test: TestCode) -> Vec<Hit> {
    outside_test_code(root, test, |node| {
        let call = ast::MacroCall::cast(node.clone())?;
        let name = call.path()?.segment()?.name_ref()?;
        if name.text() != "json" || empty_object(&call) {
            return None;
        }
        Some(Hit::new(
            usize::from(name.syntax().text_range().start()),
            Rule::JsonMacro,
            MESSAGE,
        ))
    })
}

/// - True when the invocation's argument is exactly one nested brace group whose inner
///   tokens are whitespace and comments only: `json!({})` or `json!( { /* none */ } )`.
///   Comments cannot carry a payload, so a comment-only body is still an empty object.
/// - Anything else -- a payload token, a second group, another nesting level -- is a real
///   object the rule still flags.
fn empty_object(call: &ast::MacroCall) -> bool {
    let Some(tree) = call
        .syntax()
        .children()
        .find(|node| node.kind() == SyntaxKind::TOKEN_TREE)
    else {
        return false;
    };
    let mut object: Option<SyntaxNode> = None;
    for element in tree.children_with_tokens() {
        match element {
            NodeOrToken::Node(node) => {
                if object.is_some() || node.kind() != SyntaxKind::TOKEN_TREE {
                    return false;
                }
                object = Some(node);
            }
            NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind != SyntaxKind::WHITESPACE
                    && kind != SyntaxKind::L_PAREN
                    && kind != SyntaxKind::R_PAREN
                {
                    return false;
                }
            }
        }
    }
    let Some(object) = object else {
        return false;
    };
    object.children_with_tokens().all(|element| match element {
        NodeOrToken::Node(_) => false,
        NodeOrToken::Token(token) => {
            let kind = token.kind();
            kind == SyntaxKind::WHITESPACE
                || kind == SyntaxKind::COMMENT
                || kind == SyntaxKind::L_CURLY
                || kind == SyntaxKind::R_CURLY
        }
    })
}

#[cfg(test)]
mod tests;
