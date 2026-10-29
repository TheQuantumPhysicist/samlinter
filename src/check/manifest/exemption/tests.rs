use super::super::super::{place, render, text_file};
use super::super::{feature_flags, member_versions};
use super::apply;
use crate::finding::Finding;
use rstest::rstest;
use toml_edit::Document;

/// The `member_version` and `feature_flags` violations of `text`, after applying the exemptions
/// written in it.
fn exempted(text: &str) -> Vec<Finding> {
    let document = Document::parse(text).unwrap();
    let root = document.as_table();
    let manifest = text_file("Cargo.toml", text);
    let violations = member_versions(root)
        .into_iter()
        .chain(feature_flags(root))
        .collect();
    apply(root, violations)
        .into_iter()
        .map(|violation| violation.into_finding(&manifest).unwrap())
        .collect()
}

#[rstest]
#[case::named_dependency(
    "[dependencies]\nrand = \"0.10\"\n\n[package.metadata.samlinter.expect.member_version]\nrand = \"specialized to this crate by definition\"\n",
    &[]
)]
#[case::dotted_key(
    "[dependencies]\nrand = \"0.10\"\n\n[package.metadata.samlinter]\nexpect.member_version.rand = \"why\"\n",
    &[]
)]
#[case::inline_table(
    "[dependencies]\nrand = \"0.10\"\n\n[package.metadata.samlinter]\nexpect = { member_version = { rand = \"why\" } }\n",
    &[]
)]
#[case::other_dependency(
    "[dependencies]\nserde = \"1\"\n\n[package.metadata.samlinter.expect.member_version]\nrand = \"why\"\n",
    &["2:1 member_version", "5:1 unfulfilled_exemption"]
)]
#[case::other_rule(
    "[features]\nfast = []\n\n[package.metadata.samlinter.expect.member_version]\nrand = \"why\"\n",
    &["1:2 feature_flags", "5:1 unfulfilled_exemption"]
)]
#[case::no_exemptions(
    "[package]\nname = \"a\"\n\n[dependencies]\nrand = \"0.10\"\n",
    &["5:1 member_version"]
)]
fn exemption_lifts_only_the_rule_and_dependency_it_names(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(render(&exempted(text)), expected);
}

#[rstest]
#[case::empty_reason("[package.metadata.samlinter.expect.member_version]\nrand = \" \"\n")]
#[case::reason_not_a_string("[package.metadata.samlinter.expect.member_version]\nrand = 1\n")]
fn malformed_exemption_exempts_nothing(#[case] exemption: &str) {
    let text = format!("[dependencies]\nrand = \"0.10\"\n\n{exemption}");
    assert_eq!(
        render(&exempted(&text)),
        ["2:1 member_version", "5:1 malformed_exemption"]
    );
}

#[rstest]
#[case::samlinter_not_a_table(
    "[package.metadata]\nsamlinter = 1\n",
    "2:1 `[package.metadata.samlinter]` must be a table"
)]
#[case::expect_not_a_table(
    "[package.metadata.samlinter]\nexpect = 1\n",
    "2:1 `expect` must be a table of rules"
)]
#[case::unknown_key(
    "[package.metadata.samlinter]\nexcept = {}\n",
    "2:1 unknown key `except`; only `expect` is allowed"
)]
#[case::rule_not_a_table(
    "[package.metadata.samlinter.expect]\nmember_version = \"x\"\n",
    "2:1 map each exempt dependency to the reason it is exempt"
)]
#[case::reason_not_a_string(
    "[package.metadata.samlinter.expect.member_version]\nrand = 1\n",
    "2:1 the reason must be a non-empty string saying why"
)]
#[case::empty_reason(
    "[package.metadata.samlinter.expect.member_version]\nrand = \" \"\n",
    "2:1 the reason must be a non-empty string saying why"
)]
#[case::unknown_rule(
    "[package.metadata.samlinter.expect.no_such_rule]\nx = \"why\"\n",
    "1:36 unknown rule `no_such_rule`"
)]
#[case::rule_without_exception_clause(
    "[package.metadata.samlinter.expect.feature_flags]\nx = \"why\"\n",
    "1:36 rule `feature_flags` cannot be exempted"
)]
#[case::rule_exempted_in_rust_source(
    "[package.metadata.samlinter.expect.rest_pattern]\nx = \"why\"\n",
    "1:36 rule `rest_pattern` is exempted in Rust source, not in Cargo.toml"
)]
fn malformed_exemption_says_what_is_wrong(#[case] text: &str, #[case] expected: &str) {
    let messages: Vec<String> = exempted(text)
        .into_iter()
        .map(|finding| format!("{} {}", place(finding.span), finding.message))
        .collect();
    assert_eq!(messages, [expected]);
}
