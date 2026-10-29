use super::super::lookup::memory::{Content, Memory};
use super::resolve;
use crate::config::ExcludeEntry;
use rstest::rstest;
use std::path::{Path, PathBuf};

/// The workspace `/ws` the entries are checked against.
fn workspace() -> Memory {
    Memory::new(
        [
            ("/ws/src/lib.rs", Content::Text("")),
            ("/ws/src/generated/mod.rs", Content::Text("")),
            ("/ws/f", Content::Forbidden),
            ("/ws/linked", Content::Link),
        ]
        .map(|(path, content)| (PathBuf::from(path), content)),
    )
}

/// `entry` checked against `workspace`: which of a few paths it covers, or the error.
fn checked(entry: &str) -> Result<Vec<&'static str>, String> {
    let entries = [ExcludeEntry::try_from(entry.to_owned()).unwrap()];
    resolve(&workspace(), Path::new("/ws"), &entries)
        .map(|excluded| {
            [
                "/ws",
                "/ws/src/lib.rs",
                "/ws/src/generated/mod.rs",
                "/ws/src/generatedx.rs",
                "/elsewhere/x.rs",
            ]
            .into_iter()
            .filter(|path| excluded.covers(Path::new(path)))
            .collect()
        })
        .map_err(|error| format!("{error:#}"))
}

#[rstest]
#[case::file("src/lib.rs", Ok(vec!["/ws/src/lib.rs"]))]
#[case::directory("src/generated", Ok(vec!["/ws/src/generated/mod.rs"]))]
#[case::directory_through_dot_dot("src/../src/generated/", Ok(vec!["/ws/src/generated/mod.rs"]))]
#[case::the_root(
    ".",
    Ok(vec!["/ws", "/ws/src/lib.rs", "/ws/src/generated/mod.rs", "/ws/src/generatedx.rs"])
)]
#[case::naming_nothing(
    "src/gone",
    Err("`exclude` entry `src/gone` names no file or directory".to_owned())
)]
#[case::below_a_file(
    "src/lib.rs/x",
    Err("`exclude` entry `src/lib.rs/x` names no file or directory".to_owned())
)]
#[case::file_ending_in_a_separator(
    "src/lib.rs/",
    Err("`exclude` entry `src/lib.rs/` names no file or directory".to_owned())
)]
#[case::through_a_symlink(
    "linked/x.rs",
    Err("`exclude` entry `linked/x.rs`: linked: samlinter does not follow symlinks".to_owned())
)]
#[case::naming_a_symlink(
    "linked",
    Err("`exclude` entry `linked`: linked: samlinter does not follow symlinks".to_owned())
)]
#[case::that_cannot_be_inspected(
    "f/x",
    Err("`exclude` entry `f/x`: cannot inspect f/x: permission denied".to_owned())
)]
fn entries_are_checked_against_the_disk(
    #[case] entry: &str,
    #[case] expected: Result<Vec<&str>, String>,
) {
    assert_eq!(checked(entry), expected);
}
