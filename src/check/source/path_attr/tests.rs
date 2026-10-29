use super::super::{hits, render};
use super::check;
use ra_ap_syntax::AstNode;
use rstest::rstest;

fn path_attrs(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax()).collect())
}

/// One finding per `path = ..`, at its `path` key, whatever the attribute sits on.
#[rstest]
#[case::outer("#[path = \"a.rs\"]\nmod a;\n", &["1:3 path_attr"])]
#[case::inner_in_an_inline_module(
    "mod m {\n    #![path = \"d\"]\n    mod n;\n}\n",
    &["2:8 path_attr"]
)]
#[case::inner_at_the_file_top("#![path = \"x\"]\nfn f() {}\n", &["1:4 path_attr"])]
#[case::cfg_attr("#[cfg_attr(unix, path = \"sys_unix.rs\")]\nmod sys;\n", &["1:18 path_attr"])]
#[case::nested_cfg_attr(
    "#[cfg_attr(unix, cfg_attr(test, path = \"t.rs\"))]\nmod sys;\n",
    &["1:33 path_attr"]
)]
#[case::inner_cfg_attr(
    "mod m {\n    #![cfg_attr(unix, path = \"u\")]\n    mod n;\n}\n",
    &["2:23 path_attr"]
)]
#[case::cfg_attr_with_other_attributes(
    "#[cfg_attr(unix, allow(dead_code), path = \"u.rs\")]\nmod u;\n",
    &["1:36 path_attr"]
)]
#[case::two_in_one_cfg_attr(
    "#[cfg_attr(unix, path = \"a.rs\", path = \"b.rs\")]\nmod a;\n",
    &["1:18 path_attr", "1:33 path_attr"]
)]
#[case::each_stacked_attribute(
    "#[cfg_attr(unix, path = \"u.rs\")]\n#[path = \"p.rs\"]\nmod p;\n",
    &["1:18 path_attr", "2:3 path_attr"]
)]
#[case::on_another_item("#[path = \"x\"]\nfn f() {}\n", &["1:3 path_attr"])]
#[case::in_a_function_body(
    "fn f() {\n    #[path = \"x.rs\"]\n    mod x;\n}\n",
    &["2:7 path_attr"]
)]
#[case::raw_identifier_key("#[r#path = \"a.rs\"]\nmod a;\n", &["1:3 path_attr"])]
#[case::not_a_string("#[path = 5]\nmod x;\n", &["1:3 path_attr"])]
// A `path = 5` never reaches the CLI: the module walk errors on the non-string path first,
// like rustc, so the unit case is the only coverage this form gets.
#[case::cfg_attr_without_path("#[cfg_attr(unix, derive(Debug))]\nstruct S;\n", &[])]
#[case::doc_naming_path("#[doc = \"path\"]\nfn f() {}\n", &[])]
#[case::cfg_predicate_named_path("#[cfg(path = \"x\")]\nfn f() {}\n", &[])]
#[case::qualified_key("#[tool::path = \"x\"]\nfn f() {}\n", &[])]
#[case::exemption(
    "#[cfg_attr(samlinter, expect(path_attr, reason = \"why\"))]\nfn f() {}\n",
    &[]
)]
#[case::macro_rules_body_is_opaque(
    "macro_rules! m {\n    () => {\n        #[path = \"x.rs\"]\n        mod x;\n    };\n}\n",
    &[]
)]
#[case::macro_call_body_is_opaque("m! {\n    #[path = \"x.rs\"]\n    mod x;\n}\n", &[])]
fn flags_every_path_attribute(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(path_attrs(text), expected);
}

#[test]
fn message_names_the_attribute() {
    let messages: Vec<String> = hits("#[path = \"a.rs\"]\nmod a;\n", |file| {
        check(file.syntax()).collect()
    })
    .into_iter()
    .map(|hit| hit.message)
    .collect();
    assert_eq!(
        messages,
        ["`path` attribute moves the module away from the file its module path names"]
    );
}
