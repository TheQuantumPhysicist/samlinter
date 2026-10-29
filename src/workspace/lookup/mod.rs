use anyhow::{Context, Result, bail};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf, is_separator};

/// - The file system as discovery uses it, so the module traversal runs in tests without disk.
/// - Only `look` (`entry`) and `FileAt::read` (`read`, on what `look` found a file) call it, so
///   every path is checked for symlinks the same way.
pub trait Files {
    /// The text of the file at `path`, which `look` found to be a file.
    fn read(&self, path: &Path) -> io::Result<String>;

    /// What `path`'s own last component is, a symlink not followed.
    fn entry(&self, path: &Path) -> io::Result<Unfollowed>;
}

/// What a path's own last component is, a symlink not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unfollowed {
    File,
    Directory,
    Symlink,
    /// Nothing, or something that is neither a file nor a directory: a socket, FIFO or device.
    Neither,
}

impl Unfollowed {
    fn of(file_type: fs::FileType) -> Self {
        if file_type.is_symlink() {
            Self::Symlink
        } else if file_type.is_file() {
            Self::File
        } else if file_type.is_dir() {
            Self::Directory
        } else {
            Self::Neither
        }
    }
}

/// - A missing path, one below a file, or a name too long to exist holds nothing.
/// - Any other failure stays an error, permission denied included: a file may hide there.
fn nothing_there(error: io::Error) -> io::Result<Unfollowed> {
    const MISSING: [io::ErrorKind; 3] = [
        io::ErrorKind::NotFound,
        io::ErrorKind::NotADirectory,
        io::ErrorKind::InvalidFilename,
    ];
    if MISSING.contains(&error.kind()) {
        Ok(Unfollowed::Neither)
    } else {
        Err(error)
    }
}

pub struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn entry(&self, path: &Path) -> io::Result<Unfollowed> {
        fs::symlink_metadata(path)
            .map(|metadata| Unfollowed::of(metadata.file_type()))
            .or_else(nothing_there)
    }
}

/// - The `exclude` entries of `samlinter.toml` once checked against the disk: absolute,
///   lexically normalized paths, each a file or a directory reached with no symlink on the way.
/// - The default covers nothing, for the lookups no entry applies to.
#[derive(Debug, Default)]
pub struct Excluded(Vec<PathBuf>);

impl Excluded {
    /// `entries` as the checked entries; only the `exclude` check builds them.
    pub const fn new(entries: Vec<PathBuf>) -> Self {
        Self(entries)
    }

    /// Whether `path`, absolute and lexically normalized, is an excluded file or lies below an
    /// excluded directory.
    pub fn covers(&self, path: &Path) -> bool {
        self.0.iter().any(|entry| path.starts_with(entry))
    }
}

/// - A path `look` found to be a file, lexically normalized; exact, since no component on the
///   way is a symlink.
/// - Only `look` makes one, so only a file found that way is read.
#[derive(Debug)]
pub struct FileAt(PathBuf);

impl FileAt {
    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn into_path(self) -> PathBuf {
        self.0
    }

    pub fn read(&self, files: &impl Files) -> io::Result<String> {
        files.read(&self.0)
    }
}

/// What a written path holds, walked by `look`.
#[derive(Debug)]
pub enum Looked {
    File(FileAt),
    /// By its lexically normalized path; exact, since no component on the way is a symlink.
    Directory(PathBuf),
    /// Nothing, or something that is neither a file nor a directory: a socket, FIFO or device.
    Neither,
    /// The walk entered a path an `exclude` entry covers and stopped there.
    Excluded,
    /// - Unknown: a `..` left the workspace root or a directory above it that is a symlink.
    /// - Only following the link tells where the path leads, and samlinter does not.
    AboveLinkedRoot,
}

/// Where a directory the walk enters or leaves lies.
enum Place {
    /// The workspace root or a directory above it, taken as given on the way in.
    Given,
    /// Any other path, inside the root or not: looked up when entered.
    Below,
}

impl Place {
    fn of(root_dir: &Path, path: &Path) -> Self {
        if root_dir.starts_with(path) {
            Self::Given
        } else {
            Self::Below
        }
    }
}

/// - How a written path ends; `Path::components` and `Path::strip_prefix` drop a trailing `/`
///   or `/.`.
/// - Such a path names a directory, so no file is there: the OS and rustc agree.
enum Ending {
    Name,
    /// A trailing `/`.
    Separator,
    /// A trailing `/.`.
    SeparatorDot,
}

impl Ending {
    fn of(written: &Path) -> Self {
        let bytes = written.as_os_str().as_encoded_bytes();
        let (before_dot, dot) = match bytes.strip_suffix(b".") {
            Some(before_dot) => (before_dot, Self::SeparatorDot),
            None => (bytes, Self::Separator),
        };
        match before_dot.last() {
            Some(&byte) if is_separator(char::from(byte)) => dot,
            Some(_) | None => Self::Name,
        }
    }
}

