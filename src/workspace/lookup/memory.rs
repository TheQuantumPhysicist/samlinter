use super::{Ending, Files, Unfollowed, nothing_there};
use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

/// What an entry of `Memory` holds.
#[derive(Clone, Copy)]
pub enum Content {
    Text(&'static str),
    NotUtf8,
    Directory,
    /// A symlink; never followed, so its target is not modeled.
    Link,
    /// A file that is gone when read, as when it is deleted mid-run.
    Vanishing,
    /// A directory that may not be searched: it is a directory, every lookup through it is
    /// denied.
    Forbidden,
}

/// - An in-memory file system holding absolute paths.
/// - Every ancestor of an entry is a directory entry; `Memory::new` adds them.
/// - A path resolves one component at a time, as on disk: a missing component, or one below
///   a file, is an error even when a later `..` would leave it.
/// - A symlink is never followed: as the last component it is reported, anywhere else it
///   panics, since `look` stops at the first one.
pub struct Memory(BTreeMap<PathBuf, Content>);

impl Memory {
    /// - `entries`, the last one for a path winning, plus a directory for each ancestor.
    /// - Panics when an ancestor is held as something else: no disk holds that tree.
    pub fn new(entries: impl IntoIterator<Item = (PathBuf, Content)>) -> Self {
        let entries: BTreeMap<PathBuf, Content> = entries.into_iter().collect();
        let ancestors: Vec<PathBuf> = entries
            .keys()
            .flat_map(|path| path.ancestors().skip(1))
            .map(Path::to_path_buf)
            .collect();
        for ancestor in &ancestors {
            match entries.get(ancestor) {
                None | Some(Content::Directory | Content::Forbidden) => {}
                Some(Content::Text(_) | Content::NotUtf8 | Content::Link | Content::Vanishing) => {
                    panic!("{} holds entries but is no directory", ancestor.display());
                }
            }
        }

        let directories = ancestors
            .into_iter()
            .map(|ancestor| (ancestor, Content::Directory));
        Self(directories.chain(entries).collect())
    }

    /// - What `path`'s last component holds.
    /// - A path ending in a separator names a directory, so a file there is not found.
    fn resolve(&self, path: &Path) -> io::Result<Content> {
        let (_, content) = path.components().try_fold(
            (PathBuf::new(), Content::Directory),
            |(resolved, content), component| {
                within(content)?;
                match component {
                    Component::Prefix(_) | Component::RootDir => {
                        Ok((resolved.join(component), Content::Directory))
                    }
                    Component::CurDir => Ok((resolved, Content::Directory)),
                    // `..` at the root stays at the root, as on disk.
                    Component::ParentDir => {
                        let parent = resolved.parent().map(Path::to_path_buf);
                        Ok((parent.unwrap_or(resolved), Content::Directory))
                    }
                    Component::Normal(name) => self.step(resolved.join(name)),
                }
            },
        )?;
        match (content, Ending::of(path)) {
            (
                Content::Text(_) | Content::NotUtf8 | Content::Vanishing,
                Ending::Separator | Ending::SeparatorDot,
            ) => Err(io::Error::from(io::ErrorKind::NotADirectory)),
            (Content::Link, Ending::Separator | Ending::SeparatorDot) => {
                panic!("a trailing separator follows a symlink; `look` never asks that")
            }
            (
                content @ (Content::Text(_)
                | Content::NotUtf8
                | Content::Vanishing
                | Content::Link
                | Content::Directory
                | Content::Forbidden),
                Ending::Name,
            )
            | (
                content @ (Content::Directory | Content::Forbidden),
                Ending::Separator | Ending::SeparatorDot,
            ) => Ok(content),
        }
    }

    /// What `path`, whose parent is a resolved directory, holds.
    fn step(&self, path: PathBuf) -> io::Result<(PathBuf, Content)> {
        let content = self
            .0
            .get(&path)
            .copied()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok((path, content))
    }
}

/// Only a directory has entries below it, and only a searchable one lets them be looked up.
fn within(content: Content) -> io::Result<()> {
    match content {
        Content::Directory => Ok(()),
        Content::Forbidden => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        Content::Text(_) | Content::NotUtf8 | Content::Vanishing => {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
        Content::Link => panic!("a lookup went through a symlink; `look` stops at the first one"),
    }
}

impl Files for Memory {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.resolve(path)? {
            Content::Text(text) => Ok(text.to_owned()),
            Content::Vanishing => Err(io::Error::from(io::ErrorKind::NotFound)),
            Content::NotUtf8 => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )),
            Content::Directory => Err(io::Error::from(io::ErrorKind::IsADirectory)),
            // Opening it is denied before it is found to be a directory.
            Content::Forbidden => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            Content::Link => panic!("read a symlink; `look` reads only what it found a file"),
        }
    }

    fn entry(&self, path: &Path) -> io::Result<Unfollowed> {
        match self.resolve(path) {
            Ok(Content::Text(_) | Content::NotUtf8 | Content::Vanishing) => Ok(Unfollowed::File),
            Ok(Content::Directory | Content::Forbidden) => Ok(Unfollowed::Directory),
            Ok(Content::Link) => Ok(Unfollowed::Symlink),
            Err(error) => nothing_there(error),
        }
    }
}
