use super::{ExcludeEntry, Settings, Word, words};
use rstest::rstest;

#[test]
fn parses_every_required_key() {
    let config = Settings::parse(
        "forbidden-words = [\"NOTES.md\", \"it is worth noting\"]\nsubscriber-crates = [\"tracing-subscriber\"]\n",
    )
    .unwrap()
    .config;
    assert_eq!(
        config.forbidden_words,
        words(&["NOTES.md", "it is worth noting"])
    );
    assert_eq!(config.subscriber_crates, words(&["tracing-subscriber"]));
}

#[test]
fn accepts_empty_lists() {
    let settings = Settings::parse("forbidden-words = []\nsubscriber-crates = []\n").unwrap();
    assert_eq!(settings.config.forbidden_words, Vec::new());
    assert_eq!(settings.config.subscriber_crates, Vec::new());
    assert_eq!(settings.exclude, Vec::new());
}

#[test]
fn parses_exclude() {
    let settings = Settings::parse(
        "forbidden-words = []\nsubscriber-crates = []\nexclude = [\"src/generated\", \"build.rs\"]\n",
    )
    .unwrap();
    let entries: Vec<&str> = settings.exclude.iter().map(ExcludeEntry::as_str).collect();
    assert_eq!(entries, ["src/generated", "build.rs"]);
}

/// An entry is a relative path that stays at or below the root, checked when the file parses.
#[rstest]
#[case::relative("src/generated", Ok("src/generated"))]
#[case::dot_dot_staying_below("src/../build.rs", Ok("src/../build.rs"))]
#[case::the_root(".", Ok("."))]
#[case::empty("", Err(BAD_ENTRY))]
#[case::padded(" src", Err(BAD_ENTRY))]
#[case::absolute(
    "/src",
    Err("`exclude` entry `/src` is absolute; write it relative to the workspace root")
)]
#[case::above_the_root("..", Err("`exclude` entry `..` climbs above the workspace root"))]
#[case::out_and_back(
    "src/../../ws/src",
    Err("`exclude` entry `src/../../ws/src` climbs above the workspace root")
)]
fn exclude_entries_stay_below_the_root(#[case] entry: &str, #[case] expected: Result<&str, &str>) {
    let parsed = ExcludeEntry::try_from(entry.to_owned());
    assert_eq!(
        parsed
            .as_ref()
            .map(ExcludeEntry::as_str)
            .map_err(String::as_str),
        expected
    );
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
    "TOML parse error at line 3, column 1\n  |\n3 | severity = \"warn\"\n  | ^^^^^^^^\nunknown field `severity`, expected one of `forbidden-words`, `subscriber-crates`, `exclude`\n"
)]
#[case::missing_subscriber_crates(
    "forbidden-words = []\n",
    "TOML parse error at line 1, column 1\n  |\n1 | forbidden-words = []\n  | ^\nmissing field `subscriber-crates`\n"
)]
#[case::missing_forbidden_words(
    "subscriber-crates = []\n",
    "TOML parse error at line 1, column 1\n  |\n1 | subscriber-crates = []\n  | ^\nmissing field `forbidden-words`\n"
)]
#[case::wrong_type(
    "forbidden-words = \"AI\"\nsubscriber-crates = []\n",
    "TOML parse error at line 1, column 19\n  |\n1 | forbidden-words = \"AI\"\n  |                   ^^^^\ninvalid type: string \"AI\", expected a sequence\n"
)]
#[case::not_toml(
    "forbidden-words = [\n",
    "TOML parse error at line 1, column 20\n  |\n1 | forbidden-words = [\n  |                    ^\nunclosed array, expected `]`\n"
)]
fn rejects_invalid_config(#[case] text: &str, #[case] expected: &str) {
    assert_eq!(Settings::parse(text).unwrap_err().to_string(), expected);
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
#[case::empty_exclude(
    "forbidden-words = []\nsubscriber-crates = []\nexclude = [\"\"]\n",
    "TOML parse error at line 3, column 11\n  |\n3 | exclude = [\"\"]\n  |           ^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
#[case::padded_subscriber(
    "forbidden-words = []\nsubscriber-crates = [\"tracing-subscriber \"]\n",
    "TOML parse error at line 2, column 21\n  |\n2 | subscriber-crates = [\"tracing-subscriber \"]\n  |                     ^^^^^^^^^^^^^^^^^^^^^^^\nentries must be non-empty, without surrounding whitespace\n"
)]
fn rejects_empty_or_padded_entries(#[case] text: &str, #[case] expected: &str) {
    assert_eq!(Settings::parse(text).unwrap_err().to_string(), expected);
}
