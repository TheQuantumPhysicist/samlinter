#[cfg_attr(
    samlinter,
    expect(
        path_attr,
        reason = "one OnDrop shared with the unit tests: this test cannot reach #[cfg(test)] code, and those tests ship in src/, the only published part"
    )
)]
#[path = "../src/workspace/lookup/tests/on_drop.rs"]
mod on_drop;

use on_drop::OnDrop;
use rstest::rstest;
use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// - Finding lines in report order: grouped by rule in rule order, then by path and position.
/// - `lib` lists a subscriber crate as a regular dependency: only its library code's install
///   call is a finding; its `#[cfg(test)]` module, `tests.rs`, integration test and example
///   install freely.
/// - `lib`'s em-dash exemption on `install` covers the doc comment above it.
const VIOLATIONS: &[&str] = &[
    "app/Cargo.toml: binary_readme: binary package has no README at app/README.md",
    "lib/src/lib.rs:10:5: discarded_result: `let _ =` discards a result without a stated reason",
    "README.md:1:11: em_dash: em-dash (U+2014)",
    "app/Cargo.toml:9:13: em_dash: em-dash (U+2014)",
    "app/src/main.rs:5:27: em_dash: em-dash (U+2014)",
    "app/src/main.rs:6:13: emoji: emoji U+1F389",
    "lib/Cargo.toml:9:2: feature_flags: `[features]` table",
    "app/src/main.rs:3:1: fn_spacing: no blank line between this function and the previous one",
    "app/src/main.rs:4:12: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
    "samlinter.toml:4:8: forbidden_word: `NOTES.md` is listed in forbidden-words in samlinter.toml",
    "app/src/main.rs:2:1: item_spacing: no blank line between the last use item and this code",
    "lib/src/lib.rs:9:36: library_subscriber: library installs a log subscriber: `.init()`",
    "lib/src/lib.rs:1:1: malformed_exemption: rule `syntax_error` cannot be exempted",
    "app/Cargo.toml:8:1: member_version: `anyhow` declares its version here; move the version to the root `[workspace.dependencies]` and write `anyhow = { workspace = true }`",
    "Cargo.toml: panic_strategy: no `[profile.release]` with `panic = \"abort\"`",
    "Cargo.toml:11:10: panic_strategy: `[profile.dev]` does not set `panic = \"abort\"`",
    "app/src/main.rs:8:20: rest_pattern: `..` absorbs every field this pattern does not name",
    "app/src/main.rs:11:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
    "lib/Cargo.toml:7:1: unfulfilled_exemption: exemption for `member_version` on `serde` matches nothing",
    "app/Cargo.toml:7:1: workspace_dependency_form: write `strum = { workspace = true, ... }` as one inline table",
];

