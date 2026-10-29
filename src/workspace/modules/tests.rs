use super::{BadPath, Directory, FileKind, candidates, declarations};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// - Each declaration in edition-2024 `text`, found in `file` of `kind`, as
///   `line: candidate kind, ...`.
/// - Every directory counts as existing, its normalized path its path as given.
fn resolve(file: &str, kind: FileKind, text: &str) -> Vec<String> {
    resolve_in(Edition::Edition2024, file, kind, text, |dir| {
        Some(dir.to_path_buf())
    })
}

/// `resolve` for `text` of `edition`, with `existing` giving the normalized path of each
/// directory that exists.
fn resolve_in(
    edition: Edition,
    file: &str,
    kind: FileKind,
    text: &str,
    existing: impl Fn(&Path) -> Option<PathBuf>,
) -> Vec<String> {
    let file = Path::new(file);
    let directory = |dir: &Path| Ok(existing(dir).map_or(Directory::Absent, Directory::At));
    declarations(text, edition)
        .unwrap()
        .iter()
        .map(|declaration| {
            let dir = file.parent().unwrap();
            let name = file.file_name().unwrap();
            let files: Vec<String> = candidates(dir, name, kind, declaration, directory)
                .unwrap()
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
#[case::module_after_an_inline_module(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    mod n;\n}\nmod a;\n",
    &["2: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs", "4: src/a.rs NonModRs, src/a/mod.rs ModRs"]
)]
#[case::sibling_inline_modules(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    mod a;\n}\nmod k {\n    mod b;\n}\n",
    &["2: src/m/a.rs NonModRs, src/m/a/mod.rs ModRs", "5: src/k/b.rs NonModRs, src/k/b/mod.rs ModRs"]
)]
#[case::declaration_after_a_nested_inline_module(
    "src/lib.rs",
    FileKind::ModRs,
    "mod m {\n    mod k {\n        mod a;\n    }\n    mod b;\n}\n",
    &["3: src/m/k/a.rs NonModRs, src/m/k/a/mod.rs ModRs", "5: src/m/b.rs NonModRs, src/m/b/mod.rs ModRs"]
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
#[case::cfg_attr_path_naming_the_default_directory(
    "src/lib.rs",
    FileKind::ModRs,
    "#[cfg_attr(unix, path = \"m\")]\nmod m {\n    mod n;\n}\n",
    &["3: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs"]
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
#[case::module_missing_its_semicolon_then_a_valid_one(
    "src/lib.rs",
    FileKind::ModRs,
    "mod a\nmod b;\n",
    &["2: src/b.rs NonModRs, src/b/mod.rs ModRs"]
)]
#[case::nameless_module_then_a_valid_one(
    "src/lib.rs",
    FileKind::ModRs,
    "mod;\nmod b;\n",
    &["2: src/b.rs NonModRs, src/b/mod.rs ModRs"]
)]
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
        resolve_in(
            edition,
            "src/lib.rs",
            FileKind::ModRs,
            "mod gen;\n",
            |dir| { Some(dir.to_path_buf()) }
        ),
        expected
    );
}

