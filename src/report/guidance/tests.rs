// - TODO: randomized variant: random lists of random single-line points render as
//   `  <label>:` then `    - <point>` lines, in order.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::Guidance;
use crate::finding::{Exemptability, Rule};
use strum::IntoEnumIterator;

const NONE: [&str; 0] = [];

fn points(guidance: &Guidance) -> impl Iterator<Item = &'static str> {
    guidance
        .why
        .iter()
        .chain(guidance.fix)
        .chain(guidance.bad_fixes)
        .copied()
}

#[test]
fn every_rule_explains_why_how_and_what_not_to_do() {
    for rule in Rule::iter() {
        let guidance = Guidance::of(rule);
        assert_ne!(guidance.why, NONE, "{} lacks why", rule.name());
        assert_ne!(guidance.fix, NONE, "{} lacks fix", rule.name());
        assert_ne!(guidance.bad_fixes, NONE, "{} lacks bad fixes", rule.name());
    }
}

#[test]
fn bad_fixes_read_as_prohibitions() {
    for rule in Rule::iter() {
        for point in Guidance::of(rule).bad_fixes {
            assert!(point.starts_with("Do not "), "{}: {point}", rule.name());
        }
    }
}

#[test]
fn points_are_single_ascii_lines() {
    for rule in Rule::iter() {
        for point in points(&Guidance::of(rule)) {
            assert!(
                point.is_ascii() && !point.contains('\n'),
                "{}: {point}",
                rule.name()
            );
        }
    }
}

#[test]
fn exemption_guidance_shows_where_each_exemptable_rule_is_exempted() {
    let guidance = Guidance::of(Rule::MalformedExemption);
    let why = guidance.why.join(" ");
    let fix = guidance.fix.join(" ");
    for rule in Rule::iter() {
        let name = rule.name();
        let named = why.contains(&format!("`{name}`"));
        match rule.exemptability() {
            Exemptability::Never => assert!(!named, "{name}"),
            Exemptability::Source(_) => {
                assert!(named, "{name}");
                assert!(
                    fix.contains(&format!("#[cfg_attr(samlinter, expect({name}, ")),
                    "{name}"
                );
            }
            Exemptability::Manifest(_) => {
                assert!(named, "{name}");
                assert!(
                    fix.contains(&format!("[package.metadata.samlinter.expect.{name}]")),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn renders_labeled_bullet_sections() {
    let guidance = Guidance {
        why: &["a"],
        fix: &["b", "c"],
        bad_fixes: &["Do not d"],
    };
    assert_eq!(
        guidance.to_string(),
        "  why:\n    - a\n  fix:\n    - b\n    - c\n  bad fixes:\n    - Do not d\n"
    );
}