/// - Every file the module tree reaches, printed by its normalized path: `tests/again.rs`
///   reaches `src/from_tests.rs` and `src/elsewhere/q.rs` through `../`.
/// - `src/a.rs` is reached as `a.rs` and declares `mod b;` and, inside an inline module,
///   `mod inner;`: two `module_shape` findings; the same shape in `src/c/mod.rs` is the
///   required one.
/// - Files outside the tree (build output, a hidden directory, an orphan `src/orphan.rs`) hold
///   the same violation and stay silent.
/// - Each `path` attribute the walk follows is itself a `path_attr` finding.
const MODULE_TREE: &[&str] = &[
    "src/a.rs:1:1: module_shape: the submodule `b` makes `a` a directory: move `a.rs` to `a/mod.rs`",
    "src/a.rs:6:5: module_shape: the submodule `inner` makes `a` a directory: move `a.rs` to `a/mod.rs`",
    "src/lib.rs:3:18: path_attr: `path` attribute moves the module away from the file its module path names",
    "src/lib.rs:4:21: path_attr: `path` attribute moves the module away from the file its module path names",
    "src/lib.rs:6:3: path_attr: `path` attribute moves the module away from the file its module path names",
    "tests/again.rs:1:3: path_attr: `path` attribute moves the module away from the file its module path names",
    "tests/again.rs:3:3: path_attr: `path` attribute moves the module away from the file its module path names",
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

/// Nesting levels in `deep_workspace`, past the main thread's stack limit, near 3000.
const DEPTH: usize = 5000;

/// Statements in the one function of `wide_workspace`.
const WIDTH: usize = 50_000;

/// `.init()` calls chained in `chain_workspace`, each one finding.
const CHAIN: usize = 20_000;

/// - What a run on a deep or wide workspace may take, in a debug build on a loaded machine.
/// - Each takes a second or two; a walk quadratic in the file size takes minutes.
const TIME_BUDGET: Duration = Duration::from_secs(60);

/// - Nesting levels in `deep_broken_workspace`.
/// - Past what the parser's own 2 MiB drop thread frees, between 10000 and 15000: a file with a
///   syntax error is always handed to that thread unless samlinter keeps its tree.
const BROKEN_DEPTH: usize = 20000;

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

/// - `layers` lists a subscriber crate as a regular dependency and installs nothing.
/// - `devlog` lists one only as a dev-dependency and calls `.init()` in library code.
#[test]
fn clean_workspace_prints_nothing_and_passes() {
    assert_clean(&samlinter("clean").unwrap());
}

#[test]
fn sources_parse_with_their_package_edition() {
    // `gen` is a keyword only from edition 2024 on.
    assert_clean(&samlinter("edition_2021").unwrap());
}

/// Its `lib.rs` and `main.rs` both install a subscriber, which a binary package may.
#[test]
fn package_with_a_library_and_a_binary_is_a_binary_package() {
    assert_findings(&samlinter("lib_and_bin").unwrap(), LIB_AND_BIN);
}

#[test]
fn findings_are_grouped_by_rule_and_fail_the_run() {
    assert_findings(&samlinter("violations").unwrap(), VIOLATIONS);
}

/// `violations_link` is a symlink to `violations`: the root is taken as given, so paths print
/// relative to it.
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

/// The `module_shape` finding with its full guidance; `sources_are_the_files_mod_declarations_reach`
/// pins the run's exit status 1.
#[test]
fn module_shape_findings_print_their_guidance() {
    let stdout = String::from_utf8(samlinter("modules").unwrap().stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains(": module_shape: "))
        .expect("the modules fixture declares a submodule in a leaf file");
    // The finding line and the indented guidance after it, ending at the next finding.
    // The group's findings first, the guidance once after them: a group per rule.
    let block: Vec<&str> = lines
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|line| line.starts_with(' ') || line.contains(": module_shape: "))
        .copied()
        .collect();
    assert_eq!(
        block,
        [
            "src/a.rs:1:1: module_shape: the submodule `b` makes `a` a directory: move `a.rs` to `a/mod.rs`",
            "src/a.rs:6:5: module_shape: the submodule `inner` makes `a` a directory: move `a.rs` to `a/mod.rs`",
            "  why:",
            "    - The module tree must mirror the file system: a module with submodules is a directory, so a reader finds every module from its path.",
            "  fix:",
            "    - Move the file into the directory shape: `git mv something.rs something/mod.rs`, keeping the submodules where they are.",
            "  bad fixes:",
            "    - Do not fake the directory with `#[path = \"something/mod.rs\"]`; `path_attr` reports every `path` attribute.",
            "    - Do not symlink the file into place; samlinter refuses symlinks.",
            "    - Do not exempt the finding without the project owner's approval.",
            "    - Do not add the file to `exclude` in samlinter.toml to silence a finding; `exclude` is for generated code.",
        ]
    );
}

