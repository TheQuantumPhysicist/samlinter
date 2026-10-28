use anyhow::Result;
use serde::Deserialize;

/// Workspace facts samlinter cannot derive; every key is required.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// - Words or phrases banned in comments, Rust and TOML, and README files.
    /// - Matched case-sensitively as whole words.
    /// - The words of a phrase match across any whitespace run, so a phrase wrapped across
    ///   lines or consecutive line comments still matches.
    pub forbidden_words: Vec<Word>,
    /// - Crates that install a log subscriber.
    /// - A package without a binary target may list them only as dev or build dependencies.
    /// - `-` and `_` in a name match each other, as on crates.io.
    pub subscriber_crates: Vec<Word>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml_edit::de::from_str(text)?)
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

/// `entries` as words; each must be valid.
#[cfg(test)]
pub fn words(entries: &[&str]) -> Vec<Word> {
    entries
        .iter()
        .map(|entry| Word::try_from((*entry).to_owned()).unwrap())
        .collect()
}

#[cfg(test)]
mod tests;
