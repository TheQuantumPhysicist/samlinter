use super::{ByManifest, Owned, Reached};
use std::cmp::{self, Reverse};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// - Each package's reached sources cut down to the ones it owns, so a file two packages
///   reach, through `#[path]` say, is linted once, with its owner's edition.
/// - The owner is the reaching package whose directory holds the file, the innermost one when
///   packages nest; a file outside them all goes to the lowest manifest path.
pub fn owned_sources(reached: ByManifest) -> BTreeMap<PathBuf, Owned> {
    let claims: Vec<Reach> = reached
        .iter()
        .map(|(manifest, package)| Reach {
            manifest: manifest.clone(),
            files: package
                .sources
                .iter()
                .map(|source| source.path.clone())
                .collect(),
        })
        .collect();

    // `claims` is in `reached`'s order, so each package pairs with its own reach.
    reached
        .into_iter()
        .zip(&claims)
        .map(|((manifest, package), asking)| {
            let Reached {
                metadata,
                manifest: walked,
                sources,
            } = package;
            let sources = sources
                .into_iter()
                .filter(|source| owner(&claims, &source.path, asking) == manifest)
                .collect();
            let owned = Owned {
                metadata,
                manifest: walked,
                sources,
            };
            (manifest, owned)
        })
        .collect()
}

/// The files one package reaches.
struct Reach {
    /// The manifest path, lexically normalized; the package directory is its parent.
    manifest: PathBuf,
    files: BTreeSet<PathBuf>,
}

/// The manifest path of the package that keeps `file`, which the `asking` package reaches.
fn owner<'a>(reaches: &'a [Reach], file: &Path, asking: &'a Reach) -> &'a Path {
    reaches
        .iter()
        .filter(|reach| reach.files.contains(file))
        .fold(asking, |strongest, reach| {
            cmp::max_by_key(strongest, reach, |candidate| claim(file, candidate))
        })
        .manifest
        .as_path()
}

/// - How strongly the package of `reach` claims `file`, strongest greatest: the depth of the
///   package directory when it holds the file, then the lower manifest path.
/// - All paths are absolute and lexically normalized, with no symlink on the way, so the root
///   package holds no file outside the root directory.
fn claim<'a>(file: &Path, reach: &'a Reach) -> (Option<usize>, Reverse<&'a Path>) {
    let depth = reach
        .manifest
        .parent()
        .filter(|dir| file.starts_with(dir))
        .map(|dir| dir.components().count());
    (depth, Reverse(reach.manifest.as_path()))
}

#[cfg(test)]
mod tests;
