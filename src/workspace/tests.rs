use super::lookup::memory::{Content, Memory};
use super::lookup::{Files, Unfollowed};
use super::{
    Package, PackageKind, PackageMetadata, Readme, Root, Section, TextFile, Workspace, assemble,
    dependencies, dependency, edition, package_metadata, section,
};
use crate::check;
use crate::finding::Rule;
use anyhow::Result;
use cargo_metadata::DependencyKind;
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::io;
use std::path::{Path, PathBuf};

const ROOT: &str = "/ws";

const CONFIG: &str = "forbidden-words = []\nsubscriber-crates = []\n";

/// - Loads a workspace at `/ws` holding `files` plus a root `Cargo.toml` and `samlinter.toml`;
///   an entry in `files` replaces either.
/// - Each package is named by its manifest path relative to the root; its targets are the
///   `src/lib.rs`, `src/main.rs` and `build.rs` beside it that `files` holds.
/// - A package with a `src/main.rs` is a binary package.
/// - Every package is edition 2024.
fn load(files: &[(&str, Content)], manifests: &[&str]) -> Result<Workspace> {
    load_in(Edition::Edition2024, files, manifests)
}

/// `load` with every package of `edition`.
fn load_in(edition: Edition, files: &[(&str, Content)], manifests: &[&str]) -> Result<Workspace> {
    let manifests: Vec<(&str, Edition)> = manifests
        .iter()
        .map(|manifest| (*manifest, edition))
        .collect();
    load_from(Path::new(ROOT), files, &manifests)
}

/// `load` with each package of its own edition and `workspace_root` as `cargo metadata` names
/// the root.
fn load_from(
    workspace_root: &Path,
    files: &[(&str, Content)],
    manifests: &[(&str, Edition)],
) -> Result<Workspace> {
    let defaults = [
        ("Cargo.toml", Content::Text("[workspace]\n")),
        ("samlinter.toml", Content::Text(CONFIG)),
    ];
    let memory = Memory::new(
        defaults
            .iter()
            .chain(files)
            .map(|(path, content)| (Path::new(ROOT).join(path), *content)),
    );
    let packages = manifests
        .iter()
        .map(|(manifest, edition)| {
            let manifest_path = Path::new(ROOT).join(manifest);
            let roots: Vec<PathBuf> = ["src/lib.rs", "src/main.rs", "build.rs"]
                .iter()
                .filter_map(|root| manifest_path.parent().map(|dir| dir.join(root)))
                .filter(|root| {
                    files
                        .iter()
                        .any(|(path, _)| Path::new(ROOT).join(path) == *root)
                })
                .collect();
            let kind = if roots.iter().any(|root| root.ends_with("src/main.rs")) {
                PackageKind::Binary
            } else {
                PackageKind::Library
            };
            PackageMetadata {
                manifest_path,
                kind,
                edition: *edition,
                dependencies: Vec::new(),
                roots,
            }
        })
        .collect();
    assemble(&memory, workspace_root, packages)
}

/// Loads library `p`, root `p/src/lib.rs`, with `files` and an empty `p/Cargo.toml`.
fn load_p(files: &[(&str, Content)]) -> Result<Workspace> {
    let files: Vec<(&str, Content)> = [("p/Cargo.toml", Content::Text(""))]
        .into_iter()
        .chain(files.iter().copied())
        .collect();
    load(&files, &["p/Cargo.toml"])
}

/// Every file each package holds, as `owner: file`; the owner of a virtual root's files is
/// `virtual`.
fn render(workspace: &Workspace) -> Vec<String> {
    let root = match &workspace.root {
        Root::Virtual { manifest, readme } => std::iter::once(manifest)
            .chain(found(readme))
            .map(|file| format!("virtual: {}", file.shown.display()))
            .collect(),
        Root::Package(package) => package_files(package),
    };
    root.into_iter()
        .chain(workspace.members.iter().flat_map(package_files))
        .collect()
}

fn found(readme: &Readme) -> Option<&TextFile> {
    match readme {
        Readme::Found(file) => Some(file),
        Readme::Missing { expected: _ } => None,
    }
}

fn package_files(package: &Package) -> Vec<String> {
    package
        .sources
        .iter()
        .chain(found(&package.readme))
        .map(|file| {
            format!(
                "{}: {}",
                package.manifest.shown.display(),
                file.shown.display()
            )
        })
        .collect()
}

#[rstest]
#[case::module_tree(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod a;\n")),
        ("p/src/a.rs", Content::Text("mod b;\n")),
        ("p/src/a/b.rs", Content::Text("")),
        ("p/src/orphan.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/a/b.rs", "p/Cargo.toml: p/src/a.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::build_script_is_a_target_root(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
        ("p/build.rs", Content::Text("mod helper;\n")),
        ("p/helper.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/build.rs", "p/Cargo.toml: p/helper.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::file_reached_twice_in_a_package_is_read_once(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod common;\n#[path = \"common.rs\"]\nmod again;\n")),
        ("p/src/main.rs", Content::Text("mod common;\n")),
        ("p/src/common.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/common.rs", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/src/main.rs"]
)]
#[case::every_route_to_a_file_has_its_children_followed(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod a;\n#[path = \"a.rs\"]\nmod again;\n")),
        ("p/src/a.rs", Content::Text("mod b;\n")),
        ("p/src/a/b.rs", Content::Text("")),
        ("p/src/b.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/a/b.rs", "p/Cargo.toml: p/src/a.rs", "p/Cargo.toml: p/src/b.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::route_back_to_a_followed_file_stops(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[path = \"lib.rs\"]\nmod me;\n")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::route_back_through_dot_dot_stops(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[path = \"../src/lib.rs\"]\nmod me;\n")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::only_the_mod_rs_file_of_a_module(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod a;\n")),
        ("p/src/a/mod.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/a/mod.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::both_files_of_a_module_are_kept(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod a;\n")),
        ("p/src/a.rs", Content::Text("")),
        ("p/src/a/mod.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/a/mod.rs", "p/Cargo.toml: p/src/a.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::every_existing_candidate_is_kept(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u.rs\")]\n#[cfg_attr(windows, path = \"w.rs\")]\nmod imp;\n")),
        ("p/src/u.rs", Content::Text("")),
        ("p/src/w.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/src/u.rs", "p/Cargo.toml: p/src/w.rs"]
)]
#[case::virtual_root_readme_and_package_readme(
    &[
        ("README.md", Content::Text("")),
        ("p/Cargo.toml", Content::Text("")),
        ("p/README.md", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "virtual: README.md", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/README.md"]
)]
#[case::root_package(
    &[
        ("src/main.rs", Content::Text("")),
        ("m/Cargo.toml", Content::Text("")),
        ("m/src/lib.rs", Content::Text("")),
    ],
    &["m/Cargo.toml", "Cargo.toml"],
    &["Cargo.toml: src/main.rs", "m/Cargo.toml: m/src/lib.rs"]
)]
#[case::absolute_path_inside_an_inline_module_with_a_missing_fixed_directory(
    &[
        ("p/Cargo.toml", Content::Text("")),
        (
            "p/src/lib.rs",
            Content::Text("#[path = \"gen\"]\nmod m {\n    #[path = \"/elsewhere/n.rs\"]\n    mod n;\n}\n"),
        ),
        ("/elsewhere/n.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: /elsewhere/n.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::inline_directory_through_dot_dot(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[path = \"x/../u\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/x/other.rs", Content::Text("")),
        ("p/src/u/n.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/src/u/n.rs"]
)]
#[case::file_in_an_existing_cfg_attr_directory(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u/n.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/src/u/n.rs"]
)]
#[case::file_two_packages_reach_is_loaded_once_by_its_owner(
    &[
        ("a/Cargo.toml", Content::Text("")),
        ("a/src/lib.rs", Content::Text("mod x;\n")),
        ("a/src/x.rs", Content::Text("")),
        ("b/Cargo.toml", Content::Text("")),
        ("b/src/lib.rs", Content::Text("#[path = \"../../a/src/x.rs\"]\nmod x;\n")),
    ],
    &["b/Cargo.toml", "a/Cargo.toml"],
    &["virtual: Cargo.toml", "a/Cargo.toml: a/src/lib.rs", "a/Cargo.toml: a/src/x.rs", "b/Cargo.toml: b/src/lib.rs"]
)]
fn loads_the_files_each_package_owns(
    #[case] files: &[(&str, Content)],
    #[case] manifests: &[&str],
    #[case] expected: &[&str],
) {
    assert_eq!(render(&load(files, manifests).unwrap()), expected);
}

