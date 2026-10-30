use super::{
    Exemptability, Finding, ManifestExemptable, Position, Rule, SourceExemptable, Span, positions,
};
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
#[case::plain("ab\ncd\u{e9}f\n", &[7, 0, 3, 7, 2, 4])]
#[case::leading_bom("\u{feff}a\nb", &[5, 0, 3, 4])]
#[case::past_the_end("ab", &[9, 1])]
#[case::none("ab", &[])]
fn positions_match_one_position_each_in_the_given_order(
    #[case] text: &str,
    #[case] offsets: &[usize],
) {
    let each: Vec<Position> = offsets
        .iter()
        .map(|offset| Position::of(text, *offset))
        .collect();
    assert_eq!(positions(text, offsets), each);
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
    Ok(Exemptability::Source(SourceExemptable(Rule::RestPattern)))
)]
#[case::em_dash("em_dash", Ok(Exemptability::Source(SourceExemptable(Rule::EmDash))))]
#[case::member_version(
    "member_version",
    Ok(Exemptability::Manifest(ManifestExemptable::MemberVersion))
)]
#[case::feature_flags(
    "feature_flags",
    Ok(Exemptability::Manifest(ManifestExemptable::FeatureFlags))
)]
#[case::module_shape(
    "module_shape",
    Ok(Exemptability::Source(SourceExemptable(Rule::ModuleShape)))
)]
#[case::not_exemptable("syntax_error", Ok(Exemptability::Never))]
#[case::unknown("rest_patterns", Err("unknown rule `rest_patterns`"))]
fn exemptions_name_existing_rules(
    #[case] name: &str,
    #[case] expected: Result<Exemptability, &str>,
) {
    assert_eq!(Exemptability::named(name), expected.map_err(str::to_owned));
}

/// Where each rule is exempted, as the README states it.
fn exempted_in(rule: Rule) -> &'static str {
    match rule.exemptability() {
        Exemptability::Never => "nowhere",
        Exemptability::Source(_) => "rust",
        Exemptability::Manifest(_) => "cargo.toml",
    }
}

#[rstest]
#[case::binary_readme(Rule::BinaryReadme, "nowhere")]
#[case::discarded_result(Rule::DiscardedResult, "nowhere")]
#[case::direct_rng(Rule::DirectRng, "rust")]
#[case::em_dash(Rule::EmDash, "rust")]
#[case::emoji(Rule::Emoji, "rust")]
#[case::enum_is(Rule::EnumIs, "rust")]
#[case::feature_flags(Rule::FeatureFlags, "cargo.toml")]
#[case::fn_spacing(Rule::FnSpacing, "rust")]
#[case::forbidden_word(Rule::ForbiddenWord, "rust")]
#[case::item_order(Rule::ItemOrder, "rust")]
#[case::item_spacing(Rule::ItemSpacing, "rust")]
#[case::json_macro(Rule::JsonMacro, "rust")]
#[case::library_subscriber(Rule::LibrarySubscriber, "rust")]
#[case::malformed_exemption(Rule::MalformedExemption, "nowhere")]
#[case::matches_macro(Rule::MatchesMacro, "rust")]
#[case::member_version(Rule::MemberVersion, "cargo.toml")]
#[case::module_shape(Rule::ModuleShape, "rust")]
#[case::panic_strategy(Rule::PanicStrategy, "nowhere")]
#[case::path_attr(Rule::PathAttr, "rust")]
#[case::regex_use(Rule::RegexUse, "rust")]
#[case::rest_pattern(Rule::RestPattern, "rust")]
#[case::restricted_visibility(Rule::RestrictedVisibility, "rust")]
#[case::system_time_now(Rule::SystemTimeNow, "rust")]
#[case::syntax_error(Rule::SyntaxError, "nowhere")]
#[case::unfulfilled_exemption(Rule::UnfulfilledExemption, "nowhere")]
#[case::workspace_dependency_form(Rule::WorkspaceDependencyForm, "nowhere")]
fn each_rule_is_exempted_where_its_findings_are(#[case] rule: Rule, #[case] expected: &str) {
    assert_eq!(exempted_in(rule), expected);
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
