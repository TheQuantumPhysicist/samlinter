mod exemption;

use super::toml_comments;
use crate::config::Config;
use crate::finding::{Finding, ManifestExemptable, Position, Rule, Span};
use crate::workspace::{Dependency, Package, PackageKind, Readme, Section, TextFile};
use anyhow::{Result, anyhow};
use cargo_metadata::cargo_platform::Platform;
use std::path::Path;
use toml_edit::{Document, Item, Key, Table, TableLike, Value};

/// What a manifest is to the workspace, which decides the rules it gets.
pub enum Role<'a> {
    VirtualRoot(&'a TextFile),
    RootPackage(&'a Package),
    Member(&'a Package),
}

impl Role<'_> {
    const fn manifest(&self) -> &TextFile {
        match self {
            Self::VirtualRoot(manifest) => manifest,
            Self::RootPackage(package) | Self::Member(package) => &package.manifest,
        }
    }
}

/// Why samlinter cannot place a finding in the manifest text.
#[derive(Clone, Copy)]
enum Reason {
    /// A `cargo metadata` dependency with no entry in the manifest text it came from; cargo
    /// reads the same text.
    MissingEntry,
    /// A key with no position in the text; `toml_edit` gives every parsed key one.
    KeyWithoutSpan,
}

/// - A finding samlinter cannot place in the manifest text, `manifest` as output shows it.
/// - Always a samlinter bug: the run stops rather than drop or misplace the finding.
fn unplaced(manifest: &Path, key: &str, reason: Reason) -> anyhow::Error {
    let manifest = manifest.display();
    match reason {
        Reason::MissingEntry => anyhow!(
            "{manifest}: `cargo metadata` lists dependency `{key}`, but samlinter finds no entry for it in this file; please report this samlinter bug"
        ),
        Reason::KeyWithoutSpan => anyhow!(
            "{manifest}: the parsed key `{key}` has no position in this file; please report this samlinter bug"
        ),
    }
}

#[derive(Clone, Copy)]
enum Location<'a> {
    /// A rule about the whole file, or a syntax error the parser gives no position.
    File,
    /// A violation at a key, positioned once the findings are built.
    Key(&'a Key),
    /// A syntax error at a byte offset.
    Offset(usize),
}

/// Rules a manifest violation can break that no exemption lifts.
#[derive(Clone, Copy, strum::EnumIter)]
enum FixedRule {
    BinaryReadme,
    FeatureFlags,
    LibrarySubscriber,
    MalformedExemption,
    PanicStrategy,
    SyntaxError,
    UnfulfilledExemption,
    WorkspaceDependencyForm,
}

impl From<FixedRule> for Rule {
    fn from(rule: FixedRule) -> Self {
        match rule {
            FixedRule::BinaryReadme => Self::BinaryReadme,
            FixedRule::FeatureFlags => Self::FeatureFlags,
            FixedRule::LibrarySubscriber => Self::LibrarySubscriber,
            FixedRule::MalformedExemption => Self::MalformedExemption,
            FixedRule::PanicStrategy => Self::PanicStrategy,
            FixedRule::SyntaxError => Self::SyntaxError,
            FixedRule::UnfulfilledExemption => Self::UnfulfilledExemption,
            FixedRule::WorkspaceDependencyForm => Self::WorkspaceDependencyForm,
        }
    }
}

/// - A rule whose violations an exemption may lift needs the data the exemption names.
/// - So such a rule has no `Fixed` form.
#[derive(Clone, Copy)]
enum Kind<'a> {
    Fixed {
        rule: FixedRule,
        location: Location<'a>,
    },
    /// Exemptions name the dependency, so the violation sits at its key.
    Exemptable {
        rule: ManifestExemptable,
        key: &'a Key,
    },
}

impl<'a> Kind<'a> {
    fn rule(self) -> Rule {
        match self {
            Self::Fixed { rule, location: _ } => Rule::from(rule),
            Self::Exemptable { rule, key: _ } => Rule::from(rule),
        }
    }

