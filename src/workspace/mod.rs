mod modules;
mod ownership;

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::cargo_platform::Platform;
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, TargetKind};
use modules::{Candidate, FileKind};
use ownership::owned_sources;
use ra_ap_syntax::Edition;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "Cargo.toml";

const README: &str = "README.md";

const CONFIG: &str = "samlinter.toml";

pub struct TextFile {
    /// The file's canonical absolute path, so `tests/../src/x.rs` or a symlink names the file
    /// it reaches.
    pub path: PathBuf,
    /// `path` as output shows it: relative to the workspace root, absolute outside it.
    pub shown: PathBuf,
    pub text: String,
}

impl TextFile {
    fn new(root_dir: &Path, path: PathBuf, text: String) -> Self {
        Self {
            shown: relative(root_dir, &path),
            path,
            text,
        }
    }
}

pub struct Package {
    pub manifest: TextFile,
    pub kind: PackageKind,
    pub readme: Readme,
    pub edition: Edition,
    pub dependencies: Vec<Dependency>,
    /// The files this package owns among its target roots and the files their `mod`
    /// declarations reach, ordered by canonical path.
    pub sources: Vec<TextFile>,
}

/// The `README.md` in the directory cargo names a package by, the parent of the manifest path
/// as given: a symlinked manifest does not move it.
pub enum Readme {
    Found(TextFile),
    Missing {
        /// Where the README belongs, as output shows it.
        expected: PathBuf,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PackageKind {
    /// Has a binary target, even next to a library target.
    Binary,
    Library,
}

/// A dependency as `cargo metadata` resolves it, renames inherited from
/// `[workspace.dependencies]` included.
pub struct Dependency {
    /// The key it is declared under.
    pub key: String,
    /// The crate it names after any `package = "..."` rename.
    pub crate_name: String,
    pub section: Section,
    /// The platform of the `[target.<platform>]` table it is declared under.
    pub target: Option<Platform>,
}

/// A dependency table, named as Cargo.toml spells it; cargo accepts the underscore
/// spellings before edition 2024.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::EnumString)]
pub enum Section {
    #[strum(serialize = "dependencies")]
    Normal,
    #[strum(serialize = "dev-dependencies", serialize = "dev_dependencies")]
    Development,
    #[strum(serialize = "build-dependencies", serialize = "build_dependencies")]
    Build,
}

/// The root manifest either only declares the workspace or is a package too.
pub enum Root {
    Virtual { manifest: TextFile, readme: Readme },
    Package(Package),
}

pub struct Workspace {
    pub root: Root,
    /// Every workspace package except the root package, ordered by manifest path.
    pub members: Vec<Package>,
    /// `samlinter.toml` at the root.
    pub config: TextFile,
}

/// The file system as discovery uses it, so the module traversal runs in tests without disk.
trait Files {
    fn read(&self, path: &Path) -> io::Result<String>;

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    fn is_file(&self, path: &Path) -> bool;
}

struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// One `cargo metadata` package, as discovery uses it.
struct PackageMetadata {
    manifest_path: PathBuf,
    kind: PackageKind,
    edition: Edition,
    dependencies: Vec<Dependency>,
    /// Every target's root file.
    roots: Vec<PathBuf>,
}

/// - One way the module tree reaches a file; the file's `mod` declarations resolve against it.
/// - Two routes to one file, as `a.rs` and through `#[path]` say, can resolve different
///   children, so the walk follows each route once.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Route {
    /// The canonical directory of the path the route names.
    dir: PathBuf,
    /// The file name as the route names it: the compiler resolves a symlinked file's children
    /// beside the link.
    name: OsString,
    kind: FileKind,
}

impl Route {
    fn of(files: &impl Files, file: &Path, kind: FileKind) -> Result<Self> {
        let (dir, name) = file
            .parent()
            .zip(file.file_name())
            .with_context(|| format!("{} names no file", file.display()))?;
        Ok(Self {
            dir: canonical(files, dir)?,
            name: name.to_os_string(),
            kind,
        })
    }

