// - TODO: randomized variant: a random root plus a random relative suffix comes back out of
//   `relative` as the suffix; a random path outside the root, a sibling sharing its prefix
//   included, comes back unchanged; a random string that is no edition makes `edition` ask
//   for an upgrade; a random tree of files, links and directories, asked about random paths
//   through it, gets the same answers from `Disk` and `Memory`; a random list of target
//   kinds makes `package_metadata` report a binary exactly when one is `bin`, every root in
//   target order; a random dependency name and rename come out of `dependency` as crate
//   name and key.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{
    Disk, Files, Package, PackageKind, PackageMetadata, Readme, Root, Section, TextFile, Workspace,
    assemble, dependencies, dependency, edition, package_metadata, relative, section,
};
use crate::check;
use crate::config::Config;
use crate::finding::Rule;
use anyhow::Result;
use cargo_metadata::DependencyKind;
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process;

const ROOT: &str = "/ws";

const CONFIG: &str = "forbidden-words = []\nsubscriber-crates = []\n";

#[derive(Clone, Copy)]
enum Content {
    Text(&'static str),
    NotUtf8,
    Directory,
    /// A symlink, by its target's path relative to `ROOT`.
    Link(&'static str),
    /// An empty file that is gone when resolved, as when it is deleted mid-run.
    Vanishing,
}

/// - The contents that can also be written to a real disk, where a file stays put.
/// - So the `Disk` contract has no form for a file deleted mid-run.
#[derive(Clone, Copy)]
enum Lasting {
    Text(&'static str),
    NotUtf8,
    Directory,
    /// By its target's path relative to the root.
    Link(&'static str),
}

impl From<Lasting> for Content {
    fn from(lasting: Lasting) -> Self {
        match lasting {
            Lasting::Text(text) => Self::Text(text),
            Lasting::NotUtf8 => Self::NotUtf8,
            Lasting::Directory => Self::Directory,
            Lasting::Link(target) => Self::Link(target),
        }
    }
}

/// What a path holds once symlinks are followed.
#[derive(Clone, Copy)]
enum Node {
    Text(&'static str),
    NotUtf8,
    Directory,
    Vanishing,
}

/// - An in-memory file system holding absolute paths.
/// - Every ancestor of an entry is a directory entry; `Memory::new` adds them.
/// - A path resolves one component at a time, as on disk: a missing component, or one below
///   a file, is an error even when a later `..` would leave it.
struct Memory(BTreeMap<PathBuf, Content>);

impl Memory {
    /// - `entries`, the last one for a path winning, plus a directory for each ancestor.
    /// - Panics when an ancestor is held as something else: no disk holds that tree.
    fn new(entries: impl IntoIterator<Item = (PathBuf, Content)>) -> Self {
        let entries: BTreeMap<PathBuf, Content> = entries.into_iter().collect();
        let ancestors: Vec<PathBuf> = entries
            .keys()
            .flat_map(|path| path.ancestors().skip(1))
            .map(Path::to_path_buf)
            .collect();
        for ancestor in &ancestors {
            match entries.get(ancestor) {
                None | Some(Content::Directory) => {}
                Some(
                    Content::Text(_) | Content::NotUtf8 | Content::Link(_) | Content::Vanishing,
                ) => panic!("{} holds entries but is no directory", ancestor.display()),
            }
        }

        let directories = ancestors
            .into_iter()
            .map(|ancestor| (ancestor, Content::Directory));
        Self(directories.chain(entries).collect())
    }

    /// The canonical form of `path` and what it holds.
    fn resolve(&self, path: &Path) -> io::Result<(PathBuf, Node)> {
        path.components().try_fold(
            (PathBuf::new(), Node::Directory),
            |(resolved, node), component| {
                within(node)?;
                match component {
                    Component::Prefix(_) | Component::RootDir => {
                        Ok((resolved.join(component), Node::Directory))
                    }
                    Component::CurDir => Ok((resolved, Node::Directory)),
                    // `..` at the root stays at the root, as on disk.
                    Component::ParentDir => {
                        let parent = resolved.parent().map(Path::to_path_buf);
                        Ok((parent.unwrap_or(resolved), Node::Directory))
                    }
                    Component::Normal(name) => self.step(resolved.join(name)),
                }
            },
        )
    }

    /// What `path`, whose parent is a resolved directory, holds.
    fn step(&self, path: PathBuf) -> io::Result<(PathBuf, Node)> {
        match self.0.get(&path) {
            Some(Content::Link(target)) => self.resolve(&Path::new(ROOT).join(target)),
            Some(Content::Text(text)) => Ok((path, Node::Text(text))),
            Some(Content::NotUtf8) => Ok((path, Node::NotUtf8)),
            Some(Content::Directory) => Ok((path, Node::Directory)),
            Some(Content::Vanishing) => Ok((path, Node::Vanishing)),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
}

/// Only a directory has entries below it.
fn within(node: Node) -> io::Result<()> {
    match node {
        Node::Directory => Ok(()),
        Node::Text(_) | Node::NotUtf8 | Node::Vanishing => {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
    }
}

impl Files for Memory {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.resolve(path)?.1 {
            Node::Text(text) => Ok(text.to_owned()),
            Node::Vanishing => Ok(String::new()),
            Node::NotUtf8 => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )),
            Node::Directory => Err(io::Error::from(io::ErrorKind::IsADirectory)),
        }
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let (resolved, node) = self.resolve(path)?;
        match node {
            Node::Vanishing => Err(io::Error::from(io::ErrorKind::NotFound)),
            Node::Text(_) | Node::NotUtf8 | Node::Directory => Ok(resolved),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        match self.resolve(path) {
            Ok((_, Node::Text(_) | Node::NotUtf8 | Node::Vanishing)) => true,
            Ok((_, Node::Directory)) | Err(_) => false,
        }
    }
}

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
    load_from(Path::new(ROOT), edition, files, manifests)
}

/// `load_in` with `workspace_root` as `cargo metadata` names the root.
fn load_from(
    workspace_root: &Path,
    edition: Edition,
    files: &[(&str, Content)],
    manifests: &[&str],
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
        .map(|manifest| {
            let manifest_path = Path::new(ROOT).join(manifest);
            let roots: Vec<PathBuf> = ["src/lib.rs", "src/main.rs", "build.rs"]
                .iter()
                .filter_map(|root| manifest_path.parent().map(|dir| dir.join(root)))
                .filter(|root| memory.is_file(root))
                .collect();
            let kind = if roots.iter().any(|root| root.ends_with("src/main.rs")) {
                PackageKind::Binary
            } else {
                PackageKind::Library
            };
            PackageMetadata {
                manifest_path,
                kind,
                edition,
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
#[case::symlinked_file_resolves_children_beside_the_link(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("mod link;\n")),
        ("p/src/link.rs", Content::Link("p/src/elsewhere/t.rs")),
        ("p/src/elsewhere/t.rs", Content::Text("mod child;\n")),
        ("p/src/link/child.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/elsewhere/t.rs", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: p/src/link/child.rs"]
)]
#[case::route_back_to_a_followed_file_stops(
    &[
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("#[path = \"lib.rs\"]\nmod me;\n")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "p/Cargo.toml: p/src/lib.rs"]
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
#[case::root_package_behind_a_symlinked_manifest(
    &[
        ("Cargo.toml", Content::Link("real/Cargo.toml")),
        ("real/Cargo.toml", Content::Text("[package]\n")),
        ("src/main.rs", Content::Text("")),
    ],
    &["Cargo.toml"],
    &["real/Cargo.toml: src/main.rs"]
)]
#[case::root_package_behind_a_symlinked_manifest_keeps_the_readme_beside_the_link(
    &[
        ("Cargo.toml", Content::Link("real/Cargo.toml")),
        ("real/Cargo.toml", Content::Text("[package]\n")),
        ("src/main.rs", Content::Text("")),
        ("README.md", Content::Text("")),
    ],
    &["Cargo.toml"],
    &["real/Cargo.toml: src/main.rs", "real/Cargo.toml: README.md"]
)]
#[case::package_behind_a_symlinked_root_manifest_holds_the_files_beside_the_link(
    &[
        ("Cargo.toml", Content::Link("real/Cargo.toml")),
        ("real/Cargo.toml", Content::Text("[package]\n")),
        ("src/main.rs", Content::Text("mod x;\n")),
        ("src/x.rs", Content::Text("")),
        ("m/Cargo.toml", Content::Text("")),
        ("m/src/lib.rs", Content::Text("#[path = \"../../src/x.rs\"]\nmod x;\n")),
    ],
    &["Cargo.toml", "m/Cargo.toml"],
    &["real/Cargo.toml: src/main.rs", "real/Cargo.toml: src/x.rs", "m/Cargo.toml: m/src/lib.rs"]
)]
#[case::package_behind_a_symlinked_member_manifest_holds_the_files_beside_the_link(
    &[
        ("src/main.rs", Content::Text("#[path = \"../m/src/x.rs\"]\nmod x;\n")),
        ("m/Cargo.toml", Content::Link("elsewhere/Cargo.toml")),
        ("elsewhere/Cargo.toml", Content::Text("")),
        ("m/src/lib.rs", Content::Text("mod x;\n")),
        ("m/src/x.rs", Content::Text("")),
    ],
    &["Cargo.toml", "m/Cargo.toml"],
    &["Cargo.toml: src/main.rs", "elsewhere/Cargo.toml: m/src/lib.rs", "elsewhere/Cargo.toml: m/src/x.rs"]
)]
#[case::readme_linked_to_the_root_readme_shows_as_the_root_readme(
    &[
        ("README.md", Content::Text("")),
        ("p/Cargo.toml", Content::Text("")),
        ("p/README.md", Content::Link("README.md")),
        ("p/src/lib.rs", Content::Text("")),
    ],
    &["p/Cargo.toml"],
    &["virtual: Cargo.toml", "virtual: README.md", "p/Cargo.toml: p/src/lib.rs", "p/Cargo.toml: README.md"]
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
#[case::path_value_not_a_string(
    &[("p/src/lib.rs", Content::Text("#[path = 5]\nmod x;\n"))],
    "p/src/lib.rs:1: `path` attribute value is not a valid string literal"
)]
#[case::module_candidate_outside_the_root(
    &[("p/src/lib.rs", Content::Text("#[path = \"/elsewhere/x.rs\"]\nmod x;\n"))],
    "p/src/lib.rs:2: no file for `mod x;`, tried /elsewhere/x.rs"
)]
#[case::member_manifest_gone_before_it_is_resolved(
    &[("p/Cargo.toml", Content::Vanishing), ("p/src/lib.rs", Content::Text(""))],
    "cannot resolve /ws/p/Cargo.toml: entity not found"
)]
#[case::root_manifest_gone_before_it_is_resolved(
    &[("Cargo.toml", Content::Vanishing), ("p/src/lib.rs", Content::Text(""))],
    "cannot resolve /ws/Cargo.toml: entity not found"
)]
#[case::member_manifest_not_utf8(
    &[("p/Cargo.toml", Content::NotUtf8), ("p/src/lib.rs", Content::Text(""))],
    "cannot read /ws/p/Cargo.toml: stream did not contain valid UTF-8"
)]
#[case::virtual_root_manifest_not_utf8(
    &[("Cargo.toml", Content::NotUtf8), ("p/src/lib.rs", Content::Text(""))],
    "cannot read /ws/Cargo.toml: stream did not contain valid UTF-8"
)]
#[case::source_not_utf8(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a.rs", Content::NotUtf8)],
    "cannot read /ws/p/src/a.rs: stream did not contain valid UTF-8"
)]
#[case::source_gone_before_it_is_resolved(
    &[("p/src/lib.rs", Content::Text("mod a;\n")), ("p/src/a.rs", Content::Vanishing)],
    "cannot resolve /ws/p/src/a.rs: entity not found"
)]
#[case::readme_gone_before_it_is_resolved(
    &[("p/src/lib.rs", Content::Text("")), ("p/README.md", Content::Vanishing)],
    "cannot resolve /ws/p/README.md: entity not found"
)]
#[case::virtual_root_readme_gone_before_it_is_resolved(
    &[("p/src/lib.rs", Content::Text("")), ("README.md", Content::Vanishing)],
    "cannot resolve /ws/README.md: entity not found"
)]
#[case::config_gone_before_it_is_resolved(
    &[("p/src/lib.rs", Content::Text("")), ("samlinter.toml", Content::Vanishing)],
    "cannot resolve /ws/samlinter.toml: entity not found"
)]
#[case::readme_unreadable(
    &[("p/src/lib.rs", Content::Text("")), ("p/README.md", Content::Directory)],
    "cannot read /ws/p/README.md: is a directory"
)]
#[case::virtual_root_readme_unreadable(
    &[("p/src/lib.rs", Content::Text("")), ("README.md", Content::Directory)],
    "cannot read /ws/README.md: is a directory"
)]
fn load_errors_name_the_file(#[case] files: &[(&str, Content)], #[case] expected: &str) {
    let error = load_p(files).err().unwrap();
    assert_eq!(format!("{error:#}"), expected);
}

