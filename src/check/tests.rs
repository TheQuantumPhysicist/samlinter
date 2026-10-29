use super::{PROFILES, run, text_file};
use crate::config::{Config, words};
use crate::workspace::{
    Dependency, Excluded, Package, PackageKind, Readme, Root, Section, TextFile, Workspace,
};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::collections::BTreeSet;
use std::path::PathBuf;

const PROSE: &str = "# lib\n\nSee NOTES.md \u{2014} done \u{1F389}\n";

/// No README; output shows `expected` as where it belongs.
fn missing(expected: &str) -> Readme {
    Readme::Missing {
        expected: PathBuf::from(expected),
    }
}

/// A config file; `run` takes the parsed config from `Workspace.settings` instead.
fn config_file() -> TextFile {
    text_file("samlinter.toml", "")
}

fn package(dir: &str, kind: PackageKind, source: &str) -> Package {
    let source_name = match kind {
        PackageKind::Binary => "src/main.rs",
        PackageKind::Library => "src/lib.rs",
    };
    Package {
        manifest: text_file(
            &format!("{dir}Cargo.toml"),
            "[package]\nname = \"p\"\n\n[dependencies]\nanyhow = \"1\"\n",
        ),
        kind,
        readme: missing(&format!("{dir}README.md")),
        edition: Edition::Edition2024,
        dependencies: Vec::new(),
        sources: vec![text_file(&format!("{dir}{source_name}"), source)],
        library_code: BTreeSet::new(),
    }
}

/// A virtual workspace whose one library member holds `source` as `lib/src/lib.rs`.
fn workspace(edition: Edition, source: &str) -> Workspace {
    Workspace {
        config: config_file(),
        settings: config(),
        excluded: Excluded::default(),
        root_dir: PathBuf::from("/workspace"),
        warnings: Vec::new(),
        root: Root::Virtual {
            manifest: text_file("Cargo.toml", PROFILES),
            readme: missing("README.md"),
        },
        members: vec![Package {
            manifest: text_file("lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
            kind: PackageKind::Library,
            readme: missing("lib/README.md"),
            edition,
            dependencies: Vec::new(),
            sources: vec![text_file("lib/src/lib.rs", source)],
            library_code: BTreeSet::new(),
        }],
    }
}

fn config() -> Config {
    Config {
        forbidden_words: words(&["NOTES.md"]),
        subscriber_crates: Vec::new(),
    }
}

fn lines(workspace: &Workspace) -> Vec<String> {
    run(workspace)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[rstest]
#[case::identifier_before_2024(Edition::Edition2021, &[])]
#[case::keyword_from_2024(
    Edition::Edition2024,
    &["lib/src/lib.rs:2:9: syntax_error: expected pattern"]
)]
fn sources_parse_with_their_package_edition(#[case] edition: Edition, #[case] expected: &[&str]) {
    assert_eq!(
        lines(&workspace(edition, "fn f() {\n    let gen = 1;\n}\n")),
        expected
    );
}

#[test]
fn identical_findings_print_once() {
    let source =
        "#[cfg_attr(samlinter, expect(rest_pattern, rest_pattern, reason = \"why\"))]\nfn f() {}\n";
    assert_eq!(
        lines(&workspace(Edition::Edition2024, source)),
        [
            "lib/src/lib.rs:1:1: unfulfilled_exemption: exemption for `rest_pattern` matches nothing here"
        ]
    );
}

#[test]
fn member_readme_is_checked_for_prose() {
    let workspace = workspace(Edition::Edition2024, "");
    let members = workspace
        .members
        .into_iter()
        .map(|member| Package {
            readme: Readme::Found(text_file("lib/README.md", PROSE)),
            ..member
        })
        .collect();
    assert_eq!(
        lines(&Workspace {
            members,
            ..workspace
        }),
        [
            "lib/README.md:3:5: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
            "lib/README.md:3:14: em_dash: em-dash (U+2014)",
            "lib/README.md:3:21: emoji: emoji U+1F389",
        ]
    );
}

#[test]
fn virtual_root_readme_is_checked_for_prose() {
    let workspace = Workspace {
        root: Root::Virtual {
            manifest: text_file("Cargo.toml", PROFILES),
            readme: Readme::Found(text_file("README.md", PROSE)),
        },
        ..workspace(Edition::Edition2024, "")
    };
    assert_eq!(
        lines(&workspace),
        [
            "README.md:3:5: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
            "README.md:3:14: em_dash: em-dash (U+2014)",
            "README.md:3:21: emoji: emoji U+1F389",
        ]
    );
}

#[test]
fn config_comments_are_checked_for_prose() {
    let workspace = Workspace {
        config: text_file(
            "samlinter.toml",
            "# See NOTES.md\nforbidden-words = [\"NOTES.md\"]\nsubscriber-crates = []\n",
        ),
        ..workspace(Edition::Edition2024, "")
    };
    assert_eq!(
        lines(&workspace),
        [
            "samlinter.toml:1:7: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml"
        ]
    );
}

#[test]
fn leading_bom_takes_no_column_in_readme_and_manifest() {
    let workspace = workspace(Edition::Edition2024, "");
    let members = workspace
        .members
        .into_iter()
        .map(|member| Package {
            manifest: text_file("lib/Cargo.toml", "\u{feff}[features]\n"),
            ..member
        })
        .collect();
    let workspace = Workspace {
        root: Root::Virtual {
            manifest: text_file("Cargo.toml", PROFILES),
            readme: Readme::Found(text_file("README.md", "\u{feff}a \u{2014}\n")),
        },
        members,
        ..workspace
    };
    assert_eq!(
        lines(&workspace),
        [
            "README.md:1:3: em_dash: em-dash (U+2014)",
            "lib/Cargo.toml:1:2: feature_flags: `[features]` table",
        ]
    );
}

