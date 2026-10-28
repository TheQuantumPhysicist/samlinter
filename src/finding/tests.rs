// - TODO: randomized variant: random text of ASCII, accented letters, an emoji and newlines,
//   cut at a random char boundary: `Position::of` gives line 1 plus the newlines before the
//   offset, column 1 plus the chars since the last newline.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{Exemptability, Finding, ManifestExemptable, Position, Rule, SourceExemptable, Span};
use rstest::rstest;
use std::path::PathBuf;
use strum::IntoEnumIterator;

#[rstest]
#[case::start("abc", 0, "1:1")]
#[case::same_line("abc", 2, "1:3")]
#[case::after_newline("ab\ncd", 3, "2:1")]
#[case::end_after_newline("ab\n", 3, "2:1")]
#[case::multibyte_counts_characters("\u{e9}\u{e9}x", 4, "1:3")]
#[case::leading_bom_takes_no_column("\u{feff}ab", 4, "1:2")]
#[case::offset_of_the_leading_bom("\u{feff}ab", 0, "1:1")]
#[case::bom_after_the_start_is_a_character("a\u{feff}b", 4, "1:3")]
#[case::leading_bom_then_lines("\u{feff}a\nb", 5, "2:1")]
fn position_counts_lines_and_characters(
    #[case] text: &str,
    #[case] offset: usize,
    #[case] expected: &str,
) {
    assert_eq!(Position::of(text, offset).to_string(), expected);
}

#[rstest]
#[case::positioned(
    Span::At(Position { line: 3, column: 7 }),
    "src/lib.rs:3:7: rest_pattern: message"
)]
#[case::whole_file(Span::File, "src/lib.rs: rest_pattern: message")]
fn finding_renders_path_position_rule_message(#[case] span: Span, #[case] expected: &str) {
    let finding = Finding {
        path: PathBuf::from("src/lib.rs"),
        span,
        rule: Rule::RestPattern,
        message: "message".to_owned(),
    };
    assert_eq!(finding.to_string(), expected);
}

#[rstest]
#[case::rest_pattern(
    "rest_pattern",
    Ok(Exemptability::Source(SourceExemptable::RestPattern))
)]
#[case::member_version(
    "member_version",
    Ok(Exemptability::Manifest(ManifestExemptable::MemberVersion))
)]
#[case::not_exemptable("em_dash", Ok(Exemptability::Never))]
#[case::unknown("rest_patterns", Err("unknown rule `rest_patterns`"))]
fn exemptions_name_existing_rules(
    #[case] name: &str,
    #[case] expected: Result<Exemptability, &str>,
) {
    assert_eq!(Exemptability::named(name), expected.map_err(str::to_owned));
}

#[test]
fn exemptable_rules_convert_back_to_their_rule() {
    for rule in Rule::iter() {
        let converted = match rule.exemptability() {
            Exemptability::Never => rule,
            Exemptability::Source(exemptable) => Rule::from(exemptable),
            Exemptability::Manifest(exemptable) => Rule::from(exemptable),
        };
        assert_eq!(converted, rule);
    }
}

#[test]
fn rule_names_are_snake_case_ids() {
    assert_eq!(
        Rule::WorkspaceDependencyForm.name(),
        "workspace_dependency_form"
    );
}