#[rstest]
#[case::identifier_before_2024(
    Edition::Edition2021,
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/gen.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::keyword_from_2024(
    Edition::Edition2024,
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
fn modules_are_found_with_the_package_edition(#[case] edition: Edition, #[case] expected: &[&str]) {
    let files = [
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod gen;\n")),
        ("p/src/gen.rs", Content::Text("")),
    ];
    assert_eq!(
        render(&load_in(edition, &files, &["p/Cargo.toml"]).unwrap()),
        expected
    );
}

/// - Each inline module has two possible directories, so the combinations double per level.
/// - Only existing directories are followed, each once however it is spelled, so 40 levels
///   stay cheap.
#[rstest]
#[case::missing_directory("x")]
#[case::same_directory(".")]
fn nested_cfg_attr_paths_follow_each_existing_directory_once(#[case] path: &str) {
    const LEVELS: usize = 40;
    let text = format!(
        "{}mod leaf;\n{}",
        format!("#[cfg_attr(never, path = \"{path}\")]\nmod m {{\n").repeat(LEVELS),
        "}\n".repeat(LEVELS)
    );
    let leaf = format!("p/src/{}leaf.rs", "m/".repeat(LEVELS));
    let workspace = load_p(&[
        ("p/src/lib.rs", Content::Text(text.leak())),
        (leaf.clone().leak(), Content::Text("")),
    ])
    .unwrap();
    assert_eq!(
        render(&workspace),
        [
            "virtual: Cargo.toml".to_owned(),
            "p/Cargo.toml: p/src/lib.rs".to_owned(),
            format!("p/Cargo.toml: {leaf}"),
        ]
    );
}

#[rstest]
#[case::declared_module_without_a_file(
    &[("p/src/lib.rs", Content::Text("mod a;\nmod gone;\n")), ("p/src/a.rs", Content::Text(""))],
    "p/src/lib.rs:2: no file for `mod gone;`, tried p/src/gone.rs, p/src/gone/mod.rs"
)]
#[case::comment_before_the_declaration(
    &[("p/src/lib.rs", Content::Text("mod a; // note\nmod gone;\n")), ("p/src/a.rs", Content::Text(""))],
    "p/src/lib.rs:2: no file for `mod gone;`, tried p/src/gone.rs, p/src/gone/mod.rs"
)]
#[case::module_only_a_second_route_declares(
    &[
        ("p/src/lib.rs", Content::Text("mod a;\n#[path = \"a.rs\"]\nmod again;\n")),
        ("p/src/a.rs", Content::Text("mod b;\n")),
        ("p/src/a/b.rs", Content::Text("")),
    ],
    "p/src/a.rs:1: no file for `mod b;`, tried p/src/b.rs, p/src/b/mod.rs"
)]
#[case::module_inside_an_inline_module_without_a_directory(
    &[("p/src/lib.rs", Content::Text("mod m {\n    mod gone;\n}\n"))],
    "p/src/lib.rs:2: no file for `mod gone;`, tried p/src/m/gone.rs, p/src/m/gone/mod.rs"
)]
#[case::inline_module_path_to_a_missing_directory_is_not_tried(
    &[("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod gone;\n}\n"))],
    "p/src/lib.rs:3: no file for `mod gone;`, tried p/src/m/gone.rs, p/src/m/gone/mod.rs"
)]
// - The `linked_*` cases: a link in each place samlinter looks stops the run.
// - `Content::Link` models no target, so a dangling or looping link is the same case here;
//   the disk contract rows `broken.rs` and `loop.rs` pin that real ones are reported as links.
#[case::linked_target_root(
    &[
        ("p/src/lib.rs", Content::Link),
        ("p/real.rs", Content::Text("")),
    ],
    "p/src/lib.rs: samlinter does not follow symlinks"
)]
#[case::linked_readme(
    &[
        ("p/src/lib.rs", Content::Text("")),
        ("p/README.md", Content::Link),
        ("p/real.md", Content::Text("")),
    ],
    "p/README.md: samlinter does not follow symlinks"
)]
#[case::linked_config(
    &[
        ("p/src/lib.rs", Content::Text("")),
        ("samlinter.toml", Content::Link),
        ("shared/samlinter.toml", Content::Text(CONFIG)),
    ],
    "samlinter.toml: samlinter does not follow symlinks"
)]
#[case::linked_member_manifest(
    &[
        ("p/src/lib.rs", Content::Text("")),
        ("p/Cargo.toml", Content::Link),
        ("real/Cargo.toml", Content::Text("")),
    ],
    "p/Cargo.toml: samlinter does not follow symlinks"
)]
#[case::linked_path_target(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"x.rs\"]\nmod x;\n")),
        ("p/src/x.rs", Content::Link),
        ("p/src/real.rs", Content::Text("")),
    ],
    "p/src/x.rs: samlinter does not follow symlinks"
)]
#[case::link_in_a_middle_component(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"d/x.rs\"]\nmod x;\n")),
        ("p/src/d", Content::Link),
        ("p/real/x.rs", Content::Text("")),
    ],
    "p/src/d: samlinter does not follow symlinks"
)]
#[case::linked_cfg_attr_inline_directory(
    &[
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u", Content::Link),
        ("p/real/n.rs", Content::Text("")),
    ],
    "p/src/u: samlinter does not follow symlinks"
)]
#[case::linked_fixed_inline_directory(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"u\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u", Content::Link),
        ("p/real/n.rs", Content::Text("")),
    ],
    "p/src/u: samlinter does not follow symlinks"
)]
#[case::link_left_by_dot_dot(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"d/../x.rs\"]\nmod x;\n")),
        ("p/src/d", Content::Link),
        ("p/real/inner.rs", Content::Text("")),
        ("p/src/x.rs", Content::Text("")),
        ("p/x.rs", Content::Text("")),
    ],
    "p/src/d: samlinter does not follow symlinks"
)]
#[case::linked_inline_module_directory(
    &[
        ("p/src/lib.rs", Content::Text("mod m {\n    mod n;\n}\n")),
        ("p/src/m", Content::Link),
        ("p/real/n.rs", Content::Text("")),
    ],
    "p/src/m: samlinter does not follow symlinks"
)]
#[case::link_outside_the_root(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"/elsewhere/x.rs\"]\nmod x;\n")),
        ("/elsewhere/x.rs", Content::Link),
        ("/elsewhere/real.rs", Content::Text("")),
    ],
    "/elsewhere/x.rs: samlinter does not follow symlinks"
)]
#[case::candidate_that_cannot_be_inspected(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a", Content::Forbidden)],
    "cannot inspect p/src/a/mod.rs: permission denied"
)]
#[case::cfg_attr_inline_path_into_a_forbidden_directory(
    &[
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u", Content::Forbidden),
    ],
    "cannot inspect p/src/u/n.rs: permission denied"
)]
#[case::fixed_inline_path_into_a_forbidden_directory(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"u\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u", Content::Forbidden),
    ],
    "cannot inspect p/src/u/n.rs: permission denied"
)]
#[case::inline_module_in_a_forbidden_directory(
    &[
        ("p/src/lib.rs", Content::Text("mod m {\n    mod n;\n}\n")),
        ("p/src/m", Content::Forbidden),
    ],
    "cannot inspect p/src/m/n.rs: permission denied"
)]
#[case::cfg_attr_inline_directory_that_cannot_be_inspected(
    &[
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"f/u\")]\nmod m {\n    mod n;\n}\n")),
        ("p/src/f", Content::Forbidden),
    ],
    "cannot inspect p/src/f/u: permission denied"
)]
#[case::fixed_inline_directory_that_cannot_be_inspected(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"f/u\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/f", Content::Forbidden),
    ],
    "cannot inspect p/src/f/u: permission denied"
)]
#[case::default_inline_directory_that_cannot_be_inspected(
    &[
        ("p/src/lib.rs", Content::Text("mod f {\n    mod m {\n        mod n;\n    }\n}\n")),
        ("p/src/f", Content::Forbidden),
    ],
    "cannot inspect p/src/f/m: permission denied"
)]
#[case::inline_directory_through_dot_dot_after_a_missing_directory(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"x/../u\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/u/n.rs", Content::Text("")),
    ],
    "p/src/lib.rs:3: no file for `mod n;`, tried p/src/x/../u/n.rs, p/src/x/../u/n/mod.rs"
)]
// A path ending in `/` or `/.` names a directory, so no file is there, as for rustc.
#[case::path_ending_in_a_separator(
    &[("p/src/lib.rs", Content::Text("#[path = \"x.rs/\"]\nmod x;\n")), ("p/src/x.rs", Content::Text(""))],
    "p/src/lib.rs:2: no file for `mod x;`, tried p/src/x.rs/"
)]
#[case::path_ending_in_a_separator_and_a_dot(
    &[("p/src/lib.rs", Content::Text("#[path = \"x.rs/.\"]\nmod x;\n")), ("p/src/x.rs", Content::Text(""))],
    "p/src/lib.rs:2: no file for `mod x;`, tried p/src/x.rs/."
)]
#[case::existing_inline_directory_is_tried_as_written(
    &[
        ("p/src/lib.rs", Content::Text("mod m {\n    mod gone;\n}\n")),
        ("p/src/m/other.rs", Content::Text("")),
    ],
    "p/src/lib.rs:2: no file for `mod gone;`, tried p/src/m/gone.rs, p/src/m/gone/mod.rs"
)]
#[case::second_spelling_of_a_directory_is_dropped(
    &[
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"../src/m\")]\nmod m {\n    mod gone;\n}\n")),
        ("p/src/m/other.rs", Content::Text("")),
    ],
    "p/src/lib.rs:3: no file for `mod gone;`, tried p/src/m/gone.rs, p/src/m/gone/mod.rs"
)]
#[case::dot_dot_after_a_missing_directory_finds_nothing(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"gone/../x.rs\"]\nmod x;\n")),
        ("p/src/x.rs", Content::Text("")),
    ],
    "p/src/lib.rs:2: no file for `mod x;`, tried p/src/gone/../x.rs"
)]
#[case::dot_dot_after_a_file_finds_nothing(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"lib.rs/../x.rs\"]\nmod x;\n")),
        ("p/src/x.rs", Content::Text("")),
    ],
    "p/src/lib.rs:2: no file for `mod x;`, tried p/src/lib.rs/../x.rs"
)]
#[case::candidate_that_is_a_directory(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a.rs", Content::Directory)],
    "p/src/lib.rs:1: no file for `mod a;`, tried p/src/a.rs, p/src/a/mod.rs"
)]
#[case::cfg_attr_inline_path_naming_a_file_is_not_entered(
    &[
        ("p/src/lib.rs", Content::Text("#[cfg_attr(unix, path = \"u\")]\nmod m {\n    mod gone;\n}\n")),
        ("p/src/u", Content::Text("")),
    ],
    "p/src/lib.rs:3: no file for `mod gone;`, tried p/src/m/gone.rs, p/src/m/gone/mod.rs"
)]
#[case::fixed_inline_path_to_a_missing_directory(
    &[("p/src/lib.rs", Content::Text("#[path = \"other\"]\nmod m {\n    mod gone;\n}\n"))],
    "p/src/lib.rs:3: no file for `mod gone;`, tried p/src/other/gone.rs, p/src/other/gone/mod.rs"
)]
#[case::fixed_inner_path_to_a_missing_directory(
    &[("p/src/lib.rs", Content::Text("mod m {\n    #![path = \"other\"]\n    mod gone;\n}\n"))],
    "p/src/lib.rs:3: no file for `mod gone;`, tried p/src/other/gone.rs, p/src/other/gone/mod.rs"
)]
#[case::path_value_not_a_string(
    &[("p/src/lib.rs", Content::Text("#[path = 5]\nmod x;\n"))],
    "p/src/lib.rs:1: `path` attribute value is not a valid string literal"
)]
#[case::module_candidate_outside_the_root(
    &[("p/src/lib.rs", Content::Text("#[path = \"/elsewhere/x.rs\"]\nmod x;\n"))],
    "p/src/lib.rs:2: no file for `mod x;`, tried /elsewhere/x.rs"
)]
#[case::member_manifest_gone_before_it_is_read(
    &[("p/Cargo.toml", Content::Vanishing), ("p/src/lib.rs", Content::Text(""))],
    "cannot read p/Cargo.toml: entity not found"
)]
#[case::root_manifest_gone_before_it_is_read(
    &[("Cargo.toml", Content::Vanishing), ("p/src/lib.rs", Content::Text(""))],
    "cannot read Cargo.toml: entity not found"
)]
#[case::member_manifest_not_utf8(
    &[("p/Cargo.toml", Content::NotUtf8), ("p/src/lib.rs", Content::Text(""))],
    "cannot read p/Cargo.toml: stream did not contain valid UTF-8"
)]
#[case::virtual_root_manifest_not_utf8(
    &[("Cargo.toml", Content::NotUtf8), ("p/src/lib.rs", Content::Text(""))],
    "cannot read Cargo.toml: stream did not contain valid UTF-8"
)]
#[case::source_not_utf8(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a.rs", Content::NotUtf8)],
    "cannot read p/src/a.rs: stream did not contain valid UTF-8"
)]
#[case::source_through_dot_dot_not_utf8(
    &[
        ("p/src/lib.rs", Content::Text("#[path = \"../src/a.rs\"]\nmod a;\n")),
        ("p/src/a.rs", Content::NotUtf8),
    ],
    "cannot read p/src/a.rs: stream did not contain valid UTF-8"
)]
#[case::source_gone_before_it_is_read(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a.rs", Content::Vanishing)],
    "cannot read p/src/a.rs: entity not found"
)]
#[case::readme_gone_before_it_is_read(
    &[("p/src/lib.rs", Content::Text("")), ("p/README.md", Content::Vanishing)],
    "cannot read p/README.md: entity not found"
)]
#[case::virtual_root_readme_gone_before_it_is_read(
    &[("p/src/lib.rs", Content::Text("")), ("README.md", Content::Vanishing)],
    "cannot read README.md: entity not found"
)]
#[case::config_gone_before_it_is_read(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", Content::Vanishing)],
    "cannot read samlinter.toml; every workspace needs one, see the README: entity not found"
)]
#[case::readme_not_utf8(
    &[("p/src/lib.rs", Content::Text("")), ("p/README.md", Content::NotUtf8)],
    "cannot read p/README.md: stream did not contain valid UTF-8"
)]
#[case::readme_unreadable(
    &[("p/src/lib.rs", Content::Text("")), ("p/README.md", Content::Directory)],
    "cannot read p/README.md: is a directory"
)]
#[case::virtual_root_readme_unreadable(
    &[("p/src/lib.rs", Content::Text("")), ("README.md", Content::Directory)],
    "cannot read README.md: is a directory"
)]
#[case::config_that_does_not_parse_stops_the_run_before_any_module_walk(
    &[
        ("p/src/lib.rs", Content::Text("mod gone;\n")),
        ("samlinter.toml", Content::Text("forbidden-words = [\n")),
    ],
    "invalid samlinter.toml: TOML parse error at line 1, column 20\n  |\n1 | forbidden-words = [\n  |                    ^\nunclosed array, expected `]`\n"
)]
#[case::absolute_exclude(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", excluding("\"/ws/p\""))],
    "invalid samlinter.toml: TOML parse error at line 3, column 11\n  |\n3 | exclude = [\"/ws/p\"]\n  |           ^^^^^^^^^\n`exclude` entry `/ws/p` is absolute; write it relative to the workspace root\n"
)]
#[case::exclude_above_the_root(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", excluding("\"..\""))],
    "invalid samlinter.toml: TOML parse error at line 3, column 11\n  |\n3 | exclude = [\"..\"]\n  |           ^^^^^^\n`exclude` entry `..` climbs above the workspace root\n"
)]
#[case::exclude_climbing_out_and_back(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", excluding("\"p/../../ws/p\""))],
    "invalid samlinter.toml: TOML parse error at line 3, column 11\n  |\n3 | exclude = [\"p/../../ws/p\"]\n  |           ^^^^^^^^^^^^^^^^\n`exclude` entry `p/../../ws/p` climbs above the workspace root\n"
)]
// The disk checks of an entry live in `exclude`; this pins that the load names the config.
#[case::exclude_naming_nothing(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", excluding("\"p/gone\""))],
    "invalid samlinter.toml: `exclude` entry `p/gone` names no file or directory"
)]
fn load_errors_name_the_file(#[case] files: &[(&str, Content)], #[case] expected: &str) {
    let error = load_p(files).err().unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

#[rstest]
#[case::manifest_naming_the_filesystem_root("/", &[], "cannot read /: is a directory")]
#[case::target_root_naming_the_filesystem_root(
    "/ws/p/Cargo.toml",
    &["/ws/.."],
    "cannot read /: is a directory"
)]
#[case::target_root_ending_in_dot_dot(
    "/ws/p/Cargo.toml",
    &["/ws/p/src/.."],
    "cannot read p/src/..: entity not found"
)]
#[case::absent_target_root(
    "/ws/p/Cargo.toml",
    &["/ws/p/lib.rs"],
    "cannot read p/lib.rs: entity not found"
)]
#[case::target_root_in_an_absent_directory(
    "/ws/p/Cargo.toml",
    &["/ws/p/gone/lib.rs"],
    "cannot read p/gone/lib.rs: entity not found"
)]
fn unusable_metadata_paths_are_errors(
    #[case] manifest_path: &str,
    #[case] roots: &[&str],
    #[case] expected: &str,
) {
    let memory = Memory::new(
        [
            ("Cargo.toml", Content::Text("[workspace]\n")),
            ("samlinter.toml", Content::Text(CONFIG)),
            ("p/Cargo.toml", Content::Text("")),
        ]
        .map(|(path, content)| (Path::new(ROOT).join(path), content)),
    );
    let package = PackageMetadata {
        manifest_path: PathBuf::from(manifest_path),
        kind: PackageKind::Library,
        edition: Edition::Edition2024,
        dependencies: Vec::new(),
        roots: roots.iter().map(PathBuf::from).collect(),
    };
    let error = assemble(&memory, Path::new(ROOT), vec![package])
        .err()
        .unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

/// The root manifest is read on its own, as a root package's manifest or a virtual root's.
#[rstest]
#[case::root_package(
    &[
        ("Cargo.toml", Content::Link),
        ("real/Cargo.toml", Content::Text("[package]\n")),
        ("src/main.rs", Content::Text("fn main() {}\n")),
    ],
    &["Cargo.toml"]
)]
#[case::virtual_root(
    &[
        ("Cargo.toml", Content::Link),
        ("real/Cargo.toml", Content::Text("[workspace]\n")),
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"]
)]
fn linked_root_manifest_stops_the_run(
    #[case] files: &[(&str, Content)],
    #[case] manifests: &[&str],
) {
    let error = load(files, manifests).err().unwrap();
    assert_eq!(
        format!("{error:#}"),
        "Cargo.toml: samlinter does not follow symlinks"
    );
}

