use anyhow::Result;
use serde::Deserialize;
use std::path::{Component, Path};

/// What the rules read from `samlinter.toml`.
#[derive(Debug)]
pub struct Config {
    /// - Words or phrases banned in comments, Rust and TOML, and README files.
    /// - Matched case-sensitively as whole words.
    /// - The words of a phrase match across any whitespace run, so a phrase wrapped across
    ///   lines or consecutive line comments still matches.
    pub forbidden_words: Vec<Word>,
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
    subscriber_crates: Vec<Word>,
    #[serde(default)]
    exclude: Vec<ExcludeEntry>,
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self> {
        let File {
            forbidden_words,
            subscriber_crates,
            exclude,
        } = toml_edit::de::from_str(text)?;
        Ok(Self {
            config: Config {
                forbidden_words,
                subscriber_crates,
            },
            exclude,
        })
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

#[cfg(test)]
mod tests;
