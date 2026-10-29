use super::super::text_file;
use super::{Installs, check};
use crate::config::{Config, words};
use ra_ap_syntax::Edition;
use rstest::rstest;

const PHRASE: &str = "it is worth noting";

fn findings_in(edition: Edition, installs: Installs, text: &str) -> Vec<String> {
    let file = text_file("src/lib.rs", text);
    let config = Config {
        forbidden_words: words(&["NOTES.md", PHRASE]),
        subscriber_crates: Vec::new(),
    };
    check(&file, edition, &config, installs)
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn findings(text: &str) -> Vec<String> {
    findings_in(Edition::Edition2024, Installs::Allowed, text)
}

/// `findings` in library code that may not install a log subscriber.
fn library_findings(text: &str) -> Vec<String> {
    findings_in(Edition::Edition2024, Installs::Flagged, text)
}

#[rstest]
#[case::rest_pattern_after_the_error(
    "fn f( {\n}\nfn g(v: V) { let V { .. } = v; }\n",
    &[
        "src/lib.rs:1:6: syntax_error: expected value parameter",
        "src/lib.rs:1:6: syntax_error: expected R_PAREN",
    ]
)]
#[case::comment_before_the_error(
    "// see NOTES.md \u{2014}\nfn f( {\n",
    &[
        "src/lib.rs:2:6: syntax_error: expected value parameter",
        "src/lib.rs:2:6: syntax_error: expected R_PAREN",
        "src/lib.rs:2:8: syntax_error: expected R_CURLY",
    ]
)]
fn file_with_syntax_errors_is_reported_not_linted(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(findings(text), expected);
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
        findings_in(
            edition,
            Installs::Allowed,
            "fn f() {\n    let gen = 1;\n}\n"
        ),
        expected
    );
}

#[test]
fn each_syntax_error_is_one_finding() {
    assert_eq!(
        findings("fn f() {\n    let gen = 1;\n    let gen = 2;\n}\n"),
        [
            "src/lib.rs:2:9: syntax_error: expected pattern",
            "src/lib.rs:3:9: syntax_error: expected pattern",
        ]
    );
}

/// `text` with `ATTR` replaced by an exemption for `rule`.
fn exempting(rule: &str, text: &str) -> String {
    text.replace(
        "ATTR",
        &format!("#[cfg_attr(samlinter, expect({rule}, reason = \"why\"))]"),
    )
}

#[rstest]
#[case::rest_pattern("rest_pattern", "ATTR\nfn f(v: V) {\n    let V { a, .. } = v;\n}\n")]
#[case::restricted_visibility("restricted_visibility", "ATTR\npub(crate) fn f() {}\n")]
#[case::item_order("item_order", "fn f() {}\n\nATTR\nuse std::io;\n")]
#[case::item_spacing("item_spacing", "use std::fmt;\n\nATTR\nuse std::io;\n")]
#[case::fn_spacing("fn_spacing", "fn f() {}\nATTR\nfn g() {}\n")]
#[case::em_dash("em_dash", "ATTR\nfn f() {\n    // a \u{2014} b\n}\n")]
#[case::emoji("emoji", "ATTR\nfn f() {\n    // done \u{1F389}\n}\n")]
#[case::forbidden_word("forbidden_word", "ATTR\nfn f() {\n    // see NOTES.md\n}\n")]
#[case::forbidden_word_in_the_doc_comment("forbidden_word", "/// see NOTES.md\nATTR\nfn f() {}\n")]
#[case::em_dash_in_the_doc_comment("em_dash", "/// a \u{2014} b\nATTR\nfn f() {}\n")]
#[case::forbidden_word_in_the_plain_comment_above(
    "forbidden_word",
    "// see NOTES.md\nATTR\nfn f() {}\n"
)]
#[case::library_subscriber("library_subscriber", "fn f(s: S) {\n    ATTR\n    s.init();\n}\n")]
#[case::path_attr("path_attr", "ATTR\n#[path = \"sys_unix.rs\"]\nmod sys;\n")]
#[case::path_attr_in_a_cfg_attr(
    "path_attr",
    "ATTR\n#[cfg_attr(unix, path = \"sys_unix.rs\")]\nmod sys;\n"
)]
#[case::inner_path_attr_on_its_inline_module(
    "path_attr",
    "ATTR\nmod m {\n    #![path = \"d\"]\n    mod n;\n}\n"
)]
fn exemption_lifts_each_rust_source_rule(#[case] rule: &str, #[case] text: &str) {
    assert_eq!(
        library_findings(&exempting(rule, text)),
        Vec::<String>::new()
    );
}

#[rstest]
#[case::rest_pattern("rest_pattern")]
#[case::restricted_visibility("restricted_visibility")]
#[case::item_order("item_order")]
#[case::item_spacing("item_spacing")]
#[case::fn_spacing("fn_spacing")]
#[case::em_dash("em_dash")]
#[case::emoji("emoji")]
#[case::forbidden_word("forbidden_word")]
#[case::library_subscriber("library_subscriber")]
#[case::path_attr("path_attr")]
fn exemption_matching_nothing_is_unfulfilled(#[case] rule: &str) {
    assert_eq!(
        library_findings(&exempting(rule, "ATTR\nfn f() {}\n")),
        [format!(
            "src/lib.rs:1:1: unfulfilled_exemption: exemption for `{rule}` matches nothing here"
        )]
    );
}