/// A member directory cargo reports is a link: the walk stops at it before reading below.
#[test]
fn linked_member_directory_stops_the_run() {
    let files = [
        ("m", Content::Link),
        ("real/Cargo.toml", Content::Text("")),
        ("real/src/lib.rs", Content::Text("")),
    ];
    let error = load(&files, &["m/Cargo.toml"]).err().unwrap();
    assert_eq!(
        format!("{error:#}"),
        "m: samlinter does not follow symlinks"
    );
}

/// Every package's module walk runs before any manifest is read, so a module error, in the same
/// package or a later one, comes before an unreadable manifest.
#[rstest]
#[case::module_error_before_its_own_manifest(
    &[
        ("a/Cargo.toml", Content::NotUtf8),
        ("a/src/lib.rs", Content::Text("mod gone;\n")),
    ],
    &["a/Cargo.toml"],
    "a/src/lib.rs:1: no file for `mod gone;`, tried a/src/gone.rs, a/src/gone/mod.rs"
)]
#[case::later_module_error_before_an_earlier_manifest(
    &[
        ("a/Cargo.toml", Content::NotUtf8),
        ("a/src/lib.rs", Content::Text("")),
        ("b/Cargo.toml", Content::Text("")),
        ("b/src/lib.rs", Content::Text("mod gone;\n")),
    ],
    &["a/Cargo.toml", "b/Cargo.toml"],
    "b/src/lib.rs:1: no file for `mod gone;`, tried b/src/gone.rs, b/src/gone/mod.rs"
)]
fn first_error_stops_the_load(
    #[case] files: &[(&str, Content)],
    #[case] manifests: &[&str],
    #[case] expected: &str,
) {
    let error = load(files, manifests).err().unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

#[test]
fn two_packages_on_one_manifest_are_an_error() {
    let files = [
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
    ];
    let error = load(&files, &["p/Cargo.toml", "p/Cargo.toml"])
        .err()
        .unwrap();
    assert_eq!(
        format!("{error:#}"),
        "p/Cargo.toml: two workspace packages resolve to this manifest"
    );
}

#[test]
fn root_named_through_dot_dot_shows_paths_relative_to_its_normalized_form() {
    let files = [
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
    ];
    let workspace = load_from(
        Path::new("/ws/p/.."),
        &files,
        &[("p/Cargo.toml", Edition::Edition2024)],
    )
    .unwrap();
    assert_eq!(
        (render(&workspace), workspace.config.shown),
        (
            vec![
                "virtual: Cargo.toml".to_owned(),
                "p/Cargo.toml: p/src/lib.rs".to_owned()
            ],
            PathBuf::from("samlinter.toml")
        )
    );
}

/// What `Above` answers for one path at or above the root, in place of `Memory`.
type Given = (&'static str, Result<Unfollowed, io::ErrorKind>);

/// - A `Memory` whose `given` paths, the workspace root `/ws/root` or directories above it,
///   answer as listed.
/// - `Memory` holds no entry below a link, so this stub answers for those paths.
struct Above {
    memory: Memory,
    given: &'static [Given],
}

impl Files for Above {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.memory.read(path)
    }

    fn entry(&self, path: &Path) -> io::Result<Unfollowed> {
        self.given
            .iter()
            .find(|(given, _)| Path::new(given) == path)
            .map_or_else(
                || self.memory.entry(path),
                |(_, answer)| answer.map_err(io::Error::from),
            )
    }
}

const LINKED_ROOT: &[Given] = &[("/ws/root", Ok(Unfollowed::Symlink))];

const LINKED_ANCESTOR: &[Given] = &[("/ws", Ok(Unfollowed::Symlink))];

/// - Loads the workspace `/ws/root` on `Above` answering `given`.
/// - Its one package has the manifest `manifest` and the target roots `roots`; `p/src/lib.rs`
///   holds `lib`.
fn load_above(
    given: &'static [Given],
    lib: &'static str,
    manifest: &str,
    roots: &[&str],
) -> Result<Workspace> {
    let memory = Memory::new(
        [
            ("root/Cargo.toml", Content::Text("[workspace]\n")),
            ("root/samlinter.toml", Content::Text(CONFIG)),
            ("root/p/Cargo.toml", Content::Text("")),
            ("root/p/src/lib.rs", Content::Text(lib)),
            (
                "root/p/src/a.rs",
                Content::Text("#[path = \"../../../x.rs\"]\nmod x;\n"),
            ),
            ("root/p/src/y.rs", Content::Text("")),
            ("root/p/src/real/n.rs", Content::Text("")),
            ("root/p/src/empty/other.rs", Content::Text("")),
            ("x.rs", Content::Text("")),
            ("n.rs", Content::Text("")),
        ]
        .map(|(path, content)| (Path::new(ROOT).join(path), content)),
    );
    let package = PackageMetadata {
        manifest_path: PathBuf::from(manifest),
        kind: PackageKind::Library,
        edition: Edition::Edition2024,
        dependencies: Vec::new(),
        roots: roots.iter().map(PathBuf::from).collect(),
    };
    assemble(
        &Above { memory, given },
        Path::new("/ws/root"),
        vec![package],
    )
}

/// `render`, then every warning as output shows it.
fn render_with_warnings(workspace: &Workspace) -> (Vec<String>, Vec<String>) {
    (
        render(workspace),
        workspace.warnings.iter().map(ToString::to_string).collect(),
    )
}

const NOT_FOLLOWED: &str =
    "`#[path]` climbs above the symlinked workspace root; module not followed";

/// - The root `/ws/root` and its ancestors are taken as given, never looked up on the way in.
/// - A `..` leaving one of them that is a symlink would lead where the link points, so the
///   module is not followed and a warning names its declaration.
/// - Leaving a directory that is no symlink resolves as written, also above the root.
#[rstest]
#[case::leaving_the_linked_root(
    LINKED_ROOT,
    "#[path = \"../../../x.rs\"]\nmod x;\n",
    &[],
    &["p/src/lib.rs:2"]
)]
#[case::leaving_a_linked_ancestor(
    LINKED_ANCESTOR,
    "#[path = \"../../../../x.rs\"]\nmod x;\n",
    &[],
    &["p/src/lib.rs:2"]
)]
#[case::below_a_linked_ancestor(
    LINKED_ANCESTOR,
    "#[path = \"../../../x.rs\"]\nmod x;\n",
    &["p/Cargo.toml: /ws/x.rs"],
    &[]
)]
#[case::nothing_linked(
    &[],
    "#[path = \"../../../x.rs\"]\nmod x;\n",
    &["p/Cargo.toml: /ws/x.rs"],
    &[]
)]
#[case::cfg_attr_path_with_the_default_file_missing(
    LINKED_ROOT,
    "#[cfg_attr(unix, path = \"../../../x.rs\")]\nmod x;\n",
    &[],
    &["p/src/lib.rs:2"]
)]
#[case::cfg_attr_path_beside_the_default_file(
    LINKED_ROOT,
    "#[cfg_attr(unix, path = \"../../../x.rs\")]\nmod y;\n",
    &["p/Cargo.toml: p/src/y.rs"],
    &["p/src/lib.rs:2"]
)]
#[case::inline_module_directory(
    LINKED_ROOT,
    "#[path = \"../../..\"]\nmod m {\n    mod n;\n}\n",
    &[],
    &["p/src/lib.rs:3"]
)]
// The climbing candidates come first, then a found one, then a missing one.
#[case::climbing_directory_then_a_cfg_attr_directory_holding_the_file(
    LINKED_ROOT,
    "#[cfg_attr(unix, path = \"real\")]\n#[path = \"../../..\"]\nmod m {\n    mod n;\n}\n",
    &["p/Cargo.toml: p/src/real/n.rs"],
    &["p/src/lib.rs:4"]
)]
// The climbing candidates come first, then only missing ones: a warning, never an error.
#[case::climbing_directory_then_an_empty_cfg_attr_directory(
    LINKED_ROOT,
    "#[cfg_attr(unix, path = \"empty\")]\n#[path = \"../../..\"]\nmod m {\n    mod n;\n}\n",
    &[],
    &["p/src/lib.rs:4"]
)]
// - `a.rs` is reached twice, as `a.rs` and through `#[path]`, and warns once.
// - Its warning sorts before the one `lib.rs`, read first, gives.
#[case::warnings_sorted_and_each_once(
    LINKED_ROOT,
    "mod a;\n#[path = \"a.rs\"]\nmod again;\n#[path = \"../../../x.rs\"]\nmod x;\n",
    &["p/Cargo.toml: p/src/a.rs"],
    &["p/src/a.rs:2", "p/src/lib.rs:5"]
)]
fn path_climbing_out_of_a_linked_root_is_not_followed(
    #[case] given: &'static [Given],
    #[case] lib: &'static str,
    #[case] reached: &[&str],
    #[case] warned: &[&str],
) {
    let workspace = load_above(
        given,
        lib,
        "/ws/root/p/Cargo.toml",
        &["/ws/root/p/src/lib.rs"],
    )
    .unwrap();
    // `render` orders a package's files by their absolute path.
    let mut sources: Vec<&str> = ["p/Cargo.toml: p/src/lib.rs"]
        .into_iter()
        .chain(reached.iter().copied())
        .collect();
    sources.sort_by_key(|line| {
        line.split_once(": ")
            .map(|(_, shown)| Path::new("/ws/root").join(shown))
    });
    let expected: Vec<String> = ["virtual: Cargo.toml"]
        .into_iter()
        .chain(sources)
        .map(ToString::to_string)
        .collect();
    let warnings: Vec<String> = warned
        .iter()
        .map(|place| format!("{place}: {NOT_FOLLOWED}"))
        .collect();
    assert_eq!(render_with_warnings(&workspace), (expected, warnings));
}

