use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Finding lines in report order: grouped by rule in rule order, then by path and position.
const VIOLATIONS: &[&str] = &[
    "app/Cargo.toml: binary_readme: binary package has no README at app/README.md",
    "README.md:1:11: em_dash: em-dash (U+2014)",
    "app/Cargo.toml:9:13: em_dash: em-dash (U+2014)",
    "app/src/main.rs:5:27: em_dash: em-dash (U+2014)",
    "app/src/main.rs:6:13: emoji: emoji U+1F389",
    "lib/Cargo.toml:9:2: feature_flags: `[features]` table",
    "app/src/main.rs:3:1: fn_spacing: no blank line between this function and the previous one",
    "app/src/main.rs:4:12: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
    "samlinter.toml:3:8: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
    "app/src/main.rs:2:1: item_spacing: no blank line between the last use item and this code",
    "lib/Cargo.toml:14:1: library_subscriber: library package depends on `tracing-subscriber`",
    "lib/Cargo.toml:17:1: library_subscriber: library package depends on `tracing-subscriber`",
    "lib/src/lib.rs:1:1: malformed_exemption: rule `em_dash` cannot be exempted",
    "app/Cargo.toml:8:1: member_version: `anyhow` declares its version here; move the version to the root `[workspace.dependencies]` and write `anyhow = { workspace = true }`",
    "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
    "Cargo.toml:11:10: panic_strategy: `[profile.dev]` does not set `panic = \"abort\"`",
    "app/src/main.rs:8:20: rest_pattern: `..` absorbs every field this pattern does not name",
    "app/src/main.rs:11:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "lib/Cargo.toml:7:1: unfulfilled_exemption: exemption for `member_version` on `serde` matches nothing",
    "app/Cargo.toml:7:1: workspace_dependency_form: write `strum = { workspace = true, ... }` as one inline table",
];

/// - Every file the module tree reaches, printed by its canonical path: `tests/again.rs`
///   reaches `src/from_tests.rs` through `../`, and the symlink `src/linked.rs` reaches
///   `src/elsewhere/q.rs`, which is linted once.
/// - Files outside the tree (build output, a hidden directory, an orphan `src/orphan.rs`)
///   hold the same violation and stay silent.
const MODULE_TREE: &[&str] = &[
    "src/a/b.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/a.rs:3:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/c/d.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/c/mod.rs:3:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/elsewhere/p.rs:3:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/elsewhere/q.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/from_tests.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/imp/unix.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/imp/windows.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "src/inline/e.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "tests/common/mod.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
];

const LIB_AND_BIN: &[&str] =
    &["Cargo.toml: binary_readme: binary package has no README at README.md"];

const ROOT_PACKAGE: &[&str] = &[
    "Cargo.toml: binary_readme: binary package has no README at README.md",
    "src/main.rs:4:16: rest_pattern: `..` absorbs every field this pattern does not name",
    "member/src/lib.rs:1:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
];

fn manifest(fixture: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture)
        .join("Cargo.toml")
}

fn command(arguments: &[&OsStr]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_samlinter"));
    command.args(arguments);
    command
}

fn run(arguments: &[&OsStr]) -> io::Result<Output> {
    command(arguments).output()
}

fn samlinter(fixture: &str) -> io::Result<Output> {
    run(&[OsStr::new("--manifest-path"), manifest(fixture).as_os_str()])
}

/// Lines starting in column one are findings; indented lines are guidance.
fn finding_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(' '))
        .collect()
}

/// Asserts the exact finding lines, exit status 1 and the finding count on stderr.
fn assert_findings(output: &Output, expected: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(finding_lines(&stdout), expected, "stderr: {stderr}");
    assert_eq!(
        stderr,
        format!("samlinter: {} finding(s)\n", expected.len())
    );
    assert_eq!(output.status.code(), Some(1));
}

/// Asserts exit status 2 and that stderr names every one of `parts`.
fn assert_error(output: &Output, parts: &[&str]) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    for part in parts {
        assert!(
            stderr.contains(part),
            "`{part}` missing from stderr: {stderr}"
        );
    }
}

#[cfg(unix)]
fn non_utf8() -> OsString {
    std::os::unix::ffi::OsStringExt::from_vec(vec![b'x', 0x80])
}

#[cfg(windows)]
fn non_utf8() -> OsString {
    std::os::windows::ffi::OsStringExt::from_wide(&[u16::from(b'x'), 0xD800])
}

