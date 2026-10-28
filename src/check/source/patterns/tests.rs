// - TODO: randomized variant: a random record, tuple or tuple-struct pattern with n fields
//   and an optional rest at a random index gives one hit, at the rest, only when the rest is
//   present.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::super::render;
use super::check;
use ra_ap_syntax::AstNode;
use rstest::rstest;

fn rest_patterns(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax()).collect())
}

#[rstest]
#[case::record("fn f(v: V) {\n    let V { a, .. } = v;\n}\n", &["2:16 rest_pattern"])]
#[case::tuple_struct("fn f(v: V) {\n    let V(a, ..) = v;\n}\n", &["2:14 rest_pattern"])]
#[case::tuple("fn f(t: (u8, u8, u8)) {\n    let (a, ..) = t;\n}\n", &["2:13 rest_pattern"])]
#[case::tuple_of_only_a_rest("fn f(t: (u8, u8)) {\n    let (..) = t;\n}\n", &["2:10 rest_pattern"])]
#[case::tuple_struct_of_only_a_rest("fn f(v: V) {\n    let V(..) = v;\n}\n", &["2:11 rest_pattern"])]
#[case::nested(
    "fn f(o: Option<V>) {\n    if let Some(V { .. }) = o {}\n}\n",
    &["2:21 rest_pattern"]
)]
#[case::match_arm(
    "fn f(e: E) {\n    match e {\n        E::A { .. } => {}\n        E::B => {}\n    }\n}\n",
    &["3:16 rest_pattern"]
)]
#[case::or_pattern(
    "fn f(e: E) {\n    match e {\n        E::A { .. } | E::B { .. } => {}\n    }\n}\n",
    &["3:16 rest_pattern", "3:30 rest_pattern"]
)]
#[case::tuple_inside_slice(
    "fn f(s: &[(u8, u8)]) {\n    if let [(a, ..), ..] = s {}\n}\n",
    &["2:17 rest_pattern"]
)]
#[case::fully_bound("fn f(v: V) {\n    let V { a, b: _ } = v;\n}\n", &[])]
#[case::slice(
    "fn f(s: &[u8]) {\n    if let [first, ..] = s {}\n    if let [head, tail @ ..] = s {}\n}\n",
    &[]
)]
#[case::struct_update_expression("fn f(base: V) -> V {\n    V { a: 1, ..base }\n}\n", &[])]
#[case::range("fn f() {\n    for _ in 0..5 {}\n}\n", &[])]
// rustc rejects a base on the left of `=`; samlinter leaves that error to it.
#[case::struct_update_base_in_an_assignment(
    "fn f(v: V, b: V) {\n    let a;\n    V { a, ..b } = v;\n}\n",
    &[]
)]
#[case::destructuring_assignment_record(
    "fn f(v: V) {\n    let a;\n    V { a, .. } = v;\n}\n",
    &["3:12 rest_pattern"]
)]
#[case::destructuring_assignment_tuple_struct(
    "fn f(v: V) {\n    let a;\n    V(a, ..) = v;\n}\n",
    &["3:10 rest_pattern"]
)]
#[case::destructuring_assignment_tuple(
    "fn f(t: (u8, u8)) {\n    let a;\n    (a, ..) = t;\n}\n",
    &["3:9 rest_pattern"]
)]
#[case::destructuring_assignment_parenthesized(
    "fn f(t: (u8, u8)) {\n    let a;\n    ((a, ..)) = t;\n}\n",
    &["3:10 rest_pattern"]
)]
#[case::destructuring_assignment_parenthesized_rest(
    "fn f(t: (u8, u8)) {\n    (..) = t;\n}\n",
    &["2:6 rest_pattern"]
)]
#[case::destructuring_assignment_doubly_parenthesized_rest(
    "fn f(t: (u8, u8)) {\n    ((..)) = t;\n}\n",
    &["2:7 rest_pattern"]
)]
#[case::destructuring_assignment_parenthesized_rest_in_tuple(
    "fn f(t: (u8, (u8, u8))) {\n    let a;\n    (a, (..)) = t;\n}\n",
    &["3:10 rest_pattern"]
)]
#[case::destructuring_assignment_parenthesized_rest_in_array(
    "fn f(s: [(u8, u8); 2]) {\n    [(..), _] = s;\n}\n",
    &["2:7 rest_pattern"]
)]
#[case::destructuring_assignment_parenthesized_rest_in_field(
    "fn f(s: S) {\n    S { f: (..) } = s;\n}\n",
    &["2:13 rest_pattern"]
)]
#[case::destructuring_assignment_bare_rest_is_a_range(
    "fn f(s: S) {\n    .. = s;\n    [..] = s;\n    S { f: .. } = s;\n}\n",
    &[]
)]
#[case::destructuring_assignment_partial_range_is_not_a_rest(
    "fn f(t: (u8, u8)) {\n    let a;\n    (a, 1..) = t;\n    (1..) = t;\n}\n",
    &[]
)]
#[case::destructuring_assignment_nested(
    "fn f(v: V, w: W) {\n    let a;\n    (V { a, .. }, W(_, ..)) = (v, w);\n}\n",
    &["3:13 rest_pattern", "3:24 rest_pattern"]
)]
#[case::destructuring_assignment_field(
    "fn f(v: V) {\n    let a;\n    V { a: (a, ..), b: _ } = v;\n}\n",
    &["3:16 rest_pattern"]
)]
#[case::destructuring_assignment_tuple_inside_array(
    "fn f(s: [(u8, u8); 2]) {\n    let a;\n    [(a, ..), _] = s;\n}\n",
    &["3:10 rest_pattern"]
)]
#[case::destructuring_assignment_array_rest(
    "fn f(s: [u8; 2]) {\n    let a;\n    [a, ..] = s;\n}\n",
    &[]
)]
#[case::range_full_value_is_not_an_assignee(
    "fn f() {\n    let t = (1, ..);\n    g(..);\n}\n",
    &[]
)]
#[case::right_side_of_an_assignment_is_not_an_assignee(
    "fn f() {\n    let t;\n    t = (1, ..);\n}\n",
    &[]
)]
#[case::compound_assignment_does_not_destructure(
    "fn f(t: T) {\n    (a, ..) += t;\n}\n",
    &[]
)]
#[case::macro_body_is_opaque("fn f(v: V) -> bool {\n    matches!(v, V { .. })\n}\n", &[])]
fn flags_rest_patterns_that_absorb_fields(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(rest_patterns(text), expected);
}