/// A path cargo reports that climbs out of the linked root is not followed either.
#[rstest]
#[case::target_root(
    "/ws/root/p/Cargo.toml",
    &["/ws/root/p/src/lib.rs", "/ws/root/p/../../x.rs"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"],
    "p/../../x.rs"
)]
#[case::manifest(
    "/ws/root/p/../../root/p/Cargo.toml",
    &["/ws/root/p/src/lib.rs"],
    &["virtual: Cargo.toml"],
    "p/../../root/p/Cargo.toml"
)]
fn cargo_path_climbing_out_of_a_linked_root_is_not_followed(
    #[case] manifest: &str,
    #[case] roots: &[&str],
    #[case] reached: &[&str],
    #[case] warned: &str,
) {
    let workspace = load_above(LINKED_ROOT, "", manifest, roots).unwrap();
    assert_eq!(
        render_with_warnings(&workspace),
        (
            reached.iter().map(ToString::to_string).collect(),
            vec![format!(
                "{warned}: climbs above the symlinked workspace root; not followed"
            )]
        )
    );
}

/// - A `..` leaving the root or a directory above it looks that directory up.
/// - It must still be a directory; a failed lookup names it.
#[rstest]
#[case::root_is_a_file(
    &[("/ws/root", Ok(Unfollowed::File))],
    "#[path = \"../../../x.rs\"]\nmod x;\n",
    ".: the workspace root or a directory above it is not a directory"
)]
#[case::ancestor_is_gone(
    &[("/ws", Ok(Unfollowed::Neither))],
    "#[path = \"../../../../x.rs\"]\nmod x;\n",
    "/ws: the workspace root or a directory above it is not a directory"
)]
#[case::root_cannot_be_inspected(
    &[("/ws/root", Err(io::ErrorKind::PermissionDenied))],
    "#[path = \"../../../x.rs\"]\nmod x;\n",
    "cannot inspect .: permission denied"
)]
#[case::ancestor_cannot_be_inspected(
    &[("/ws", Err(io::ErrorKind::PermissionDenied))],
    "#[path = \"../../../../x.rs\"]\nmod x;\n",
    "cannot inspect /ws: permission denied"
)]
fn leaving_the_root_or_an_ancestor_needs_a_directory(
    #[case] given: &'static [Given],
    #[case] lib: &'static str,
    #[case] expected: &str,
) {
    let error = load_above(
        given,
        lib,
        "/ws/root/p/Cargo.toml",
        &["/ws/root/p/src/lib.rs"],
    )
    .err()
    .unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

/// `samlinter.toml` with no forbidden words and no subscriber crates, excluding `entries`.
fn excluding(entries: &str) -> Content {
    Content::Text(format!("{CONFIG}exclude = [{entries}]\n").leak())
}

/// An excluded Rust file is never read, so neither its text nor its `mod` declarations matter.
#[rstest]
#[case::directory(
    &[
        ("samlinter.toml", excluding("\"p/src/generated\"")),
        ("p/src/lib.rs", Content::Text("mod generated;\nmod kept;\n")),
        ("p/src/generated/mod.rs", Content::NotUtf8),
        ("p/src/kept.rs", Content::Text("")),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/kept.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::file_whose_modules_are_not_followed(
    &[
        ("samlinter.toml", excluding("\"p/src/generated.rs\"")),
        ("p/src/lib.rs", Content::Text("mod generated;\n")),
        ("p/src/generated.rs", Content::Text("mod gone;\n")),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::target_root(
    &[
        ("samlinter.toml", excluding("\"p/src/lib.rs\"")),
        ("p/src/lib.rs", Content::NotUtf8),
    ],
    &["virtual: Cargo.toml"]
)]
#[case::entry_through_dot_dot(
    &[
        ("samlinter.toml", excluding("\"p/src/../src/generated.rs\"")),
        ("p/src/lib.rs", Content::Text("mod generated;\n")),
        ("p/src/generated.rs", Content::NotUtf8),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::sibling_sharing_a_prefix_is_kept(
    &[
        ("samlinter.toml", excluding("\"p/src/gen\"")),
        ("p/src/lib.rs", Content::Text("mod generated;\n")),
        ("p/src/gen", Content::Directory),
        ("p/src/generated.rs", Content::Text("")),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/generated.rs", "p/Cargo.toml: p/src/lib.rs"]
)]
// The lookup stops at the excluded directory: a symlink below it is never met.
#[case::symlink_below_an_excluded_directory(
    &[
        ("samlinter.toml", excluding("\"p/src/generated\"")),
        ("p/src/lib.rs", Content::Text("mod generated;\n")),
        ("p/src/generated/mod.rs", Content::Link),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::denied_directory_below_an_excluded_one(
    &[
        ("samlinter.toml", excluding("\"p/src/generated\"")),
        ("p/src/lib.rs", Content::Text("#[path = \"generated/f/x.rs\"]\nmod x;\n")),
        ("p/src/generated/f", Content::Forbidden),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
// The first written component an entry covers ends the lookup, a later `..` included.
#[case::path_through_an_excluded_directory(
    &[
        ("samlinter.toml", excluding("\"p/src/generated\"")),
        ("p/src/lib.rs", Content::Text("#[path = \"generated/../kept.rs\"]\nmod kept;\n")),
        ("p/src/generated/other.rs", Content::Text("")),
        ("p/src/kept.rs", Content::Text("")),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
#[case::inline_module_directory(
    &[
        ("samlinter.toml", excluding("\"p/src/generated\"")),
        ("p/src/lib.rs", Content::Text("#[path = \"generated\"]\nmod m {\n    mod n;\n}\n")),
        ("p/src/generated/n.rs", Content::NotUtf8),
    ],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
)]
fn excluded_sources_are_not_read(#[case] files: &[(&str, Content)], #[case] expected: &[&str]) {
    assert_eq!(render(&load_p(files).unwrap()), expected);
}

/// No rule reports a finding in an excluded file: manifests, READMEs and `samlinter.toml` are
/// still read, their findings dropped.
#[rstest]
#[case::nothing_excluded(
    "",
    &[
        "Cargo.toml: panic_strategy: no `[profile.dev]` with `panic = \"abort\"`",
        "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
        "README.md:1:3: em_dash: em-dash (U+2014)",
        "p/Cargo.toml:1:2: feature_flags: `[features]` table",
        "p/README.md:1:3: em_dash: em-dash (U+2014)",
        "p/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
        "samlinter.toml:1:5: em_dash: em-dash (U+2014)",
    ]
)]
#[case::root_manifest(
    "\"Cargo.toml\"",
    &[
        "README.md:1:3: em_dash: em-dash (U+2014)",
        "p/Cargo.toml:1:2: feature_flags: `[features]` table",
        "p/README.md:1:3: em_dash: em-dash (U+2014)",
        "p/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
        "samlinter.toml:1:5: em_dash: em-dash (U+2014)",
    ]
)]
#[case::member_manifest(
    "\"p/Cargo.toml\"",
    &[
        "Cargo.toml: panic_strategy: no `[profile.dev]` with `panic = \"abort\"`",
        "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
        "README.md:1:3: em_dash: em-dash (U+2014)",
        "p/README.md:1:3: em_dash: em-dash (U+2014)",
        "p/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
        "samlinter.toml:1:5: em_dash: em-dash (U+2014)",
    ]
)]
#[case::readmes_and_config(
    "\"README.md\", \"p/README.md\", \"samlinter.toml\"",
    &[
        "Cargo.toml: panic_strategy: no `[profile.dev]` with `panic = \"abort\"`",
        "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
        "p/Cargo.toml:1:2: feature_flags: `[features]` table",
        "p/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    ]
)]
#[case::package_directory(
    "\"p\"",
    &[
        "Cargo.toml: panic_strategy: no `[profile.dev]` with `panic = \"abort\"`",
        "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
        "README.md:1:3: em_dash: em-dash (U+2014)",
        "samlinter.toml:1:5: em_dash: em-dash (U+2014)",
    ]
)]
#[case::the_whole_root("\".\"", &[])]
fn excluded_files_get_no_findings(#[case] entries: &str, #[case] expected: &[&str]) {
    let config = format!("# a \u{2014}\n{CONFIG}exclude = [{entries}]\n");
    let files = [
        ("samlinter.toml", Content::Text(config.leak())),
        ("README.md", Content::Text("a \u{2014}\n")),
        ("p/Cargo.toml", Content::Text("[features]\n")),
        ("p/README.md", Content::Text("a \u{2014}\n")),
        ("p/src/lib.rs", Content::Text("pub(crate) fn f() {}\n")),
    ];
    let workspace = load(&files, &["p/Cargo.toml"]).unwrap();
    let findings: Vec<String> = check::run(&workspace)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(findings, expected);
}

