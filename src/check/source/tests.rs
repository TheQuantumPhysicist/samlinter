// - TODO: randomized variant: random text with CRLF line ends and an optional BOM gives the
//   same findings as its LF form, positions included.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::super::text_file;
use super::check;
use crate::config::{Config, words};
use ra_ap_syntax::Edition;
use rstest::rstest;

const PHRASE: &str = "it is worth noting";

fn findings_in(edition: Edition, text: &str) -> Vec<String> {
    let file = text_file("src/lib.rs", text);
    let config = Config {
        forbidden_words: words(&["NOTES.md", PHRASE]),
        subscriber_crates: Vec::new(),
    };
    check(&file, edition, &config)
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn findings(text: &str) -> Vec<String> {
    findings_in(Edition::Edition2024, text)
}

#[rstest]
#[case::rest_pattern_after_the_error("fn f( {\n}\nfn g(v: V) { let V { .. } = v; }\n")]
#[case::comment_before_the_error("// see NOTES.md \u{2014}\nfn f( {\n")]
fn file_with_syntax_errors_is_reported_not_linted(#[case] text: &str) {
    let found = findings(text);
    assert_ne!(found, Vec::<String>::new());
    assert!(
        found.iter().all(|line| line.contains(": syntax_error: ")),
        "{found:?}"
    );
}

#[rstest]
#[case::line_comment(
    "// see NOTES.md\nfn f() {}\n",
    &["src/lib.rs:1:8: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
#[case::block_comment(
    "/* NOTES.md */\nfn f() {}\n",
    &["src/lib.rs:1:4: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
#[case::doc_comment(
    "/// a \u{2014} b\nfn f() {}\n",
    &["src/lib.rs:1:7: em_dash: em-dash (U+2014)"]
)]
#[case::inner_doc_comment("//! a \u{2014} b\n", &["src/lib.rs:1:7: em_dash: em-dash (U+2014)"])]
#[case::comment_in_macro_body(
    "macro_rules! m {\n    () => {\n        // a \u{2014} b\n        fn f() {}\n    };\n}\n",
    &["src/lib.rs:3:14: em_dash: em-dash (U+2014)"]
)]
#[case::doc_comment_in_macro_body(
    "macro_rules! m {\n    () => {\n        /// a \u{2014} b\n        fn f() {}\n    };\n}\n",
    &["src/lib.rs:3:15: em_dash: em-dash (U+2014)"]
)]
#[case::block_doc_comment(
    "/** see NOTES.md */\nfn f() {}\n",
    &["src/lib.rs:1:9: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
#[case::inner_block_doc_comment(
    "/*! see NOTES.md */\n",
    &["src/lib.rs:1:9: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
#[case::string_literal_is_not_a_comment("const S: &str = \"NOTES.md \u{2014}\";\n", &[])]
fn scans_comments_not_string_literals(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(findings(text), expected);
}

/// - Each finding in `text` as `line:column` when it is the forbidden `PHRASE`.
/// - Any other finding stays whole, so it shows in a failed comparison.
fn phrase_findings(text: &str) -> Vec<String> {
    let suffix =
        format!(": forbidden_word: `{PHRASE}` is listed in forbidden-words in samlinter.toml");
    findings(text)
        .into_iter()
        .map(|line| {
            line.strip_prefix("src/lib.rs:")
                .and_then(|rest| rest.strip_suffix(suffix.as_str()))
                .map_or_else(|| line.clone(), str::to_owned)
        })
        .collect()
}

#[rstest]
#[case::wrapped_across_line_comments("// it is worth\n// noting\nfn f() {}\n", &["1:4"])]
#[case::wrapped_across_indented_line_comments(
    "fn f() {\n    // it is worth\n    // noting\n}\n",
    &["2:8"]
)]
#[case::on_the_second_line_of_a_run("// a\n// it is worth noting\n", &["2:4"])]
#[case::wrapped_across_doc_comments("/// it is worth\n/// noting\nfn f() {}\n", &["1:5"])]
#[case::wrapped_across_inner_doc_comments("//! it is worth\n//! noting\n", &["1:5"])]
#[case::wrapped_inside_a_block_comment("/* it is worth\n   noting */\nfn f() {}\n", &["1:4"])]
#[case::trailing_comment_joins_the_next_line("fn f() {} // it is worth\n// noting\n", &["1:14"])]
#[case::code_between_ends_the_run("// it is worth\nfn f() {}\n// noting\n", &[])]
#[case::blank_line_ends_the_run("// it is worth\n\n// noting\nfn f() {}\n", &[])]
#[case::doc_comment_does_not_join_a_plain_one("// it is worth\n/// noting\nfn f() {}\n", &[])]
#[case::inner_doc_comment_does_not_join_an_outer_one(
    "//! it is worth\n/// noting\nfn f() {}\n",
    &[]
)]
#[case::block_comments_do_not_join("/* it is worth */\n/* noting */\nfn f() {}\n", &[])]
#[case::block_comment_does_not_join_a_line_comment(
    "// it is worth\n/* noting */\nfn f() {}\n",
    &[]
)]
fn phrases_match_across_consecutive_line_comments(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(phrase_findings(text), expected);
}

#[rstest]
#[case::crlf_inside_string_literal("const S: &str = \"a\r\nb\";\r\n", &[])]
#[case::crlf_after_trailing_comment("fn a() {} // note\r\n\r\nfn b() {}\r\n", &[])]
#[case::crlf_blank_line_inside_use_block(
    "use std::fmt; // note\r\n\r\nuse std::io;\r\n",
    &["src/lib.rs:3:1: item_spacing: blank line between consecutive use items"]
)]
#[case::crlf_position(
    "// a\r\n// see NOTES.md\r\n",
    &["src/lib.rs:2:8: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
#[case::byte_order_mark("\u{feff}fn f() {}\n", &[])]
#[case::byte_order_mark_position(
    "\u{feff}// see NOTES.md\n",
    &["src/lib.rs:1:8: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"]
)]
fn reads_line_endings_and_byte_order_mark_as_rustc_does(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(findings(text), expected);
}

#[rstest]
#[case::identifier_before_2024(Edition::Edition2021, &[])]
#[case::keyword_from_2024(
    Edition::Edition2024,
    &["src/lib.rs:2:9: syntax_error: expected pattern"]
)]
fn parses_with_the_package_edition(#[case] edition: Edition, #[case] expected: &[&str]) {
    assert_eq!(
        findings_in(edition, "fn f() {\n    let gen = 1;\n}\n"),
        expected
    );
}

#[test]
fn exemption_lifts_only_the_rule_it_names() {
    let text = "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\npub(crate) fn f(v: V) {\n    let V { a, .. } = v;\n}\n";
    assert_eq!(
        findings(text),
        ["src/lib.rs:2:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy"]
    );
}
