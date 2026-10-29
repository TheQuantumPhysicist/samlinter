use super::super::{hits, patterns, render};
use super::apply;
use ra_ap_syntax::AstNode;
use rstest::rstest;

const USAGE: &str =
    "expected exactly `expect(<rule>, reason = \"<why>\")` inside `cfg_attr(samlinter, ...)`";

/// Rest-pattern hits after exemptions, plus findings about the exemptions themselves.
fn exempted(text: &str) -> Vec<String> {
    render(text, |file| {
        apply(file.syntax(), patterns::check(file.syntax()).collect())
    })
}

/// Messages of the findings about the exemptions in `text`, as if it held no violation.
fn exemption_messages(text: &str) -> Vec<String> {
    hits(text, |file| apply(file.syntax(), Vec::new()))
        .into_iter()
        .map(|hit| hit.message)
        .collect()
}

/// A function whose body holds one rest pattern at 3:16, under the attribute `{attr}`.
fn annotated_fn(attr: &str) -> String {
    format!("#[{attr}]\nfn f(v: V) {{\n    let V {{ a, .. }} = v;\n}}\n")
}

#[rstest]
#[case::on_function(&annotated_fn(
    "cfg_attr(samlinter, expect(rest_pattern, reason = \"V is a foreign non_exhaustive struct\"))"
))]
#[case::trailing_comma(&annotated_fn(
    "cfg_attr(samlinter, expect(rest_pattern, reason = \"why\",))"
))]
#[case::raw_string_reason(&annotated_fn(
    "cfg_attr(samlinter, expect(rest_pattern, reason = r\"why\"))"
))]
#[case::on_match_arm(
    "fn f(e: E) {\n    match e {\n        #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n        E::A { .. } => {}\n        E::B => {}\n    }\n}\n"
)]
#[case::on_let_statement(
    "fn f(v: V) {\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n    let V { a, .. } = v;\n}\n"
)]
#[case::on_parameter(
    "fn f(#[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))] V { a, .. }: V) {}\n"
)]
#[case::on_pattern_field(
    "fn f(s: S) {\n    let S {\n        #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n        inner: V { a, .. },\n        b,\n    } = s;\n}\n"
)]
#[case::on_block_around_an_assignment(
    "fn f(t: (u8, u8)) {\n    let a;\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n    {\n        (a, ..) = t;\n    }\n}\n"
)]
#[case::on_block_around_an_assignment_with_a_semicolon(
    "fn f(t: (u8, u8)) {\n    let a;\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n    {\n        (a, ..) = t;\n    };\n}\n"
)]
#[case::on_block_around_an_assignment_before_another_statement(
    "fn f(t: (u8, u8)) -> u8 {\n    let a;\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n    {\n        (a, ..) = t;\n    }\n    a\n}\n"
)]
#[case::on_function_around_an_assignment(
    "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\nfn f(t: (u8, u8)) {\n    let a;\n    (a, ..) = t;\n}\n"
)]
fn exemption_covers_its_node(#[case] text: &str) {
    assert_eq!(exempted(text), Vec::<String>::new());
}

/// An exemption covers its own item, not its siblings: `g`'s violation stands, and only it.
#[test]
fn exemption_does_not_cover_siblings() {
    let text = "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\nfn f(v: V) {\n    let V { a, .. } = v;\n}\nfn g(v: V) {\n    let V { a, .. } = v;\n}\n";
    assert_eq!(exempted(text).len(), 1);
}

