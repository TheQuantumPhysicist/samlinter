use super::{DROP_THREAD_DEPTH, Depth, ModuleBody, module_body, module_items, parse, unraw};
use ra_ap_syntax::{
    AstNode, Edition, ast,
    ast::{HasModuleItem, HasName},
};
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

/// `unraw`: one `r#` prefix off, whichever spelling rustc accepted.
#[rstest]
#[case::bare("path", "path")]
#[case::raw("r#path", "path")]
#[case::double_raw("r#r#path", "r#path")]
fn unraw_takes_one_prefix(#[case] text: &str, #[case] expected: &str) {
    assert_eq!(unraw(text), expected);
}

/// `module_body`: how a `mod` item gives its contents, `None` only where the parser errors.
#[rstest]
#[case::out_of_line("mod a;\n", "OutOfLine")]
#[case::inline("mod a {\n    fn f() {}\n}\n", "Inline")]
#[case::malformed("mod a\n", "None")]
fn body_of_a_mod_item(#[case] text: &str, #[case] expected: &str) {
    let (file, _) = parse(text, Edition::Edition2024);
    let module = module_items(file.items())
        .next()
        .expect("the text holds one `mod` item");
    let body = match module_body(&module) {
        Some(ModuleBody::Inline(_)) => "Inline",
        Some(ModuleBody::OutOfLine) => "OutOfLine",
        None => "None",
    };
    assert_eq!(body, expected);
}

/// `module_items`: only the `mod` items, whatever else the file holds.
#[test]
fn module_items_filter_the_rest() {
    let (file, _) = parse(
        "use std::fmt;\nfn f() {}\nmod a;\nstruct S;\n",
        Edition::Edition2024,
    );
    let names: Vec<String> = module_items(file.items())
        .map(|module| {
            module
                .name()
                .expect("a module item has a name")
                .text()
                .to_owned()
        })
        .collect();
    assert_eq!(names, vec!["a"]);
}
