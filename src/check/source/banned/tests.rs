use super::super::{exemption::apply, hits, render};
use super::check;
use crate::finding::{Position, Rule};
use ra_ap_syntax::AstNode;
use rstest::rstest;

fn banned(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax()))
}

/// Violation hits after exemptions, plus findings about the exemptions themselves.
fn exempted(text: &str) -> Vec<String> {
    render(text, |file| apply(file.syntax(), check(file.syntax())))
}

/// Each violation hit as `line:column rule: message`, what a finding shows without the path.
fn rendered(text: &str) -> Vec<String> {
    hits(text, |file| check(file.syntax()))
        .into_iter()
        .map(|hit| {
            format!(
                "{} {}: {}",
                Position::of(text, hit.offset),
                hit.rule.name(),
                hit.message
            )
        })
        .collect()
}

/// Messages of the findings about the exemptions in `text`, the violation hits dropped.
fn exemption_messages(text: &str) -> Vec<String> {
    hits(text, |file| apply(file.syntax(), check(file.syntax())))
        .into_iter()
        .filter(|hit| hit.rule == Rule::MalformedExemption)
        .map(|hit| hit.message)
        .collect()
}

#[rstest]
#[case::bare("fn f(v: V) -> bool {\n    matches!(v, V::A | V::B)\n}\n", &["2:5 matches_macro"])]
#[case::core("fn f(v: V) -> bool {\n    core::matches!(v, V::A)\n}\n", &["2:11 matches_macro"])]
#[case::std("fn f(v: V) -> bool {\n    std::matches!(v, V::A)\n}\n", &["2:10 matches_macro"])]
#[case::in_a_condition(
    "fn f(v: V) {\n    if matches!(v, V::A) {}\n}\n",
    &["2:8 matches_macro"]
)]
#[case::other_macros_stay_silent(
    "fn f() {\n    assert!(1 == 1);\n    format!(\"{}\", 1);\n}\n",
    &[]
)]
#[case::a_function_named_matches_stays_silent(
    "fn matches(v: u8) -> bool {\n    v > 1\n}\nfn g(v: u8) -> bool {\n    matches(v)\n}\n",
    &[]
)]
#[case::pattern_and_expression_on_separate_lines(
    "fn f(v: V) -> bool {\n    matches!(\n        v,\n        V::A | V::B\n    )\n}\n",
    &["2:5 matches_macro"]
)]
fn flags_the_matches_macro(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(banned(text), expected);
}

#[rstest]
#[case::bare("#[derive(EnumIs)]\nenum E {\n    A,\n}\n", &["1:10 enum_is"])]
#[case::qualified(
    "#[derive(strum::EnumIs)]\nenum E {\n    A,\n}\n",
    &["1:17 enum_is"]
)]
#[case::in_a_list(
    "#[derive(Debug, EnumIs)]\nenum E {\n    A,\n}\n",
    &["1:17 enum_is"]
)]
#[case::other_derives_stay_silent(
    "#[derive(Debug, strum::EnumString, strum::EnumCount)]\nenum E {\n    A,\n}\n",
    &[]
)]
#[case::list_spanning_lines(
    "#[derive(\n    Debug,\n    EnumIs,\n)]\nenum E {\n    A,\n}\n",
    &["3:5 enum_is"]
)]
#[case::a_degenerate_duplicate_is_one_finding(
    "#[derive(EnumIs, EnumIs)]\nenum E {\n    A,\n}\n",
    &["1:10 enum_is"]
)]
#[case::a_derive_wrapped_in_cfg_attr_is_flagged(
    "#[cfg_attr(feature = \"x\", derive(EnumIs))]\nenum E {\n    A,\n}\n",
    &["1:34 enum_is"]
)]
fn flags_enum_is_in_a_derive_list(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(banned(text), expected);
}

