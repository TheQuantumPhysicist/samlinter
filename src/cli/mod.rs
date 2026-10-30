use anyhow::{Result, bail};
use std::ffi::OsString;
use std::path::PathBuf;

pub const HELP: &str = "\
samlinter: syntax gate for Rust workspace conventions

Usage:
  samlinter [--manifest-path <Cargo.toml>] [--directory <DIR>]
  samlinter --help | -h
  samlinter --version | -V

Needs cargo on PATH and samlinter.toml at the workspace root. Prints findings
grouped by rule, one per line as path:line:column: rule: message; whole-file
findings omit line:column.
Paths are relative to the workspace root, absolute outside it.
Every file (sources, manifests, READMEs, samlinter.toml) shows by its path, . and
.. resolved; binary_readme names where the missing README belongs. Symlinks are
not followed: a symlink samlinter reads or enters, other than the workspace root
and its parents, stops the run (exit 2).
Each group ends with why the rule exists, how to fix it, and which fixes only
hide the problem. The finding count goes to stderr.
--directory DIR runs as if the working directory were DIR, for this run only: a relative
--manifest-path resolves under DIR, and with no --manifest-path the workspace is found
from DIR. The shell's working directory never changes.
Exit status: 0 clean, 1 findings, 2 error.
";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Check {
        manifest_path: Option<PathBuf>,
        directory: Option<PathBuf>,
    },
    Help,
    Version,
}

/// - Arguments stay `OsString`: a path need not be UTF-8.
/// - Flags are matched as UTF-8 words; the path is taken from the same position unchanged.
pub fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Command> {
    let arguments: Vec<OsString> = arguments.collect();
    let words: Vec<(Option<&str>, &OsString)> = arguments
        .iter()
        .map(|argument| (argument.to_str(), argument))
        .collect();
    match words.as_slice() {
        [] => Ok(Command::Check {
            manifest_path: None,
            directory: None,
        }),
        [(Some("--manifest-path"), _), (_, path)] => Ok(Command::Check {
            manifest_path: Some(PathBuf::from(path)),
            directory: None,
        }),
        [(Some("--directory"), _), (_, directory)] => Ok(Command::Check {
            manifest_path: None,
            directory: Some(PathBuf::from(directory)),
        }),
        [
            (Some("--manifest-path"), _),
            (_, path),
            (Some("--directory"), _),
            (_, directory),
        ]
        | [
            (Some("--directory"), _),
            (_, directory),
            (Some("--manifest-path"), _),
            (_, path),
        ] => Ok(Command::Check {
            manifest_path: Some(PathBuf::from(path)),
            directory: Some(PathBuf::from(directory)),
        }),
        [(Some("--help" | "-h"), _)] => Ok(Command::Help),
        [(Some("--version" | "-V"), _)] => Ok(Command::Version),
        // Arguments are open-ended user input: every other list is a usage error.
        _ => bail!("unexpected arguments {arguments:?}; run `samlinter --help`"),
    }
}

#[cfg(test)]
mod tests;