    const fn location(self) -> Location<'a> {
        match self {
            Self::Fixed { rule: _, location } => location,
            Self::Exemptable { rule: _, key } => Location::Key(key),
        }
    }
}

struct Violation<'a> {
    kind: Kind<'a>,
    message: String,
}

impl<'a> Violation<'a> {
    fn at_key(key: &'a Key, rule: FixedRule, message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Fixed {
                rule,
                location: Location::Key(key),
            },
            message: message.into(),
        }
    }

    fn file(rule: FixedRule, message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Fixed {
                rule,
                location: Location::File,
            },
            message: message.into(),
        }
    }

    fn into_finding(self, manifest: &TextFile) -> Result<Finding> {
        let at = |offset| Span::At(Position::of(&manifest.text, offset));
        let span = match self.kind.location() {
            Location::File => Span::File,
            Location::Key(key) => {
                let range = key
                    .span()
                    .ok_or_else(|| unplaced(&manifest.shown, key.get(), Reason::KeyWithoutSpan))?;
                at(range.start)
            }
            Location::Offset(offset) => at(offset),
        };
        Ok(Finding {
            path: manifest.shown.clone(),
            span,
            rule: self.kind.rule(),
            message: self.message,
        })
    }
}

/// An entry of a dependency table.
struct Entry<'a> {
    key: &'a Key,
    declaration: Declaration<'a>,
    section: Section,
    /// The `<platform>` of the `[target.<platform>]` table holding the entry, as written.
    target: Option<&'a str>,
}

impl Entry<'_> {
    fn name(&self) -> &str {
        self.key.get()
    }

    /// Compares parsed platforms, so spacing inside `cfg(...)` does not matter.
    fn declares(&self, dependency: &Dependency) -> bool {
        let same_target = match (self.target, &dependency.target) {
            (None, None) => true,
            (Some(written), Some(platform)) => written
                .parse::<Platform>()
                .is_ok_and(|parsed| parsed == *platform),
            (None, Some(_)) | (Some(_), None) => false,
        };
        same_target && self.section == dependency.section && self.name() == dependency.key
    }
}

/// How a dependency entry is written.
#[derive(Clone, Copy)]
enum Declaration<'a> {
    /// `name = "<version requirement>"`
    Version,
    /// `name = { ... }`
    Inline(&'a dyn TableLike),
    /// `[dependencies.name]`, or dotted keys such as `name.workspace = true`.
    Table(&'a dyn TableLike),
    /// Any other value; cargo rejects it, so no rule looks inside.
    Other,
}

impl<'a> Declaration<'a> {
    const fn of(item: &'a Item) -> Self {
        match item {
            Item::Value(Value::String(_)) => Self::Version,
            Item::Value(Value::InlineTable(fields)) => Self::Inline(fields),
            Item::Table(fields) => Self::Table(fields),
            Item::None
            | Item::ArrayOfTables(_)
            | Item::Value(
                Value::Integer(_)
                | Value::Float(_)
                | Value::Boolean(_)
                | Value::Datetime(_)
                | Value::Array(_),
            ) => Self::Other,
        }
    }

    /// The keys of an entry written as a table.
    const fn fields(self) -> Option<&'a dyn TableLike> {
        match self {
            Self::Inline(fields) | Self::Table(fields) => Some(fields),
            Self::Version | Self::Other => None,
        }
    }
}

/// A manifest with a syntax error is reported, never linted half-parsed, comments included.
pub fn check(role: &Role, config: &Config) -> Result<Vec<Finding>> {
    let manifest = role.manifest();
    match &Document::parse(manifest.text.as_str()) {
        Ok(document) => Ok([
            placed(lint(document.as_table(), role, config)?, manifest)?,
            toml_comments::check(manifest, &config.forbidden_words),
        ]
        .concat()),
        Err(error) => placed(
            vec![Violation {
                kind: Kind::Fixed {
                    rule: FixedRule::SyntaxError,
                    location: error
                        .span()
                        .map_or(Location::File, |span| Location::Offset(span.start)),
                },
                message: error.message().to_owned(),
            }],
            manifest,
        ),
    }
}

