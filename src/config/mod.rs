use anyhow::{Result, anyhow};
#[cfg_attr(
    samlinter,
    expect(
        regex_use,
        reason = "the linter itself needs the regex crate to match the user's configured `forbidden-patterns`"
    )
)]
use regex::Regex;
use serde::Deserialize;
use std::path::{Component, Path};

/// What the rules read from `samlinter.toml`.
#[derive(Debug)]
pub struct Config {
    /// - Words or phrases banned in comments, Rust and TOML, and README files, each with
    ///   the case class of the key it was listed under.
    /// - The words of a phrase match across any whitespace run, so a phrase wrapped across
    ///   lines or consecutive line comments still matches.
    pub forbidden_words: Vec<ForbiddenWord>,
    /// - Regexes banned in comments, Rust and TOML, and README files, compiled at parse so
    ///   a pattern that cannot match a span is a config error, never a silent no-op.
    /// - A regex matches wherever it matches; word boundaries are the pattern's own job
    ///   (`\b`), because a pattern exists exactly when the word boundary rule cannot
    ///   express the shape, such as `CORE-\d+`.
    pub forbidden_patterns: Vec<ForbiddenPattern>,
    /// - Crates that install a log subscriber.
    /// - A package without a binary target that lists one as a regular dependency gets its
    ///   library code checked: each install call there (`.init()`, `set_global_default(..)`,
    ///   ...) is a finding. The dependency alone is not.
    /// - `-` and `_` in a name match each other, as on crates.io.
    pub subscriber_crates: Vec<Word>,
}

/// `samlinter.toml` parsed: what the rules read, and the paths no rule applies to.
#[derive(Debug)]
pub struct Settings {
    pub config: Config,
    /// Checked against the disk by the workspace loader, which keeps only the checked form.
    pub exclude: Vec<ExcludeEntry>,
}

/// `samlinter.toml` as written: workspace facts samlinter cannot derive; every key but
/// `exclude` and `forbidden-patterns` is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct File {
    forbidden_words: Vec<Word>,
    forbidden_words_insensitive: Vec<Word>,
    #[serde(default)]
    forbidden_patterns: Vec<Word>,
    subscriber_crates: Vec<Word>,
    #[serde(default)]
    exclude: Vec<ExcludeEntry>,
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self> {
        let File {
            forbidden_words,
            forbidden_words_insensitive,
            forbidden_patterns,
            subscriber_crates,
            exclude,
        } = toml_edit::de::from_str(text)?;
        let forbidden_patterns = forbidden_patterns
            .into_iter()
            .map(ForbiddenPattern::compile)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            config: Config {
                forbidden_words: forbidden_words
                    .into_iter()
                    .map(|word| listed_word(word, Case::Exact))
                    .chain(
                        forbidden_words_insensitive
                            .into_iter()
                            .map(|word| listed_word(word, Case::Insensitive)),
                    )
                    .collect(),
                forbidden_patterns,
                subscriber_crates,
            },
            exclude,
        })
    }
}

const fn listed_word(word: Word, case: Case) -> ForbiddenWord {
    ForbiddenWord { word, case }
}

/// A forbidden regex with the text it was listed as, compiled once at parse.
#[derive(Debug)]
pub struct ForbiddenPattern {
    pattern: Word,
    regex: Regex,
}
/// The listed text decides identity: the same text compiles to the same regex.
impl PartialEq for ForbiddenPattern {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl ForbiddenPattern {
    /// The key that lists patterns in samlinter.toml.
    pub const KEY: &str = "forbidden-patterns";

    #[cfg_attr(
        samlinter,
        expect(
            regex_use,
            reason = "the linter itself needs the regex crate to match the user's configured `forbidden-patterns`"
        )
    )]
    fn compile(pattern: Word) -> Result<Self> {
        let regex = Regex::new(pattern.as_str()).map_err(|error| {
            anyhow!(
                "`{key}` entry `{entry}` is not a valid regex: {error}",
                key = Self::KEY,
                entry = pattern.as_str(),
            )
        })?;
        if regex.is_match("") {
            return Err(anyhow!(
                "`{key}` entry `{entry}` matches the empty string",
                key = Self::KEY,
                entry = pattern.as_str(),
            ));
        }
        Ok(Self { pattern, regex })
    }

    pub fn as_str(&self) -> &str {
        self.pattern.as_str()
    }

    pub const fn regex(&self) -> &Regex {
        &self.regex
    }
}