    fn file(&self) -> PathBuf {
        self.dir.join(&self.name)
    }
}

/// Source files read so far, by the route that reached each.
type Sources = BTreeMap<Route, TextFile>;

/// One package as discovery reads it, before ownership is decided.
struct Reached {
    metadata: PackageMetadata,
    /// The canonical form of the directory cargo names the package by, the parent of the
    /// manifest path as given: a symlinked manifest does not move it.
    dir: PathBuf,
    /// The source files it reaches.
    sources: Vec<TextFile>,
}

/// One package once ownership is decided, before it becomes a `Package`.
struct Owned {
    metadata: PackageMetadata,
    /// As `Reached.dir`.
    dir: PathBuf,
    /// The reached source files it owns.
    sources: Vec<TextFile>,
}

/// Each package by its canonical manifest path.
type ByManifest = BTreeMap<PathBuf, Reached>;

pub fn load(manifest_path: Option<&Path>) -> Result<Workspace> {
    let metadata = metadata(manifest_path)?;
    let packages = metadata
        .workspace_packages()
        .into_iter()
        .map(package_metadata)
        .collect::<Result<Vec<_>>>()?;
    assemble(&Disk, metadata.workspace_root.as_std_path(), packages)
}

fn metadata(manifest_path: Option<&Path>) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();
    if let Some(path) = manifest_path {
        command.manifest_path(path);
    }
    command.exec().context("`cargo metadata` failed")
}

fn package_metadata(package: &cargo_metadata::Package) -> Result<PackageMetadata> {
    let manifest_path = package.manifest_path.as_std_path().to_path_buf();
    let edition = edition(&manifest_path, package.edition.as_str())?;
    let dependencies = dependencies(&manifest_path, &package.dependencies)?;
    Ok(PackageMetadata {
        kind: package_kind(package),
        edition,
        dependencies,
        roots: package
            .targets
            .iter()
            .map(|target| target.src_path.as_std_path().to_path_buf())
            .collect(),
        manifest_path,
    })
}