/// A root binary package holding `source` as `src/main.rs`, with an aborting profile.
fn root_package(source: &str) -> Package {
    Package {
        manifest: text_file(
            "Cargo.toml",
            &format!("[package]\nname = \"p\"\n\n[dependencies]\nanyhow = \"1\"\n\n{PROFILES}"),
        ),
        ..package("", PackageKind::Binary, source)
    }
}

#[test]
fn root_package_gets_root_rules_and_its_files_checked() {
    let root = root_package("fn main() {}\n\nfn f(v: V) {\n    let V { a, .. } = v;\n}\n");
    let workspace = Workspace {
        root: Root::Package(root),
        members: vec![package(
            "member/",
            PackageKind::Library,
            "pub(crate) fn g() {}\n",
        )],
        ..workspace(Edition::Edition2024, "")
    };
    assert_eq!(
        lines(&workspace),
        [
            "Cargo.toml: binary_readme: binary package has no README at README.md",
            "member/Cargo.toml:5:1: member_version: `anyhow` declares its version here; move the version to the root `[workspace.dependencies]` and write `anyhow = { workspace = true }`",
            "member/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
            "src/main.rs:4:16: rest_pattern: `..` absorbs every field this pattern does not name",
        ]
    );
}

#[test]
fn file_outside_the_root_shows_its_absolute_path() {
    let workspace = Workspace {
        members: vec![package("/elsewhere/", PackageKind::Library, "")],
        ..workspace(Edition::Edition2024, "")
    };
    assert_eq!(
        lines(&workspace),
        [
            "/elsewhere/Cargo.toml:5:1: member_version: `anyhow` declares its version here; move the version to the root `[workspace.dependencies]` and write `anyhow = { workspace = true }`"
        ]
    );
}

#[test]
fn root_package_readme_is_checked_for_prose() {
    let workspace = Workspace {
        root: Root::Package(Package {
            readme: Readme::Found(text_file("README.md", PROSE)),
            ..root_package("fn main() {}\n")
        }),
        members: Vec::new(),
        ..workspace(Edition::Edition2024, "")
    };
    assert_eq!(
        lines(&workspace),
        [
            "README.md:3:5: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
            "README.md:3:14: em_dash: em-dash (U+2014)",
            "README.md:3:21: emoji: emoji U+1F389",
        ]
    );
}

const INSTALL: &str = "fn f(s: S) {\n    s.init();\n}\n";

/// - Library members `lib`, which lists a subscriber crate as a regular dependency, and `member`,
///   which does not.
/// - Each owns its `src/lib.rs`, holding `INSTALL`; `member` also owns `member/src/helper.rs`.
/// - `lib_code` and `member_code` are the files, as output shows them, each reaches as library
///   code.
fn subscriber_workspace(lib_code: &[&str], member_code: &[&str]) -> Workspace {
    let workspace = workspace(Edition::Edition2024, "");
    let code = |shown: &[&str]| -> BTreeSet<PathBuf> {
        shown
            .iter()
            .map(|shown| text_file(shown, "").path)
            .collect()
    };
    let lib = Package {
        manifest: text_file("lib/Cargo.toml", "[package]\nname = \"lib\"\n"),
        kind: PackageKind::Library,
        readme: missing("lib/README.md"),
        edition: Edition::Edition2024,
        dependencies: vec![Dependency {
            crate_name: "tracing-subscriber".to_owned(),
            section: Section::Normal,
        }],
        sources: vec![text_file("lib/src/lib.rs", INSTALL)],
        library_code: code(lib_code),
    };
    let member = Package {
        manifest: text_file("member/Cargo.toml", "[package]\nname = \"member\"\n"),
        readme: missing("member/README.md"),
        dependencies: Vec::new(),
        sources: vec![
            text_file("member/src/helper.rs", INSTALL),
            text_file("member/src/lib.rs", INSTALL),
        ],
        library_code: code(member_code),
        kind: PackageKind::Library,
        edition: Edition::Edition2024,
    };
    Workspace {
        members: vec![lib, member],
        settings: Config {
            subscriber_crates: words(&["tracing-subscriber"]),
            ..config()
        },
        ..workspace
    }
}

#[rstest]
#[case::own_library_code(
    &["lib/src/lib.rs"],
    &["member/src/helper.rs", "member/src/lib.rs"],
    &["lib/src/lib.rs:2:7: library_subscriber: library installs a log subscriber: `.init()`"]
)]
#[case::own_test_code(&[], &["member/src/helper.rs", "member/src/lib.rs"], &[])]
#[case::library_code_another_package_owns(
    &["lib/src/lib.rs", "member/src/helper.rs"],
    &["member/src/helper.rs", "member/src/lib.rs"],
    &[
        "lib/src/lib.rs:2:7: library_subscriber: library installs a log subscriber: `.init()`",
        "member/src/helper.rs:2:7: library_subscriber: library installs a log subscriber: `.init()`",
    ]
)]
fn library_code_may_not_install_a_subscriber(
    #[case] lib_code: &[&str],
    #[case] member_code: &[&str],
    #[case] expected: &[&str],
) {
    assert_eq!(
        lines(&subscriber_workspace(lib_code, member_code)),
        expected
    );
}
