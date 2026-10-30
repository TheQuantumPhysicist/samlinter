use super::super::exemption::apply_exemptions;
use super::{FixedRule, Kind, Violation, entries};
use crate::finding::{Exemptability, ManifestExemptable, Rule};
use toml_edit::{Item, Key, Table};

/// An entry of `[package.metadata.samlinter.expect.<rule>]`: the name the violation sits at
/// (a dependency for `member_version`; the `[features]` table or the optional dependency's
/// name for `feature_flags`) -> reason.
struct Exemption<'a> {
    rule: ManifestExemptable,
    key: &'a Key,
}

impl<'a> Exemption<'a> {
    fn name(&self) -> &str {
        self.key.get()
    }

    fn covers(&self, violation: &Violation<'a>) -> bool {
        match violation.kind {
            Kind::Exemptable { rule, key } => rule == self.rule && key.get() == self.name(),
            Kind::Fixed {
                rule: _,
                location: _,
            } => false,
        }
    }

    fn unfulfilled(&self) -> Violation<'a> {
        Violation::at_key(
            self.key,
            FixedRule::UnfulfilledExemption,
            format!(
                "exemption for `{}` on `{}` matches nothing",
                Rule::from(self.rule).name(),
                self.name()
            ),
        )
    }
}

/// - Drops violations covered by a `[package.metadata.samlinter.expect.<rule>]` entry.
/// - Adds a violation for each malformed entry and for each entry that matched nothing.
pub fn apply<'a>(root: &'a Table, violations: Vec<Violation<'a>>) -> Vec<Violation<'a>> {
    apply_exemptions(
        violations,
        exemptions(root),
        Exemption::covers,
        // TOML keys are unique, so at most one entry covers a violation: no width to compare.
        |_| (),
        Exemption::unfulfilled,
    )
}

fn exemptions(root: &Table) -> Vec<Result<Exemption<'_>, Violation<'_>>> {
    root.get("package")
        .and_then(Item::as_table_like)
        .and_then(|package| package.get("metadata"))
        .and_then(Item::as_table_like)
        .and_then(|metadata| metadata.get_key_value("samlinter"))
        .map(|(key, item)| samlinter_section(key, item))
        .unwrap_or_default()
}

fn samlinter_section<'a>(
    key: &'a Key,
    item: &'a Item,
) -> Vec<Result<Exemption<'a>, Violation<'a>>> {
    match item.as_table_like() {
        Some(section) => entries(section)
            .into_iter()
            .flat_map(|(key, value)| {
                if key.get() == "expect" {
                    expect_section(key, value)
                } else {
                    vec![Err(malformed(
                        key,
                        format!("unknown key `{}`; only `expect` is allowed", key.get()),
                    ))]
                }
            })
            .collect(),
        None => vec![Err(malformed(
            key,
            "`[package.metadata.samlinter]` must be a table",
        ))],
    }
}

fn expect_section<'a>(key: &'a Key, item: &'a Item) -> Vec<Result<Exemption<'a>, Violation<'a>>> {
    match item.as_table_like() {
        Some(rules) => entries(rules)
            .into_iter()
            .flat_map(|(key, dependencies)| rule_exemptions(key, dependencies))
            .collect(),
        None => vec![Err(malformed(key, "`expect` must be a table of rules"))],
    }
}

fn rule_exemptions<'a>(key: &'a Key, item: &'a Item) -> Vec<Result<Exemption<'a>, Violation<'a>>> {
    match (manifest_rule(key.get()), item.as_table_like()) {
        (Err(message), Some(_) | None) => vec![Err(malformed(key, message))],
        (Ok(_), None) => vec![Err(malformed(
            key,
            "map each exempt dependency to the reason it is exempt",
        ))],
        (Ok(rule), Some(dependencies)) => entries(dependencies)
            .into_iter()
            .map(|(dependency, reason)| match reason.as_str() {
                Some(text) if !text.trim().is_empty() => Ok(Exemption {
                    rule,
                    key: dependency,
                }),
                Some(_) | None => Err(malformed(
                    dependency,
                    "the reason must be a non-empty string saying why",
                )),
            })
            .collect(),
    }
}

fn manifest_rule(name: &str) -> Result<ManifestExemptable, String> {
    match Exemptability::named(name)? {
        Exemptability::Manifest(rule) => Ok(rule),
        Exemptability::Source(_) => Err(format!(
            "rule `{name}` is exempted in Rust source, not in Cargo.toml"
        )),
        Exemptability::Never => Err(format!("rule `{name}` cannot be exempted")),
    }
}

fn malformed(key: &Key, message: impl Into<String>) -> Violation<'_> {
    Violation::at_key(key, FixedRule::MalformedExemption, message)
}

#[cfg(test)]
mod tests;
