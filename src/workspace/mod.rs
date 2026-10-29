mod exclude;
mod lookup;
mod modules;
mod ownership;

use crate::config::{Config, Settings};
use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::cargo_platform::Platform;
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, TargetKind};
pub use lookup::Excluded;
use lookup::{Disk, FileAt, Files, Looked, lexical, look, relative};
use modules::{Candidate, Declaration, Directory, FileKind};
use ownership::owned_sources;
use ra_ap_syntax::Edition;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "Cargo.toml";

const README: &str = "README.md";

const CONFIG: &str = "samlinter.toml";

pub struct TextFile {
    /// The file's absolute path, lexically normalized: `tests/../src/x.rs` is `src/x.rs`.
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
    /// declarations reach, ordered by path.
    pub sources: Vec<TextFile>,
}

/// The `README.md` beside the package's manifest.
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
    /// What the rules read from `config`.
    pub settings: Config,
    /// The `exclude` entries of `config`, checked against the disk.
    pub excluded: Excluded,
    /// The workspace root, absolute and lexically normalized.
    pub root_dir: PathBuf,
    /// The modules and cargo paths samlinter does not follow, sorted, each once.
    pub warnings: Vec<Warning>,
}

impl Workspace {
    /// - Whether the file output shows as `shown` is excluded.
    /// - `shown` joined to the root is the absolute path again; an absolute `shown` stays itself.
    pub fn excludes(&self, shown: &Path) -> bool {
        self.excluded.covers(&self.root_dir.join(shown))
    }
}

/// - A path samlinter does not follow: it climbs with `..` out of a symlinked workspace root,
///   or a symlinked directory above it, where only following the link tells where it leads.
/// - Paths as output shows them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Warning {
    /// A `mod` declaration with such a candidate, by its file and line.
    Module { file: PathBuf, line: usize },
    /// A target root or manifest `cargo metadata` reports, as cargo writes it.
    Cargo { path: PathBuf },
}

impl fmt::Display for Warning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module { file, line } => write!(
                formatter,
                "{}:{line}: `#[path]` climbs above the symlinked workspace root; module not followed",
                file.display()
            ),
            Self::Cargo { path } => write!(
                formatter,
                "{}: climbs above the symlinked workspace root; not followed",
                path.display()
            ),
        }
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
    /// The directory of the path the route names.
    dir: PathBuf,
    name: OsString,
    kind: FileKind,
}

impl Route {
    fn of(root_dir: &Path, file: &Path, kind: FileKind) -> Result<Self> {
        let (dir, name) = file
            .parent()
            .zip(file.file_name())
            .with_context(|| format!("{} names no file", relative(root_dir, file).display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            name: name.to_os_string(),
            kind,
        })
    }
}

/// Source files read so far, by the route that reached each.
type Sources = BTreeMap<Route, TextFile>;

/// What one package's module walk has found so far.
#[derive(Default)]
struct Tree {
    sources: Sources,
    warnings: BTreeSet<Warning>,
}

impl Tree {
    fn warned(self, warning: Warning) -> Self {
        Self {
            warnings: self.warnings.into_iter().chain([warning]).collect(),
            ..self
        }
    }
}

/// One package as discovery reads it, before ownership is decided.
struct Reached {
    metadata: PackageMetadata,
    /// - The manifest, walked once; `load_package` reads it.
    /// - Read only after every package's module walk, so module errors come first.
    manifest: Looked,
    /// The source files it reaches.
    sources: Vec<TextFile>,
}

/// One package once ownership is decided, before it becomes a `Package`.
struct Owned {
    metadata: PackageMetadata,
    /// As `Reached.manifest`.
    manifest: Looked,
    /// The reached source files it owns.
    sources: Vec<TextFile>,
}

/// Each package by its manifest path, lexically normalized.
type ByManifest = BTreeMap<PathBuf, Reached>;

/// What walking one package comes to.
enum PackageWalk {
    Reached {
        package: Reached,
        warnings: BTreeSet<Warning>,
    },
    /// Its manifest climbs out of a symlinked root, so the package is not followed.
    NotFollowed(Warning),
}

pub fn load(manifest_path: Option<&Path>) -> Result<Workspace> {
    let metadata = metadata(manifest_path)?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let packages = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package_metadata(&lexical(workspace_root), package))
        .collect::<Result<Vec<_>>>()?;
    assemble(&Disk, workspace_root, packages)
}

