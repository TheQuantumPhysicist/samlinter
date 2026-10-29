use super::super::{hits, render};
use super::check;
use crate::workspace::FileKind;
use rstest::rstest;
use std::path::Path;

/// `check` on edition-2024 `text`, shown as `src/tests.rs`: the canonical victim, a tests
/// module file declaring helper files.
fn shapes_in(kind: FileKind, text: &str) -> Vec<String> {
    render(text, |file| check(file, kind, Path::new("src/tests.rs")))
}

/// `shapes_in` for a file the walk reached as `something.rs`.
fn shapes(text: &str) -> Vec<String> {
    shapes_in(FileKind::NonModRs, text)
}

#[rstest]
#[case::leaf_file_is_clean("fn f() {}\n", &[])]
#[case::one_submodule("mod b;\n", &["1:1 module_shape"])]
#[case::two_submodules(
    "mod b;\nmod c;\n",
    &["1:1 module_shape", "2:1 module_shape"]
)]
#[case::three_submodules(
    "mod b;\nmod c;\nmod d;\n",
    &["1:1 module_shape", "2:1 module_shape", "3:1 module_shape"]
)]
#[case::inline_module_only("mod outer {\n    fn f() {}\n}\n", &[])]
#[case::inline_module_with_an_inner_out_of_line(
    "mod outer {\n    mod inner;\n}\n",
    &["2:5 module_shape"]
)]
#[case::nested_inline_with_the_innermost_out_of_line(
    "mod outer {\n    mod middle {\n        mod inner;\n    }\n}\n",
    &["3:9 module_shape"]
)]
#[case::test_gated_submodule("#[cfg(test)]\nmod helper;\n", &["2:1 module_shape"])]
#[case::raw_identifier_module("mod r#async;\n", &["1:1 module_shape"])]
#[case::function_body_is_not_the_module_tree("fn f() {\n    mod x;\n}\n", &[])]
#[case::macro_body_is_opaque(
    "macro_rules! m {\n    () => {\n        mod x;\n    };\n}\n",
    &[]
)]
fn a_file_reached_as_something_rs_declaring_submodules(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(shapes(text), expected);
}

#[rstest]
#[case::mod_rs_file("mod a;\nmod b;\n")]
#[case::target_root("#[cfg(test)]\nmod tests;\n\nmod a;\n")]
fn a_mod_rs_file_declaring_submodules_is_the_required_shape(#[case] text: &str) {
    assert_eq!(shapes_in(FileKind::ModRs, text), Vec::<String>::new());
}

#[test]
fn message_names_the_module_and_the_directory_shape() {
    let messages: Vec<String> = hits("mod b;\n", |file| {
        check(file, FileKind::NonModRs, Path::new("src/tests.rs"))
    })
    .into_iter()
    .map(|hit| hit.message)
    .collect();
    assert_eq!(
        messages,
        ["the submodule `b` makes `tests` a directory: move `tests.rs` to `tests/mod.rs`"]
    );
}

#[test]
fn raw_identifier_module_is_named_without_r_hash() {
    let messages: Vec<String> = hits("mod r#async;\n", |file| {
        check(file, FileKind::NonModRs, Path::new("src/tests.rs"))
    })
    .into_iter()
    .map(|hit| hit.message)
    .collect();
    assert_eq!(
        messages,
        ["the submodule `async` makes `tests` a directory: move `tests.rs` to `tests/mod.rs`"]
    );
}

#[test]
fn each_declaration_gets_its_own_message() {
    let messages: Vec<String> = hits("mod b;\nmod c;\n", |file| {
        check(file, FileKind::NonModRs, Path::new("src/tests.rs"))
    })
    .into_iter()
    .map(|hit| hit.message)
    .collect();
    assert_eq!(messages.len(), 2);
    assert_ne!(messages.first(), messages.get(1));
}
