// - TODO: randomized variant: 0..8 random trimmed words per key round-trip through TOML and
//   `Config::parse`; one entry padded with whitespace is rejected.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{Config, Word, words};
use rstest::rstest;

#[test]
fn parses_every_required_key() {
    let config = Config::parse(
        "forbidden-words = [\"NOTES.md\", \"it is worth noting\"]\nsubscriber-crates = [\"tracing-subscriber\"]\n",
    )
    .unwrap();
    assert_eq!(
        config.forbidden_words,
        words(&["NOTES.md", "it is worth noting"])
    );
    assert_eq!(config.subscriber_crates, words(&["tracing-subscriber"]));
}

#[test]
fn accepts_empty_lists() {
    let config = Config::parse("forbidden-words = []\nsubscriber-crates = []\n").unwrap();
    assert_eq!(config.forbidden_words, Vec::new());
    assert_eq!(config.subscriber_crates, Vec::new());
}

#[rstest]
#[case::word("NOTES.md", Ok("NOTES.md"))]
#[case::phrase("it is worth noting", Ok("it is worth noting"))]
#[case::empty("", Err(BAD_ENTRY))]
#[case::only_whitespace(" ", Err(BAD_ENTRY))]
#[case::leading_whitespace(" AI", Err(BAD_ENTRY))]
#[case::trailing_newline("AI\n", Err(BAD_ENTRY))]
fn words_are_non_empty_and_trimmed(#[case] entry: &str, #[case] expected: Result<&str, &str>) {
    assert_eq!(
        Word::try_from(entry.to_owned())
            .as_ref()
            .map(Word::as_str)
            .map_err(|error| *error),
        expected
    );
}

const BAD_ENTRY: &str = "entries must be non-empty, without surrounding whitespace";

#[rstest]
#[case::unknown_key(
    "forbidden-words = []\nsubscriber-crates = []\nseverity = \"warn\"\n",
    "unknown field `severity`"
)]
#[case::missing_subscriber_crates("forbidden-words = []\n", "missing field `subscriber-crates`")]
#[case::missing_forbidden_words("subscriber-crates = []\n", "missing field `forbidden-words`")]
#[case::wrong_type(
    "forbidden-words = \"AI\"\nsubscriber-crates = []\n",
    "invalid type: string \"AI\", expected a sequence"
)]
#[case::not_toml("forbidden-words = [\n", "unclosed array")]
fn rejects_invalid_config(#[case] text: &str, #[case] reason: &str) {
    let error = Config::parse(text).unwrap_err().to_string();
    assert!(error.contains(reason), "`{reason}` missing from: {error}");
}

/// The error quotes the line of the key and states the problem.
#[rstest]
#[case::empty_word(
    "forbidden-words = [\"\"]\nsubscriber-crates = []\n",
    "TOML parse error at line 1, column 19\n  |\n1 | forbidden-words = [\"\"]\n  |                   ^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
#[case::padded_word(
    "forbidden-words = [\" AI\"]\nsubscriber-crates = []\n",
    "TOML parse error at line 1, column 19\n  |\n1 | forbidden-words = [\" AI\"]\n  |                   ^^^^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
#[case::empty_subscriber(
    "forbidden-words = []\nsubscriber-crates = [\"\"]\n",
    "TOML parse error at line 2, column 21\n  |\n2 | subscriber-crates = [\"\"]\n  |                     ^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
#[case::padded_subscriber(
    "forbidden-words = []\nsubscriber-crates = [\"tracing-subscriber \"]\n",
    "TOML parse error at line 2, column 21\n  |\n2 | subscriber-crates = [\"tracing-subscriber \"]\n  |                     ^^^^^^^^^^^^^^^^^^^^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
fn rejects_empty_or_padded_entries(#[case] text: &str, #[case] expected: &str) {
    assert_eq!(Config::parse(text).unwrap_err().to_string(), expected);
}