fn metadata(manifest_path: Option<&Path>) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();
    if let Some(path) = manifest_path {
        command.manifest_path(path);
    }
    command.exec().context("`cargo metadata` failed")
}

/// One package `cargo metadata` reports; an error names its manifest as output shows it.
fn package_metadata(root_dir: &Path, package: &cargo_metadata::Package) -> Result<PackageMetadata> {
    let manifest_path = package.manifest_path.as_std_path().to_path_buf();
    let shown = relative(root_dir, &manifest_path);
    let edition = edition(&shown, package.edition.as_str())?;
    let dependencies = dependencies(&shown, &package.dependencies)?;
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

/// - Reads the config, since its `exclude` decides which sources are read.
/// - Then reads every package's sources, gives each source file to one package, builds the
///   packages and splits off the root.
fn assemble(
    files: &impl Files,
    workspace_root: &Path,
    packages: Vec<PackageMetadata>,
) -> Result<Workspace> {
    // Taken as given, never looked up on the way in: it may sit below a system symlink, such as
    // `/tmp` on macOS.
    let root_dir = &lexical(workspace_root);
    let config = config(files, root_dir)?;
    let invalid = || format!("invalid {}", config.shown.display());
    let Settings {
        config: settings,
        exclude,
    } = Settings::parse(&config.text).with_context(invalid)?;
    let excluded = exclude::resolve(files, root_dir, &exclude).with_context(invalid)?;

    let (reached, warnings) = packages.into_iter().try_fold(
        (Vec::new(), BTreeSet::new()),
        |(reached, warnings): (Vec<Reached>, BTreeSet<Warning>), package| {
            Ok::<_, anyhow::Error>(
                match reached_sources(files, root_dir, &excluded, package)? {
                    PackageWalk::Reached {
                        package,
                        warnings: found,
                    } => (
                        reached.into_iter().chain([package]).collect(),
                        warnings.into_iter().chain(found).collect(),
                    ),
                    PackageWalk::NotFollowed(warning) => {
                        (reached, warnings.into_iter().chain([warning]).collect())
                    }
                },
            )
        },
    )?;
    let reached = by_manifest(root_dir, reached)?;

    let packages = owned_sources(reached)
        .into_iter()
        .map(|(manifest, package)| Ok((manifest, load_package(files, root_dir, package)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;

    let (root, members) = split_root(files, root_dir, packages)?;
    Ok(Workspace {
        root,
        members,
        config,
        settings,
        excluded,
        root_dir: root_dir.clone(),
        warnings: warnings.into_iter().collect(),
    })
}

/// - The package with every source file it reaches outside `excluded`, and the paths it does
///   not follow.
/// - Cargo's paths, the manifest and each target root, are walked from scratch here, like the
///   README (`readme`) and `samlinter.toml` (`config`) elsewhere; every module file below is
///   walked once, in `look_up`.
/// - Only module files, target roots included, can be excluded; a manifest is always read.
fn reached_sources(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    package: PackageMetadata,
) -> Result<PackageWalk> {
    match look(
        files,
        root_dir,
        &Excluded::default(),
        &package.manifest_path,
    )? {
        Looked::AboveLinkedRoot => Ok(PackageWalk::NotFollowed(Warning::Cargo {
            path: relative(root_dir, &package.manifest_path),
        })),
        manifest
        @ (Looked::File(_) | Looked::Directory(_) | Looked::Neither | Looked::Excluded) => {
            let tree = package
                .roots
                .iter()
                .try_fold(Tree::default(), |tree, root| {
                    target_root(files, root_dir, package.edition, excluded, tree, root)
                })?;
            Ok(PackageWalk::Reached {
                package: Reached {
                    metadata: package,
                    manifest,
                    sources: one_per_file(tree.sources),
                },
                warnings: tree.warnings,
            })
        }
    }
}

/// `tree` plus every file the target root cargo reports at `written` reaches.
fn target_root(
    files: &impl Files,
    root_dir: &Path,
    edition: Edition,
    excluded: &Excluded,
    tree: Tree,
    written: &Path,
) -> Result<Tree> {
    match look(files, root_dir, excluded, written)? {
        Looked::File(file) => module_tree(
            files,
            root_dir,
            edition,
            excluded,
            tree,
            file,
            FileKind::ModRs,
        ),
        Looked::Excluded => Ok(tree),
        Looked::AboveLinkedRoot => Ok(tree.warned(Warning::Cargo {
            path: relative(root_dir, written),
        })),
        looked @ (Looked::Directory(_) | Looked::Neither) => {
            read_looked(files, root_dir, written, looked).map(|_| tree)
        }
    }
}

/// - `reached` keyed by manifest path.
/// - Two packages on one manifest are an error: either would hide the other.
fn by_manifest(root_dir: &Path, reached: Vec<Reached>) -> Result<ByManifest> {
    reached
        .into_iter()
        .try_fold(ByManifest::new(), |mut packages, package| {
            match packages.entry(lexical(&package.metadata.manifest_path)) {
                Entry::Vacant(slot) => {
                    slot.insert(package);
                    Ok(packages)
                }
                Entry::Occupied(slot) => Err(anyhow!(
                    "{}: two workspace packages resolve to this manifest",
                    relative(root_dir, slot.key()).display()
                )),
            }
        })
}

/// - The package whose manifest is the root `Cargo.toml` is the root package.
/// - Without one, the root is virtual.
/// - Every other package is a member.
/// - `packages` is keyed by manifest path, lexically normalized.
fn split_root(
    files: &impl Files,
    root_dir: &Path,
    mut packages: BTreeMap<PathBuf, Package>,
) -> Result<(Root, Vec<Package>)> {
    let root_manifest = root_dir.join(MANIFEST);
    let root = match packages.remove(&root_manifest) {
        Some(package) => Root::Package(package),
        None => Root::Virtual {
            manifest: read(files, root_dir, &root_manifest)?,
            readme: readme(files, root_dir, root_dir)?,
        },
    };
    Ok((root, packages.into_values().collect()))
}

/// The package from what discovery found; its README sits beside the manifest read.
fn load_package(files: &impl Files, root_dir: &Path, owned: Owned) -> Result<Package> {
    let Owned {
        metadata,
        manifest,
        sources,
    } = owned;
    let manifest = read_looked(files, root_dir, &metadata.manifest_path, manifest)?;
    let dir = manifest.path.parent().with_context(|| {
        format!(
            "{} has no parent directory",
            relative(root_dir, &manifest.path).display()
        )
    })?;
    Ok(Package {
        readme: readme(files, root_dir, dir)?,
        manifest,
        kind: metadata.kind,
        edition: metadata.edition,
        dependencies: metadata.dependencies,
        sources,
    })
}

/// `samlinter.toml` at the root, read first; every workspace needs one.
fn config(files: &impl Files, root_dir: &Path) -> Result<TextFile> {
    let written = root_dir.join(CONFIG);
    let looked = look(files, root_dir, &Excluded::default(), &written)?;
    text_of(files, &written, looked)
        .map(|(path, text)| TextFile::new(root_dir, path, text))
        .map_err(|(path, error)| {
            anyhow::Error::new(error).context(format!(
                "cannot read {}; every workspace needs one, see the README",
                relative(root_dir, &path).display()
            ))
        })
}

/// A file several routes reach is linted once, ordered by its path.
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

/// `tree` plus `file` and every file its `mod` declarations reach along this route.
fn module_tree(
    files: &impl Files,
    root_dir: &Path,
    edition: Edition,
    excluded: &Excluded,
    tree: Tree,
    file: FileAt,
    kind: FileKind,
) -> Result<Tree> {
    let Tree {
        mut sources,
        warnings,
    } = tree;
    let children = match sources.entry(Route::of(root_dir, file.path(), kind)?) {
        // Followed already, along with the files below it.
        Entry::Occupied(_) => Children::default(),
        Entry::Vacant(slot) => {
            let source = read_file(files, root_dir, file)?;
            let children = child_files(files, root_dir, excluded, slot.key(), &source, edition)?;
            slot.insert(source);
            children
        }
    };

    let tree = Tree {
        sources,
        warnings: warnings.into_iter().chain(children.warnings).collect(),
    };
    children
        .found
        .into_iter()
        .try_fold(tree, |tree, Child { file, kind }| {
            module_tree(files, root_dir, edition, excluded, tree, file, kind)
        })
}

/// What the `mod` declarations of one file lead to.
#[derive(Default)]
struct Children {
    /// Every existing candidate of each declaration.
    found: Vec<Child>,
    /// The declarations with a candidate climbing out of a symlinked root.
    warnings: Vec<Warning>,
}

impl Children {
    /// - `self` plus what `declaration`, in the file output shows as `shown`, comes to.
    /// - A declaration with no file is an error: `cargo check` rejects it too unless cfg
    ///   removes the module, and skipping it would hide that module's code.
    fn with(
        self,
        root_dir: &Path,
        shown: &Path,
        declaration: &Declaration,
        resolution: Resolution,
    ) -> Result<Self> {
        match resolution {
            Resolution::Missing(tried) => bail!(
                "{}:{}: no file for `mod {};`, tried {}",
                shown.display(),
                declaration.line,
                declaration.module.name,
                tried
                    .iter()
                    .map(|path| relative(root_dir, path).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Resolution::Found(found) => Ok(Self {
                found: self.found.into_iter().chain(found).collect(),
                warnings: self.warnings,
            }),
            Resolution::AboveLinkedRoot(found) => Ok(Self {
                found: self.found.into_iter().chain(found).collect(),
                warnings: self
                    .warnings
                    .into_iter()
                    .chain([Warning::Module {
                        file: shown.to_path_buf(),
                        line: declaration.line,
                    }])
                    .collect(),
            }),
        }
    }
}

/// The files the `mod` declarations in `source` name, every existing candidate of each.
fn child_files(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    route: &Route,
    source: &TextFile,
    edition: Edition,
) -> Result<Children> {
    let shown = &source.shown;
    let declarations = modules::declarations(&source.text, edition).map_err(|bad| {
        anyhow!(
            "{}:{}: `path` attribute value is not a valid string literal",
            shown.display(),
            bad.line
        )
    })?;

    declarations
        .iter()
        .try_fold(Children::default(), |children, declaration| {
            let resolution = look_up(files, root_dir, excluded, route, declaration)?
                .into_iter()
                .fold(Resolution::Missing(Vec::new()), Resolution::with);
            children.with(root_dir, shown, declaration, resolution)
        })
}

/// A file a `mod` declaration names, found by `look`.
struct Child {
    file: FileAt,
    kind: FileKind,
}

/// One candidate file of a `mod` declaration, once looked up.
enum Lookup {
    Found(Child),
    /// No file, as written, as rustc names it.
    Missing(PathBuf),
    /// An `exclude` entry covers it: found, and never read.
    Excluded,
    /// Climbs out of a symlinked root, so where it leads is unknown.
    AboveLinkedRoot,
}

/// What one `mod` declaration's candidates come to.
enum Resolution {
    /// No candidate is a file, excluded, or climbing out of a symlinked root: every path tried,
    /// as written.
    Missing(Vec<PathBuf>),
    /// The candidates that are files; any one found or excluded makes the declaration resolve.
    Found(Vec<Child>),
    /// - A candidate climbs out of a symlinked root: the declaration is warned about, never an
    ///   error, since a file may be where the link leads.
    /// - The candidates that are files are followed all the same.
    AboveLinkedRoot(Vec<Child>),
}

impl Resolution {
    /// `self` with one more candidate looked up.
    fn with(self, lookup: Lookup) -> Self {
        match (self, lookup) {
            (Self::Missing(tried), Lookup::Missing(path)) => {
                Self::Missing(tried.into_iter().chain([path]).collect())
            }
            (Self::Missing(_), Lookup::Found(child)) => Self::Found(vec![child]),
            (Self::Missing(_), Lookup::Excluded) => Self::Found(Vec::new()),
            (Self::Missing(_), Lookup::AboveLinkedRoot) => Self::AboveLinkedRoot(Vec::new()),
            (Self::Found(found), Lookup::Missing(_) | Lookup::Excluded) => Self::Found(found),
            (Self::Found(found), Lookup::Found(child)) => {
                Self::Found(found.into_iter().chain([child]).collect())
            }
            (Self::Found(found), Lookup::AboveLinkedRoot)
            | (
                Self::AboveLinkedRoot(found),
                Lookup::Missing(_) | Lookup::Excluded | Lookup::AboveLinkedRoot,
            ) => Self::AboveLinkedRoot(found),
            (Self::AboveLinkedRoot(found), Lookup::Found(child)) => {
                Self::AboveLinkedRoot(found.into_iter().chain([child]).collect())
            }
        }
    }
}

/// Each candidate file of `declaration`, found along `route`, looked up.
fn look_up(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    route: &Route,
    declaration: &Declaration,
) -> Result<Vec<Lookup>> {
    let candidates =
        modules::candidates(&route.dir, &route.name, route.kind, declaration, |dir| {
            Ok(match look(files, root_dir, excluded, dir)? {
                Looked::Directory(path) => Directory::At(path),
                Looked::Excluded | Looked::AboveLinkedRoot => Directory::AsWritten,
                Looked::File(_) | Looked::Neither => Directory::Absent,
            })
        })?;
    candidates
        .into_iter()
        .map(|Candidate { path, kind }| {
            Ok(match look(files, root_dir, excluded, &path)? {
                Looked::File(file) => Lookup::Found(Child { file, kind }),
                Looked::Directory(_) | Looked::Neither => Lookup::Missing(path),
                Looked::Excluded => Lookup::Excluded,
                Looked::AboveLinkedRoot => Lookup::AboveLinkedRoot,
            })
        })
        .collect()
}

/// The dependencies of the package whose manifest output shows as `manifest`; an error names
/// it.
fn dependencies(
    manifest: &Path,
    dependencies: &[cargo_metadata::Dependency],
) -> Result<Vec<Dependency>> {
    dependencies
        .iter()
        .map(dependency)
        .collect::<Result<_>>()
        .with_context(|| manifest.display().to_string())
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

/// - `Missing` only when nothing is there, by the same rule as module files (`look`).
/// - A directory or an unreadable file is an error, not a missing README.
fn readme(files: &impl Files, root_dir: &Path, dir: &Path) -> Result<Readme> {
    let written = dir.join(README);
    match look(files, root_dir, &Excluded::default(), &written)? {
        Looked::Neither => Ok(Readme::Missing {
            expected: relative(root_dir, &written),
        }),
        looked @ (Looked::File(_)
        | Looked::Directory(_)
        | Looked::Excluded
        | Looked::AboveLinkedRoot) => {
            read_looked(files, root_dir, &written, looked).map(Readme::Found)
        }
    }
}

/// The file at `written`, which no `exclude` entry applies to.
fn read(files: &impl Files, root_dir: &Path, written: &Path) -> Result<TextFile> {
    let looked = look(files, root_dir, &Excluded::default(), written)?;
    read_looked(files, root_dir, written, looked)
}

fn read_file(files: &impl Files, root_dir: &Path, file: FileAt) -> Result<TextFile> {
    let written = file.path().to_path_buf();
    read_looked(files, root_dir, &written, Looked::File(file))
}

/// - The text file `looked`, walked from `written`, names.
/// - Anything but a readable file is an error naming the path as output shows it.
fn read_looked(
    files: &impl Files,
    root_dir: &Path,
    written: &Path,
    looked: Looked,
) -> Result<TextFile> {
    text_of(files, written, looked)
        .map(|(path, text)| TextFile::new(root_dir, path, text))
        .map_err(|(path, error)| {
            anyhow::Error::new(error).context(format!(
                "cannot read {}",
                relative(root_dir, &path).display()
            ))
        })
}

/// - The text of the file `looked`, walked from `written`, names, with its normalized path.
/// - Otherwise why there is none, with the path to name: the normalized one when known,
///   `written` when not; each caller words the error.
fn text_of(
    files: &impl Files,
    written: &Path,
    looked: Looked,
) -> Result<(PathBuf, String), (PathBuf, io::Error)> {
    match looked {
        Looked::File(file) => match file.read(files) {
            Ok(text) => Ok((file.into_path(), text)),
            Err(error) => Err((file.into_path(), error)),
        },
        Looked::Directory(path) => Err((path, io::Error::from(io::ErrorKind::IsADirectory))),
        Looked::Neither => Err((
            written.to_path_buf(),
            io::Error::from(io::ErrorKind::NotFound),
        )),
        Looked::Excluded => Err((
            written.to_path_buf(),
            io::Error::other("an `exclude` entry covers it"),
        )),
        Looked::AboveLinkedRoot => Err((
            written.to_path_buf(),
            io::Error::other(
                "it climbs above the symlinked workspace root, which samlinter does not follow",
            ),
        )),
    }
}

#[cfg(test)]
mod tests;
