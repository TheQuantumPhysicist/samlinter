use super::super::lookup::Looked;
use super::super::{ByManifest, PackageKind, PackageMetadata, Reached, TextFile};
use super::{Reach, claim, owned_sources, owner};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};

/// The package with `manifest`, reaching `files`.
fn reach(manifest: &str, files: &[&str]) -> Reach {
    Reach {
        manifest: PathBuf::from(manifest),
        files: files.iter().map(PathBuf::from).collect(),
    }
}

/// Each package by its manifest path, with the one file it reaches.
fn reaches(packages: &[(&str, &str)]) -> Vec<Reach> {
    packages
        .iter()
        .map(|(manifest, file)| reach(manifest, &[file]))
        .collect()
}

#[rstest]
#[case::file_only_the_asking_package_reaches(
    &[("/ws/a/Cargo.toml", "/ws/a/src/lib.rs")],
    "/ws/a/src/lib.rs",
    "/ws/a/Cargo.toml",
    "/ws/a/Cargo.toml"
)]
#[case::file_outside_the_only_package_reaching_it(
    &[("/ws/z/Cargo.toml", "/ws/shared/x.rs"), ("/ws/a/Cargo.toml", "/ws/a/src/lib.rs")],
    "/ws/shared/x.rs",
    "/ws/z/Cargo.toml",
    "/ws/z/Cargo.toml"
)]
#[case::sibling_package_holding_the_file(
    &[("/ws/a/Cargo.toml", "/ws/a/src/x.rs"), ("/ws/b/Cargo.toml", "/ws/a/src/x.rs")],
    "/ws/a/src/x.rs",
    "/ws/b/Cargo.toml",
    "/ws/a/Cargo.toml"
)]
#[case::holding_package_asking(
    &[("/ws/a/Cargo.toml", "/ws/a/src/x.rs"), ("/ws/b/Cargo.toml", "/ws/a/src/x.rs")],
    "/ws/a/src/x.rs",
    "/ws/a/Cargo.toml",
    "/ws/a/Cargo.toml"
)]
#[case::innermost_of_nested_packages(
    &[("/ws/Cargo.toml", "/ws/m/src/x.rs"), ("/ws/m/Cargo.toml", "/ws/m/src/x.rs")],
    "/ws/m/src/x.rs",
    "/ws/Cargo.toml",
    "/ws/m/Cargo.toml"
)]
#[case::file_outside_every_package_goes_to_the_lowest_manifest_path(
    &[("/ws/a/Cargo.toml", "/ws/shared/x.rs"), ("/ws/b/Cargo.toml", "/ws/shared/x.rs")],
    "/ws/shared/x.rs",
    "/ws/b/Cargo.toml",
    "/ws/a/Cargo.toml"
)]
#[case::package_holding_but_not_reaching_the_file_has_no_claim(
    &[("/ws/Cargo.toml", "/ws/m/src/x.rs"), ("/ws/m/Cargo.toml", "/ws/m/src/lib.rs")],
    "/ws/m/src/x.rs",
    "/ws/Cargo.toml",
    "/ws/Cargo.toml"
)]
fn the_strongest_reaching_package_owns_a_file(
    #[case] packages: &[(&str, &str)],
    #[case] file: &str,
    #[case] asking: &str,
    #[case] expected: &str,
) {
    let reaches = reaches(packages);
    let asking = reaches
        .iter()
        .find(|reach| reach.manifest == Path::new(asking))
        .unwrap();
    assert_eq!(
        owner(&reaches, Path::new(file), asking),
        Path::new(expected)
    );
}

#[rstest]
#[case::package_holding_the_file("/ws/m/src/x.rs", "/ws/m/Cargo.toml", Some(3))]
#[case::outer_package_holding_the_file("/ws/m/src/x.rs", "/ws/Cargo.toml", Some(2))]
#[case::file_outside_the_package("/ws/a/src/x.rs", "/ws/b/Cargo.toml", None)]
#[case::root_package_and_a_file_outside_the_root("/shared/x.rs", "/ws/Cargo.toml", None)]
#[case::directory_sharing_a_name_prefix("/ws2/x.rs", "/ws/Cargo.toml", None)]
fn a_claim_is_the_holding_directory_depth_then_the_lower_manifest(
    #[case] file: &str,
    #[case] manifest: &str,
    #[case] depth: Option<usize>,
) {
    assert_eq!(
        claim(Path::new(file), &reach(manifest, &[file])),
        (depth, Reverse(Path::new(manifest)))
    );
}

#[test]
fn each_package_keeps_only_the_files_it_owns() {
    let package = |manifest: &str, sources: &[&str]| {
        let metadata = PackageMetadata {
            manifest_path: PathBuf::from(manifest),
            kind: PackageKind::Library,
            edition: Edition::Edition2024,
            dependencies: Vec::new(),
            roots: Vec::new(),
        };
        let sources = sources
            .iter()
            .map(|path| TextFile {
                path: PathBuf::from(path),
                shown: PathBuf::from(path),
                text: String::new(),
            })
            .collect();
        let package = Reached {
            metadata,
            manifest: Looked::Neither,
            sources,
        };
        (PathBuf::from(manifest), package)
    };
    let reached: ByManifest = [
        package("/ws/a/Cargo.toml", &["/ws/a/src/lib.rs", "/ws/a/src/x.rs"]),
        package("/ws/b/Cargo.toml", &["/ws/a/src/x.rs", "/ws/b/src/lib.rs"]),
    ]
    .into_iter()
    .collect();

    let owned: Vec<(PathBuf, Vec<PathBuf>)> = owned_sources(reached)
        .into_iter()
        .map(|(manifest, package)| {
            (
                manifest,
                package
                    .sources
                    .into_iter()
                    .map(|source| source.path)
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        owned,
        [
            (
                PathBuf::from("/ws/a/Cargo.toml"),
                vec![
                    PathBuf::from("/ws/a/src/lib.rs"),
                    PathBuf::from("/ws/a/src/x.rs")
                ]
            ),
            (
                PathBuf::from("/ws/b/Cargo.toml"),
                vec![PathBuf::from("/ws/b/src/lib.rs")]
            ),
        ]
    );
}
