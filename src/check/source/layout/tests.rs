use super::super::{hits, render};
use super::check;
use rstest::rstest;

const CANONICAL: &str = "\
//! Module docs.

mod alpha;
mod beta;

use std::fmt;
// io is needed for the reader below.
use std::io;

#[cfg(test)]
use std::fs;

const LIMIT: u8 = 3;

fn first() {}

fn second() {}

#[cfg(test)]
mod support;

#[cfg(test)]
mod tests;
";

const TESTS_GATE: &str = "`mod tests;` needs exactly one cfg attribute, #[cfg(test)]; further conditions belong inside tests.rs";

fn layout(text: &str) -> Vec<String> {
    render(text, check)
}

#[test]
fn canonical_module_passes() {
    assert_eq!(layout(CANONICAL), Vec::<String>::new());
}

#[rstest]
#[case::use_after_code("fn f() {}\n\nuse std::fmt;\n", &["3:1 item_order"])]
#[case::each_item_after_code(
    "fn f() {}\n\nuse a;\nuse b;\n",
    &["3:1 item_order", "4:1 item_order"]
)]
#[case::mod_after_use("use std::fmt;\n\nmod alpha;\n", &["3:1 item_order"])]
#[case::code_after_tests_module("#[cfg(test)]\nmod tests;\n\nfn f() {}\n", &["4:1 item_order"])]
#[case::regular_use_after_test_use(
    "#[cfg(test)]\nuse std::fs;\n\nuse std::io;\n",
    &["4:1 item_order"]
)]
#[case::test_use_with_a_further_cfg(
    "#[cfg(unix)]\n#[cfg(test)]\nuse a;\n\nuse b;\n",
    &["5:1 item_order"]
)]
#[case::use_gated_on_test_and_more_is_a_test_use(
    "#[cfg(all(test, unix))]\nuse a;\n\nuse b;\n",
    &["4:1 item_order"]
)]
#[case::stacked_and_all_gates_are_both_test_uses(
    "#[cfg(test)]\nuse a;\n#[cfg(all(test, unix))]\nuse b;\n",
    &[]
)]
#[case::nested_all_gate_is_a_test_use(
    "#[cfg(all(unix, all(test)))]\nuse a;\n\nuse b;\n",
    &["4:1 item_order"]
)]
#[case::any_gate_is_regular("#[cfg(any(test, unix))]\nuse a;\nuse b;\n", &[])]
#[case::not_gate_is_regular("#[cfg(not(test))]\nuse a;\nuse b;\n", &[])]
#[case::all_with_not_test_is_regular("#[cfg(all(unix, not(test)))]\nuse a;\nuse b;\n", &[])]
#[case::test_with_a_value_is_regular("#[cfg(test = \"x\")]\nuse a;\nuse b;\n", &[])]
#[case::all_with_a_test_value_is_regular("#[cfg(all(test = \"x\"))]\nuse a;\nuse b;\n", &[])]
#[case::cfg_attr_on_test_is_no_test_gate(
    "#[cfg(test)]\nuse a;\n\n#[cfg_attr(test, allow(unused))]\nuse b;\n",
    &["4:1 item_order"]
)]
#[case::ungated_tests_module("fn f() {}\n\nmod tests;\n", &["3:1 item_order"])]
#[case::tests_module_gate_with_more_conditions("#[cfg(all(test, unix))]\nmod tests;\n", &["1:1 item_order"])]
#[case::tests_module_with_a_second_cfg("#[cfg(test)]\n#[cfg(unix)]\nmod tests;\n", &["1:1 item_order"])]
#[case::tests_module_with_an_earlier_cfg("#[cfg(unix)]\n#[cfg(test)]\nmod tests;\n", &["1:1 item_order"])]
#[case::tests_module_gate_with_a_value("#[cfg(test = \"x\")]\nmod tests;\n", &["1:1 item_order"])]
#[case::test_module_before_use("#[cfg(test)]\nmod helpers;\n\nuse std::io;\n", &["4:1 item_order"])]
#[case::code_after_test_module("#[cfg(test)]\nmod helpers;\n\nfn f() {}\n", &["4:1 item_order"])]
#[case::test_module_after_tests_module(
    "#[cfg(test)]\nmod tests;\n#[cfg(test)]\nmod helpers;\n",
    &["3:1 item_order"]
)]
#[case::public_test_module_at_the_end("fn f() {}\n\n#[cfg(test)]\npub mod helpers;\n", &[])]
#[case::test_and_more_module_at_the_end("fn f() {}\n#[cfg(all(test, unix))]\nmod helpers;\n", &[])]
#[case::inline_test_module_before_code(
    "#[cfg(test)]\nmod helpers {\n    fn h() {}\n}\n\nfn f() {}\n",
    &["6:1 item_order"]
)]
#[case::inner_gated_module_before_code(
    "mod helpers {\n    #![cfg(test)]\n\n    fn h() {}\n}\n\nfn f() {}\n",
    &["7:1 item_order"]
)]
#[case::inner_gated_module_at_the_end(
    "fn f() {}\n\nmod helpers {\n    #![cfg(test)]\n\n    fn h() {}\n}\n\n#[cfg(test)]\nmod tests;\n",
    &[]
)]
#[case::test_gated_function_stays_code("#[cfg(test)]\nfn t() {}\n\nfn f() {}\n", &[])]
#[case::any_gated_module_is_a_mod_declaration("#[cfg(any(test, unix))]\nmod helpers;\n\nuse a;\n", &[])]
#[case::tests_module_with_another_attribute("#[cfg(test)]\n#[path = \"t.rs\"]\nmod tests;\n", &[])]
#[case::inline_module_body("mod m {\n    fn f() {}\n\n    use std::io;\n}\n", &["4:5 item_order"])]
#[case::inline_tests_module_counts_as_code("mod tests {\n    fn t() {}\n}\n\nfn f() {}\n", &[])]
#[case::extern_crate_with_use_items("extern crate alloc;\nuse std::fmt;\n", &[])]
#[case::extern_crate_after_code("fn f() {}\n\nextern crate alloc;\n", &["3:1 item_order"])]
#[case::test_gated_extern_crate_is_a_test_use(
    "use a;\n\n#[cfg(test)]\nuse b;\n#[cfg(test)]\nextern crate c;\n",
    &[]
)]
#[case::regular_extern_crate_after_test_use(
    "#[cfg(test)]\nuse a;\n\nextern crate c;\n",
    &["4:1 item_order"]
)]
#[case::trailing_comment_of_previous_item("fn f() {} // note\nuse std::fmt;\n", &["2:1 item_order"])]
#[case::doc_comment_starts_the_item("fn f() {}\n\n/// docs\nuse std::fmt;\n", &["3:1 item_order"])]
fn flags_items_out_of_order(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(layout(text), expected);
}

