use super::{ByManifest, Owned, Reach, Reached, Source};
use std::cmp::{self, Reverse};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// - Each package's reached sources cut down to the ones it owns, so a file two packages
///   reach, through `#[path]` say, is linted once, with its owner's edition.
/// - The owner is the reaching package whose directory holds the file, the innermost one when
///   packages nest; a file outside them all goes to the lowest manifest path.
/// - Each package also keeps every file it reaches as library code, owned or not.
pub fn owned_sources(reached: ByManifest) -> BTreeMap<PathBuf, Owned> {
    let claimants: Vec<Claimant> = reached
        .iter()
        .map(|(manifest, package)| Claimant {
            manifest: manifest.clone(),
            files: package
                .sources
                .iter()
                .map(|source| source.file.path.clone())
                .collect(),
        })
        .collect();

    // `claimants` is in `reached`'s order, so each package pairs with its own claimant.
    reached
        .into_iter()
        .zip(&claimants)
        .map(|((manifest, package), asking)| {
            let Reached {
                metadata,
                manifest: walked,
                sources,
            } = package;
            let library_code = library_code(&sources);
            let sources = sources
                .into_iter()
                .filter(|source| owner(&claimants, &source.file.path, asking) == manifest)
                .map(|source| source.file)
                .collect();
            let owned = Owned {
                metadata,
                manifest: walked,
                sources,
                library_code,
            };
            (manifest, owned)
        })
        .collect()
}

/// The paths of the `sources` reached as library code.
fn library_code(sources: &[Source]) -> BTreeSet<PathBuf> {
    sources
        .iter()
        .filter(|source| match source.reach {
            Reach::Library => true,
            Reach::NotLibrary => false,
        })
        .map(|source| source.file.path.clone())
        .collect()
}

/// The files one package reaches.
struct Claimant {
    /// The manifest path, lexically normalized; the package directory is its parent.
    manifest: PathBuf,
    files: BTreeSet<PathBuf>,
}

/// The manifest path of the package that keeps `file`, which the `asking` package reaches.
fn owner<'a>(claimants: &'a [Claimant], file: &Path, asking: &'a Claimant) -> &'a Path {
    claimants
        .iter()
        .filter(|claimant| claimant.files.contains(file))
        .fold(asking, |strongest, claimant| {
            cmp::max_by_key(strongest, claimant, |candidate| claim(file, candidate))
        })
        .manifest
        .as_path()
}

/// - How strongly the package of `claimant` claims `file`, strongest greatest: the depth of the
///   package directory when it holds the file, then the lower manifest path.
/// - All paths are absolute and lexically normalized, with no symlink on the way, so the root
///   package holds no file outside the root directory.
fn claim<'a>(file: &Path, claimant: &'a Claimant) -> (Option<usize>, Reverse<&'a Path>) {
    let depth = claimant
        .manifest
        .parent()
        .filter(|dir| file.starts_with(dir))
        .map(|dir| dir.components().count());
    (depth, Reverse(claimant.manifest.as_path()))
}

#[cfg(test)]
mod tests;
