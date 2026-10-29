use super::{Piece, check, forbidden};
use crate::config::words;
use rstest::rstest;

/// Hits as `offset rule`.
fn render(text: &str) -> Vec<String> {
    let words = words(&["NOTES.md", "AI", "it is worth noting", "x-x"]);
    check(text, &words)
        .iter()
        .map(|hit| format!("{} {}", hit.offset, hit.rule.name()))
        .collect()
}

#[rstest]
#[case::em_dash("a \u{2014} b", &["2 em_dash"])]
#[case::en_dash_is_allowed("a \u{2013} b", &[])]
#[case::double_hyphen_is_allowed("a -- b", &[])]
#[case::emoji("ok \u{1F600}", &["3 emoji"])]
#[case::skin_toned_emoji_is_one_finding("a \u{1F44D}\u{1F3FD} b", &["2 emoji"])]
#[case::em_dash_after_a_joiner("\u{1F600}\u{200d}\u{2014}", &["7 em_dash", "0 emoji"])]
#[case::forbidden_word("see NOTES.md first", &["4 forbidden_word"])]
#[case::forbidden_phrase("it is worth noting that", &["0 forbidden_word"])]
#[case::phrase_wrapped_across_lines("it is worth\nnoting", &["0 forbidden_word"])]
#[case::phrase_across_any_whitespace_run("it  is\t\n  worth noting", &["0 forbidden_word"])]
#[case::phrase_needs_whitespace_between_its_words("it isworth noting", &[])]
#[case::phrase_broken_by_other_text("it is worth - noting", &[])]
#[case::inside_other_words("MAIN AIM PAID", &[])]
#[case::case_sensitive("ai", &[])]
#[case::punctuation_bounds_words("(AI), AI.", &["1 forbidden_word", "6 forbidden_word"])]
#[case::underscore_joins_words("AI_LIMIT", &[])]
#[case::non_ascii_letters_join_words("\u{e9}AI AI\u{e9}", &[])]
#[case::overlapping_occurrences("ax-x-x", &["3 forbidden_word"])]
#[case::overlapping_occurrences_both_whole("x-x-x", &["0 forbidden_word", "2 forbidden_word"])]
#[case::empty_text("", &[])]
fn flags_dashes_emoji_and_forbidden_words(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(render(text), expected);
}

#[test]
fn forbidden_words_read_pieces_as_one_text_at_their_file_offsets() {
    let pieces = [
        Piece {
            offset: 10,
            text: " it is worth",
        },
        Piece {
            offset: 40,
            text: " noting AI",
        },
    ];
    let offsets: Vec<usize> = forbidden(&pieces, &words(&["AI", "it is worth noting"]))
        .iter()
        .map(|hit| hit.offset)
        .collect();
    assert_eq!(offsets, [48, 11]);
}

#[rstest]
#[case::code_point_below_u_1000_is_padded("1\u{fe0f}\u{20e3}", &["emoji U+0031"])]
#[case::code_point_above_u_ffff("\u{1F600}", &["emoji U+1F600"])]
#[case::em_dash("\u{2014}", &["em-dash (U+2014)"])]
fn messages_name_the_code_point(#[case] text: &str, #[case] expected: &[&str]) {
    let messages: Vec<String> = check(text, &[])
        .into_iter()
        .map(|hit| hit.message)
        .collect();
    assert_eq!(messages, expected);
}