/// - What `written` holds, walked one written component at a time, no symlink followed.
/// - Each name is looked up as it is entered, so a symlink that a later `..` leaves is still
///   met; `..` and `.` then resolve lexically.
/// - The workspace root and its ancestors are taken as given on the way in; any other
///   component that is a symlink stops the run, outside the root too.
/// - The walk stops at the first component `excluded` covers, before looking it up.
/// - Below a file or a missing component nothing is there, as on disk.
/// - A `..` leaving the root or a directory above it that is a symlink makes the rest unknown
///   (`leave`): the compiler would follow the link.
/// - Errors name paths as output shows them (`relative`).
pub fn look(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    written: &Path,
) -> Result<Looked> {
    let looked = written.components().try_fold(
        Looked::Directory(PathBuf::new()),
        |looked, component| match (component, looked) {
            // `.` keeps what the walk found; nothing below an unanswered path is looked up.
            (
                Component::CurDir,
                looked @ (Looked::File(_)
                | Looked::Directory(_)
                | Looked::Neither
                | Looked::Excluded
                | Looked::AboveLinkedRoot),
            )
            | (
                Component::Prefix(_)
                | Component::RootDir
                | Component::Normal(_)
                | Component::ParentDir,
                looked @ (Looked::Excluded | Looked::AboveLinkedRoot),
            ) => Ok(looked),
            (Component::Prefix(_) | Component::RootDir, Looked::Directory(dir)) => {
                Ok(Looked::Directory(joined(dir, component)))
            }
            (Component::Normal(_), Looked::Directory(dir)) => {
                enter(files, root_dir, excluded, joined(dir, component))
            }
            (Component::ParentDir, Looked::Directory(dir)) => leave(files, root_dir, dir),
            (
                Component::Prefix(_)
                | Component::RootDir
                | Component::Normal(_)
                | Component::ParentDir,
                Looked::File(_) | Looked::Neither,
            ) => Ok(Looked::Neither),
        },
    )?;
    Ok(match (looked, Ending::of(written)) {
        (Looked::File(file), Ending::Name) => Looked::File(file),
        (Looked::File(_), Ending::Separator | Ending::SeparatorDot) => Looked::Neither,
        (
            looked @ (Looked::Directory(_)
            | Looked::Neither
            | Looked::Excluded
            | Looked::AboveLinkedRoot),
            Ending::Name | Ending::Separator | Ending::SeparatorDot,
        ) => looked,
    })
}

/// What `path`, entered from the directory it sits in, holds.
fn enter(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    path: PathBuf,
) -> Result<Looked> {
    match (excluded.covers(&path), Place::of(root_dir, &path)) {
        (true, Place::Given | Place::Below) => Ok(Looked::Excluded),
        (false, Place::Given) => Ok(Looked::Directory(path)),
        (false, Place::Below) => step(files, root_dir, path),
    }
}

/// What `path`, below the root or outside it, holds; a symlink stops the run.
fn step(files: &impl Files, root_dir: &Path, path: PathBuf) -> Result<Looked> {
    let unfollowed = files
        .entry(&path)
        .with_context(|| format!("cannot inspect {}", relative(root_dir, &path).display()))?;
    match unfollowed {
        Unfollowed::File => Ok(Looked::File(FileAt(path))),
        Unfollowed::Directory => Ok(Looked::Directory(path)),
        Unfollowed::Neither => Ok(Looked::Neither),
        Unfollowed::Symlink => bail!(
            "{}: samlinter does not follow symlinks",
            relative(root_dir, &path).display()
        ),
    }
}

/// - What a `..` leaving the directory `dir` reaches.
/// - Below the root, `dir` was found a directory and no symlink when entered, so its parent is
///   the written one.
/// - The root and its ancestors were taken as given, so they are looked up now: one that is a
///   symlink has its parent where the link points, so the rest is unknown; one that is no
///   directory anymore is an error.
fn leave(files: &impl Files, root_dir: &Path, dir: PathBuf) -> Result<Looked> {
    match Place::of(root_dir, &dir) {
        Place::Below => Ok(Looked::Directory(parent(dir))),
        Place::Given => {
            let shown = relative(root_dir, &dir);
            let unfollowed = files
                .entry(&dir)
                .with_context(|| format!("cannot inspect {}", shown.display()))?;
            match unfollowed {
                Unfollowed::Directory => Ok(Looked::Directory(parent(dir))),
                Unfollowed::Symlink => Ok(Looked::AboveLinkedRoot),
                Unfollowed::File | Unfollowed::Neither => bail!(
                    "{}: the workspace root or a directory above it is not a directory",
                    shown.display()
                ),
            }
        }
    }
}

/// - `path` with `.` dropped and each `..` taking the name before it, the disk unread.
/// - Only for the workspace root, which is taken as given; every other path goes through
///   `look`, which checks each name before a `..` leaves it.
pub fn lexical(path: &Path) -> PathBuf {
    path.components()
        .fold(PathBuf::new(), |path, component| match component {
            Component::CurDir => path,
            Component::ParentDir => parent(path),
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                joined(path, component)
            }
        })
}

/// - `path` with `component` appended, in place.
/// - The walk grows one owned path, so a deep path costs linear time, not quadratic.
fn joined(mut path: PathBuf, component: Component<'_>) -> PathBuf {
    path.push(component);
    path
}

/// `path` without its last name, in place; the filesystem root stays, as on disk.
fn parent(mut path: PathBuf) -> PathBuf {
    // `false` only at the filesystem root or an empty path, which stay as they are.
    path.pop();
    path
}

/// - `path` as output shows it: relative to `root_dir` when inside it, `.` for the root itself,
///   absolute outside it.
/// - A trailing `/` or `/.` stays, since it says the path names a directory.
pub fn relative(root_dir: &Path, path: &Path) -> PathBuf {
    match (path.strip_prefix(root_dir), Ending::of(path)) {
        (Ok(inside), Ending::Name | Ending::Separator | Ending::SeparatorDot)
            if inside.as_os_str().is_empty() =>
        {
            PathBuf::from(".")
        }
        (Ok(inside), Ending::Name) => inside.to_path_buf(),
        (Ok(inside), Ending::Separator) => inside.join(""),
        (Ok(inside), Ending::SeparatorDot) => inside.join("."),
        (Err(_), Ending::Name | Ending::Separator | Ending::SeparatorDot) => path.to_path_buf(),
    }
}

#[cfg(test)]
pub mod memory;

#[cfg(test)]
mod tests;