fn placed(violations: Vec<Violation>, manifest: &TextFile) -> Result<Vec<Finding>> {
    violations
        .into_iter()
        .map(|violation| violation.into_finding(manifest))
        .collect()
}

fn lint<'a>(root: &'a Table, role: &Role, config: &Config) -> Result<Vec<Violation<'a>>> {
    let by_role: Vec<Violation> = match role {
        Role::VirtualRoot(_) => panic_strategy(root),
        Role::RootPackage(package) => [panic_strategy(root), package_rules(root, package, config)?]
            .into_iter()
            .flatten()
            .collect(),
        Role::Member(package) => [member_versions(root), package_rules(root, package, config)?]
            .into_iter()
            .flatten()
            .collect(),
    };
    let violations = feature_flags(root)
        .into_iter()
        .chain(dependency_form(root))
        .chain(by_role)
        .collect();

    Ok(exemption::apply(root, violations))
}

fn feature_flags(root: &Table) -> Vec<Violation<'_>> {
    let table = root
        .key("features")
        .map(|key| Violation::at_key(key, FixedRule::FeatureFlags, "`[features]` table"));
    table
        .into_iter()
        .chain(optional_dependencies(root))
        .collect()
}

/// `optional = true` makes cargo create a feature named after the dependency.
fn optional_dependencies(root: &Table) -> Vec<Violation<'_>> {
    dependencies(root)
        .into_iter()
        .filter_map(|entry| {
            let (key, value) = entry.declaration.fields()?.get_key_value("optional")?;
            (value.as_bool() == Some(true)).then(|| {
                Violation::at_key(
                    key,
                    FixedRule::FeatureFlags,
                    format!("optional dependency `{}` is a feature flag", entry.name()),
                )
            })
        })
        .collect()
}

fn dependency_form(root: &Table) -> Vec<Violation<'_>> {
    dependencies(root)
        .into_iter()
        .filter(|entry| match entry.declaration {
            Declaration::Table(fields) => fields.contains_key("workspace"),
            Declaration::Version | Declaration::Inline(_) | Declaration::Other => false,
        })
        .map(|entry| {
            Violation::at_key(
                entry.key,
                FixedRule::WorkspaceDependencyForm,
                format!(
                    "write `{} = {{ workspace = true, ... }}` as one inline table",
                    entry.name()
                ),
            )
        })
        .collect()
}

fn member_versions(root: &Table) -> Vec<Violation<'_>> {
    dependencies(root)
        .into_iter()
        .filter(|entry| match entry.declaration {
            Declaration::Version => true,
            Declaration::Inline(fields) | Declaration::Table(fields) => {
                fields.contains_key("version")
            }
            Declaration::Other => false,
        })
        .map(|entry| Violation {
            kind: Kind::Exemptable {
                rule: ManifestExemptable::MemberVersion,
                key: entry.key,
            },
            message: format!(
                "`{0}` declares its version here; move the version to the root `[workspace.dependencies]` and write `{0} = {{ workspace = true }}`",
                entry.name()
            ),
        })
        .collect()
}

fn panic_strategy(root: &Table) -> Vec<Violation<'_>> {
    let profiles = root.get("profile").and_then(Item::as_table_like);
    ["dev", "release"]
        .into_iter()
        .filter_map(
            |name| match profiles.and_then(|profiles| profiles.get_key_value(name)) {
                Some((key, profile)) => {
                    let panic = profile
                        .as_table_like()
                        .and_then(|table| table.get("panic"))
                        .and_then(Item::as_str);
                    (panic != Some("abort")).then(|| {
                        Violation::at_key(
                            key,
                            FixedRule::PanicStrategy,
                            format!("`[profile.{name}]` does not set `panic = \"abort\"`"),
                        )
                    })
                }
                None => Some(Violation::file(
                    FixedRule::PanicStrategy,
                    format!("no `[profile.{name}]` with `panic = \"abort\"`"),
                )),
            },
        )
        .collect()
}

