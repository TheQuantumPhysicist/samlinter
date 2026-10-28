use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// - TODO: rules still planned, each needing a stated reason at the site: discarded results
//   (`let _ =`), `try_from(..).unwrap_or(..)` without a clamp, lint allows, test skips, shared
//   mutable collections; and TODOs without a reason and a plan.
// - Postponed: two open decisions: whether an exemption attribute's `reason` counts as the
//   stated reason, and the TODO format to check.
// - Plan: once decided, add each as a `Rule` variant with its check, guidance and tests.
/// - Every check samlinter runs.
/// - The `snake_case` name is the id used in output and exemptions.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::EnumIter,
    strum::EnumString,
    strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum Rule {
    BinaryReadme,
    EmDash,
    Emoji,
    FeatureFlags,
    FnSpacing,
    ForbiddenWord,
    ItemOrder,
    ItemSpacing,
    LibrarySubscriber,
    MalformedExemption,
    MemberVersion,
    PanicStrategy,
    RestPattern,
    RestrictedVisibility,
    SyntaxError,
    UnfulfilledExemption,
    WorkspaceDependencyForm,
}

impl Rule {
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// - Only rules whose written form carries an exception clause accept exemptions.
    /// - Each is exempted in the one kind of file its findings come from.
    /// - Exemption bookkeeping rules never are, so an exemption cannot hide itself.
    pub const fn exemptability(self) -> Exemptability {
        match self {
            Self::RestPattern => Exemptability::Source(SourceExemptable::RestPattern),
            Self::MemberVersion => Exemptability::Manifest(ManifestExemptable::MemberVersion),
            Self::BinaryReadme
            | Self::EmDash
            | Self::Emoji
            | Self::FeatureFlags
            | Self::FnSpacing
            | Self::ForbiddenWord
            | Self::ItemOrder
            | Self::ItemSpacing
            | Self::LibrarySubscriber
            | Self::MalformedExemption
            | Self::PanicStrategy
            | Self::RestrictedVisibility
            | Self::SyntaxError
            | Self::UnfulfilledExemption
            | Self::WorkspaceDependencyForm => Exemptability::Never,
        }
    }
}

/// Where an exemption may lift a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exemptability {
    Never,
    /// By `#[cfg_attr(samlinter, expect(..))]` in Rust source.
    Source(SourceExemptable),
    /// By `[package.metadata.samlinter.expect.<rule>]` in Cargo.toml.
    Manifest(ManifestExemptable),
}

impl Exemptability {
    /// The exemptability of the rule an exemption names.
    pub fn named(name: &str) -> Result<Self, String> {
        Rule::from_str(name)
            .map(Rule::exemptability)
            .map_err(|_| format!("unknown rule `{name}`"))
    }
}

/// Rules an exemption in Rust source may lift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceExemptable {
    RestPattern,
}

impl From<SourceExemptable> for Rule {
    fn from(rule: SourceExemptable) -> Self {
        match rule {
            SourceExemptable::RestPattern => Self::RestPattern,
        }
    }
}

/// Rules an exemption in Cargo.toml may lift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestExemptable {
    MemberVersion,
}

impl From<ManifestExemptable> for Rule {
    fn from(rule: ManifestExemptable) -> Self {
        match rule {
            ManifestExemptable::MemberVersion => Self::MemberVersion,
        }
    }
}

/// The byte order mark, which some editors write at the start of a file.
pub const BOM: char = '\u{feff}';

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    /// 1-based.
    pub line: usize,
    /// 1-based, counting characters, not bytes.
    pub column: usize,
}

impl Position {
    /// A leading byte order mark is invisible, so it takes no column.
    pub fn of(text: &str, offset: usize) -> Self {
        let (text, offset) = match text.strip_prefix(BOM) {
            Some(rest) => (rest, offset.saturating_sub(BOM.len_utf8())),
            None => (text, offset),
        };

        let (line, column) = text
            .char_indices()
            .take_while(|(index, _)| *index < offset)
            .fold((1, 1), |(line, column), (_, character)| {
                if character == '\n' {
                    (line + 1, 1)
                } else {
                    (line, column + 1)
                }
            });
        Self { line, column }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Span {
    File,
    At(Position),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// The file's path as output shows it.
    pub path: PathBuf,
    pub span: Span,
    pub rule: Rule,
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        let rule = self.rule.name();
        match self.span {
            Span::File => write!(formatter, "{path}: {rule}: {}", self.message),
            Span::At(position) => write!(formatter, "{path}:{position}: {rule}: {}", self.message),
        }
    }
}

/// A finding located by byte offset, before it is tied to a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub offset: usize,
    pub rule: Rule,
    pub message: String,
}

impl Hit {
    pub fn new(offset: usize, rule: Rule, message: impl Into<String>) -> Self {
        Self {
            offset,
            rule,
            message: message.into(),
        }
    }

    pub fn into_finding(self, path: &Path, text: &str) -> Finding {
        Finding {
            path: path.to_path_buf(),
            span: Span::At(Position::of(text, self.offset)),
            rule: self.rule,
            message: self.message,
        }
    }
}

#[cfg(test)]
mod tests;