#[rstest]
#[case::manifest_without_a_parent("/", &[], "/ has no parent directory")]
#[case::target_root_naming_no_file(
    "/ws/p/Cargo.toml",
    &["/ws/p/src/.."],
    "/ws/p/src/.. names no file"
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

#[test]
#[should_panic(expected = "/ws/a.rs holds entries but is no directory")]
fn memory_holds_no_entry_below_a_file() {
    Memory::new([
        (PathBuf::from("/ws/a.rs"), Content::Text("")),
        (PathBuf::from("/ws/a.rs/b.rs"), Content::Text("")),
    ]);
}

#[test]
fn two_packages_resolving_to_one_manifest_are_an_error() {
    let files = [
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
        ("q/Cargo.toml", Content::Link("p/Cargo.toml")),
    ];
    let error = load(&files, &["p/Cargo.toml", "q/Cargo.toml"])
        .err()
        .unwrap();
    assert_eq!(
        format!("{error:#}"),
        "p/Cargo.toml: two workspace packages resolve to this manifest"
    );
}

#[test]
fn root_named_through_dot_dot_shows_paths_relative_to_its_canonical_form() {
    let files = [
        ("p/Cargo.toml", Content::Text("")),
        ("p/src/lib.rs", Content::Text("")),
    ];
    let workspace = load_from(
        Path::new("/ws/p/.."),
        Edition::Edition2024,
        &files,
        &["p/Cargo.toml"],
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

/// - Cargo names a package by the directory of its manifest path as given, link included.
/// - So the README a binary needs sits beside the link, and the finding names that path.
/// - Outside the root, the path shows absolute, like every path there.
#[rstest]
#[case::beside_a_symlinked_member_manifest(
    &[
        ("m/Cargo.toml", Content::Link("real/Cargo.toml")),
        ("real/Cargo.toml", Content::Text("")),
        ("real/README.md", Content::Text("")),
        ("m/src/main.rs", Content::Text("fn main() {}\n")),
    ],
    "m/Cargo.toml",
    "real/Cargo.toml: binary_readme: binary package has no README at m/README.md"
)]
#[case::readme_that_is_a_broken_symlink(
    &[
        ("m/Cargo.toml", Content::Text("")),
        ("m/src/main.rs", Content::Text("fn main() {}\n")),
        ("m/README.md", Content::Link("m/nowhere.md")),
    ],
    "m/Cargo.toml",
    "m/Cargo.toml: binary_readme: binary package has no README at m/README.md"
)]
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
    let config = Config::parse(CONFIG).unwrap();
    let findings: Vec<String> = check::run(&workspace, &config)
        .unwrap()
        .iter()
        .filter(|finding| finding.rule == Rule::BinaryReadme)
        .map(ToString::to_string)
        .collect();
    assert_eq!(findings, [expected]);
}