/// - An inline module's `path` directory is entered only when it exists.
/// - Its default directory is always entered, so a missing module's error names it.
#[rstest]
#[case::no_directory_exists(
    &[],
    &["5: src/m/k/n.rs NonModRs, src/m/k/n/mod.rs ModRs"]
)]
#[case::path_directory_exists(
    &["src/u"],
    &["5: src/m/k/n.rs NonModRs, src/m/k/n/mod.rs ModRs, src/u/k/n.rs NonModRs, src/u/k/n/mod.rs ModRs"]
)]
#[case::path_directory_inside_the_default_one(
    &["src/m", "src/m/v"],
    &["5: src/m/k/n.rs NonModRs, src/m/k/n/mod.rs ModRs, src/m/v/n.rs NonModRs, src/m/v/n/mod.rs ModRs"]
)]
#[case::path_directories_at_both_levels(
    &["src/u", "src/u/v"],
    &["5: src/m/k/n.rs NonModRs, src/m/k/n/mod.rs ModRs, src/u/k/n.rs NonModRs, src/u/k/n/mod.rs ModRs, src/u/v/n.rs NonModRs, src/u/v/n/mod.rs ModRs"]
)]
fn inline_path_locations_need_an_existing_directory(
    #[case] directories: &[&str],
    #[case] expected: &[&str],
) {
    let text = "#[cfg_attr(unix, path = \"u\")]\nmod m {\n    #[cfg_attr(unix, path = \"v\")]\n    mod k {\n        mod n;\n    }\n}\n";
    let existing = |dir: &Path| {
        directories
            .iter()
            .any(|known| dir == Path::new(known))
            .then(|| dir.to_path_buf())
    };
    assert_eq!(
        resolve_in(
            Edition::Edition2024,
            "src/lib.rs",
            FileKind::ModRs,
            text,
            existing
        ),
        expected
    );
}

/// - A directory the lookup gives no answer for is entered as written, a `cfg_attr` one too.
/// - So each candidate file below it gets the lookup's answer in turn.
#[test]
fn unanswered_directory_is_entered_as_written() {
    let text = "#[cfg_attr(unix, path = \"../u\")]\nmod m {\n    mod n;\n}\n";
    let declarations = declarations(text, Edition::Edition2024).unwrap();
    let declaration = declarations.first().unwrap();
    let directory = |dir: &Path| {
        Ok(if dir == Path::new("src/../u") {
            Directory::AsWritten
        } else {
            Directory::Absent
        })
    };
    let files: Vec<String> = candidates(
        Path::new("src"),
        OsStr::new("lib.rs"),
        FileKind::ModRs,
        declaration,
        directory,
    )
    .unwrap()
    .iter()
    .map(|candidate| candidate.path.display().to_string())
    .collect();
    assert_eq!(
        files,
        [
            "src/m/n.rs",
            "src/m/n/mod.rs",
            "src/../u/n.rs",
            "src/../u/n/mod.rs"
        ]
    );
}

/// - An existing directory is named by the normalized path the lookup gives, and entered once.
/// - `directories`: each existing directory as written, with its normalized path.
#[rstest]
#[case::cfg_attr_directory_is_named_normalized(
    "x/../u",
    &[("src/x/../u", "src/u")],
    &["3: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs, src/u/n.rs NonModRs, src/u/n/mod.rs ModRs"]
)]
#[case::two_spellings_of_one_directory_are_entered_once(
    "../src/m",
    &[("src/m", "src/m"), ("src/../src/m", "src/m")],
    &["3: src/m/n.rs NonModRs, src/m/n/mod.rs ModRs"]
)]
fn existing_directories_are_named_by_their_normalized_path(
    #[case] path: &str,
    #[case] directories: &[(&str, &str)],
    #[case] expected: &[&str],
) {
    let text = format!("#[cfg_attr(unix, path = \"{path}\")]\nmod m {{\n    mod n;\n}}\n");
    let existing = |dir: &Path| {
        directories
            .iter()
            .find(|(known, _)| dir == Path::new(known))
            .map(|(_, normalized)| PathBuf::from(normalized))
    };
    assert_eq!(
        resolve_in(
            Edition::Edition2024,
            "src/lib.rs",
            FileKind::ModRs,
            &text,
            existing
        ),
        expected
    );
}

/// A fixed `path` replaces the default location, so it is entered even when it is missing.
#[rstest]
#[case::outer_path("#[path = \"other\"]\nmod m {\n    mod n;\n}\n")]
#[case::inner_path("mod m {\n    #![path = \"other\"]\n    mod n;\n}\n")]
fn fixed_inline_path_is_entered_without_a_directory(#[case] text: &str) {
    assert_eq!(
        resolve_in(
            Edition::Edition2024,
            "src/lib.rs",
            FileKind::ModRs,
            text,
            |_| None
        ),
        ["3: src/other/n.rs NonModRs, src/other/n/mod.rs ModRs"]
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
