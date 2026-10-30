use super::super::{hits, render};
use super::check;
use crate::config::Config;
use crate::workspace::TestCode;
use ra_ap_syntax::{AstNode, Edition};
use rstest::rstest;

fn json_macros(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax(), TestCode::NotTest))
}

/// `json_macros` for a file the walk reached as test code whole-file.
fn json_macros_in_a_test_file(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax(), TestCode::Test))
}

/// Every `json!` invocation with a payload outside test code, at the macro's name, any path
/// form; an invocation whose argument is exactly an empty object is exempt.
#[rstest]
#[case::bare("fn f() { json!({\"k\": 1}); }\n", &["1:10 json_macro"])]
#[case::through_the_crate_path("fn f() { serde_json::json!(1); }\n", &["1:22 json_macro"])]
#[case::through_any_leading_path("fn f() { a::b::json!(1); }\n", &["1:16 json_macro"])]
#[case::in_item_position("json!({\"k\": 1});\n", &["1:1 json_macro"])]
#[case::an_empty_object_is_exempt("fn f() { json!({}); }\n", &[])]
#[case::empty_object_with_inner_whitespace_is_exempt("fn f() { json!( { } ); }\n", &[])]
#[case::empty_object_with_a_comment_is_exempt("fn f() { json!({ /* wired elsewhere */ }); }\n", &[])]
#[case::empty_object_in_item_position_is_exempt("json!({});\n", &[])]
#[case::a_payload_with_an_empty_string_key_still_flags("fn f() { json!({\"\": 1}); }\n", &["1:10 json_macro"])]
#[case::beside_a_test_module(
    "#[cfg(test)]\nmod tests {\n    fn a() { json!(1); }\n}\n\nfn b() { json!(1); }\n",
    &["6:10 json_macro"]
)]
#[case::a_non_test_cfg_still_flags(
    "#[cfg(feature = \"json\")]\nfn f() { json!(1); }\n",
    &["2:10 json_macro"]
)]
fn flags_json_macros_outside_test_code(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(json_macros(text), expected);
}

/// Test code is the carve-out: under a cfg that requires `test`, or in a test function.
#[rstest]
#[case::cfg_test_module("#[cfg(test)]\nmod tests {\n    fn a() { json!(1); }\n}\n", &[])]
#[case::inner_cfg_test_module(
    "mod tests {\n    #![cfg(test)]\n    fn a() { json!(1); }\n}\n",
    &[]
)]
#[case::test_function("#[test]\nfn t() { json!(1); }\n", &[])]
#[case::tokio_test_function("#[tokio::test]\nfn t() { json!(1); }\n", &[])]
#[case::test_function_in_a_plain_module(
    "mod m {\n    #[test]\n    fn t() { json!(1); }\n}\n",
    &[]
)]
fn silent_inside_test_code(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(json_macros(text), expected);
}

/// A file the walk reached as test code is silent whole-file, wherever the `json!` sits in it.
#[rstest]
#[case::in_a_plain_function("fn f() { json!(1); }\n", &[])]
#[case::in_item_position("json!({});\n", &[])]
fn silent_in_a_test_reached_file(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(json_macros_in_a_test_file(text), expected);
}

/// Only the macro named `json` counts.
#[rstest]
#[case::another_macro("fn f() { m!(1); }\n", &[])]
#[case::a_name_ending_in_json("fn f() { other_json!(1); }\n", &[])]
fn silent_on_other_macro_names(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(json_macros(text), expected);
}

#[test]
fn message_names_the_macro() {
    let messages: Vec<String> = hits("fn f() { json!(1); }\n", |file| {
        check(file.syntax(), TestCode::NotTest)
    })
    .into_iter()
    .map(|hit| hit.message)
    .collect();
    assert_eq!(messages, ["`json!` constructs untyped JSON"]);
}

/// In non-test code a single justified use is exempted with an expect on the function.
#[test]
fn exempted_with_an_expect_on_the_function() {
    let config = Config {
        forbidden_words: Vec::new(),
        subscriber_crates: Vec::new(),
    };
    let file = super::super::super::text_file(
        "src/lib.rs",
        "#[cfg_attr(samlinter, expect(json_macro, reason = \"why\"))]\nfn f() { json!(1); }\n",
    );
    let findings = super::super::check(
        &file,
        Edition::Edition2024,
        &config,
        super::super::Installs::Allowed,
        super::super::FileKind::ModRs,
        TestCode::NotTest,
    );
    assert!(findings.is_empty(), "{findings:?}");
}
