use super::{Case, ExcludeEntry, Settings, Word, forbidden_words, words};
use rstest::rstest;
use std::fs;
use std::path::Path;

#[test]
fn parses_every_required_key() {
    let config = Settings::parse(
        "forbidden-words = [\"NOTES.md\", \"it is worth noting\"]\nforbidden-words-insensitive = [\"behaviour\"]\nsubscriber-crates = [\"tracing-subscriber\"]\n",
    )
    .unwrap()
    .config;
    assert_eq!(
        config.forbidden_words,
        forbidden_words(&["NOTES.md", "it is worth noting"], &["behaviour"])
    );
    assert_eq!(config.subscriber_crates, words(&["tracing-subscriber"]));
}

#[test]
fn accepts_empty_lists() {
    let settings = Settings::parse(
        "forbidden-words = []\nforbidden-words-insensitive = []\nsubscriber-crates = []\n",
    )
    .unwrap();
    assert_eq!(settings.config.forbidden_words, Vec::new());
    assert_eq!(settings.config.subscriber_crates, Vec::new());
    assert_eq!(settings.exclude, Vec::new());
}

#[test]
fn parses_exclude() {
    let settings = Settings::parse(
        "forbidden-words = []\nforbidden-words-insensitive = []\nsubscriber-crates = []\nexclude = [\"src/generated\", \"build.rs\"]\n",
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
    "forbidden-words = []\nforbidden-words-insensitive = []\nsubscriber-crates = []\nseverity = \"warn\"\n",
    "TOML parse error at line 4, column 1\n  |\n4 | severity = \"warn\"\n  | ^^^^^^^^\nunknown field `severity`, expected one of `forbidden-words`, `forbidden-words-insensitive`, `subscriber-crates`, `exclude`\n"
)]
#[case::missing_subscriber_crates(
    "forbidden-words = []\nforbidden-words-insensitive = []\n",
    "TOML parse error at line 1, column 1\n  |\n1 | forbidden-words = []\n  | ^\nmissing field `subscriber-crates`\n"
)]
#[case::missing_forbidden_words_insensitive(
    "forbidden-words = []\nsubscriber-crates = []\n",
    "TOML parse error at line 1, column 1\n  |\n1 | forbidden-words = []\n  | ^\nmissing field `forbidden-words-insensitive`\n"
)]
#[case::missing_forbidden_words(
    "forbidden-words-insensitive = []\nsubscriber-crates = []\n",
    "TOML parse error at line 1, column 1\n  |\n1 | forbidden-words-insensitive = []\n  | ^\nmissing field `forbidden-words`\n"
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

/// The word classes this workspace's code rules enforce, spelled exactly as they must match,
/// with the case class each is ruled to carry.
#[rstest]
#[case::document_names_a_reader_cannot_open(Case::Exact, &[
    "RULES.md", "CODEMAP.md", "AGENTS.md", "HANDOFF.md", "TRACKS.md", "INVARIANTS.md",
    "DESIGN.md", "CLAUDE.md",
])]
#[case::the_tools_used_to_write_the_code(Case::Exact, &[
    "Claude", "Anthropic", "OpenAI", "ChatGPT", "GPT", "Copilot", "AI", "LLM", "LLMs",
])]
#[case::spellings_that_must_stay_american(Case::Insensitive, &[
    "behaviour", "behavioural", "initialise", "initialised", "initialising",
    "colour", "colourful", "favour", "favourable", "labour", "honour",
    "analyse", "analysed", "analysing", "optimise", "optimised", "optimising",
    "licence", "defence", "offence", "centre", "metre", "grey", "catalogue",
])]
fn the_seed_list_carries_the_enforced_word_classes(#[case] case: Case, #[case] enforced: &[&str]) {
    let owned = repo_forbidden_words();
    for word in enforced {
        let classes: Vec<Case> = owned
            .iter()
            .filter(|(entry, _)| entry == word)
            .map(|(_, class)| *class)
            .collect();
        assert_eq!(
            classes,
            vec![case],
            "`{word}` is listed under the wrong case class"
        );
    }
}

/// The forbidden words parsed from this repo's own samlinter.toml, with their case classes.
fn repo_forbidden_words() -> Vec<(String, Case)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("samlinter.toml");
    let text = fs::read_to_string(path).unwrap();
    Settings::parse(&text)
        .unwrap()
        .config
        .forbidden_words
        .iter()
        .map(|word| (word.as_str().to_owned(), word.case()))
        .collect()
}

/// - The forms the seed list deliberately omits: a future addition must restart the
///   policy conversation, not slip in.
/// - Possessives need no entry (the matcher already reaches the listed word inside one)
///   and `cancelled` needs none (the doubled-l spelling is valid American too).
#[test]
fn the_seed_list_omits_the_ruled_out_forms() {
    let owned = repo_forbidden_words();
    for word in ["cancelled", "cancelling"] {
        assert!(
            !owned.iter().any(|(entry, _)| entry == word),
            "`{word}` entered the list"
        );
    }
    let possessives: Vec<&(String, Case)> = owned
        .iter()
        .filter(|(entry, _)| entry.ends_with("'s") || entry.ends_with("s'"))
        .collect();
    assert_eq!(possessives, Vec::<&(String, Case)>::new());
}