#[rstest]
#[case::missing_reason(
    "cfg_attr(samlinter, expect(rest_pattern))",
    "missing `reason = \"<why>\"` at the end of the list"
)]
#[case::empty_reason(
    "cfg_attr(samlinter, expect(rest_pattern, reason = \"  \"))",
    "`reason` is empty; name the concrete case"
)]
#[case::reason_not_a_string(
    "cfg_attr(samlinter, expect(rest_pattern, reason = 5))",
    "`reason` must be a string literal"
)]
#[case::reason_with_bare_carriage_return(
    "cfg_attr(samlinter, expect(rest_pattern, reason = \"a\rb\"))",
    "`reason` holds a bare carriage return or a line continuation rustc warns about"
)]
#[case::unknown_rule(
    "cfg_attr(samlinter, expect(rest_patterns, reason = \"why\"))",
    "unknown rule `rest_patterns`"
)]
#[case::syntax_error(
    "cfg_attr(samlinter, expect(syntax_error, reason = \"why\"))",
    "rule `syntax_error` cannot be exempted"
)]
#[case::malformed_exemption(
    "cfg_attr(samlinter, expect(malformed_exemption, reason = \"why\"))",
    "rule `malformed_exemption` cannot be exempted"
)]
#[case::unfulfilled_exemption(
    "cfg_attr(samlinter, expect(unfulfilled_exemption, reason = \"why\"))",
    "rule `unfulfilled_exemption` cannot be exempted"
)]
#[case::manifest_rule_without_exception_clause(
    "cfg_attr(samlinter, expect(feature_flags, reason = \"why\"))",
    "rule `feature_flags` cannot be exempted"
)]
#[case::rule_exempted_in_cargo_toml(
    "cfg_attr(samlinter, expect(member_version, reason = \"why\"))",
    "rule `member_version` is exempted in Cargo.toml, not in Rust source"
)]
#[case::one_rule_exempted_in_cargo_toml(
    "cfg_attr(samlinter, expect(rest_pattern, member_version, reason = \"why\"))",
    "rule `member_version` is exempted in Cargo.toml, not in Rust source"
)]
#[case::rule_after_reason(
    "cfg_attr(samlinter, expect(reason = \"why\", rest_pattern))",
    "missing `reason = \"<why>\"` at the end of the list"
)]
#[case::reason_not_last(
    "cfg_attr(samlinter, expect(reason = \"a\", rest_pattern, reason = \"b\"))",
    "`reason` must be the last argument"
)]
#[case::no_rule(
    "cfg_attr(samlinter, expect(reason = \"why\"))",
    "name at least one rule before `reason`"
)]
#[case::empty_list("cfg_attr(samlinter, expect())", USAGE)]
#[case::bare_expect("cfg_attr(samlinter, expect)", USAGE)]
#[case::no_attribute("cfg_attr(samlinter,)", USAGE)]
#[case::rule_as_string(
    "cfg_attr(samlinter, expect(\"rest_pattern\", reason = \"why\"))",
    "unexpected `\"rest_pattern\"` inside `expect(...)`"
)]
#[case::empty_argument(
    "cfg_attr(samlinter, expect(rest_pattern,, reason = \"why\"))",
    "empty argument inside `expect(...)`"
)]
#[case::missing_comma(
    "cfg_attr(samlinter, expect(rest_pattern reason = \"why\"))",
    "unexpected `rest_pattern reason = \"why\"` inside `expect(...)`"
)]
#[case::nested_group(
    "cfg_attr(samlinter, expect((rest_pattern), reason = \"why\"))",
    "unexpected `(rest_pattern)` inside `expect(...)`"
)]
#[case::not_expect("cfg_attr(samlinter, allow(rest_pattern, reason = \"why\"))", USAGE)]
#[case::key_value("cfg_attr(samlinter, expect = \"why\")", USAGE)]
#[case::two_attributes(
    "cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"), inline)",
    USAGE
)]
fn malformed_exemption_says_what_is_wrong_and_exempts_nothing(
    #[case] attr: &str,
    #[case] message: &str,
) {
    let text = annotated_fn(attr);
    assert_eq!(
        exempted(&text),
        ["3:16 rest_pattern", "1:1 malformed_exemption"]
    );
    assert_eq!(exemption_messages(&text), [message]);
}

#[test]
fn reason_continuing_over_a_blank_line_is_malformed() {
    let text = annotated_fn("cfg_attr(samlinter, expect(rest_pattern, reason = \"a\\\n\nb\"))");
    assert_eq!(
        exempted(&text),
        ["5:16 rest_pattern", "1:1 malformed_exemption"]
    );
    assert_eq!(
        exemption_messages(&text),
        ["`reason` holds a bare carriage return or a line continuation rustc warns about"]
    );
}