#[rstest]
#[case::blank_line_inside_use_block("use std::fmt;\n\nuse std::io;\n", &["3:1 item_spacing"])]
#[case::blank_line_inside_test_use_block(
    "#[cfg(test)]\nuse a;\n\n#[cfg(test)]\nuse b;\n",
    &["4:1 item_spacing"]
)]
#[case::mods_touch_uses("mod alpha;\nuse std::io;\n", &["2:1 item_spacing"])]
#[case::mods_touch_code("mod alpha;\nfn f() {}\n", &["2:1 item_spacing"])]
#[case::uses_touch_code("use std::io;\nfn f() {}\n", &["2:1 item_spacing"])]
#[case::test_uses_touch_regular_uses(
    "use std::io;\n#[cfg(test)]\nuse std::fs;\n",
    &["2:1 item_spacing"]
)]
#[case::test_uses_touch_code("#[cfg(test)]\nuse std::fs;\nfn f() {}\n", &["3:1 item_spacing"])]
#[case::blank_line_between_mod_declarations("mod alpha;\n\nmod beta;\n", &[])]
#[case::tests_module_may_touch_code("fn f() {}\n#[cfg(test)]\nmod tests;\n", &[])]
#[case::mods_touch_tests_module("mod a;\n#[cfg(test)]\nmod tests;\n", &["2:1 item_spacing"])]
#[case::comment_line_is_not_blank("use std::fmt;\n// why io\nuse std::io;\n", &[])]
#[case::trailing_comment_of_previous_item("use std::fmt; // note\nfn f() {}\n", &["2:1 item_spacing"])]
#[case::whitespace_line_after_a_comment_inside_use_block(
    "use a;\n// c\n \nuse b;\n",
    &["4:1 item_spacing"]
)]
#[case::whitespace_line_after_a_comment_separates_groups("mod a;\n// c\n \nuse b;\n", &[])]
#[case::uses_touch_code_in_an_inline_module(
    "mod m {\n    use a;\n    fn f() {}\n}\n",
    &["3:5 item_spacing"]
)]
fn flags_blank_lines_around_mod_and_use_groups(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(layout(text), expected);
}

