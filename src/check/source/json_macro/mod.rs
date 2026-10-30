use super::outside_test_code;
use crate::finding::{Hit, Rule};
use crate::workspace::TestCode;
use ra_ap_syntax::{AstNode, SyntaxNode, ast};

const MESSAGE: &str = "`json!` constructs untyped JSON";

/// - Every `json!` macro invocation, at the macro's name: `json!`, `serde_json::json!`, any
///   leading path, in item or expression position.
/// - Silent inside test code: a file the walk reached as test code whole-file, a node whose
///   own attributes require `test`, and test functions. `json!` is allowed in tests, and an
///   expect attribute on every test's use would be churn.
/// - Macro bodies are token trees, so an invocation inside one is not seen.
pub fn check(root: &SyntaxNode, test: TestCode) -> Vec<Hit> {
    outside_test_code(root, test, |node| {
        let call = ast::MacroCall::cast(node.clone())?;
        let name = call.path()?.segment()?.name_ref()?;
        (name.text() == "json").then(|| {
            Hit::new(
                usize::from(name.syntax().text_range().start()),
                Rule::JsonMacro,
                MESSAGE,
            )
        })
    })
}

#[cfg(test)]
mod tests;