#[rstest]
#[case::system_time(
    "fn f() {\n    let t = SystemTime::now();\n}\n",
    &["2:25 system_time_now"]
)]
#[case::qualified_system_time(
    "fn f() {\n    let t = std::time::SystemTime::now();\n}\n",
    &["2:36 system_time_now"]
)]
#[case::tokio_instant(
    "fn f() {\n    let t = tokio::time::Instant::now();\n}\n",
    &["2:35 system_time_now"]
)]
#[case::utc("fn f() {\n    let t = Utc::now();\n}\n", &["2:18 system_time_now"])]
#[case::chrono_local(
    "fn f() {\n    let t = chrono::Local::now();\n}\n",
    &["2:28 system_time_now"]
)]
#[case::duration_arithmetic_stays_silent(
    "fn f() {\n    let d = Duration::from_secs(2) + Duration::from_secs(3);\n}\n",
    &[]
)]
#[case::a_method_call_on_a_value_stays_silent(
    "fn f(t: T) {\n    let n = t.now();\n}\n",
    &[]
)]
#[case::call_at_the_end_of_an_argument_list(
    "fn f() {\n    g(\n        1,\n        Utc::now(),\n    );\n}\n",
    &["4:14 system_time_now"]
)]
#[case::timedelta_arithmetic_stays_silent(
    "fn f() {\n    let d = chrono::TimeDelta::seconds(2) + chrono::TimeDelta::seconds(3);\n}\n",
    &[]
)]
#[case::timedelta_added_to_a_clock_read_is_one_finding(
    "fn f() {\n    let t = Utc::now() + TimeDelta::seconds(30);\n}\n",
    &["2:18 system_time_now"]
)]
#[case::bare_instant_after_a_use_import(
    "use std::time::Instant;\nfn f() {\n    let t = Instant::now();\n}\n",
    &["3:22 system_time_now"]
)]
#[case::bare_local_after_a_use_import(
    "use chrono::Local;\nfn f() {\n    let t = Local::now();\n}\n",
    &["3:20 system_time_now"]
)]
fn flags_direct_clock_reads(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(banned(text), expected);
}

#[rstest]
#[case::use_tree("use regex::Regex;\nfn f() {}\n", &["1:5 regex_use"])]
#[case::nested_use_tree(
    "use regex::internal::Replacer;\nfn f() {}\n",
    &["1:5 regex_use"]
)]
#[case::use_tree_list(
    "use regex::{Regex, RegexBuilder};\nfn f() {}\n",
    &["1:5 regex_use"]
)]
#[case::constructor(
    "fn f() {\n    let r = regex::Regex::new(\"a\");\n}\n",
    &["2:13 regex_use"]
)]
#[case::constructor_by_another_route(
    "fn f() {\n    let r = util::Regex::new(\"a\");\n}\n",
    &["2:26 regex_use"]
)]
#[case::builder_by_another_route(
    "fn f() {\n    let b = util::RegexBuilder::new(\"a\");\n}\n",
    &["2:33 regex_use"]
)]
#[case::a_crate_named_alike_stays_silent("use myregex::Thing;\nfn f() {}\n", &[])]
#[case::hand_rolled_checks_stay_silent(
    "fn f(s: &str) {\n    s.len() == 4 && s.chars().all(|c| c.is_ascii_alphanumeric())\n}\n",
    &[]
)]
#[case::crate_segment_on_a_later_line(
    "use\n    regex::Replacer;\nfn f() {}\n",
    &["2:5 regex_use"]
)]
#[case::constructor_on_a_later_line(
    "fn f() {\n    let r = util::Regex::\n        new(\"a\");\n}\n",
    &["3:9 regex_use"]
)]
#[case::the_type_in_type_position_stays_silent(
    "fn f() {\n    let r: Regex = g();\n}\n",
    &[]
)]
#[case::a_path_of_the_type_without_a_call_stays_silent(
    "fn g(r: util::Regex) {}\n",
    &[]
)]
fn flags_regex_use(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(banned(text), expected);
}

#[rstest]
#[case::thread_rng(
    "fn f() {\n    let mut r = thread_rng();\n}\n",
    &["2:17 direct_rng"]
)]
#[case::qualified_thread_rng(
    "fn f() {\n    let mut r = rand::thread_rng();\n}\n",
    &["2:23 direct_rng"]
)]
#[case::rng_type("fn f() {\n    let mut r = OsRng;\n}\n", &["2:17 direct_rng"])]
#[case::qualified_rng_type(
    "fn f() {\n    let mut r = rand::rngs::OsRng;\n}\n",
    &["2:29 direct_rng"]
)]
#[case::from_entropy(
    "fn f() {\n    let mut r = StdRng::from_entropy();\n}\n",
    &["2:17 direct_rng"]
)]
#[case::from_seed_is_a_construction(
    "fn f() {\n    let mut r = ChaChaRng::from_seed([0; 32]);\n}\n",
    &["2:17 direct_rng"]
)]
#[case::seed_from_u64_by_path(
    "fn f() {\n    let mut r = SmallRng::seed_from_u64(3);\n}\n",
    &["2:17 direct_rng"]
)]
#[case::bare_random("fn f() {\n    let x = random();\n}\n", &["2:13 direct_rng"])]
#[case::qualified_random(
    "fn f() {\n    let x = rand::random();\n}\n",
    &["2:19 direct_rng"]
)]
#[case::turbofish_random(
    "fn f() {\n    let x = random::<u8>();\n}\n",
    &["2:13 direct_rng"]
)]
#[case::free_from_entropy(
    "fn f() {\n    let mut r = from_entropy();\n}\n",
    &["2:17 direct_rng"]
)]
#[case::seed_from_u64_by_method(
    "fn f(mut r: R) {\n    r.seed_from_u64(3);\n}\n",
    &["2:7 direct_rng"]
)]
#[case::trait_methods_stay_silent(
    "fn f(r: &mut R) {\n    let x = r.gen_range(1..10);\n}\n",
    &[]
)]
#[case::the_sanctioned_generators_stay_silent(
    "fn f() {\n    let mut r = make_pseudo_rng();\n    let t = make_true_rng();\n}\n",
    &[]
)]
#[case::call_in_a_multi_line_initializer(
    "fn f() {\n    let mut r = (\n        rand::thread_rng(),\n        1,\n    );\n}\n",
    &["3:15 direct_rng"]
)]
fn flags_direct_rng(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(banned(text), expected);
}