/// `exclude = ["."]` covers the root, never a member outside it.
#[test]
fn excluding_the_root_keeps_files_outside_it() {
    let files = [
        ("samlinter.toml", excluding("\".\"")),
        ("/elsewhere/Cargo.toml", Content::Text("[features]\n")),
        (
            "/elsewhere/src/lib.rs",
            Content::Text("pub(crate) fn f() {}\n"),
        ),
    ];
    let workspace = load(&files, &["/elsewhere/Cargo.toml"]).unwrap();
    let findings: Vec<String> = check::run(&workspace)
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        findings,
        [
            "/elsewhere/Cargo.toml:1:2: feature_flags: `[features]` table",
            "/elsewhere/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
        ]
    );
}

/// - The README a binary needs sits beside its manifest, and the finding names that path.
/// - Outside the root, the path shows absolute, like every path there.
#[rstest]
#[case::package_outside_the_root(
    &[
        ("/elsewhere/Cargo.toml", Content::Text("")),
        ("/elsewhere/src/main.rs", Content::Text("fn main() {}\n")),
    ],
    "/elsewhere/Cargo.toml",
    "/elsewhere/Cargo.toml: binary_readme: binary package has no README at /elsewhere/README.md"
)]
fn missing_binary_readme_is_named_where_it_belongs(
    #[case] files: &[(&str, Content)],
    #[case] manifest: &str,
    #[case] expected: &str,
) {
    let workspace = load(files, &[manifest]).unwrap();
    let findings: Vec<String> = check::run(&workspace)
        .unwrap()
        .iter()
        .filter(|finding| finding.rule == Rule::BinaryReadme)
        .map(ToString::to_string)
        .collect();
    assert_eq!(findings, [expected]);
}

