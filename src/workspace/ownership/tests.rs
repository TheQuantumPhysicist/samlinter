use super::super::lookup::Looked;
use super::super::{ByManifest, PackageKind, PackageMetadata, Reach, Reached, Source, TextFile};
use super::{Claimant, claim, owned_sources, owner};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};

/// The package with `manifest`, reaching `files`.
fn claimant(manifest: &str, files: &[&str]) -> Claimant {
    Claimant {
        manifest: PathBuf::from(manifest),
        files: files.iter().map(PathBuf::from).collect(),
    }
}

/// Each package by its manifest path, with the one file it reaches.
fn claimants(packages: &[(&str, &str)]) -> Vec<Claimant> {
    packages
        .iter()
        .map(|(manifest, file)| claimant(manifest, &[file]))
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
    let claimants = claimants(packages);
    let asking = claimants
        .iter()
        .find(|claimant| claimant.manifest == Path::new(asking))
        .unwrap();
    assert_eq!(
        owner(&claimants, Path::new(file), asking),
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
        claim(Path::new(file), &claimant(manifest, &[file])),
        (depth, Reverse(Path::new(manifest)))
    );
}

/// Each package by its manifest, its owned files, then the files it reaches as library code.
type Kept = (PathBuf, Vec<PathBuf>, Vec<PathBuf>);

#[test]
fn each_package_keeps_the_files_it_owns_and_its_library_code() {
    let package = |manifest: &str, sources: &[(&str, Reach)]| {
        let metadata = PackageMetadata {
            manifest_path: PathBuf::from(manifest),
            kind: PackageKind::Library,
            edition: Edition::Edition2024,
            dependencies: Vec::new(),
            targets: Vec::new(),
        };
        let sources = sources
            .iter()
            .map(|(path, reach)| Source {
                file: TextFile {
                    path: PathBuf::from(path),
                    shown: PathBuf::from(path),
                    text: String::new(),
                },
                reach: *reach,
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
        package(
            "/ws/a/Cargo.toml",
            &[
                ("/ws/a/src/lib.rs", Reach::Library),
                ("/ws/a/src/tests.rs", Reach::NotLibrary),
                ("/ws/a/src/x.rs", Reach::NotLibrary),
            ],
        ),
        package(
            "/ws/b/Cargo.toml",
            &[
                ("/ws/a/src/x.rs", Reach::Library),
                ("/ws/b/src/lib.rs", Reach::Library),
            ],
        ),
    ]
    .into_iter()
    .collect();

    let kept: Vec<Kept> = owned_sources(reached)
        .into_iter()
        .map(|(manifest, package)| {
            (
                manifest,
                package
                    .sources
                    .into_iter()
                    .map(|source| source.path)
                    .collect(),
                package.library_code.into_iter().collect(),
            )
        })
        .collect();
    let paths = |paths: &[&str]| paths.iter().map(PathBuf::from).collect::<Vec<_>>();
    assert_eq!(
        kept,
        [
            (
                PathBuf::from("/ws/a/Cargo.toml"),
                paths(&["/ws/a/src/lib.rs", "/ws/a/src/tests.rs", "/ws/a/src/x.rs"]),
                paths(&["/ws/a/src/lib.rs"]),
            ),
            (
                PathBuf::from("/ws/b/Cargo.toml"),
                paths(&["/ws/b/src/lib.rs"]),
                paths(&["/ws/a/src/x.rs", "/ws/b/src/lib.rs"]),
            ),
        ]
    );
}
