mod check;
mod cli;
mod config;
mod finding;
mod report;
mod workspace;

use anyhow::{Context, Result};
use cli::Command;
use config::Config;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;

enum Outcome {
    Clean,
    Findings(NonZeroUsize),
}

fn main() -> ExitCode {
    match cli::parse(std::env::args_os().skip(1)).and_then(execute) {
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

fn execute(command: Command) -> Result<Outcome> {
    match command {
        Command::Check { manifest_path } => lint(manifest_path.as_deref()),
        Command::Help => print(cli::HELP).map(|()| Outcome::Clean),
        Command::Version => {
            print(&format!("samlinter {}\n", env!("CARGO_PKG_VERSION"))).map(|()| Outcome::Clean)
        }
    }
}

fn lint(manifest_path: Option<&Path>) -> Result<Outcome> {
    let workspace = workspace::load(manifest_path)?;
    let config = Config::parse(&workspace.config.text)
        .with_context(|| format!("invalid {}", workspace.config.path.display()))?;

    let findings = check::run(&workspace, &config)?;
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