/// `a/src/x.rs` is owned by `a` and reached from `b` too; it parses with its owner's edition.
#[rstest]
#[case::owner_on_2024(
    Edition::Edition2024,
    Edition::Edition2021,
    &["a/src/x.rs:2:9: syntax_error: expected pattern"]
)]
#[case::owner_on_2021(Edition::Edition2021, Edition::Edition2024, &[])]
fn shared_file_parses_with_its_owners_edition(
    #[case] owner: Edition,
    #[case] other: Edition,
    #[case] expected: &[&str],
) {
    let files = [
        ("a/Cargo.toml", Content::Text("")),
        ("a/src/lib.rs", Content::Text("mod x;\n")),
        (
            "a/src/x.rs",
            Content::Text("fn f() {\n    let gen = 1;\n}\n"),
        ),
        ("b/Cargo.toml", Content::Text("")),
        (
            "b/src/lib.rs",
            Content::Text("#[path = \"../../a/src/x.rs\"]\nmod x;\n"),
        ),
    ];
    let workspace = load_from(
        Path::new(ROOT),
        &files,
        &[("a/Cargo.toml", owner), ("b/Cargo.toml", other)],
    )
    .unwrap();
    let findings: Vec<String> = check::run(&workspace)
        .unwrap()
        .iter()
        .filter(|finding| finding.path == Path::new("a/src/x.rs"))
        .map(ToString::to_string)
        .collect();
    assert_eq!(findings, expected);
}