/// A `mod` with neither `;` nor `{ }` names no file to find; the source check reports it.
#[test]
fn module_missing_its_semicolon_is_a_syntax_error() {
    let workspace = load_p(&[("p/src/lib.rs", Content::Text("mod gone\n"))]).unwrap();
    let config = Config::parse(CONFIG).unwrap();
    let rules: Vec<Rule> = check::run(&workspace, &config)
        .unwrap()
        .iter()
        .filter(|finding| finding.path == Path::new("p/src/lib.rs"))
        .map(|finding| finding.rule)
        .collect();
    assert_eq!(rules, [Rule::SyntaxError]);
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
    "cannot read /ws/samlinter.toml; every workspace needs one, see the README: entity not found"
)]
#[case::unreadable(
    Some(Content::Directory),
    "cannot read /ws/samlinter.toml; every workspace needs one, see the README: is a directory"
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
    let metadata = package_metadata(&cargo_package(edition, targets)).unwrap();
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
#[case::inside_the_root("/ws/lib/src/lib.rs", "lib/src/lib.rs")]
#[case::the_root_manifest("/ws/Cargo.toml", "Cargo.toml")]
#[case::member_outside_the_root("/elsewhere/src/lib.rs", "/elsewhere/src/lib.rs")]
#[case::sibling_with_a_shared_prefix("/ws2/src/lib.rs", "/ws2/src/lib.rs")]
fn paths_are_relative_to_the_root_when_inside_it(#[case] path: &str, #[case] expected: &str) {
    assert_eq!(
        relative(Path::new("/ws"), Path::new(path)),
        PathBuf::from(expected)
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

/// The files both `Files` implementations hold for the contract, by path relative to the root.
const CONTRACT_FILES: &[(&str, Lasting)] = &[
    ("text.rs", Lasting::Text("text")),
    ("link.rs", Lasting::Link("text.rs")),
    ("broken.rs", Lasting::Link("nowhere.rs")),
    ("dir/inner.rs", Lasting::Text("")),
    ("empty", Lasting::Directory),
    ("bad.rs", Lasting::NotUtf8),
];

/// What a `Files` implementation answers for one path: paths relative to the root, errors by
/// kind.
#[derive(Debug, PartialEq, Eq)]
struct Answers {
    read: Result<String, io::ErrorKind>,
    canonicalize: Result<PathBuf, io::ErrorKind>,
    is_file: bool,
}

fn answers(
    read: Result<&str, io::ErrorKind>,
    canonicalize: Result<&str, io::ErrorKind>,
    is_file: bool,
) -> Answers {
    Answers {
        read: read.map(str::to_owned),
        canonicalize: canonicalize.map(PathBuf::from),
        is_file,
    }
}

/// Every path the contract asks about, with the answers the disk gives.
fn contract() -> Vec<(&'static str, Answers)> {
    let not_found = io::ErrorKind::NotFound;
    let not_a_directory = io::ErrorKind::NotADirectory;
    vec![
        ("text.rs", answers(Ok("text"), Ok("text.rs"), true)),
        ("link.rs", answers(Ok("text"), Ok("text.rs"), true)),
        ("dir/../text.rs", answers(Ok("text"), Ok("text.rs"), true)),
        (
            "dir",
            answers(Err(io::ErrorKind::IsADirectory), Ok("dir"), false),
        ),
        (
            "empty",
            answers(Err(io::ErrorKind::IsADirectory), Ok("empty"), false),
        ),
        ("missing.rs", answers(Err(not_found), Err(not_found), false)),
        ("broken.rs", answers(Err(not_found), Err(not_found), false)),
        (
            "gone/../text.rs",
            answers(Err(not_found), Err(not_found), false),
        ),
        (
            "text.rs/x",
            answers(Err(not_a_directory), Err(not_a_directory), false),
        ),
        (
            "bad.rs",
            answers(Err(io::ErrorKind::InvalidData), Ok("bad.rs"), true),
        ),
    ]
}

/// What `files` answers for every path of the contract, under `root`.
fn ask(files: &impl Files, root: &Path) -> Vec<(&'static str, Answers)> {
    contract()
        .into_iter()
        .map(|(path, _)| {
            let full = root.join(path);
            let canonicalize = files
                .canonicalize(&full)
                .map(|resolved| resolved.strip_prefix(root).unwrap().to_path_buf());
            let answers = Answers {
                read: files.read(&full).map_err(|error| error.kind()),
                canonicalize: canonicalize.map_err(|error| error.kind()),
                is_file: files.is_file(&full),
            };
            (path, answers)
        })
        .collect()
}

/// Runs its closure when dropped, so cleanup runs even when an assertion fails.
struct OnDrop<F: FnMut()>(F);

impl<F: FnMut()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Writes `content` at `path` under `root`; a link's target is relative to `root`, as in
/// `Memory`.
fn write_on_disk(root: &Path, path: &str, content: Lasting) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    match content {
        Lasting::Text(text) => fs::write(&path, text).unwrap(),
        Lasting::NotUtf8 => fs::write(&path, [0x80]).unwrap(),
        Lasting::Directory => fs::create_dir(&path).unwrap(),
        Lasting::Link(target) => symlink(&root.join(target), &path).unwrap(),
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[test]
fn disk_keeps_the_files_contract() {
    let scratch = std::env::temp_dir().join(format!("samlinter-files-contract-{}", process::id()));
    fs::create_dir(&scratch).unwrap();
    let _cleanup = OnDrop(|| {
        fs::remove_dir_all(&scratch)
            .unwrap_or_else(|error| eprintln!("cannot remove {}: {error}", scratch.display()));
    });
    let root = fs::canonicalize(&scratch).unwrap();
    for (path, content) in CONTRACT_FILES {
        write_on_disk(&root, path, *content);
    }

    assert_eq!(ask(&Disk, &root), contract());
}

#[test]
fn memory_keeps_the_files_contract() {
    let memory = Memory::new(
        CONTRACT_FILES
            .iter()
            .map(|(path, content)| (Path::new(ROOT).join(path), Content::from(*content))),
    );
    assert_eq!(ask(&memory, Path::new(ROOT)), contract());
}
