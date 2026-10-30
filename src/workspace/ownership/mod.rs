use super::{ByManifest, Looked, Owned, PackageMetadata, Reach, Reached, Source, SourceFile};
use std::cmp::{self, Reverse};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A package whose sources are being cut to the ones it owns: everything `Owned` needs except
/// the owned files themselves, which the fold over all packages fills in last.
struct Cut {
    metadata: PackageMetadata,
    /// As `Reached.manifest`.
    manifest: Looked,
    /// As `Owned.library_code`; computed from the package's own routes before the cut.
    library_code: BTreeSet<PathBuf>,
}

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

    // - One pass over every reached source of every package: each source joins the map of the
    //   package that owns it, its kind and route facts merged with the entry there, so a
    //   `NonModRs` route of any package makes the file `NonModRs` and one route that lints it
    //   keeps it linted.
    // - The package data rides along; no package's `Owned` is final until the last source is
    //   in, since a later package can own an earlier one's file.
    let mut files: BTreeMap<PathBuf, BTreeMap<PathBuf, SourceFile>> = BTreeMap::new();
    let mut cut: BTreeMap<PathBuf, Cut> = BTreeMap::new();
    for ((manifest, package), asking) in reached.into_iter().zip(&claimants) {
        let Reached {
            metadata,
            manifest: walked,
            sources,
        } = package;
        let library = library_code(&sources);
        for source in sources {
            match files
                .entry(owner(&claimants, &source.file.path, asking).to_path_buf())
                .or_default()
                .entry(source.file.path.clone())
            {
                Entry::Vacant(slot) => {
                    slot.insert(SourceFile {
                        file: source.file,
                        kind: source.kind,
                        test: source.code.test,
                    });
                }
                Entry::Occupied(mut slot) => {
                    slot.get_mut().kind = slot.get().kind.either(source.kind);
                    slot.get_mut().test = slot.get().test.either(source.code.test);
                }
            }
        }
        cut.insert(
            manifest,
            Cut {
                metadata,
                manifest: walked,
                library_code: library,
            },
        );
    }

    // - A package none of whose reached files it owns keeps no sources: an empty cut, not a
    //   missing one.
    cut.into_iter()
        .map(|(manifest, cut)| {
            let Cut {
                metadata,
                manifest: walked,
                library_code,
            } = cut;
            let sources = files
                .remove(&manifest)
                .unwrap_or_default()
                .into_values()
                .collect();
            (
                manifest,
                Owned {
                    metadata,
                    manifest: walked,
                    sources,
                    library_code,
                },
            )
        })
        .collect()
}

/// The paths of the `sources` reached as library code.
fn library_code(sources: &[Source]) -> BTreeSet<PathBuf> {
    sources
        .iter()
        .filter(|source| match source.code.reach {
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
