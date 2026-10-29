mod on_drop;

use super::memory::{Content, Memory};
use super::{Disk, Excluded, Files, Looked, Unfollowed, look, nothing_there, relative};
use on_drop::OnDrop;
use rstest::rstest;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

const ROOT: &str = "/ws";

/// - The contents that can also be written to a real disk, where a file stays put.
/// - So the `Disk` contract has no form for a file deleted mid-run.
#[derive(Clone, Copy)]
enum Lasting {
    Text(&'static str),
    NotUtf8,
    Directory,
    /// By its target's path relative to the root.
    Link(&'static str),
    /// A directory with no permissions, written after the entries below it; unix only.
    #[cfg(unix)]
    Forbidden,
}

impl From<Lasting> for Content {
    fn from(lasting: Lasting) -> Self {
        match lasting {
            Lasting::Text(text) => Self::Text(text),
            Lasting::NotUtf8 => Self::NotUtf8,
            Lasting::Directory => Self::Directory,
            Lasting::Link(_) => Self::Link,
            #[cfg(unix)]
            Lasting::Forbidden => Self::Forbidden,
        }
    }
}

/// The files `look_answers` walks, under the root `/ws`.
fn walked() -> Memory {
    Memory::new(
        [
            ("/ws/a/x.rs", Content::Text("")),
            ("/ws/generated/link.rs", Content::Link),
            ("/ws/generatedx.rs", Content::Text("")),
            ("/ws/linked", Content::Link),
            ("/elsewhere/y.rs", Content::Text("")),
        ]
        .map(|(path, content)| (PathBuf::from(path), content)),
    )
}

/// What `look` answers, as `file <path>`, `directory <path>`, `nothing`, `excluded` or `above`.
fn shown(looked: Looked) -> String {
    match looked {
        Looked::File(file) => format!("file {}", file.path().display()),
        Looked::Directory(path) => format!("directory {}", path.display()),
        Looked::Neither => "nothing".to_owned(),
        Looked::Excluded => "excluded".to_owned(),
        Looked::AboveLinkedRoot => "above".to_owned(),
    }
}

/// `look` on `walked` with `/ws/generated` excluded.
#[rstest]
#[case::file("/ws/a/x.rs", "file /ws/a/x.rs")]
#[case::directory("/ws/a", "directory /ws/a")]
#[case::directory_ending_in_a_separator("/ws/a/", "directory /ws/a")]
#[case::file_ending_in_a_separator("/ws/a/x.rs/", "nothing")]
#[case::file_ending_in_a_separator_and_a_dot("/ws/a/./x.rs/.", "nothing")]
#[case::dot_dot_below_the_root("/ws/a/../a/x.rs", "file /ws/a/x.rs")]
#[case::below_a_file("/ws/a/x.rs/y.rs", "nothing")]
#[case::missing("/ws/gone.rs", "nothing")]
#[case::dot_dot_after_a_missing_directory("/ws/gone/../a/x.rs", "nothing")]
#[case::excluded_directory("/ws/generated", "excluded")]
#[case::link_below_an_excluded_directory("/ws/generated/link.rs", "excluded")]
#[case::sibling_sharing_a_prefix("/ws/generatedx.rs", "file /ws/generatedx.rs")]
#[case::leaving_the_root_to_a_sibling("/ws/../elsewhere/y.rs", "file /elsewhere/y.rs")]
fn look_answers(#[case] written: &str, #[case] expected: &str) {
    let excluded = Excluded::new(vec![PathBuf::from("/ws/generated")]);
    let looked = look(&walked(), Path::new(ROOT), &excluded, Path::new(written)).unwrap();
    assert_eq!(shown(looked), expected);
}

#[rstest]
#[case::link_on_the_way("/ws/linked/x.rs", "linked: samlinter does not follow symlinks")]
#[case::link_left_by_dot_dot("/ws/linked/../a/x.rs", "linked: samlinter does not follow symlinks")]
fn look_stops_at_a_symlink(#[case] written: &str, #[case] expected: &str) {
    let error = look(
        &walked(),
        Path::new(ROOT),
        &Excluded::default(),
        Path::new(written),
    )
    .unwrap_err();
    assert_eq!(format!("{error:#}"), expected);
}

#[rstest]
#[case::inside_the_root("/ws/lib/src/lib.rs", "lib/src/lib.rs")]
#[case::the_root_manifest("/ws/Cargo.toml", "Cargo.toml")]
#[case::the_root_itself("/ws", ".")]
#[case::ending_in_a_separator("/ws/p/x.rs/", "p/x.rs/")]
#[case::ending_in_a_separator_and_a_dot("/ws/p/x.rs/.", "p/x.rs/.")]
#[case::member_outside_the_root("/elsewhere/src/lib.rs", "/elsewhere/src/lib.rs")]
#[case::sibling_with_a_shared_prefix("/ws2/src/lib.rs", "/ws2/src/lib.rs")]
fn paths_are_relative_to_the_root_when_inside_it(#[case] path: &str, #[case] expected: &str) {
    assert_eq!(
        relative(Path::new("/ws"), Path::new(path)),
        PathBuf::from(expected)
    );
}

/// `exclude = ["."]` covers the root and everything below it, never a path outside it.
#[rstest]
#[case::the_root("/ws", true)]
#[case::below_it("/ws/p/src/lib.rs", true)]
#[case::outside_it("/elsewhere/src/lib.rs", false)]
#[case::sibling_sharing_a_prefix("/ws2/src/lib.rs", false)]
fn excluded_root_covers_only_paths_inside_it(#[case] path: &str, #[case] expected: bool) {
    assert_eq!(
        Excluded::new(vec![PathBuf::from(ROOT)]).covers(Path::new(path)),
        expected
    );
}

#[test]
#[should_panic(expected = "/ws/a.rs holds entries but is no directory")]
fn memory_holds_no_entry_below_a_file() {
    Memory::new([
        (PathBuf::from("/ws/a.rs"), Content::Text("")),
        (PathBuf::from("/ws/a.rs/b.rs"), Content::Text("")),
    ]);
}

/// The files both `Files` implementations hold for the contract, by path relative to the root.
const CONTRACT_FILES: &[(&str, Lasting)] = &[
    ("text.rs", Lasting::Text("text")),
    ("link.rs", Lasting::Link("text.rs")),
    ("broken.rs", Lasting::Link("nowhere.rs")),
    ("loop.rs", Lasting::Link("loop.rs")),
    ("dir/inner.rs", Lasting::Text("")),
    ("dirlink", Lasting::Link("dir")),
    ("empty", Lasting::Directory),
    ("bad.rs", Lasting::NotUtf8),
];

/// - Contract files only a unix disk can hold: a directory that may not be searched.
/// - Mode 000 denies nothing to root, so run as uid 0 the disk answers differently and
///   `disk_keeps_the_files_contract` fails.
#[cfg(unix)]
const PLATFORM_FILES: &[(&str, Lasting)] = &[
    ("forbidden/inner.rs", Lasting::Text("")),
    ("forbidden", Lasting::Forbidden),
    ("deniedlink", Lasting::Link("forbidden/inner.rs")),
];
#[cfg(windows)]
const PLATFORM_FILES: &[(&str, Lasting)] = &[];

/// Every file of the contract, by path relative to the root.
fn contract_files() -> impl Iterator<Item = (&'static str, Lasting)> {
    CONTRACT_FILES.iter().chain(PLATFORM_FILES).copied()
}

/// The answers for `PLATFORM_FILES`.
#[cfg(unix)]
fn platform_contract() -> Vec<(&'static str, Answer)> {
    vec![
        ("forbidden", Answer::Directory),
        (
            "forbidden/inner.rs",
            Answer::Failed(io::ErrorKind::PermissionDenied),
        ),
        ("deniedlink", Answer::Symlink),
    ]
}

#[cfg(windows)]
fn platform_contract() -> Vec<(&'static str, Answer)> {
    Vec::new()
}

/// - What a `Files` implementation answers for one path, asked as `look` and `FileAt::read`
///   ask: `entry` first, then `read` for a file.
/// - Errors by kind.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    File(Result<String, io::ErrorKind>),
    Directory,
    Symlink,
    Neither,
    Failed(io::ErrorKind),
}

/// Every path the contract asks about, with the answers the disk gives.
fn contract() -> Vec<(&'static str, Answer)> {
    common_contract()
        .into_iter()
        .chain(platform_contract())
        .collect()
}

/// The answers for `CONTRACT_FILES`.
fn common_contract() -> Vec<(&'static str, Answer)> {
    vec![
        ("text.rs", Answer::File(Ok("text".to_owned()))),
        ("link.rs", Answer::Symlink),
        ("dir/../text.rs", Answer::File(Ok("text".to_owned()))),
        ("dir", Answer::Directory),
        ("dir/.", Answer::Directory),
        ("dirlink", Answer::Symlink),
        ("empty", Answer::Directory),
        ("dir/../empty", Answer::Directory),
        ("missing.rs", Answer::Neither),
        ("broken.rs", Answer::Symlink),
        ("loop.rs", Answer::Symlink),
        ("gone/../text.rs", Answer::Neither),
        ("gone/../empty", Answer::Neither),
        ("text.rs/x", Answer::Neither),
        ("text.rs/", Answer::Neither),
        ("text.rs/.", Answer::Neither),
        ("dir/", Answer::Directory),
        ("bad.rs", Answer::File(Err(io::ErrorKind::InvalidData))),
    ]
}

/// What `files` answers for every path of the contract, under `root`.
fn ask(files: &impl Files, root: &Path) -> Vec<(&'static str, Answer)> {
    contract()
        .into_iter()
        .map(|(path, _)| {
            let full = root.join(path);
            let answer = match files.entry(&full) {
                Ok(Unfollowed::File) => {
                    Answer::File(files.read(&full).map_err(|error| error.kind()))
                }
                Ok(Unfollowed::Directory) => Answer::Directory,
                Ok(Unfollowed::Symlink) => Answer::Symlink,
                Ok(Unfollowed::Neither) => Answer::Neither,
                Err(error) => Answer::Failed(error.kind()),
            };
            (path, answer)
        })
        .collect()
}

/// Writes `content` at `path` under `root`; a link's target is relative to `root`, as in
/// `Memory`.
fn write_on_disk(root: &Path, path: &str, content: Lasting) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    match content {
        Lasting::Text(text) => fs::write(&path, text).unwrap(),
        Lasting::NotUtf8 => fs::write(&path, [0x80]).unwrap(),
        Lasting::Directory => fs::create_dir(&path).unwrap(),
        Lasting::Link(target) => symlink(&root.join(target), &path).unwrap(),
        #[cfg(unix)]
        Lasting::Forbidden => set_mode(&path, 0o000).unwrap(),
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Windows needs the target's kind; `CONTRACT_FILES` lists every link after its target.
#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// Gives every forbidden contract directory under `scratch` its permissions back.
#[cfg(unix)]
fn allow_removal(scratch: &Path) {
    for (path, content) in contract_files() {
        match content {
            Lasting::Forbidden => set_mode(&scratch.join(path), 0o755)
                .unwrap_or_else(|error| eprintln!("cannot unlock {path}: {error}")),
            Lasting::Text(_) | Lasting::NotUtf8 | Lasting::Directory | Lasting::Link(_) => {}
        }
    }
}

/// Windows has no forbidden contract directory to unlock.
#[cfg(windows)]
fn allow_removal(_scratch: &Path) {}

#[test]
fn disk_keeps_the_files_contract() {
    let scratch = std::env::temp_dir().join(format!("samlinter-files-contract-{}", process::id()));
    fs::create_dir(&scratch).unwrap();
    let _cleanup = OnDrop(|| {
        allow_removal(&scratch);
        fs::remove_dir_all(&scratch)
            .unwrap_or_else(|error| eprintln!("cannot remove {}: {error}", scratch.display()));
    });
    for (path, content) in contract_files() {
        write_on_disk(&scratch, path, content);
    }

    assert_eq!(ask(&Disk, &scratch), contract());
}

/// A failed lookup means nothing is there only for a missing path, one below a file, or a name
/// too long; every other failure stays an error.
#[rstest]
#[case::not_found(io::ErrorKind::NotFound, Ok(Unfollowed::Neither))]
#[case::below_a_file(io::ErrorKind::NotADirectory, Ok(Unfollowed::Neither))]
#[case::name_too_long(io::ErrorKind::InvalidFilename, Ok(Unfollowed::Neither))]
#[case::denied(io::ErrorKind::PermissionDenied, Err(io::ErrorKind::PermissionDenied))]
#[case::invalid_input(io::ErrorKind::InvalidInput, Err(io::ErrorKind::InvalidInput))]
#[case::other(io::ErrorKind::Other, Err(io::ErrorKind::Other))]
fn nothing_there_only_for_a_missing_path(
    #[case] kind: io::ErrorKind,
    #[case] expected: Result<Unfollowed, io::ErrorKind>,
) {
    assert_eq!(
        nothing_there(io::Error::from(kind)).map_err(|error| error.kind()),
        expected
    );
}

/// A device is neither a file nor a directory.
#[cfg(unix)]
#[test]
fn disk_sees_a_device_as_neither() {
    assert_eq!(
        Disk.entry(Path::new("/dev/null")).unwrap(),
        Unfollowed::Neither
    );
}

#[test]
fn disk_passes_other_lookup_errors_through() {
    assert_eq!(
        Disk.entry(Path::new("a\0b")).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn memory_keeps_the_files_contract() {
    let memory = Memory::new(
        contract_files()
            .map(|(path, content)| (Path::new(ROOT).join(path), Content::from(content))),
    );
    assert_eq!(ask(&memory, Path::new(ROOT)), contract());
}
