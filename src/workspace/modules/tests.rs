// - TODO: randomized variant: random module names, inline nesting and `#[path]` values:
//   `candidates` lists the files the module path rules give for mod-rs and non-mod-rs files.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{BadPath, FileKind, candidates, declarations};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::path::Path;

/// Each declaration in edition-2024 `text`, found in `file` of `kind`, as
/// `line: candidate kind, ...`.
fn resolve(file: &str, kind: FileKind, text: &str) -> Vec<String> {
    resolve_in(Edition::Edition2024, file, kind, text)
}

/// `resolve` for `text` of `edition`.
fn resolve_in(edition: Edition, file: &str, kind: FileKind, text: &str) -> Vec<String> {
    let file = Path::new(file);
    declarations(text, edition)
        .unwrap()
        .iter()
        .map(|declaration| {
            let dir = file.parent().unwrap();
            let name = file.file_name().unwrap();
            let files: Vec<String> = candidates(dir, name, kind, declaration)
                .iter()
                .map(|candidate| format!("{} {:?}", candidate.path.display(), candidate.kind))
                .collect();
            format!("{}: {}", declaration.line, files.join(", "))
        })
        .collect()
}

#[rstest]
#[case::mod_rs_file(
    "src/lib.rs",
    FileKind::ModRs,
    "mod a;\n",
    &["1: src/a.rs NonModRs, src/a/mod.rs ModRs"]
)]
#[case::mod_rs_named_mod(
    "src/c/mod.rs",
    FileKind::ModRs,
    "mod d;\n",
    &["1: src/c/d.rs NonModRs, src/c/d/mod.rs ModRs"]
)]
#[case::non_mod_rs_file(
    "src/a.rs",
    FileKind::NonModRs,
    "mod b;\n",
    &["1: src/a/b.rs NonModRs, src/a/b/mod.rs ModRs"]
)]
#[case::raw_identifier(
    "src/lib.rs",
    FileKind::ModRs,
    "mod r#match;\n",
    &["1: src/match.rs NonModRs, src/match/mod.rs ModRs"]
)]
#[case::path_in_mod_rs_file(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"elsewhere/p.rs\"]\nmod p;\n",
    &["2: src/elsewhere/p.rs ModRs"]
)]
#[case::absolute_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"/abs/x.rs\"]\nmod x;\n",
    &["2: /abs/x.rs ModRs"]
)]
#[case::path_in_non_mod_rs_file_is_beside_it(
    "src/a.rs",
    FileKind::NonModRs,
    "#[path = \"x.rs\"]\nmod b;\n",
    &["2: src/x.rs ModRs"]
)]
#[case::raw_string_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = r\"x.rs\"]\nmod b;\n",
    &["2: src/x.rs ModRs"]
)]
#[case::inline_module_in_mod_rs_file(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    mod n;\n}\n",
    &["2: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs"]
)]
#[case::inline_module_in_non_mod_rs_file(
    "src/a.rs",
    FileKind::NonModRs,
    "mod m {\n    mod n;\n}\n",
    &["2: src/a/m/n.rs NonModRs, src/a/m/n/mod.rs ModRs"]
)]
#[case::nested_inline_modules(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    mod k {\n        mod n;\n    }\n}\n",
    &["3: src/m/k/n.rs NonModRs, src/m/k/n/mod.rs ModRs"]
)]
#[case::path_inside_inline_module_in_mod_rs_file(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    #[path = \"x.rs\"]\n    mod n;\n}\n",
    &["3: src/m/x.rs ModRs"]
)]
#[case::path_inside_inline_module_in_non_mod_rs_file(
    "src/a.rs",
    FileKind::NonModRs,
    "mod m {\n    #[path = \"x.rs\"]\n    mod n;\n}\n",
    &["3: src/a/m/x.rs ModRs"]
)]
#[case::inline_module_with_a_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"other\"]\nmod m {\n    mod n;\n}\n",
    &["3: src/other/n.rs NonModRs, src/other/n/mod.rs ModRs"]
)]
#[case::cfg_attr_paths_add_candidates(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"u.rs\")]\n#[cfg_attr(windows, path = \"w.rs\")]\nmod imp;\n",
    &["3: src/imp.rs NonModRs, src/imp/mod.rs ModRs, src/u.rs ModRs, src/w.rs ModRs"]
)]
#[case::nested_cfg_attr_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, cfg_attr(test, path = \"u.rs\"))]\nmod imp;\n",
    &["2: src/imp.rs NonModRs, src/imp/mod.rs ModRs, src/u.rs ModRs"]
)]
#[case::cfg_attr_path_before_a_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"u.rs\")]\n#[path = \"p.rs\"]\nmod imp;\n",
    &["3: src/p.rs ModRs, src/u.rs ModRs"]
)]
#[case::path_after_a_path_is_ignored(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"p.rs\"]\n#[cfg_attr(unix, path = \"u.rs\")]\nmod imp;\n",
    &["3: src/p.rs ModRs"]
)]
#[case::second_path_in_one_cfg_attr_is_ignored(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"u.rs\", path = \"v.rs\")]\nmod imp;\n",
    &["2: src/imp.rs NonModRs, src/imp/mod.rs ModRs, src/u.rs ModRs"]
)]
#[case::cfg_attr_path_on_inline_module(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod n;\n}\n",
    &["3: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs, src/u/n.rs NonModRs, src/u/n/mod.rs ModRs"]
)]
#[case::inner_path_in_inline_module(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    #![path = \"other\"]\n    mod n;\n}\n",
    &["3: src/other/n.rs NonModRs, src/other/n/mod.rs ModRs"]
)]
#[case::outer_path_before_inner_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"outer\"]\nmod m {\n    #![path = \"inner\"]\n    mod n;\n}\n",
    &["4: src/outer/n.rs NonModRs, src/outer/n/mod.rs ModRs"]
)]
#[case::outer_cfg_attr_path_then_inner_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"u\")]\nmod m {\n    #![path = \"inner\"]\n    mod n;\n}\n",
    &["4: src/inner/n.rs NonModRs, src/inner/n/mod.rs ModRs, src/u/n.rs NonModRs, src/u/n/mod.rs ModRs"]
)]
#[case::inline_module_with_a_path_in_non_mod_rs_file(
    "src/a.rs",
    FileKind::NonModRs,
    "#[path = \"x\"]\nmod m {\n    mod n;\n}\n",
    &["3: src/x/n.rs NonModRs, src/x/n/mod.rs ModRs"]
)]
#[case::path_on_inline_module_nested_in_non_mod_rs_file(
    "src/a.rs",
    FileKind::NonModRs,
    "mod k {\n    #[path = \"y\"]\n    mod j {\n        mod n;\n    }\n}\n",
    &["4: src/a/k/y/n.rs NonModRs, src/a/k/y/n/mod.rs ModRs"]
)]
#[case::line_of_the_mod_keyword(
    "src/lib.rs",
    FileKind::ModRs,
    "mod a; // note\n/// Docs.\n#[cfg(unix)]\nmod b;\n",
    &["1: src/a.rs NonModRs, src/a/mod.rs ModRs", "4: src/b.rs NonModRs, src/b/mod.rs ModRs"]
)]
#[case::other_key_value_attribute_is_not_a_path(
    "src/lib.rs",
    FileKind::ModRs,
    "#[doc = \"x\"]\nmod a;\n",
    &["2: src/a.rs NonModRs, src/a/mod.rs ModRs"]
)]
#[case::leading_bom(
    "src/lib.rs",
    FileKind::ModRs,
    "\u{feff}mod a;\n",
    &["1: src/a.rs NonModRs, src/a/mod.rs ModRs"]
)]
#[case::empty_file("src/lib.rs", FileKind::ModRs, "", &[])]
#[case::nameless_module("src/lib.rs", FileKind::ModRs, "mod;\n", &[])]
#[case::module_missing_its_semicolon("src/lib.rs", FileKind::ModRs, "mod a\n", &[])]
#[case::module_in_a_function_body(
    "src/lib.rs",
    FileKind::ModRs,
    "fn f() {\n    mod x;\n}\n",
    &[]
)]
#[case::module_in_a_macro_rules_body(
    "src/lib.rs",
    FileKind::ModRs,
    "macro_rules! m {\n    () => {\n        mod x;\n    };\n}\n",
    &[]
)]
#[case::module_in_a_macro_call_body("src/lib.rs", FileKind::ModRs, "m! {\n    mod x;\n}\n", &[])]
#[case::test_module_followed_like_any_other(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg(test)]\nmod tests;\n",
    &["2: src/tests.rs NonModRs, src/tests/mod.rs ModRs"]
)]
// rustc rejects the second attribute, but the module's file is already known.
#[case::bad_path_after_the_fixed_one_is_never_read(
    "src/lib.rs",
    FileKind::ModRs,
    "#[path = \"p.rs\"]\n#[path = 5]\nmod x;\n",
    &["3: src/p.rs ModRs"]
)]
#[case::other_items_ignored(
    "src/lib.rs",
    FileKind::ModRs,
    "use std::fmt;\n\nfn f() {\n    let x = 1;\n}\n\nmod m {}\n",
    &[]
)]
fn resolves_module_files(
    #[case] file: &str,
    #[case] kind: FileKind,
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(resolve(file, kind, text), expected);
}

