// - TODO: randomized variant: random findings render each line once, one `  why:` per
//   distinct rule, groups in ascending rule order.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::guidance::Guidance;
use super::render;
use crate::finding::{Finding, Position, Rule, Span};
use std::path::PathBuf;

fn finding(path: &str, line: usize, rule: Rule) -> Finding {
    Finding {
        path: PathBuf::from(path),
        span: Span::At(Position { line, column: 1 }),
        rule,
        message: "message".to_owned(),
    }
}

#[test]
fn groups_findings_by_rule_with_guidance_once_per_rule() {
    let findings = [
        finding("a.rs", 1, Rule::RestPattern),
        finding("a.rs", 2, Rule::EmDash),
        finding("b.rs", 3, Rule::RestPattern),
    ];
    let [first_rest, em_dash, second_rest] = &findings;
    let expected = format!(
        "{em_dash}\n{}\n{first_rest}\n{second_rest}\n{}",
        Guidance::of(Rule::EmDash),
        Guidance::of(Rule::RestPattern)
    );
    assert_eq!(render(&findings), expected);
}

#[test]
fn no_findings_render_nothing() {
    assert_eq!(render(&[]), "");
}