#[rstest]
#[case::matches_macro(
    "#[cfg_attr(samlinter, expect(matches_macro, reason = \"open-ended input\"))]\nfn f(v: V) -> bool {\n    matches!(v, V::A)\n}\n"
)]
#[case::enum_is(
    "#[cfg_attr(samlinter, expect(enum_is, reason = \"foreign type\"))]\n#[derive(EnumIs)]\nenum E {\n    A,\n}\n"
)]
#[case::system_time_now(
    "#[cfg_attr(samlinter, expect(system_time_now, reason = \"the Time wrapper itself\"))]\nfn f() {\n    let t = Utc::now();\n}\n"
)]
#[case::regex_use(
    "#[cfg_attr(samlinter, expect(regex_use, reason = \"recorded consent\"))]\nfn f() {\n    let r = regex::Regex::new(\"a\");\n}\n"
)]
#[case::regex_use_on_a_use_item(
    "#[cfg_attr(samlinter, expect(regex_use, reason = \"recorded consent\"))]\nuse regex::Regex;\n"
)]
#[case::direct_rng(
    "#[cfg_attr(samlinter, expect(direct_rng, reason = \"the randomness crate itself\"))]\nfn f() {\n    let mut r = thread_rng();\n}\n"
)]
fn an_expect_with_a_reason_lifts_the_finding(#[case] text: &str) {
    assert_eq!(exempted(text), Vec::<String>::new());
}

#[rstest]
#[case::matches_macro(
    "#[cfg_attr(samlinter, expect(matches_macro))]\nfn f(v: V) -> bool {\n    matches!(v, V::A)\n}\n",
    &["3:5 matches_macro", "1:1 malformed_exemption"]
)]
#[case::enum_is(
    "#[cfg_attr(samlinter, expect(enum_is))]\n#[derive(EnumIs)]\nenum E {\n    A,\n}\n",
    &["2:10 enum_is", "1:1 malformed_exemption"]
)]
#[case::system_time_now(
    "#[cfg_attr(samlinter, expect(system_time_now))]\nfn f() {\n    let t = Utc::now();\n}\n",
    &["3:18 system_time_now", "1:1 malformed_exemption"]
)]
#[case::regex_use(
    "#[cfg_attr(samlinter, expect(regex_use))]\nfn f() {\n    let r = regex::Regex::new(\"a\");\n}\n",
    &["3:13 regex_use", "1:1 malformed_exemption"]
)]
#[case::direct_rng(
    "#[cfg_attr(samlinter, expect(direct_rng))]\nfn f() {\n    let mut r = thread_rng();\n}\n",
    &["3:17 direct_rng", "1:1 malformed_exemption"]
)]
// The name resolves as a known rule, so only the missing reason is reported.
fn a_malformed_expect_lifts_nothing(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(exempted(text), expected);
    assert_eq!(
        exemption_messages(text),
        ["missing `reason = \"<why>\"` at the end of the list"]
    );
}

#[rstest]
#[case::matches_macro(
    "fn f(v: V) -> bool {\n    matches!(v, V::A | V::B)\n}\n",
    &["2:5 matches_macro: `matches!` absorbs every variant the expression does not name"]
)]
#[case::enum_is(
    "#[derive(EnumIs)]\nenum E {\n    A,\n}\n",
    &["1:10 enum_is: `EnumIs` generates `.is_variant()` checks that absorb new variants"]
)]
#[case::system_time_now(
    "fn f() {\n    let t = Utc::now();\n}\n",
    &["2:18 system_time_now: direct clock read: `Utc::now()`"]
)]
#[case::regex_use(
    "use regex::Regex;\nfn f() {}\n",
    &["1:5 regex_use: regex crate use: `regex::Regex`"]
)]
#[case::direct_rng(
    "fn f() {\n    let mut r = thread_rng();\n}\n",
    &["2:17 direct_rng: direct RNG: `thread_rng()`"]
)]
fn a_canonical_finding_names_the_rule_and_its_symbol(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(rendered(text), expected);
}