#[rstest]
#[case::identifier_before_2024(
    Edition::Edition2021,
    &["1: src/gen.rs NonModRs, src/gen/mod.rs ModRs"]
)]
#[case::keyword_from_2024(Edition::Edition2024, &[])]
fn module_names_follow_the_edition(#[case] edition: Edition, #[case] expected: &[&str]) {
    assert_eq!(
        resolve_in(edition, "src/lib.rs", FileKind::ModRs, "mod gen;\n"),
        expected
    );
}

#[rstest]
#[case::not_a_string("mod a;\n#[path = 5]\nmod b;\n", 2)]
#[case::invalid_escape("#[cfg_attr(unix, path = \"\\q\")]\nmod b;\n", 1)]
#[case::line_of_the_attribute("#[cfg(unix)]\n#[path = 5]\nmod b;\n", 2)]
#[case::line_of_the_cfg_attr("#[cfg(unix)]\n#[cfg_attr(unix, path = 5)]\nmod b;\n", 2)]
#[case::inner_path_in_inline_module("mod m {\n    #![path = 5]\n    mod n;\n}\n", 2)]
fn rejects_a_path_that_is_not_a_string(#[case] text: &str, #[case] line: usize) {
    assert_eq!(
        declarations(text, Edition::Edition2024).unwrap_err(),
        BadPath { line }
    );
}
