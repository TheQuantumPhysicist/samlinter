use super::{cfg_attributes, has_test_attribute, is_bare_cfg_atom, is_test_gated, is_test_module};
use ra_ap_syntax::ast::{self, HasAttrs, HasModuleItem};
use ra_ap_syntax::{AstNode, Edition};
use rstest::rstest;

/// The first item of `text`.
fn item(text: &str) -> ast::Item {
    let parse = ast::SourceFile::parse(text, Edition::Edition2024);
    assert!(parse.errors().is_empty(), "{:?}", parse.errors());
    parse.tree().items().next().unwrap()
}

#[rstest]
#[case::bare_test("#[cfg(test)]\nfn f() {}\n", true)]
#[case::stacked("#[cfg(unix)]\n#[cfg(test)]\nfn f() {}\n", true)]
#[case::after_a_doc_comment("/// a\n#[cfg(test)]\nfn f() {}\n", true)]
#[case::inside_all("#[cfg(all(unix, test))]\nfn f() {}\n", true)]
#[case::nested_all("#[cfg(all(unix, all(test)))]\nfn f() {}\n", true)]
#[case::inside_any("#[cfg(any(unix, test))]\nfn f() {}\n", false)]
#[case::inside_not("#[cfg(not(test))]\nfn f() {}\n", false)]
#[case::with_a_value("#[cfg(test = \"x\")]\nfn f() {}\n", false)]
#[case::inside_cfg_attr("#[cfg_attr(test, cfg(test))]\nfn f() {}\n", false)]
#[case::test_attribute("#[test]\nfn f() {}\n", false)]
#[case::no_attribute("fn f() {}\n", false)]
fn gate_needs_a_cfg_that_requires_test(#[case] text: &str, #[case] expected: bool) {
    assert_eq!(is_test_gated(item(text).syntax()), expected);
}

#[test]
fn file_gate_is_its_inner_attribute() {
    let parse = ast::SourceFile::parse("#![cfg(test)]\n\nfn f() {}\n", Edition::Edition2024);
    assert!(is_test_gated(parse.tree().syntax()));
}

#[rstest]
#[case::outer("#[cfg(test)]\nmod m;\n", true)]
#[case::outer_inline("#[cfg(test)]\nmod m {}\n", true)]
#[case::inner("mod m {\n    #![cfg(test)]\n\n    fn f() {}\n}\n", true)]
#[case::inner_after_a_doc_comment("mod m {\n    //! a\n    #![cfg(test)]\n}\n", true)]
#[case::inner_other("mod m {\n    #![cfg(unix)]\n}\n", false)]
#[case::gated_item_inside("mod m {\n    #[cfg(test)]\n    fn f() {}\n}\n", false)]
#[case::ungated("mod m;\n", false)]
fn test_module_is_gated_outside_or_inside(#[case] text: &str, #[case] expected: bool) {
    let ast::Item::Module(module) = item(text) else {
        panic!("not a module: {text}");
    };
    assert_eq!(is_test_module(&module), expected);
}

#[rstest]
#[case::bare_test("#[test]\nfn f() {}\n", true)]
#[case::tokio("#[tokio::test]\nfn f() {}\n", true)]
#[case::with_arguments("#[tokio::test(flavor = \"multi_thread\")]\nfn f() {}\n", true)]
#[case::test_log("#[test_log::test]\nfn f() {}\n", true)]
#[case::after_another_attribute("#[ignore]\n#[test]\nfn f() {}\n", true)]
#[case::cfg_test("#[cfg(test)]\nfn f() {}\n", false)]
#[case::test_case("#[test_case(1)]\nfn f() {}\n", false)]
#[case::test_as_a_prefix("#[test::other]\nfn f() {}\n", false)]
#[case::no_attribute("fn f() {}\n", false)]
fn test_attribute_path_ends_in_test(#[case] text: &str, #[case] expected: bool) {
    assert_eq!(has_test_attribute(item(text).syntax()), expected);
}

#[rstest]
#[case::bare("#[cfg(samlinter)]\nfn f() {}\n", true)]
#[case::other_name("#[cfg(unix)]\nfn f() {}\n", false)]
#[case::with_a_value("#[cfg(samlinter = \"x\")]\nfn f() {}\n", false)]
#[case::composite("#[cfg(all(samlinter))]\nfn f() {}\n", false)]
fn bare_cfg_atom_has_the_name_and_no_value(#[case] text: &str, #[case] expected: bool) {
    let predicate = cfg_attributes(item(text).attrs())
        .next()
        .and_then(|cfg| cfg.cfg_predicate())
        .unwrap();
    assert_eq!(is_bare_cfg_atom(&predicate, "samlinter"), expected);
}
