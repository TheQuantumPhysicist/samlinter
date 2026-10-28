// - TODO: randomized variant: a random visibility on a random item kind gives a hit only for
//   `pub(crate)`, `pub(super)` and `pub(in ..)`.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::super::{hits, render};
use super::check;
use ra_ap_syntax::AstNode;
use rstest::rstest;

fn restricted(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax()).collect())
}

#[rstest]
#[case::crate_visibility("pub(crate) fn f() {}\n", &["1:1 restricted_visibility"])]
#[case::super_visibility("mod m {\n    pub(super) struct S;\n}\n", &["2:5 restricted_visibility"])]
#[case::in_path("pub(in crate::m) fn f() {}\n", &["1:1 restricted_visibility"])]
#[case::tuple_field("struct S(pub(crate) u8);\n", &["1:10 restricted_visibility"])]
#[case::named_field("struct S {\n    pub(super) a: u8,\n}\n", &["2:5 restricted_visibility"])]
#[case::plain_pub("pub fn f() {}\n", &[])]
#[case::self_is_plain_privacy("pub(self) fn f() {}\n", &[])]
#[case::in_self_is_plain_privacy("pub(in self) fn f() {}\n", &[])]
#[case::in_super("pub(in super) fn f() {}\n", &["1:1 restricted_visibility"])]
#[case::private("fn f() {}\n", &[])]
#[case::macro_body_is_opaque(
    "macro_rules! m {\n    () => {\n        pub(crate) fn f() {}\n    };\n}\n",
    &[]
)]
fn flags_visibility_that_bypasses_the_hierarchy(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(restricted(text), expected);
}

#[rstest]
#[case::crate_visibility("pub(crate) fn f() {}\n", "`pub(crate)` bypasses the module hierarchy")]
#[case::super_visibility(
    "mod m {\n    pub(super) struct S;\n}\n",
    "`pub(super)` bypasses the module hierarchy"
)]
#[case::in_path(
    "pub(in crate::m) fn f() {}\n",
    "`pub(in crate::m)` bypasses the module hierarchy"
)]
fn message_quotes_the_visibility(#[case] text: &str, #[case] expected: &str) {
    let messages: Vec<String> = hits(text, |file| check(file.syntax()).collect())
        .into_iter()
        .map(|hit| hit.message)
        .collect();
    assert_eq!(messages, [expected]);
}
