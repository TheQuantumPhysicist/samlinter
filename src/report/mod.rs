mod guidance;

use crate::finding::{Finding, Rule};
use guidance::Guidance;
use std::collections::BTreeSet;

/// - Findings grouped by rule, in rule order, each group followed by that rule's guidance.
/// - Guidance appears once per rule, right under the findings it explains.
pub fn render(findings: &[Finding]) -> String {
    let rules: BTreeSet<Rule> = findings.iter().map(|finding| finding.rule).collect();
    rules
        .into_iter()
        .map(|rule| {
            let lines: Vec<String> = findings
                .iter()
                .filter(|finding| finding.rule == rule)
                .map(ToString::to_string)
                .collect();
            format!("{}\n{}", lines.join("\n"), Guidance::of(rule))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