/// Each finding as `line:column rule`.
fn placed(text: &str) -> Vec<String> {
    findings(text)
        .iter()
        .filter_map(|line| {
            let mut parts = line.strip_prefix("src/lib.rs:")?.splitn(4, ':');
            let (row, column, rule) = (parts.next()?, parts.next()?, parts.next()?);
            Some(format!("{row}:{column}{rule}"))
        })
        .collect()
}

/// - The parser files doc comments and the plain comments directly above an item or a field
///   under that node, so its exemption covers them.
/// - A plain comment above a `let`, a match arm or a parameter sits outside it, and a file's
///   `//!` docs, a comment a blank line apart from the next item and one after the last item
///   belong to no node: no exemption reaches them.
#[rstest]
#[case::doc_comment_of_a_field(
    "struct S {\n    /// see NOTES.md\n    ATTR\n    a: u8,\n}\n",
    &[]
)]
#[case::plain_comment_above_a_field(
    "struct S {\n    // see NOTES.md\n    ATTR\n    a: u8,\n}\n",
    &[]
)]
#[case::plain_comment_above_a_let(
    "fn f() {\n    // see NOTES.md\n    ATTR\n    let a = 1;\n}\n",
    &["2:12 forbidden_word", "3:5 unfulfilled_exemption"]
)]
#[case::plain_comment_above_a_match_arm(
    "fn f(e: E) {\n    match e {\n        // see NOTES.md\n        ATTR\n        E::A => {}\n    }\n}\n",
    &["3:16 forbidden_word", "4:9 unfulfilled_exemption"]
)]
#[case::file_docs(
    "//! see NOTES.md\n\nATTR\nfn f() {}\n",
    &["1:9 forbidden_word", "3:1 unfulfilled_exemption"]
)]
#[case::comment_a_blank_line_apart(
    "// see NOTES.md\n\nATTR\nfn f() {}\n",
    &["1:8 forbidden_word", "3:1 unfulfilled_exemption"]
)]
#[case::comment_after_the_last_item(
    "ATTR\nfn f() {}\n\n// see NOTES.md\n",
    &["4:8 forbidden_word", "1:1 unfulfilled_exemption"]
)]
fn exemption_covers_the_comments_its_node_owns(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(placed(&exempting("forbidden_word", text)), expected);
}

/// - An outer exemption sits on an item, and a file top is no item: an expect on the first
///   item neither covers a file-top `#![path]` nor goes away.
/// - The covering form is an inner expect at the top of the same file.
#[test]
fn file_top_path_attribute_is_not_covered_by_an_expect_on_an_item() {
    assert_eq!(
        placed(&exempting(
            "path_attr",
            "#![path = \"x\"]\n\nATTR\nfn f() {}\n"
        )),
        ["1:4 path_attr", "3:1 unfulfilled_exemption"]
    );
}

/// A file-top `#![path]` is exempted by an inner expect at the top of the same file, on the
/// same node, like an inline module's inner expect owns its inner `#![path]`.
#[test]
fn file_top_path_attribute_is_exempted_by_an_inner_expect() {
    assert_eq!(
        placed(
            "#![path = \"x\"]\n#![cfg_attr(samlinter, expect(path_attr, reason = \"why\"))]\nfn f() {}\n"
        ),
        Vec::<String>::new()
    );
}

/// An inner expect at the top of an inline module covers its inner `#![path]`, like an
/// outer expect on that module does.
#[test]
fn inner_path_attribute_is_exempted_at_the_top_of_its_inline_module() {
    assert_eq!(
        placed(
            "mod m {\n    #![cfg_attr(samlinter, expect(path_attr, reason = \"why\"))]\n    #![path = \"d\"]\n    mod n;\n}\n"
        ),
        Vec::<String>::new()
    );
}

/// An inner expect is valid at the top of a file even when nothing there matches: an
/// unfulfilled exemption, not a malformed one.
#[test]
fn stray_inner_exemption_at_the_file_top_is_unfulfilled() {
    assert_eq!(
        placed("#![cfg_attr(samlinter, expect(path_attr, reason = \"why\"))]\nfn f() {}\n"),
        ["1:1 unfulfilled_exemption"]
    );
}

/// A comment after the item on its last line is outside the item's node.
#[test]
fn exemption_misses_a_trailing_comment_after_its_node() {
    assert_eq!(
        findings(&exempting(
            "forbidden_word",
            "ATTR\nfn f() {} // see NOTES.md\n"
        )),
        [
            "src/lib.rs:2:18: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
            "src/lib.rs:1:1: unfulfilled_exemption: exemption for `forbidden_word` matches nothing here",
        ]
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