/// Asserts empty output and exit status 0.
fn assert_clean(output: &Output) {
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn clean_workspace_prints_nothing_and_passes() {
    assert_clean(&samlinter("clean").unwrap());
}

#[test]
fn sources_parse_with_their_package_edition() {
    // `gen` is a keyword only from edition 2024 on.
    assert_clean(&samlinter("edition_2021").unwrap());
}

#[test]
fn package_with_a_library_and_a_binary_is_a_binary_package() {
    assert_findings(&samlinter("lib_and_bin").unwrap(), LIB_AND_BIN);
}

#[test]
fn findings_are_grouped_by_rule_and_fail_the_run() {
    assert_findings(&samlinter("violations").unwrap(), VIOLATIONS);
}

/// `violations_link` is a symlink to `violations`: paths print relative to the canonical root.
#[test]
fn workspace_behind_a_symlinked_directory_prints_the_same_paths() {
    assert_findings(&samlinter("violations_link").unwrap(), VIOLATIONS);
}

#[test]
fn member_manifest_lints_the_whole_workspace() {
    let member = manifest("violations")
        .with_file_name("app")
        .join("Cargo.toml");
    assert_findings(
        &run(&[OsStr::new("--manifest-path"), member.as_os_str()]).unwrap(),
        VIOLATIONS,
    );
}

#[test]
fn no_arguments_in_a_member_directory_lints_the_whole_workspace() {
    let member_dir = manifest("violations").with_file_name("app");
    assert_findings(
        &command(&[]).current_dir(member_dir).output().unwrap(),
        VIOLATIONS,
    );
}

#[test]
fn each_reported_rule_is_explained_once() {
    let stdout = String::from_utf8(samlinter("violations").unwrap().stdout).unwrap();
    let rules: BTreeSet<&str> = VIOLATIONS
        .iter()
        .filter_map(|line| line.split(": ").nth(1))
        .collect();
    assert_eq!(
        stdout.lines().filter(|line| *line == "  why:").count(),
        rules.len()
    );
}

#[test]
fn sources_are_the_files_mod_declarations_reach() {
    assert_findings(&samlinter("modules").unwrap(), MODULE_TREE);
}

#[test]
fn root_package_is_linted_with_its_members() {
    assert_findings(&samlinter("root_package").unwrap(), ROOT_PACKAGE);
}

#[test]
fn declared_module_without_a_file_is_an_error() {
    assert_error(
        &samlinter("missing_module").unwrap(),
        &[
            "src/lib.rs:2",
            "`mod gone;`",
            "src/gone.rs",
            "src/gone/mod.rs",
        ],
    );
}

#[test]
fn missing_config_is_an_error() {
    assert_error(
        &samlinter("unconfigured").unwrap(),
        &[
            "unconfigured/samlinter.toml",
            "every workspace needs one, see the README",
        ],
    );
}

#[test]
fn invalid_config_is_an_error() {
    assert_error(
        &samlinter("misconfigured").unwrap(),
        &["invalid", "misconfigured/samlinter.toml", "severity"],
    );
}

#[test]
fn unexpected_arguments_are_an_error() {
    assert_error(
        &run(&[OsStr::new("--fix")]).unwrap(),
        &["unexpected arguments", "run `samlinter --help`"],
    );
}

#[test]
fn absent_manifest_is_an_error() {
    assert_error(&samlinter("absent").unwrap(), &["`cargo metadata` failed"]);
}

#[test]
fn non_utf8_manifest_path_is_an_error_not_a_crash() {
    assert_error(
        &run(&[OsStr::new("--manifest-path"), &non_utf8()]).unwrap(),
        &["`cargo metadata` failed"],
    );
}

#[test]
fn unwritable_stdout_is_an_error_not_a_crash() {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = command(&[OsStr::new("--version")])
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_error(&output, &["cannot write to stdout"]);
}

#[test]
fn unwritable_stdout_during_a_run_with_findings_is_an_error() {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = command(&[
        OsStr::new("--manifest-path"),
        manifest("violations").as_os_str(),
    ])
    .stdout(writer)
    .stderr(Stdio::piped())
    .output()
    .unwrap();
    assert_error(&output, &["cannot write to stdout"]);
}

#[test]
fn unwritable_stderr_is_an_error_not_a_crash() {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = command(&[
        OsStr::new("--manifest-path"),
        manifest("violations").as_os_str(),
    ])
    .stdout(Stdio::piped())
    .stderr(writer)
    .output()
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn help_prints_usage_and_passes() {
    let output = run(&[OsStr::new("--help")]).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    // The binary's `HELP` is out of reach here, so its first and last lines pin the whole text.
    assert!(stdout.starts_with("samlinter: syntax gate for Rust workspace conventions\n"));
    assert!(stdout.ends_with("\nExit status: 0 clean, 1 findings, 2 error.\n"));
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn version_prints_the_package_version_and_passes() {
    let output = run(&[OsStr::new("--version")]).unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("samlinter {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(output.status.code(), Some(0));
}
