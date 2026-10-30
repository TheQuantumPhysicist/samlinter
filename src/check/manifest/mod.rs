mod exemption;

use super::toml_comments;
use crate::config::Config;
use crate::finding::{Finding, ManifestExemptable, Position, Rule, Span};
use crate::workspace::{Package, PackageKind, Readme, Section, TextFile};
use anyhow::{Result, anyhow};
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

/// - A finding at a key with no position in the text, `manifest` as output shows it.
/// - `toml_edit` gives every parsed key one, so this is always a samlinter bug: the run stops
///   rather than drop or misplace the finding.
fn unplaced(manifest: &Path, key: &str) -> anyhow::Error {
    anyhow!(
        "{}: the parsed key `{key}` has no position in this file; please report this samlinter bug",
        manifest.display()
    )
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
    /// Exemptions name what the violation sits at (a dependency, or the `[features]`
    /// table), so the violation sits at that key.
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
                    .ok_or_else(|| unplaced(&manifest.shown, key.get()))?;
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
}

impl Entry<'_> {
    fn name(&self) -> &str {
        self.key.get()
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
            placed(lint(document.as_table(), role), manifest)?,
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

fn lint<'a>(root: &'a Table, role: &Role) -> Vec<Violation<'a>> {
    let by_role: Vec<Violation> = match role {
        Role::VirtualRoot(_) => panic_strategy(root),
        Role::RootPackage(package) => [panic_strategy(root), package_rules(package)]
            .into_iter()
            .flatten()
            .collect(),
        Role::Member(package) => [member_versions(root), package_rules(package)]
            .into_iter()
            .flatten()
            .collect(),
    };
    let violations = feature_flags(root)
        .into_iter()
        .chain(dependency_form(root))
        .chain(by_role)
        .collect();

    exemption::apply(root, violations)
}

fn feature_flags(root: &Table) -> Vec<Violation<'_>> {
    let table = root.key("features").map(|key| Violation {
        kind: Kind::Exemptable {
            rule: ManifestExemptable::FeatureFlags,
            key,
        },
        message: "`[features]` table".to_owned(),
    });
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
            let optional = entry.declaration.fields()?.get("optional")?;
            (optional.as_bool() == Some(true)).then(|| Violation {
                kind: Kind::Exemptable {
                    rule: ManifestExemptable::FeatureFlags,
                    key: entry.key,
                },
                message: format!("optional dependency `{}` is a feature flag", entry.name()),
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

/// A library's `library_subscriber` check reads its sources, not its manifest.
fn package_rules<'a>(package: &Package) -> Vec<Violation<'a>> {
    match package.kind {
        PackageKind::Binary => binary_readme(package),
        PackageKind::Library => Vec::new(),
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

/// Entries of the dependency tables at the top level and under every `[target.<platform>]`.
fn dependencies(root: &Table) -> Vec<Entry<'_>> {
    let top_level: &dyn TableLike = root;
    let targets = root.get("target").and_then(Item::as_table_like);
    let target_scopes = targets
        .into_iter()
        .flat_map(|targets| targets.iter().filter_map(|(_, item)| item.as_table_like()));
    std::iter::once(top_level)
        .chain(target_scopes)
        .flat_map(|scope| {
            sections(scope).into_iter().flat_map(|table| {
                entries(table).into_iter().map(|(key, item)| Entry {
                    key,
                    declaration: Declaration::of(item),
                })
            })
        })
        .collect()
}

/// The dependency tables of one scope; its other keys are not dependency tables.
fn sections(scope: &dyn TableLike) -> Vec<&dyn TableLike> {
    scope
        .iter()
        .filter(|(name, _)| name.parse::<Section>().is_ok())
        .filter_map(|(_, item)| item.as_table_like())
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
