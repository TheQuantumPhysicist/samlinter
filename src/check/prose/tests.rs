use super::{Piece, check, forbidden};
use crate::config::forbidden_words;
use rstest::rstest;

/// Hits as `offset rule`.
fn render(text: &str) -> Vec<String> {
    let listed = forbidden_words(&["NOTES.md", "AI", "it is worth noting", "x-x"], &[]);
    check(text, &listed)
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
    let offsets: Vec<usize> = forbidden(
        &pieces,
        &forbidden_words(&["AI", "it is worth noting"], &[]),
    )
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

/// Hits as `offset rule`, with the words of each case class listed.
fn hits(exact: &[&str], insensitive: &[&str], text: &str) -> Vec<String> {
    forbidden(
        &[Piece { offset: 0, text }],
        &forbidden_words(exact, insensitive),
    )
    .iter()
    .map(|hit| format!("{} {}", hit.offset, hit.rule.name()))
    .collect()
}

/// - An exact-class word (listed under `forbidden-words`) fires only on its exact case, as
///   a whole word.
#[rstest]
#[case::the_file_name_fires_as_written(&["RULES.md"], &[], "see RULES.md", &["4 forbidden_word"])]
#[case::the_lowercase_file_name_stays_silent(&["RULES.md"], &[], "see rules.md", &[])]
#[case::a_proper_noun_fires_as_written(&["Claude"], &[], "ask Claude", &["4 forbidden_word"])]
#[case::the_lowercase_proper_noun_stays_silent(&["Claude"], &[], "ask claude", &[])]
#[case::the_listed_plural_fires(&["LLM", "LLMs"], &[], "three LLMs", &["6 forbidden_word"])]
#[case::a_plural_of_one_entry_stays_silent(&["LLM"], &[], "three LLMs", &[])]
#[case::the_possessive_fires(&["Claude"], &[], "Claude's API", &["0 forbidden_word"])]
fn exact_class_matches_only_its_exact_case_as_whole_words(
    #[case] exact: &[&str],
    #[case] insensitive: &[&str],
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(hits(exact, insensitive, text), expected);
}

/// - An insensitive-class word (listed under `forbidden-words-insensitive`) fires on any
///   case, a sentence start included: the listed spelling is banned whatever case it wears.
/// - Folding never widens the match: a longer word that merely contains one stays silent,
///   and the apostrophe of a possessive still bounds the word.
#[rstest]
#[case::the_listed_spelling_fires(&[], &["behaviour"], "the behaviour of", &["4 forbidden_word"])]
#[case::a_sentence_start_fires(&[], &["behaviour"], "Behaviour differs", &["0 forbidden_word"])]
#[case::any_casing_fires(&[], &["behaviour"], "the BEHAVIOUR of", &["4 forbidden_word"])]
#[case::the_possessive_fires_under_folding(&[], &["behaviour"], "the behaviour's", &["4 forbidden_word"])]
#[case::the_american_spelling_stays_silent(&[], &["behaviour", "behavioural"], "its behavior", &[])]
#[case::the_folded_whole_word_negative_stays_silent(&[], &["grey"], "the Greyhound", &[])]
#[case::the_lowercase_word_inside_a_longer_one_stays_silent(&[], &["grey"], "the greyhound", &[])]
#[case::a_derived_form_is_a_different_word(&[], &["centre"], "centred on", &[])]
fn insensitive_class_matches_any_case_as_whole_words(
    #[case] exact: &[&str],
    #[case] insensitive: &[&str],
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(hits(exact, insensitive, text), expected);
}

/// - A hit's message names the `samlinter.toml` key of the word's own case class, so the
///   reader hunts for the word in the list that banned it.
#[rstest]
#[case::the_exact_class_names_forbidden_words(&["RULES.md"], &[], "see RULES.md", "`RULES.md` is listed in forbidden-words in samlinter.toml")]
#[case::the_insensitive_class_names_forbidden_words_insensitive(&[], &["behaviour"], "the behaviour of", "`behaviour` is listed in forbidden-words-insensitive in samlinter.toml")]
fn messages_name_the_key_of_the_words_own_class(
    #[case] exact: &[&str],
    #[case] insensitive: &[&str],
    #[case] text: &str,
    #[case] expected: &str,
) {
    let messages: Vec<String> = forbidden(
        &[Piece { offset: 0, text }],
        &forbidden_words(exact, insensitive),
    )
    .iter()
    .map(|hit| hit.message.clone())
    .collect();
    assert_eq!(messages, [expected.to_owned()]);
}