/// A `mod` with neither `;` nor `{ }` names no file to find; the source check reports it.
#[test]
fn module_missing_its_semicolon_is_a_syntax_error() {
    let workspace = load_p(&[("p/src/lib.rs", Content::Text("mod gone\n"))]).unwrap();
    let findings: Vec<String> = check::run(&workspace)
        .unwrap()
        .iter()
        .filter(|finding| finding.path == Path::new("p/src/lib.rs"))
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        findings,
        ["p/src/lib.rs:1:9: syntax_error: expected `;` or `{`"]
    );
}

#[test]
fn config_is_read_from_the_root() {
    let workspace = load_p(&[("p/src/lib.rs", Content::Text(""))]).unwrap();
    assert_eq!(
        (workspace.config.shown, workspace.config.text.as_str()),
        (PathBuf::from("samlinter.toml"), CONFIG)
    );
}

#[rstest]
#[case::absent(
    None,
    "cannot read samlinter.toml; every workspace needs one, see the README: entity not found"
)]
#[case::unreadable(
    Some(Content::Directory),
    "cannot read samlinter.toml; every workspace needs one, see the README: is a directory"
)]
fn config_errors_name_the_file(#[case] config: Option<Content>, #[case] expected: &str) {
    let memory = Memory::new(
        [
            ("Cargo.toml", Some(Content::Text("[workspace]\n"))),
            ("samlinter.toml", config),
        ]
        .into_iter()
        .filter_map(|(path, content)| Some((Path::new(ROOT).join(path), content?))),
    );
    let error = assemble(&memory, Path::new(ROOT), Vec::new())
        .err()
        .unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

#[rstest]
#[case::normal(DependencyKind::Normal, Some(Section::Normal))]
#[case::development(DependencyKind::Development, Some(Section::Development))]
#[case::build(DependencyKind::Build, Some(Section::Build))]
#[case::newer_than_samlinter(DependencyKind::Unknown, None)]
fn dependency_kinds_name_their_section(
    #[case] kind: DependencyKind,
    #[case] expected: Option<Section>,
) {
    assert_eq!(section(kind), expected);
}

#[test]
fn unknown_dependency_kind_asks_for_an_upgrade() {
    // `cargo metadata` output as cargo newer than samlinter could write it.
    let dependency: cargo_metadata::Dependency = toml_edit::de::from_str(
        "name = \"x\"\nreq = \"*\"\nkind = \"future\"\noptional = false\nuses_default_features = true\nfeatures = []\n",
    )
    .unwrap();
    let error = dependencies(Path::new("lib/Cargo.toml"), &[dependency])
        .err()
        .unwrap();
    assert_eq!(
        format!("{error:#}"),
        "lib/Cargo.toml: dependency `x` has a kind this samlinter does not know; upgrade samlinter"
    );
}

/// `cargo metadata` output for package `p` at `/ws/p/Cargo.toml` of `edition`, with one target
/// per `(kind, root)`, each root relative to `/ws/p`.
fn cargo_package(edition: &str, targets: &[(&str, &str)]) -> cargo_metadata::Package {
    let tables: Vec<String> = targets
        .iter()
        .map(|(kind, root)| {
            format!("[[targets]]\nname = \"p\"\nkind = [\"{kind}\"]\nsrc_path = \"/ws/p/{root}\"\n")
        })
        .collect();
    toml_edit::de::from_str(&format!(
        "name = \"p\"\nversion = \"0.1.0\"\nid = \"p 0.1.0\"\nmanifest_path = \"/ws/p/Cargo.toml\"\nedition = \"{edition}\"\ndependencies = []\nfeatures = {{}}\n{}",
        tables.concat()
    ))
    .unwrap()
}

#[rstest]
#[case::lib_only(
    "2024",
    &[("lib", "src/lib.rs")],
    PackageKind::Library,
    Edition::Edition2024,
    &["/ws/p/src/lib.rs"]
)]
#[case::bin_only(
    "2021",
    &[("bin", "src/main.rs")],
    PackageKind::Binary,
    Edition::Edition2021,
    &["/ws/p/src/main.rs"]
)]
#[case::lib_and_bin(
    "2018",
    &[("lib", "src/lib.rs"), ("bin", "src/main.rs")],
    PackageKind::Binary,
    Edition::Edition2018,
    &["/ws/p/src/lib.rs", "/ws/p/src/main.rs"]
)]
#[case::example_only(
    "2015",
    &[("example", "examples/demo.rs")],
    PackageKind::Library,
    Edition::Edition2015,
    &["/ws/p/examples/demo.rs"]
)]
#[case::build_script_only(
    "2024",
    &[("custom-build", "build.rs")],
    PackageKind::Library,
    Edition::Edition2024,
    &["/ws/p/build.rs"]
)]
#[case::lib_and_example(
    "2024",
    &[("lib", "src/lib.rs"), ("example", "examples/demo.rs")],
    PackageKind::Library,
    Edition::Edition2024,
    &["/ws/p/src/lib.rs", "/ws/p/examples/demo.rs"]
)]
fn package_metadata_reads_kind_edition_and_target_roots(
    #[case] edition: &str,
    #[case] targets: &[(&str, &str)],
    #[case] kind: PackageKind,
    #[case] expected_edition: Edition,
    #[case] roots: &[&str],
) {
    let metadata = package_metadata(Path::new(ROOT), &cargo_package(edition, targets)).unwrap();
    let expected_roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    assert_eq!(metadata.manifest_path, PathBuf::from("/ws/p/Cargo.toml"));
    assert_eq!(metadata.kind, kind);
    assert_eq!(metadata.edition, expected_edition);
    assert_eq!(metadata.roots, expected_roots);
}

