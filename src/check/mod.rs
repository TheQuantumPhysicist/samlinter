mod exemption;
mod manifest;
mod prose;
mod source;
mod toml_comments;

use crate::config::Config;
use crate::finding::Finding;
use crate::workspace::{Package, Readme, Root, Workspace};
use anyhow::Result;
use manifest::Role;
use std::collections::BTreeSet;

#[cfg(test)]
use crate::finding::Span;
#[cfg(test)]
use crate::workspace::TextFile;
#[cfg(test)]
use std::path::{Path, PathBuf};

/// - Every finding in the workspace, sorted by path as output shows it, position and rule.
/// - Identical findings print once: an exemption that names a rule twice reports one
///   unfulfilled finding.
/// - A finding in an excluded file is dropped here, the one place for every rule.
/// - An error is a finding samlinter cannot place, always a samlinter bug.
pub fn run(workspace: &Workspace) -> Result<Vec<Finding>> {
    let config = &workspace.settings;
    let root = root_findings(&workspace.root, config)?;
    let members = workspace
        .members
        .iter()
        .map(|package| member_findings(package, config))
        .collect::<Result<Vec<_>>>()?;

    let findings: BTreeSet<Finding> = root
        .into_iter()
        .chain(members.into_iter().flatten())
        .chain(toml_comments::check(
            &workspace.config,
            &config.forbidden_words,
        ))
        .filter(|finding| !workspace.excludes(&finding.path))
        .collect();
    Ok(findings.into_iter().collect())
}

fn root_findings(root: &Root, config: &Config) -> Result<Vec<Finding>> {
    let (manifest, files) = match root {
        Root::Virtual { manifest, readme } => (
            manifest::check(&Role::VirtualRoot(manifest), config)?,
            readme_findings(readme, config),
        ),
        Root::Package(package) => (
            manifest::check(&Role::RootPackage(package), config)?,
            package_files(package, config),
        ),
    };
    Ok([manifest, files].concat())
}

fn member_findings(package: &Package, config: &Config) -> Result<Vec<Finding>> {
    Ok([
        manifest::check(&Role::Member(package), config)?,
        package_files(package, config),
    ]
    .concat())
}

fn package_files(package: &Package, config: &Config) -> Vec<Finding> {
    let sources = package
        .sources
        .iter()
        .flat_map(|file| source::check(file, package.edition, config));
    sources
        .chain(readme_findings(&package.readme, config))
        .collect()
}

fn readme_findings(readme: &Readme, config: &Config) -> Vec<Finding> {
    match readme {
        Readme::Found(readme) => prose::check(&readme.text, &config.forbidden_words)
            .into_iter()
            .map(|hit| hit.into_finding(&readme.shown, &readme.text))
            .collect(),
        Readme::Missing { expected: _ } => Vec::new(),
    }
}

/// A file under `/workspace` that output shows as `shown`; an absolute `shown` lies outside it.
#[cfg(test)]
fn text_file(shown: &str, text: &str) -> TextFile {
    TextFile {
        path: Path::new("/workspace").join(shown),
        shown: PathBuf::from(shown),
        text: text.to_owned(),
    }
}

/// Root manifest profiles that satisfy `panic_strategy`.
#[cfg(test)]
const PROFILES: &str = "[profile.dev]\npanic = \"abort\"\n\n[profile.release]\npanic = \"abort\"\n";

/// Each finding as `<place> <rule>`.
#[cfg(test)]
fn render(findings: &[Finding]) -> Vec<String> {
    findings
        .iter()
        .map(|finding| format!("{} {}", place(finding.span), finding.rule.name()))
        .collect()
}

/// `line:column`, or `file` for a whole-file finding.
#[cfg(test)]
fn place(span: Span) -> String {
    match span {
        Span::File => "file".to_owned(),
        Span::At(position) => position.to_string(),
    }
}

#[cfg(test)]
mod tests;
