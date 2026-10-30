use super::super::{hits, render};
use super::check;
use crate::config::Config;
use ra_ap_syntax::{AstNode, Edition};
use rstest::rstest;

fn discards(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax(), text))
}

/// Every `let _ = ..;` with no comment on its own line or on the line directly above.
#[rstest]
#[case::bare("fn f() {\n    let _ = g();\n}\n", &["2:5 discarded_result"])]
#[case::type_annotated("fn f() {\n    let _: u8 = g();\n}\n", &["2:5 discarded_result"])]
#[case::multi_line_expression(
    "fn f() {\n    let _ = g(\n        1,\n    );\n}\n",
    &["2:5 discarded_result"]
)]
#[case::blank_line_breaks_the_comment_above(
    "fn f() {\n    // the error cannot happen\n\n    let _ = g();\n}\n",
    &["4:5 discarded_result"]
)]
#[case::comment_above_a_different_statement(
    "fn f() {\n    // the error cannot happen\n    let x = g();\n    let _ = h();\n}\n",
    &["4:5 discarded_result"]
)]
#[case::also_in_a_test_function(
    "#[test]\nfn t() {\n    let _ = g();\n}\n",
    &["3:5 discarded_result"]
)]
fn flags_discards_without_a_stated_reason(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(discards(text), expected);
}

/// The stated reason is the rule's own satisfied condition: a comment on the statement's
/// line, or on the line directly above.
#[rstest]
#[case::comment_on_the_same_line("fn f() {\n    let _ = g(); // the error cannot happen\n}\n", &[])]
#[case::block_comment_on_the_same_line("fn f() {\n    let _ = g(); /* why */\n}\n", &[])]
#[case::comment_directly_above(
    "fn f() {\n    // the error cannot happen\n    let _ = g();\n}\n",
    &[]
)]
#[case::trailing_comment_on_the_last_line(
    "fn f() {\n    let _ = g(\n        1,\n    ); // the error cannot happen\n}\n",
    &[]
)]
fn silent_when_a_comment_states_the_reason(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(discards(text), expected);
}

/// Only the wildcard pattern discards: every other pattern binds or names a field.
#[rstest]
#[case::named_binding("fn f() {\n    let x = g();\n}\n", &[])]
#[case::underscore_prefixed_binding("fn f() {\n    let _x = g();\n}\n", &[])]
#[case::destructuring("fn f() {\n    let (a, _) = g();\n}\n", &[])]
#[case::no_initializer("fn f() {\n    let _;\n}\n", &[])]
fn silent_on_bindings_that_keep_a_handle(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(discards(text), expected);
}

#[test]
fn message_names_the_discard() {
    let text = "fn f() {\n    let _ = g();\n}\n";
    let messages: Vec<String> = hits(text, |file| check(file.syntax(), text))
        .into_iter()
        .map(|hit| hit.message)
        .collect();
    assert_eq!(
        messages,
        ["`let _ =` discards a result without a stated reason"]
    );
}

/// The comment is the rule's satisfied condition, so an expect attribute lifts nothing:
/// the discard stays, and the exemption machinery reports the expect as malformed.
#[test]
fn an_expect_does_not_lift_it() {
    let config = Config {
        forbidden_words: Vec::new(),
        subscriber_crates: Vec::new(),
    };
    let text = "#[cfg_attr(samlinter, expect(discarded_result, reason = \"why\"))]\nfn f() {\n    let _ = g();\n}\n";
    let file = super::super::super::text_file("src/lib.rs", text);
    let findings = super::super::check(
        &file,
        Edition::Edition2024,
        &config,
        super::super::Installs::Allowed,
        super::super::FileKind::ModRs,
    )
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            "src/lib.rs:3:5: discarded_result: `let _ =` discards a result without a stated reason",
            "src/lib.rs:1:1: malformed_exemption: rule `discarded_result` cannot be exempted",
        ]
    );
}
