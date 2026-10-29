use super::super::{render, text_file};
use super::check;
use crate::config::words;
use rstest::rstest;

/// Each finding in `text` as `line:column rule`, with `NOTES.md` and a phrase forbidden.
fn comments(text: &str) -> Vec<String> {
    render(&check(
        &text_file("Cargo.toml", text),
        &words(&["NOTES.md", "it is worth noting"]),
    ))
}

#[rstest]
#[case::comment_line("# a \u{2014} b\nx = 1\n", &["1:5 em_dash"])]
#[case::trailing_comment("x = 1 # see NOTES.md\n", &["1:13 forbidden_word"])]
#[case::emoji("# done \u{1F389}\n", &["1:8 emoji"])]
#[case::comment_inside_a_table("[a]\nx = 1\n# \u{2014}\n", &["3:3 em_dash"])]
#[case::comment_inside_an_array("x = [\n  1, # \u{2014}\n  2,\n]\n", &["2:8 em_dash"])]
#[case::basic_string_is_not_a_comment("x = \"# \u{2014} NOTES.md\"\n", &[])]
#[case::literal_string_is_not_a_comment("x = '# \u{2014}'\n", &[])]
#[case::multi_line_basic_string_is_not_a_comment("x = \"\"\"\n# \u{2014}\n\"\"\"\n", &[])]
#[case::multi_line_literal_string_is_not_a_comment("x = '''\n# \u{2014}\n'''\n", &[])]
#[case::quoted_key_is_not_a_comment("\"# \u{2014}\" = 1\n", &[])]
#[case::string_then_comment("x = \"#\" # \u{2014}\n", &["1:11 em_dash"])]
#[case::crlf_line_endings("x = 1\r\n# \u{2014}\r\n", &["2:3 em_dash"])]
#[case::leading_bom("\u{feff}# \u{2014}\n", &["1:3 em_dash"])]
#[case::no_comments("x = 1\n", &[])]
#[case::phrase_wrapped_across_comment_lines(
    "# it is worth\n# noting\nx = 1\n",
    &["1:3 forbidden_word"]
)]
#[case::phrase_wrapped_from_a_trailing_comment(
    "x = 1 # it is worth\n  # noting\n",
    &["1:9 forbidden_word"]
)]
#[case::phrase_wrapped_across_repeated_markers(
    "## it is worth\n## noting\n",
    &["1:4 forbidden_word"]
)]
#[case::phrase_wrapped_across_crlf_lines(
    "# it is worth\r\n# noting\r\n",
    &["1:3 forbidden_word"]
)]
#[case::key_between_comment_lines_ends_the_run("# it is worth\nx = 1\n# noting\n", &[])]
#[case::blank_line_between_comment_lines_ends_the_run("# it is worth\n\n# noting\n", &[])]
fn checks_comment_prose(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(comments(text), expected);
}