/// Reads every package's sources, gives each source file to one package, builds the packages,
/// splits off the root, then reads the config.
fn assemble(
    files: &impl Files,
    workspace_root: &Path,
    packages: Vec<PackageMetadata>,
) -> Result<Workspace> {
    // Canonical, like the file paths compared with it.
    let root_dir = canonical(files, workspace_root)?;
    let reached = packages
        .into_iter()
        .map(|package| reached_sources(files, &root_dir, package))
        .collect::<Result<Vec<_>>>()?;
    let reached = by_manifest(&root_dir, reached)?;

    let packages = owned_sources(reached)
        .into_iter()
        .map(|(manifest, package)| Ok((manifest, load_package(files, &root_dir, package)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;

    let (root, members) = split_root(files, &root_dir, packages)?;
    Ok(Workspace {
        root,
        members,
        config: config(files, &root_dir)?,
    })
}

/// The package's canonical manifest path, and the package with every source file it reaches.
fn reached_sources(
    files: &impl Files,
    root_dir: &Path,
    package: PackageMetadata,
) -> Result<(PathBuf, Reached)> {
    let dir = package.manifest_path.parent().with_context(|| {
        format!(
            "{} has no parent directory",
            package.manifest_path.display()
        )
    })?;
    let dir = canonical(files, dir)?;
    let sources = package
        .roots
        .iter()
        .try_fold(Sources::new(), |sources, root| {
            module_tree(
                files,
                root_dir,
                package.edition,
                sources,
                root,
                FileKind::ModRs,
            )
        })?;
    Ok((
        canonical(files, &package.manifest_path)?,
        Reached {
            metadata: package,
            dir,
            sources: one_per_file(sources),
        },
    ))
}

/// - `reached` keyed by canonical manifest path.
/// - Two packages resolving to one manifest, through a symlink say, are an error: either
///   would hide the other.
fn by_manifest(root_dir: &Path, reached: Vec<(PathBuf, Reached)>) -> Result<ByManifest> {
    reached
        .into_iter()
        .try_fold(
            ByManifest::new(),
            |mut packages, (manifest, package)| match packages.entry(manifest) {
                Entry::Vacant(slot) => {
                    slot.insert(package);
                    Ok(packages)
                }
                Entry::Occupied(slot) => Err(anyhow!(
                    "{}: two workspace packages resolve to this manifest",
                    relative(root_dir, slot.key()).display()
                )),
            },
        )
}

/// - The package whose manifest is the root `Cargo.toml` is the root package.
/// - Without one, the root is virtual.
/// - Every other package is a member.
/// - `packages` is keyed by canonical manifest path, so the root manifest is looked up by its
///   canonical path too.
fn split_root(
    files: &impl Files,
    root_dir: &Path,
    mut packages: BTreeMap<PathBuf, Package>,
) -> Result<(Root, Vec<Package>)> {
    let root_manifest = canonical(files, &root_dir.join(MANIFEST))?;
    let root = match packages.remove(&root_manifest) {
        Some(package) => Root::Package(package),
        None => Root::Virtual {
            manifest: read(files, root_dir, &root_manifest)?,
            readme: readme(files, root_dir, root_dir)?,
        },
    };
    Ok((root, packages.into_values().collect()))
}

fn load_package(files: &impl Files, root_dir: &Path, owned: Owned) -> Result<Package> {
    let Owned {
        metadata,
        dir,
        sources,
    } = owned;
    Ok(Package {
        manifest: read(files, root_dir, &metadata.manifest_path)?,
        kind: metadata.kind,
        readme: readme(files, root_dir, &dir)?,
        edition: metadata.edition,
        dependencies: metadata.dependencies,
        sources,
    })
}

/// `samlinter.toml` at the root; every workspace needs one.
fn config(files: &impl Files, root_dir: &Path) -> Result<TextFile> {
    let path = root_dir.join(CONFIG);
    let text = files.read(&path).map_err(|error| {
        anyhow::Error::new(error).context(format!(
            "cannot read {}; every workspace needs one, see the README",
            path.display()
        ))
    })?;
    Ok(TextFile::new(root_dir, canonical(files, &path)?, text))
}

/// A file several routes reach is linted once, ordered by its canonical path.
fn one_per_file(sources: Sources) -> Vec<TextFile> {
    let by_path: BTreeMap<PathBuf, TextFile> = sources
        .into_values()
        .map(|source| (source.path.clone(), source))
        .collect();
    by_path.into_values().collect()
}

fn edition(manifest_path: &Path, edition: &str) -> Result<Edition> {
    edition.parse().ok().with_context(|| {
        format!(
            "{}: edition {edition} is newer than this samlinter understands; upgrade samlinter",
            manifest_path.display()
        )
    })
}

/// `sources` plus `file` and every file its `mod` declarations reach along this route.
fn module_tree(
    files: &impl Files,
    root_dir: &Path,
    edition: Edition,
    mut sources: Sources,
    file: &Path,
    kind: FileKind,
) -> Result<Sources> {
    let children = match sources.entry(Route::of(files, file, kind)?) {
        // Followed already, along with the files below it.
        Entry::Occupied(_) => Vec::new(),
        Entry::Vacant(slot) => {
            let route = slot.key();
            let file = route.file();
            let source =
                TextFile::new(root_dir, canonical(files, &file)?, read_text(files, &file)?);
            let children = child_files(files, root_dir, route, &source, edition)?;
            slot.insert(source);
            children
        }
    };

    children.iter().try_fold(sources, |sources, child| {
        module_tree(files, root_dir, edition, sources, &child.path, child.kind)
    })
}

/// - The files the `mod` declarations in `source` name, every existing candidate of each.
/// - A declaration with no file is an error: `cargo check` rejects it too unless cfg removes
///   the module, and skipping it would hide that module's code.
fn child_files(
    files: &impl Files,
    root_dir: &Path,
    route: &Route,
    source: &TextFile,
    edition: Edition,
) -> Result<Vec<Candidate>> {
    let shown = &source.shown;
    let declarations = modules::declarations(&source.text, edition).map_err(|bad| {
        anyhow!(
            "{}:{}: `path` attribute value is not a valid string literal",
            shown.display(),
            bad.line
        )
    })?;

    let found = declarations
        .iter()
        .map(|declaration| {
            let (existing, missing): (Vec<Candidate>, Vec<Candidate>) =
                modules::candidates(&route.dir, &route.name, route.kind, declaration)
                    .into_iter()
                    .partition(|candidate| files.is_file(&candidate.path));
            match existing.as_slice() {
                [] => bail!(
                    "{}:{}: no file for `mod {};`, tried {}",
                    shown.display(),
                    declaration.line,
                    declaration.module.name,
                    missing
                        .iter()
                        .map(|candidate| relative(root_dir, &candidate.path).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                [_, ..] => Ok(existing),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(found.into_iter().flatten().collect())
}

/// The dependencies of the package at `manifest_path`; an error names the manifest.
fn dependencies(
    manifest_path: &Path,
    dependencies: &[cargo_metadata::Dependency],
) -> Result<Vec<Dependency>> {
    dependencies
        .iter()
        .map(dependency)
        .collect::<Result<_>>()
        .with_context(|| manifest_path.display().to_string())
}

fn dependency(dependency: &cargo_metadata::Dependency) -> Result<Dependency> {
    let section = section(dependency.kind).with_context(|| {
        format!(
            "dependency `{}` has a kind this samlinter does not know; upgrade samlinter",
            dependency.name
        )
    })?;
    Ok(Dependency {
        key: dependency
            .rename
            .clone()
            .unwrap_or_else(|| dependency.name.clone()),
        crate_name: dependency.name.clone(),
        section,
        target: dependency.target.clone(),
    })
}

/// `None` for a kind newer than this samlinter.
const fn section(kind: DependencyKind) -> Option<Section> {
    match kind {
        DependencyKind::Normal => Some(Section::Normal),
        DependencyKind::Development => Some(Section::Development),
        DependencyKind::Build => Some(Section::Build),
        DependencyKind::Unknown => None,
    }
}

fn package_kind(package: &cargo_metadata::Package) -> PackageKind {
    if package
        .targets
        .iter()
        .any(|target| target.kind.contains(&TargetKind::Bin))
    {
        PackageKind::Binary
    } else {
        PackageKind::Library
    }
}

/// `Missing` only when there is no `README.md`: an unreadable one is an error, not a missing
/// one.
fn readme(files: &impl Files, root_dir: &Path, dir: &Path) -> Result<Readme> {
    let path = dir.join(README);
    match files.read(&path) {
        Ok(text) => Ok(Readme::Found(TextFile::new(
            root_dir,
            canonical(files, &path)?,
            text,
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Readme::Missing {
            expected: relative(root_dir, &path),
        }),
        Err(error) => Err(cannot_read(error, &path)),
    }
}

fn read(files: &impl Files, root_dir: &Path, path: &Path) -> Result<TextFile> {
    let text = read_text(files, path)?;
    Ok(TextFile::new(root_dir, canonical(files, path)?, text))
}

fn read_text(files: &impl Files, path: &Path) -> Result<String> {
    files.read(path).map_err(|error| cannot_read(error, path))
}

fn cannot_read(error: io::Error, path: &Path) -> anyhow::Error {
    anyhow::Error::new(error).context(format!("cannot read {}", path.display()))
}

fn canonical(files: &impl Files, path: &Path) -> Result<PathBuf> {
    files
        .canonicalize(path)
        .with_context(|| format!("cannot resolve {}", path.display()))
}

/// `path` relative to `root_dir`; a path outside it stays absolute.
fn relative(root_dir: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root_dir)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests;
