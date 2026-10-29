use super::{DROP_THREAD_DEPTH, Depth};
use ra_ap_syntax::{AstNode, Edition, ast};
use rstest::rstest;

/// - `levels` nested inline modules: the source file, then a module and its item list per level.
/// - So `DROP_THREAD_DEPTH / 2 - 1` levels sit one below the limit, `DROP_THREAD_DEPTH / 2` one
///   past it.
fn nested_modules(levels: usize) -> String {
    format!("{}{}", "mod m {\n".repeat(levels), "}\n".repeat(levels))
}

#[rstest]
#[case::ordinary_file("pub fn f(x: u8) -> u8 {\n    x + 1\n}\n".to_owned(), Depth::Shallow)]
#[case::just_below_the_limit(nested_modules(DROP_THREAD_DEPTH / 2 - 1), Depth::Shallow)]
#[case::just_past_the_limit(nested_modules(DROP_THREAD_DEPTH / 2), Depth::Deep)]
fn depth_against_the_drop_thread(#[case] text: String, #[case] expected: Depth) {
    let parse = ast::SourceFile::parse(&text, Edition::Edition2024);
    assert_eq!(Depth::of(parse.tree().syntax()), expected);
}