#[test]
fn path_attribute_findings_print_their_guidance() {
    let stdout = String::from_utf8(samlinter("modules").unwrap().stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let after_last = lines
        .iter()
        .rposition(|line| line.contains(": path_attr: "))
        .and_then(|last| lines.get(last + 1..last + 3));
    assert_eq!(
        after_last,
        Some(
            [
                "  why:",
                "    - `#[path]` overrides Rust's file-name-to-module mapping, so a module can live anywhere and the module tree stops mirroring the file system.",
            ]
            .as_slice()
        )
    );
}

/// What the one package of a scratch workspace says about logging.
#[derive(Clone, Copy, Debug)]
enum Logging {
    /// No subscriber crate configured or depended on: `library_subscriber` reads nothing.
    Silent,
    /// The library lists a configured subscriber crate as a regular dependency, so
    /// `library_subscriber` walks every file.
    Subscriber,
}

/// The manifest and `samlinter.toml` of a one-package workspace named `name`.
fn scratch_config(name: &str, logging: Logging) -> [(&'static str, String); 2] {
    let (dependencies, subscribers) = match logging {
        Logging::Silent => ("", ""),
        Logging::Subscriber => (
            "[dependencies]\ntracing-subscriber = \"0.3\"\n\n",
            "\"tracing-subscriber\"",
        ),
    };
    [
        (
            "Cargo.toml",
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                 {dependencies}[workspace]\n\n[profile.dev]\npanic = \"abort\"\n\n\
                 [profile.release]\npanic = \"abort\"\n"
            ),
        ),
        (
            "samlinter.toml",
            format!(
                "forbidden-words = []\nforbidden-words-insensitive = []\nsubscriber-crates = [{subscribers}]\n"
            ),
        ),
    ]
}

/// - Writes a one-package workspace named `name` holding `sources` in a directory of its own
///   under the OS temp directory, one per process.
/// - Returns its manifest and a guard that removes the directory when dropped.
fn scratch_workspace(
    name: &str,
    sources: Vec<(&str, String)>,
) -> io::Result<(PathBuf, OnDrop<impl FnMut()>)> {
    logged_workspace(name, Logging::Silent, sources)
}

/// `scratch_workspace` with `logging`.
fn logged_workspace(
    name: &str,
    logging: Logging,
    sources: Vec<(&str, String)>,
) -> io::Result<(PathBuf, OnDrop<impl FnMut()>)> {
    let config = scratch_config(name, logging);
    let files: Vec<(&str, String)> = config.into_iter().chain(sources).collect();

    let root = env::temp_dir().join(format!("samlinter-{name}-{}", process::id()));
    fs::create_dir_all(root.join("src"))?;
    let manifest = root.join("Cargo.toml");
    let cleanup = OnDrop(move || {
        fs::remove_dir_all(&root)
            .unwrap_or_else(|error| eprintln!("cannot remove {}: {error}", root.display()));
    });
    for (path, text) in files {
        let path = manifest.with_file_name(path);
        fs::create_dir_all(path.parent().expect("every written source names a file"))?;
        fs::write(path, text)?;
    }
    Ok((manifest, cleanup))
}

/// - A workspace whose sources nest `DEPTH` levels deep.
/// - `src/lib.rs`: `DEPTH` parentheses around a `..` left of `=`.
/// - `src/deep.rs`: `DEPTH` inline modules around a `pub(crate)` item.
fn deep_workspace(logging: Logging) -> io::Result<(PathBuf, OnDrop<impl FnMut()>)> {
    logged_workspace(
        "deep",
        logging,
        vec![
            (
                "src/lib.rs",
                format!(
                    "mod deep;\n\npub fn f(t: ()) {{\n    {}..{} = t;\n}}\n",
                    "(".repeat(DEPTH),
                    ")".repeat(DEPTH)
                ),
            ),
            (
                "src/deep.rs",
                format!(
                    "{}pub(crate) struct S;\n{}",
                    "mod m {\n".repeat(DEPTH),
                    "}\n".repeat(DEPTH)
                ),
            ),
        ],
    )
}

/// A workspace whose `src/broken.rs` nests `BROKEN_DEPTH` inline modules around a `mod` with no
/// `;`.
fn deep_broken_workspace(logging: Logging) -> io::Result<(PathBuf, OnDrop<impl FnMut()>)> {
    logged_workspace(
        "deep_broken",
        logging,
        vec![
            ("src/lib.rs", "mod broken;\n".to_owned()),
            (
                "src/broken.rs",
                format!(
                    "{}mod gone\n{}",
                    "mod m {\n".repeat(BROKEN_DEPTH),
                    "}\n".repeat(BROKEN_DEPTH)
                ),
            ),
        ],
    )
}

/// Runs samlinter on `manifest`, asserting it finishes within `TIME_BUDGET`.
fn run_timed(manifest: &Path) -> io::Result<Output> {
    #[cfg_attr(
        samlinter,
        expect(
            system_time_now,
            reason = "asserts TIME_BUDGET on the run's own wall time; test instrumentation, not logic"
        )
    )]
    let started = Instant::now();
    let output = run(&[OsStr::new("--manifest-path"), manifest.as_os_str()])?;
    let elapsed = started.elapsed();
    assert!(elapsed < TIME_BUDGET, "took {elapsed:?}");
    Ok(output)
}

