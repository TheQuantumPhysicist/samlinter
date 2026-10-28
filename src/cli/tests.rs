// - TODO: randomized variant: random argument lists of 0..4 tokens from a pool of flags and
//   paths parse only in the accepted shapes.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{Command, HELP, parse};
use rstest::rstest;
use std::ffi::OsString;
use std::path::PathBuf;

fn owned(arguments: &[&str]) -> impl Iterator<Item = OsString> {
    arguments
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>()
        .into_iter()
}

#[cfg(unix)]
fn non_utf8() -> OsString {
    std::os::unix::ffi::OsStringExt::from_vec(vec![b'x', 0x80])
}

#[cfg(windows)]
fn non_utf8() -> OsString {
    std::os::windows::ffi::OsStringExt::from_wide(&[u16::from(b'x'), 0xD800])
}

#[rstest]
#[case::no_arguments(&[], Command::Check { manifest_path: None })]
#[case::manifest_path(
    &["--manifest-path", "crates/a/Cargo.toml"],
    Command::Check { manifest_path: Some(PathBuf::from("crates/a/Cargo.toml")) }
)]
#[case::help(&["--help"], Command::Help)]
#[case::short_help(&["-h"], Command::Help)]
#[case::version(&["--version"], Command::Version)]
#[case::short_version(&["-V"], Command::Version)]
fn accepts_documented_arguments(#[case] arguments: &[&str], #[case] expected: Command) {
    assert_eq!(parse(owned(arguments)).unwrap(), expected);
}

/// One usage line per form `accepts_documented_arguments` parses.
#[rstest]
#[case::check("  samlinter [--manifest-path <Cargo.toml>]\n")]
#[case::help("  samlinter --help | -h\n")]
#[case::version("  samlinter --version | -V\n")]
fn help_shows_every_accepted_form(#[case] usage: &str) {
    assert!(HELP.contains(usage), "{HELP}");
}

#[test]
fn keeps_a_non_utf8_manifest_path() {
    let arguments = [OsString::from("--manifest-path"), non_utf8()];
    assert_eq!(
        parse(arguments.into_iter()).unwrap(),
        Command::Check {
            manifest_path: Some(PathBuf::from(non_utf8()))
        }
    );
}

#[test]
fn help_says_where_paths_are_relative_to() {
    assert!(
        HELP.contains("\nPaths are relative to the workspace root, absolute outside it.\n"),
        "{HELP}"
    );
}

#[rstest]
#[case::unknown_flag(&["--fix"], r#"["--fix"]"#)]
#[case::missing_path(&["--manifest-path"], r#"["--manifest-path"]"#)]
#[case::trailing_argument(&["--help", "extra"], r#"["--help", "extra"]"#)]
#[case::bare_path(&["Cargo.toml"], r#"["Cargo.toml"]"#)]
#[case::manifest_path_with_equals(&["--manifest-path=x"], r#"["--manifest-path=x"]"#)]
#[case::two_manifest_paths(&["--manifest-path", "a", "b"], r#"["--manifest-path", "a", "b"]"#)]
#[case::version_with_argument(&["--version", "x"], r#"["--version", "x"]"#)]
fn rejects_other_arguments(#[case] arguments: &[&str], #[case] shown: &str) {
    assert_eq!(
        parse(owned(arguments)).unwrap_err().to_string(),
        format!("unexpected arguments {shown}; run `samlinter --help`")
    );
}

/// The non-UTF-8 argument as the error shows it.
#[cfg(unix)]
const NON_UTF8_SHOWN: &str = r#"["x\x80"]"#;

#[cfg(windows)]
const NON_UTF8_SHOWN: &str = r#"["x\u{d800}"]"#;

#[test]
fn rejects_a_non_utf8_flag() {
    assert_eq!(
        parse([non_utf8()].into_iter()).unwrap_err().to_string(),
        format!("unexpected arguments {NON_UTF8_SHOWN}; run `samlinter --help`")
    );
}
