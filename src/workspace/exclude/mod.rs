use super::lookup::{Excluded, Files, Looked, look};
use crate::config::ExcludeEntry;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// - The `exclude` entries checked against the disk, each kept as its absolute, lexically
///   normalized path.
/// - An entry must name a file or a directory with no symlink on the way, so a stale or
///   mistyped one cannot silently exclude nothing, or something else.
pub fn resolve(files: &impl Files, root_dir: &Path, entries: &[ExcludeEntry]) -> Result<Excluded> {
    entries
        .iter()
        .map(|entry| checked(files, root_dir, entry.as_str()))
        .collect::<Result<_>>()
        .map(Excluded::new)
}

fn checked(files: &impl Files, root_dir: &Path, entry: &str) -> Result<PathBuf> {
    let looked = look(files, root_dir, &Excluded::default(), &root_dir.join(entry))
        .with_context(|| format!("`exclude` entry `{entry}`"))?;
    match looked {
        Looked::File(file) => Ok(file.into_path()),
        Looked::Directory(path) => Ok(path),
        Looked::Neither => bail!("`exclude` entry `{entry}` names no file or directory"),
        Looked::Excluded | Looked::AboveLinkedRoot => unreachable!(
            "no entry is checked yet, and an `ExcludeEntry` never climbs above the root"
        ),
    }
}

#[cfg(test)]
mod tests;