#[rstest]
#[case::silent(Logging::Silent)]
#[case::subscriber(Logging::Subscriber)]
fn deeply_nested_sources_are_linted_not_a_crash(#[case] logging: Logging) {
    let (manifest, _cleanup) = deep_workspace(logging).unwrap();
    assert_findings(
        &run_timed(&manifest).unwrap(),
        &[
            &format!(
                "src/lib.rs:4:{}: rest_pattern: `..` absorbs every field this pattern does not name",
                DEPTH + 5
            ),
            &format!(
                "src/deep.rs:{}:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy",
                DEPTH + 1
            ),
        ],
    );
}

#[rstest]
#[case::silent(Logging::Silent)]
#[case::subscriber(Logging::Subscriber)]
fn deeply_nested_syntax_error_is_reported_not_a_crash(#[case] logging: Logging) {
    let (manifest, _cleanup) = deep_broken_workspace(logging).unwrap();
    assert_findings(
        &run_timed(&manifest).unwrap(),
        &[&format!(
            "src/broken.rs:{}:9: syntax_error: expected `;` or `{{`",
            BROKEN_DEPTH + 1
        )],
    );
}

/// `WIDTH` statements in one function, none an install, in a library with a subscriber.
#[test]
fn wide_function_is_linted_in_linear_time() {
    let (manifest, _cleanup) = logged_workspace(
        "wide",
        Logging::Subscriber,
        vec![(
            "src/lib.rs",
            format!(
                "pub fn f(x: u8) -> u8 {{\n    x\n}}\n\npub fn g() {{\n{}}}\n",
                "    f(0);\n".repeat(WIDTH)
            ),
        )],
    )
    .unwrap();
    assert_clean(&run_timed(&manifest).unwrap());
}

/// `CHAIN` chained `.init()` calls in a library with a subscriber: one finding each.
#[test]
fn long_install_chain_is_linted_in_linear_time() {
    let (manifest, _cleanup) = logged_workspace(
        "chain",
        Logging::Subscriber,
        vec![(
            "src/lib.rs",
            format!("pub fn f(s: S) {{\n    s{};\n}}\n", ".init()".repeat(CHAIN)),
        )],
    )
    .unwrap();
    let expected: Vec<String> = (0..CHAIN)
        .map(|index| {
            format!(
                "src/lib.rs:2:{}: library_subscriber: library installs a log subscriber: `.init()`",
                7 + 7 * index
            )
        })
        .collect();
    let expected: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_findings(&run_timed(&manifest).unwrap(), &expected);
}