#[rstest]
// - A function body holds nothing an inner attribute can own, so an inner exemption there
//   is a stray one: malformed, the only place an inner exemption still is.
#[case::function_body(
    "fn f(v: V) {\n    #![cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n    let V { a, .. } = v;\n}\n",
    &["3:16 rest_pattern", "2:5 malformed_exemption"]
)]
fn inner_exemption_away_from_a_file_or_module_top_is_malformed(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(exempted(text), expected);
    assert_eq!(
        exemption_messages(text),
        [
            "an exemption is an outer `#[...]` on the node it covers, or an inner `#![...]` at the top of a file or inline module; write it on the item, field, match arm, statement or parameter that holds the violation"
        ]
    );
}

/// At the top of a file an inner exemption is valid and covers the file's findings, its
/// scope the attached file node's text.
#[test]
fn inner_exemption_at_the_file_top_covers_the_file() {
    let text = "#![cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n\nfn f(v: V) {\n    let V { a, .. } = v;\n}\n";
    assert_eq!(exempted(text), Vec::<String>::new());
}

#[rstest]
#[case::other_cfg("cfg_attr(test, expect(rest_pattern, reason = \"why\"))")]
#[case::samlinter_cfg_with_a_value(
    "cfg_attr(samlinter = \"x\", expect(rest_pattern, reason = \"why\"))"
)]
#[case::composite_cfg("cfg_attr(any(samlinter), expect(rest_pattern, reason = \"why\"))")]
fn other_cfg_attr_is_not_an_exemption(#[case] attr: &str) {
    assert_eq!(exempted(&annotated_fn(attr)), ["3:16 rest_pattern"]);
}

#[test]
fn exemption_on_a_sibling_is_unfulfilled() {
    let text = "fn f(e: E) {\n    match e {\n        E::A { .. } => {}\n        #[cfg_attr(samlinter, expect(rest_pattern, reason = \"why\"))]\n        E::B => {}\n    }\n}\n";
    assert_eq!(
        exempted(text),
        ["3:16 rest_pattern", "4:9 unfulfilled_exemption"]
    );
    assert_eq!(
        exemption_messages(text),
        ["exemption for `rest_pattern` matches nothing here"]
    );
}

#[test]
fn each_listed_rule_is_a_separate_exemption() {
    let text =
        "#[cfg_attr(samlinter, expect(rest_pattern, em_dash, reason = \"why\"))]\nfn f() {}\n";
    assert_eq!(
        exempted(text),
        ["1:1 unfulfilled_exemption", "1:1 unfulfilled_exemption"]
    );
}

#[rstest]
#[case::inner_takes_the_only_finding(
    "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"outer\"))]\nfn f(v: V) {\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"inner\"))]\n    let V { a, .. } = v;\n}\n",
    &["1:1 unfulfilled_exemption"]
)]
#[case::outer_keeps_a_finding_the_inner_does_not_cover(
    "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"outer\"))]\nfn f(v: V, w: V) {\n    #[cfg_attr(samlinter, expect(rest_pattern, reason = \"inner\"))]\n    let V { a, .. } = v;\n    let V { b, .. } = w;\n}\n",
    &[]
)]
#[case::rule_repeated_on_one_node(
    "#[cfg_attr(samlinter, expect(rest_pattern, rest_pattern, reason = \"why\"))]\nfn f(v: V) {\n    let V { a, .. } = v;\n}\n",
    &["1:1 unfulfilled_exemption"]
)]
#[case::first_of_two_attributes_on_one_node(
    "#[cfg_attr(samlinter, expect(rest_pattern, reason = \"a\"))]\n#[cfg_attr(samlinter, expect(rest_pattern, reason = \"b\"))]\nfn f(v: V) {\n    let V { a, .. } = v;\n}\n",
    &["2:1 unfulfilled_exemption"]
)]
fn each_finding_is_credited_to_the_innermost_exemption(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(exempted(text), expected);
}
