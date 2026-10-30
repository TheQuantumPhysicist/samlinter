use super::super::FileKind::{ModRs, NonModRs};
use super::super::Reach::{Library, NotLibrary};
use super::super::TestCode::{NotTest, Test};
use super::super::lookup::Looked;
use super::super::{
    ByManifest, FileKind, PackageKind, PackageMetadata, Reach, Reached, RouteCode, Source,
    TestCode, TextFile,
};
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

/// Each package by its manifest, its owned files with the kinds and testness every route to
/// them gives, then the files it reaches as library code.
type Kept = (PathBuf, Vec<(PathBuf, FileKind, TestCode)>, Vec<PathBuf>);

/// The shared fixture's packages, before ownership: each by its manifest, its reached files
/// with the route facts.
fn reached() -> ByManifest {
    let package = |manifest: &str, sources: &[(&str, FileKind, Reach, TestCode)]| {
        let metadata = PackageMetadata {
            manifest_path: PathBuf::from(manifest),
            kind: PackageKind::Library,
            edition: Edition::Edition2024,
            dependencies: Vec::new(),
            targets: Vec::new(),
        };
        let sources = sources
            .iter()
            .map(|(path, kind, reach, test)| Source {
                file: TextFile {
                    path: PathBuf::from(path),
                    shown: PathBuf::from(path),
                    text: String::new(),
                },
                kind: *kind,
                code: RouteCode {
                    reach: *reach,
                    test: *test,
                },
            })
            .collect();
        let package = Reached {
            metadata,
            manifest: Looked::Neither,
            sources,
        };
        (PathBuf::from(manifest), package)
    };
    [
        package(
            "/ws/a/Cargo.toml",
            &[
                ("/ws/a/src/lib.rs", ModRs, Library, NotTest),
                ("/ws/a/src/tests.rs", NonModRs, NotLibrary, Test),
                // Reached again by `/ws/b` as library code, so test silence gives there.
                ("/ws/a/src/x.rs", NonModRs, NotLibrary, Test),
                ("/ws/a/src/y.rs", ModRs, Library, NotTest),
            ],
        ),
        package(
            "/ws/b/Cargo.toml",
            &[
                // `a/src/x.rs` also reached through a `#[path]` route, which gives `ModRs`.
                ("/ws/a/src/x.rs", ModRs, Library, NotTest),
                ("/ws/b/src/lib.rs", ModRs, Library, NotTest),
                ("/ws/c/src/owned.rs", NonModRs, Library, NotTest),
            ],
        ),
        package(
            "/ws/c/Cargo.toml",
            &[
                ("/ws/a/src/y.rs", ModRs, NotLibrary, Test),
                ("/ws/c/src/owned.rs", ModRs, NotLibrary, Test),
            ],
        ),
        package(
            "/ws/e/Cargo.toml",
            &[("/ws/a/src/y.rs", ModRs, NotLibrary, Test)],
        ),
    ]
    .into_iter()
    .collect()
}

/// - `owned_sources` over the shared fixture: package `/ws/a` owns `x.rs` and `y.rs`, which
///   other packages reach too; `/ws/c` owns `owned.rs`, which `/ws/b` reaches first in
///   manifest order; every file `/ws/e` reaches, `/ws/a` owns.
fn kept() -> Vec<Kept> {
    owned_sources(reached())
        .into_iter()
        .map(|(manifest, package)| {
            (
                manifest,
                package
                    .sources
                    .into_iter()
                    .map(|source| (source.file.path, source.kind, source.test))
                    .collect(),
                package.library_code.into_iter().collect(),
            )
        })
        .collect()
}

#[test]
fn each_package_keeps_the_files_it_owns_and_its_library_code() {
    let paths = |paths: &[&str]| paths.iter().map(PathBuf::from).collect::<Vec<_>>();
    let owned = |files: &[(&str, FileKind, TestCode)]| {
        files
            .iter()
            .map(|(path, kind, test)| (PathBuf::from(path), *kind, *test))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kept(),
        [
            (
                PathBuf::from("/ws/a/Cargo.toml"),
                owned(&[
                    ("/ws/a/src/lib.rs", FileKind::ModRs, TestCode::NotTest),
                    ("/ws/a/src/tests.rs", FileKind::NonModRs, TestCode::Test),
                    // A `NonModRs` route of any package makes it `NonModRs`.
                    ("/ws/a/src/x.rs", FileKind::NonModRs, TestCode::NotTest),
                    ("/ws/a/src/y.rs", FileKind::ModRs, TestCode::NotTest),
                ]),
                paths(&["/ws/a/src/lib.rs", "/ws/a/src/y.rs"]),
            ),
            (
                PathBuf::from("/ws/b/Cargo.toml"),
                owned(&[("/ws/b/src/lib.rs", FileKind::ModRs, TestCode::NotTest)]),
                paths(&["/ws/a/src/x.rs", "/ws/b/src/lib.rs", "/ws/c/src/owned.rs"]),
            ),
            (
                // - `/ws/c` sorts after `/ws/b`, which also reaches `owned.rs`: the owner wins
                //   even though the fold meets the reacher first, and a `NonModRs` route of
                //   any package still makes it `NonModRs`.
                PathBuf::from("/ws/c/Cargo.toml"),
                owned(&[("/ws/c/src/owned.rs", FileKind::NonModRs, TestCode::NotTest)]),
                paths(&[]),
            ),
            (
                // - Every file this package reaches, a stronger claimant owns: its cut keeps
                //   no files, and it is still there, linted for its manifest and README.
                PathBuf::from("/ws/e/Cargo.toml"),
                Vec::new(),
                paths(&[]),
            ),
        ]
    );
}