/// - A `mod gone;` inside `DEPTH` inline modules names the files it tried, `DEPTH` levels deep.
/// - Each level looks up a longer directory, so this pins that the walk stays fast.
#[test]
fn deeply_nested_missing_module_is_an_error_not_a_crash() {
    let (manifest, _cleanup) = scratch_workspace(
        "deep_missing",
        vec![
            ("src/lib.rs", "mod deep;\n".to_owned()),
            (
                "src/deep.rs",
                format!(
                    "{}mod gone;\n{}",
                    "mod m {\n".repeat(DEPTH),
                    "}\n".repeat(DEPTH)
                ),
            ),
        ],
    )
    .unwrap();
    let output = run(&[OsStr::new("--manifest-path"), manifest.as_os_str()]).unwrap();
    let dir = format!("src/deep/{}", "m/".repeat(DEPTH));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "samlinter: src/deep.rs:{}: no file for `mod gone;`, tried {dir}gone.rs, {dir}gone/mod.rs\n",
            DEPTH + 1
        )
    );
    assert_eq!(output.status.code(), Some(2));
}

/// A name too long for the file system names no file, as for rustc.
#[test]
fn path_attribute_too_long_for_a_file_name_is_a_missing_module() {
    let name = format!("{}.rs", "a".repeat(300));
    let (manifest, _cleanup) = scratch_workspace(
        "long_path",
        vec![("src/lib.rs", format!("#[path = \"{name}\"]\nmod x;\n"))],
    )
    .unwrap();
    let output = run(&[OsStr::new("--manifest-path"), manifest.as_os_str()]).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("samlinter: src/lib.rs:2: no file for `mod x;`, tried src/{name}\n")
    );
    assert_eq!(output.status.code(), Some(2));
}

/// - Writes a workspace whose `src/lib.rs` holds `lib`, with `src/real.rs` and `src/real/mod.rs`
///   present, then links `link` to `target` (both relative to the workspace).
/// - Returns the run's output.
#[cfg(unix)]
fn run_with_link(name: &str, lib: &str, link: &str, target: &str) -> io::Result<Output> {
    let (manifest, _cleanup) = scratch_workspace(
        name,
        vec![
            ("src/lib.rs", lib.to_owned()),
            ("src/real.rs", String::new()),
        ],
    )?;
    fs::create_dir(manifest.with_file_name("src/real"))?;
    fs::write(manifest.with_file_name("src/real/mod.rs"), "")?;
    std::os::unix::fs::symlink(
        manifest.with_file_name(target),
        manifest.with_file_name(link),
    )?;
    run(&[OsStr::new("--manifest-path"), manifest.as_os_str()])
}

#[cfg(unix)]
#[test]
fn symlinked_module_file_stops_the_run() {
    let output = run_with_link("linked_file", "mod a;\n", "src/a.rs", "src/real.rs").unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "samlinter: src/a.rs: samlinter does not follow symlinks\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn symlinked_module_directory_stops_the_run() {
    let output = run_with_link("linked_dir", "mod m;\n", "src/m", "src/real").unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "samlinter: src/m: samlinter does not follow symlinks\n"
    );
    assert_eq!(output.status.code(), Some(2));
}

/// - Writes a workspace named `name` whose `src/lib.rs` holds `lib`, links a scratch path to its
///   directory and runs the command `configure` sets up on the manifest through that link.
/// - Cargo then names the link as the workspace root.
#[cfg(unix)]
fn run_through_a_linked_root(
    name: &str,
    lib: &str,
    configure: impl FnOnce(&mut Command),
) -> io::Result<Output> {
    let (manifest, _cleanup) = scratch_workspace(name, vec![("src/lib.rs", lib.to_owned())])?;
    let link = env::temp_dir().join(format!("samlinter-{name}-link-{}", process::id()));
    let root = manifest
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    std::os::unix::fs::symlink(root, &link)?;
    let _unlink = OnDrop(|| {
        fs::remove_file(&link)
            .unwrap_or_else(|error| eprintln!("cannot remove {}: {error}", link.display()));
    });
    let mut command = command(&[
        OsStr::new("--manifest-path"),
        link.join("Cargo.toml").as_os_str(),
    ]);
    configure(&mut command);
    command.output()
}

/// - A `src/lib.rs` whose `mod x;` on line 3 climbs out of the workspace root.
/// - Its `path_attr` finding is exempted, so the warning is all a run reports about it.
const LINKED_ROOT_LIB: &str = "#[cfg_attr(samlinter, expect(path_attr, reason = \"climbs out of the root on purpose\"))]\n#[path = \"../../x.rs\"]\nmod x;\n";