#[rstest]
#[case::adjacent_functions("fn a() {}\nfn b() {}\n", &["2:1 fn_spacing"])]
#[case::comment_is_not_a_blank_line("fn a() {}\n// about b\nfn b() {}\n", &["3:1 fn_spacing"])]
#[case::attribute_starts_the_function("fn a() {}\n#[inline]\nfn b() {}\n", &["2:1 fn_spacing"])]
#[case::doc_comment_starts_the_function("fn a() {}\n/// b\nfn b() {}\n", &["2:1 fn_spacing"])]
#[case::doc_comment_before_the_attribute(
    "fn a() {}\n/// b\n#[inline]\nfn b() {}\n",
    &["2:1 fn_spacing"]
)]
#[case::blank_line_belongs_above_the_doc_comment(
    "fn a() {}\n/// b\n\nfn b() {}\n",
    &["2:1 fn_spacing"]
)]
#[case::plain_comment_above_the_doc_comment(
    "fn a() {}\n// note\n/// b\nfn b() {}\n",
    &["3:1 fn_spacing"]
)]
#[case::trailing_comment_of_previous_function("fn a() {} // note\nfn b() {}\n", &["2:1 fn_spacing"])]
#[case::adjacent_methods(
    "impl S {\n    fn a(&self) {}\n    fn b(&self) {}\n}\n",
    &["3:5 fn_spacing"]
)]
#[case::adjacent_default_methods(
    "trait T {\n    fn a(&self) {}\n    fn b(&self) {}\n}\n",
    &["3:5 fn_spacing"]
)]
#[case::separated_functions("fn a() {}\n\nfn b() {}\n", &[])]
#[case::bodyless_declarations("trait T {\n    fn a(&self);\n    fn b(&self);\n}\n", &[])]
#[case::function_next_to_other_item("fn a() {}\nstruct S;\n", &[])]
#[case::associated_const_next_to_method(
    "impl S {\n    const C: u8 = 1;\n    fn a(&self) {}\n}\n",
    &[]
)]
#[case::declaration_next_to_default_method(
    "trait T {\n    fn a(&self);\n    fn b(&self) {}\n}\n",
    &[]
)]
#[case::extern_block_declarations("extern \"C\" {\n    fn a();\n    fn b();\n}\n", &[])]
#[case::comment_then_blank_line("fn a() {}\n// c\n\nfn b() {}\n", &[])]
#[case::comment_then_whitespace_line("fn a() {}\n// c\n \nfn b() {}\n", &[])]
#[case::adjacent_functions_in_an_inline_module(
    "mod m {\n    fn a() {}\n    fn b() {}\n}\n",
    &["3:5 fn_spacing"]
)]
#[case::adjacent_functions_in_a_block(
    "fn outer() {\n    fn a() {}\n    fn b() {}\n}\n",
    &["3:5 fn_spacing"]
)]
#[case::separated_functions_in_a_block("fn outer() {\n    fn a() {}\n\n    fn b() {}\n}\n", &[])]
#[case::statement_between_functions_in_a_block(
    "fn outer() {\n    fn a() {}\n    let x = 1;\n    fn b() {}\n}\n",
    &[]
)]
#[case::macro_body_is_opaque(
    "macro_rules! m {\n    () => {\n        fn a() {}\n        fn b() {}\n    };\n}\n",
    &[]
)]
#[case::block_items_have_no_group_order(
    "fn outer() {\n    fn a() {}\n\n    use std::fmt;\n    struct S;\n}\n",
    &[]
)]
fn flags_functions_without_a_blank_line_between(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(layout(text), expected);
}

#[rstest]
#[case::use_after_code("fn f() {}\n\nuse std::fmt;\n", "use item after code")]
#[case::mod_after_use("use std::fmt;\n\nmod alpha;\n", "mod declaration after use item")]
#[case::regular_use_after_test_use(
    "#[cfg(test)]\nuse std::fs;\n\nuse std::io;\n",
    "use item after #[cfg(test)] use item"
)]
#[case::code_after_tests_module(
    "#[cfg(test)]\nmod tests;\n\nfn f() {}\n",
    "code after `mod tests;`"
)]
#[case::use_after_test_module(
    "#[cfg(test)]\nmod helpers;\n\nuse std::io;\n",
    "use item after #[cfg(test)] module"
)]
#[case::ungated_tests_module("fn f() {}\n\nmod tests;\n", TESTS_GATE)]
#[case::tests_module_gate_with_more_conditions("#[cfg(all(test, unix))]\nmod tests;\n", TESTS_GATE)]
#[case::tests_module_with_a_second_cfg("#[cfg(test)]\n#[cfg(unix)]\nmod tests;\n", TESTS_GATE)]
#[case::blank_line_inside_use_block(
    "use std::fmt;\n\nuse std::io;\n",
    "blank line between consecutive use items"
)]
#[case::blank_line_inside_test_use_block(
    "#[cfg(test)]\nuse a;\n\n#[cfg(test)]\nuse b;\n",
    "blank line between consecutive #[cfg(test)] use items"
)]
#[case::mods_touch_uses(
    "mod alpha;\nuse std::io;\n",
    "no blank line between the last mod declaration and this use item"
)]
#[case::test_uses_touch_code(
    "#[cfg(test)]\nuse std::fs;\nfn f() {}\n",
    "no blank line between the last #[cfg(test)] use item and this code"
)]
#[case::mods_touch_tests_module(
    "mod a;\n#[cfg(test)]\nmod tests;\n",
    "no blank line between the last mod declaration and this `mod tests;`"
)]
#[case::adjacent_functions(
    "fn a() {}\nfn b() {}\n",
    "no blank line between this function and the previous one"
)]
fn findings_say_what_is_out_of_place(#[case] text: &str, #[case] expected: &str) {
    let messages: Vec<String> = hits(text, check)
        .into_iter()
        .map(|hit| hit.message)
        .collect();
    assert_eq!(messages, [expected]);
}
