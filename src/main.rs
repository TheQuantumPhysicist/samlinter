mod check;
mod cli;
mod config;
mod finding;
mod report;
mod syntax;
mod workspace;

use anyhow::{Context, Result, anyhow};
use cli::Command;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::thread;

/// - The parser and the syntax walks recurse once per nesting level in a file.
/// - The main thread's 8 MiB overflows near 3000 levels, in files a cfg can hide from rustc.
/// - Tested: 5000 levels, and 20000 for a file with a syntax error (`tests/cli.rs`).
/// - The parser's own drop thread never frees a deep tree (`syntax::parse`), so its 2 MiB stack
///   is no limit.
/// - TODO: a chain of about 200000 prefix operators overflows even this stack.
/// - Postponed: a sound pre-parse bound must follow every construct the parser recurses on,
///   not only delimiters, so it is a design decision.
/// - Plan: either bound nesting before parsing and report the file as an error, or move to a
///   parser release that frees trees without recursion on a default-stack thread.
const STACK_SIZE: usize = 256 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Clean,
    Findings(NonZeroUsize),
}

fn main() -> ExitCode {
    match run_on_stack(STACK_SIZE, || {
        cli::parse(std::env::args_os().skip(1)).and_then(execute)
    }) {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Findings(count)) => tell(&format!("{count} finding(s)"), ExitCode::from(1)),
        Err(error) => tell(&format!("{error:#}"), ExitCode::from(2)),
    }
}

/// - Prints `message` on stderr and exits with `code`.
/// - Exits 2 when stderr cannot be written: no channel is left to report that failure on.
fn tell(message: &str, code: ExitCode) -> ExitCode {
    match writeln!(io::stderr().lock(), "samlinter: {message}") {
        Ok(()) => code,
        Err(_) => ExitCode::from(2),
    }
}

/// A thread that fails to start or panics is an error.
fn run_on_stack(
    size: usize,
    work: impl FnOnce() -> Result<Outcome> + Send + 'static,
) -> Result<Outcome> {
    let thread = thread::Builder::new()
        .stack_size(size)
        .spawn(work)
        .with_context(|| format!("cannot start the lint thread with a {size}-byte stack"))?;
    // The panic hook has already printed the payload.
    thread
        .join()
        .map_err(|_| anyhow!("the lint thread panicked"))?
}

fn execute(command: Command) -> Result<Outcome> {
    match command {
        Command::Check {
            manifest_path,
            directory,
        } => lint(manifest_path.as_deref(), directory.as_deref()),
        Command::Help => print(cli::HELP).map(|()| Outcome::Clean),
        Command::Version => {
            print(&format!("samlinter {}\n", env!("CARGO_PKG_VERSION"))).map(|()| Outcome::Clean)
        }
    }
}

/// - Warnings go to stderr before the findings; they leave the exit status alone.
/// - A warning that cannot be written is an error, like any other output.
fn lint(manifest_path: Option<&Path>, directory: Option<&Path>) -> Result<Outcome> {
    let workspace = workspace::load(manifest_path, directory)?;
    workspace.warnings.iter().try_for_each(|warning| {
        writeln!(io::stderr().lock(), "samlinter: warning: {warning}")
            .context("cannot write to stderr")
    })?;

    let findings = check::run(&workspace)?;
    print(&report::render(&findings))?;
    Ok(NonZeroUsize::new(findings.len()).map_or(Outcome::Clean, Outcome::Findings))
}

/// A closed pipe, as in `samlinter | head`, is an error, not a crash.
fn print(text: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .context("cannot write to stdout")
}

#[cfg(test)]
mod tests;