/// The warning `LINKED_ROOT_LIB` gets in `run_through_a_linked_root`.
const LINKED_ROOT_WARNING: &str = "samlinter: warning: src/lib.rs:3: `#[path]` climbs above the symlinked workspace root; module not followed\n";

/// - `../../x.rs` leaves the linked root, so where it leads is unknown without following it.
/// - A warning names the declaration; the exit status stays that of a clean run.
#[cfg(unix)]
#[test]
fn path_climbing_out_of_a_symlinked_root_warns_and_is_not_followed() {
    let output = run_through_a_linked_root("linked_root", LINKED_ROOT_LIB, |_| ()).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stderr), LINKED_ROOT_WARNING);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(output.status.code(), Some(0));
}

/// Warnings come before the findings and leave their exit status alone.
#[cfg(unix)]
#[test]
fn warning_beside_a_finding_keeps_the_findings_exit_status() {
    let output = run_through_a_linked_root(
        "linked_root_finding",
        &format!("{LINKED_ROOT_LIB}\npub(crate) fn f() {{}}\n"),
        |_| (),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        finding_lines(&stdout),
        ["src/lib.rs:5:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy"]
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{LINKED_ROOT_WARNING}samlinter: 1 finding(s)\n")
    );
    assert_eq!(output.status.code(), Some(1));
}

/// A warning that cannot be written is an error, even on a run with no findings.
#[cfg(unix)]
#[test]
fn unwritable_stderr_with_only_a_warning_is_an_error() {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    let output = run_through_a_linked_root("linked_root_closed", LINKED_ROOT_LIB, |command| {
        command.stdout(Stdio::piped()).stderr(writer);
    })
    .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// An excluded file is not read: its syntax error, its finding and its missing module do not
/// count.
#[test]
fn excluded_files_are_not_linted() {
    let (manifest, _cleanup) = scratch_workspace(
        "excluded",
        vec![
            (
                "samlinter.toml",
                "forbidden-words = []\nforbidden-words-insensitive = []\nsubscriber-crates = []\nexclude = [\"src/generated.rs\"]\n"
                    .to_owned(),
            ),
            ("src/lib.rs", "mod generated;\n".to_owned()),
            (
                "src/generated.rs",
                "mod gone;\npub(crate) fn f( {}\n".to_owned(),
            ),
        ],
    )
    .unwrap();
    assert_clean(&run(&[OsStr::new("--manifest-path"), manifest.as_os_str()]).unwrap());
}

/// - rustc rejects a module found at both `a.rs` and `a/mod.rs`; samlinter walks and lints
///   both files, each by the kind of its own candidate, and the run exits by its findings
///   alone: no error, and clean here, so empty output and exit 0.
#[test]
fn a_file_and_a_mod_rs_directory_exit_by_their_findings() {
    let (manifest, _cleanup) = scratch_workspace(
        "both_shapes",
        vec![
            ("src/lib.rs", "mod a;\n".to_owned()),
            ("src/a.rs", String::new()),
            ("src/a/mod.rs", String::new()),
        ],
    )
    .unwrap();
    assert_clean(&run(&[OsStr::new("--manifest-path"), manifest.as_os_str()]).unwrap());
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
        &["cannot read samlinter.toml; every workspace needs one, see the README"],
    );
}

#[test]
fn invalid_config_is_an_error() {
    let output = samlinter("misconfigured").unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "samlinter: invalid samlinter.toml: TOML parse error at line 4, column 1\n  |\n4 | severity = \"warn\"\n  | ^^^^^^^^\nunknown field `severity`, expected one of `forbidden-words`, `forbidden-words-insensitive`, `subscriber-crates`, `exclude`\n\n"
    );
    assert_eq!(output.status.code(), Some(2));
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
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
    assert_eq!(output.status.code(), Some(0));
}