fn package_rules<'a>(
    root: &'a Table,
    package: &Package,
    config: &Config,
) -> Result<Vec<Violation<'a>>> {
    match package.kind {
        PackageKind::Binary => Ok(binary_readme(package)),
        PackageKind::Library => library_subscribers(root, package, config),
    }
}

fn binary_readme<'a>(package: &Package) -> Vec<Violation<'a>> {
    match &package.readme {
        Readme::Found(_) => Vec::new(),
        Readme::Missing { expected } => vec![Violation::file(
            FixedRule::BinaryReadme,
            format!("binary package has no README at {}", expected.display()),
        )],
    }
}

/// - Driven by the `cargo metadata` dependencies, which carry each real crate name after every
///   rename; the manifest text only places each finding.
/// - Dev and build dependencies may use a subscriber; they never reach the library's users.
fn library_subscribers<'a>(
    root: &'a Table,
    package: &Package,
    config: &Config,
) -> Result<Vec<Violation<'a>>> {
    let entries = dependencies(root);
    package
        .dependencies
        .iter()
        .filter(|dependency| match dependency.section {
            Section::Normal => true,
            Section::Development | Section::Build => false,
        })
        .filter(|dependency| {
            config
                .subscriber_crates
                .iter()
                .any(|subscriber| same_crate_name(subscriber.as_str(), &dependency.crate_name))
        })
        .map(|dependency| {
            let entry = entries
                .iter()
                .find(|entry| entry.declares(dependency))
                .ok_or_else(|| {
                    unplaced(
                        &package.manifest.shown,
                        &dependency.key,
                        Reason::MissingEntry,
                    )
                })?;
            Ok(Violation::at_key(
                entry.key,
                FixedRule::LibrarySubscriber,
                format!("library package depends on `{}`", dependency.crate_name),
            ))
        })
        .collect()
}

/// crates.io treats `-` and `_` in a crate name as one character, so both name one crate.
fn same_crate_name(first: &str, second: &str) -> bool {
    let unified = |character| if character == '_' { '-' } else { character };
    first.chars().map(unified).eq(second.chars().map(unified))
}

/// Entries of the dependency tables at the top level and under every `[target.<platform>]`.
fn dependencies(root: &Table) -> Vec<Entry<'_>> {
    let top_level: (Option<&str>, &dyn TableLike) = (None, root);
    let targets = root.get("target").and_then(Item::as_table_like);
    let target_scopes = targets.into_iter().flat_map(|targets| {
        targets
            .iter()
            .filter_map(|(platform, item)| Some((Some(platform), item.as_table_like()?)))
    });
    std::iter::once(top_level)
        .chain(target_scopes)
        .flat_map(|(target, scope)| {
            sections(scope)
                .into_iter()
                .flat_map(move |(table, section)| {
                    entries(table).into_iter().map(move |(key, item)| Entry {
                        key,
                        declaration: Declaration::of(item),
                        section,
                        target,
                    })
                })
        })
        .collect()
}

/// The dependency tables of one scope; its other keys are not dependency tables.
fn sections(scope: &dyn TableLike) -> Vec<(&dyn TableLike, Section)> {
    scope
        .iter()
        .filter_map(|(name, item)| Some((item.as_table_like()?, name.parse().ok()?)))
        .collect()
}

fn entries(table: &dyn TableLike) -> Vec<(&Key, &Item)> {
    table
        .iter()
        .map(|(name, item)| {
            // - Invariant: `iter` yields only names the table holds, and each has its key.
            // - A missing key is a toml_edit bug, not bad input, so there is nothing to report.
            let key = table
                .key(name)
                .expect("toml_edit holds a key for every name its table yields");
            (key, item)
        })
        .collect()
}

#[cfg(test)]
mod tests;
