use anyhow::Result;
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
/// `exclude` is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct File {
    forbidden_words: Vec<Word>,
    forbidden_words_insensitive: Vec<Word>,
    subscriber_crates: Vec<Word>,
    #[serde(default)]
    exclude: Vec<ExcludeEntry>,
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self> {
        let File {
            forbidden_words,
            forbidden_words_insensitive,
            subscriber_crates,
            exclude,
        } = toml_edit::de::from_str(text)?;
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
                subscriber_crates,
            },
            exclude,
        })
    }
}

const fn listed_word(word: Word, case: Case) -> ForbiddenWord {
    ForbiddenWord { word, case }
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

#[cfg(test)]
mod tests;