/// - A forbidden word with the case class of the key it was listed under.
/// - `forbidden-words` holds proper nouns and file names, `forbidden-words-insensitive`
///   spellings that must stay American.
#[derive(Debug, PartialEq, Eq)]
pub struct ForbiddenWord {
    word: Word,
    case: Case,
}

impl ForbiddenWord {
    pub fn as_str(&self) -> &str {
        self.word.as_str()
    }

    pub const fn case(&self) -> Case {
        self.case
    }
}

/// The case class of a forbidden word: how its text compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// Byte-exact, so an entry fires only on text of its own case and a capitalized
    /// sentence start escapes a lowercase entry.
    Exact,
    /// Case-folded, so an entry fires on any case, a sentence start included.
    Insensitive,
}

impl Case {
    /// The `samlinter.toml` key that lists words of this class.
    pub const fn key(self) -> &'static str {
        match self {
            Case::Exact => "forbidden-words",
            Case::Insensitive => "forbidden-words-insensitive",
        }
    }
}

/// - A list entry: non-empty, without surrounding whitespace.
/// - An empty forbidden word would match everywhere; a padded crate name would never match.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Word(String);

impl Word {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Word {
    type Error = &'static str;

    fn try_from(entry: String) -> Result<Self, Self::Error> {
        if entry.is_empty() || entry.trim() != entry {
            Err("entries must be non-empty, without surrounding whitespace")
        } else {
            Ok(Self(entry))
        }
    }
}

/// - An `exclude` entry: a `Word` that is a relative path, none of whose `..` climbs above the
///   workspace root it is relative to.
/// - Checked when the file parses, so the error quotes the key's line; the disk is checked
///   later, by the workspace loader.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ExcludeEntry(Word);

impl ExcludeEntry {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for ExcludeEntry {
    type Error = String;

    fn try_from(entry: String) -> Result<Self, Self::Error> {
        let word = Word::try_from(entry).map_err(str::to_owned)?;
        let text = word.as_str();
        Path::new(text)
            .components()
            .try_fold(0_usize, |depth, component| match component {
                Component::Prefix(_) | Component::RootDir => Err(format!(
                    "`exclude` entry `{text}` is absolute; write it relative to the workspace root"
                )),
                Component::CurDir => Ok(depth),
                Component::ParentDir => depth.checked_sub(1).ok_or_else(|| {
                    format!("`exclude` entry `{text}` climbs above the workspace root")
                }),
                Component::Normal(_) => Ok(depth.saturating_add(1)),
            })?;
        Ok(Self(word))
    }
}

/// `entries` as words; each must be valid.
#[cfg(test)]
pub fn words(entries: &[&str]) -> Vec<Word> {
    entries
        .iter()
        .map(|entry| Word::try_from((*entry).to_owned()).unwrap())
        .collect()
}

/// `exact` and `insensitive` as forbidden words of their case class; each must be valid.
#[cfg(test)]
pub fn forbidden_words(exact: &[&str], insensitive: &[&str]) -> Vec<ForbiddenWord> {
    let mut listed: Vec<ForbiddenWord> = exact
        .iter()
        .map(|entry| test_word(entry, Case::Exact))
        .collect();
    listed.extend(
        insensitive
            .iter()
            .map(|entry| test_word(entry, Case::Insensitive)),
    );
    listed
}

#[cfg(test)]
fn test_word(entry: &str, case: Case) -> ForbiddenWord {
    ForbiddenWord {
        word: Word::try_from(entry.to_owned()).unwrap(),
        case,
    }
}

/// `entries` as compiled forbidden patterns; each must be valid.
#[cfg(test)]
pub fn forbidden_patterns(entries: &[&str]) -> Vec<ForbiddenPattern> {
    entries
        .iter()
        .map(|entry| {
            ForbiddenPattern::compile(Word::try_from((*entry).to_owned()).unwrap()).unwrap()
        })
        .collect()
}

#[cfg(test)]
mod tests;