#[rstest]
#[case::plain("name = \"serde\"\nkind = \"normal\"\n", ("serde", "serde", Section::Normal, None))]
#[case::renamed(
    "name = \"tracing-subscriber\"\nrename = \"logging\"\nkind = \"normal\"\n",
    ("logging", "tracing-subscriber", Section::Normal, None)
)]
#[case::target_specific(
    "name = \"libc\"\nkind = \"dev\"\ntarget = \"cfg(unix)\"\n",
    ("libc", "libc", Section::Development, Some("cfg(unix)"))
)]
#[case::build("name = \"cc\"\nkind = \"build\"\n", ("cc", "cc", Section::Build, None))]
fn dependency_keeps_key_crate_name_section_and_platform(
    #[case] fields: &str,
    #[case] expected: (&str, &str, Section, Option<&str>),
) {
    // `cargo metadata` output for one dependency, with the fields samlinter does not read fixed.
    let metadata: cargo_metadata::Dependency = toml_edit::de::from_str(&format!(
        "{fields}req = \"*\"\noptional = false\nuses_default_features = true\nfeatures = []\n"
    ))
    .unwrap();
    let read = dependency(&metadata).unwrap();
    let target = read.target.as_ref().map(ToString::to_string);
    assert_eq!(
        (
            read.key.as_str(),
            read.crate_name.as_str(),
            read.section,
            target.as_deref()
        ),
        expected
    );
}

#[rstest]
#[case::edition_2015("2015", Edition::Edition2015)]
#[case::edition_2018("2018", Edition::Edition2018)]
#[case::edition_2021("2021", Edition::Edition2021)]
#[case::edition_2024("2024", Edition::Edition2024)]
fn known_editions_parse(#[case] name: &str, #[case] expected: Edition) {
    assert_eq!(edition(Path::new("Cargo.toml"), name).unwrap(), expected);
}

#[test]
fn unknown_edition_asks_for_an_upgrade() {
    let error = edition(Path::new("lib/Cargo.toml"), "2099").unwrap_err();
    assert_eq!(
        error.to_string(),
        "lib/Cargo.toml: edition 2099 is newer than this samlinter understands; upgrade samlinter"
    );
}
