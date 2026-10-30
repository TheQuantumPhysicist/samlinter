mod exclude;
mod lookup;
mod modules;
mod ownership;

use crate::config::{Config, Settings};
use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, TargetKind};
pub use lookup::Excluded;
use lookup::{Disk, FileAt, Files, Looked, lexical, look, relative};
use modules::{Candidate, Declaration, Directory, RouteCode};
pub use modules::{FileKind, TestCode};
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
    pub sources: Vec<SourceFile>,
    /// - Every file this package reaches as library code (`Reach::Library`), by path, whichever
    ///   package owns it.
    /// - A file another package owns can still be this library's code, through `#[path]`.
    pub library_code: BTreeSet<PathBuf>,
}

/// A Rust source file the module walk reached, and what its code is to the package's library.
struct Source {
    file: TextFile,
    /// The file kind of every route that reached it, merged by `FileKind::either`: a
    /// `NonModRs` route makes it `NonModRs`.
    kind: FileKind,
    /// What every route that reached it says, merged by `RouteCode::either`: one route that
    /// lints the file keeps it linted.
    code: RouteCode,
}

/// A Rust source file the package owns, with the file kind every route to it agrees on: a
/// `NonModRs` route (`something.rs`, not `something/mod.rs`) makes it `NonModRs`. `test`
/// says whether every route reaches it as test code, whole-file.
pub struct SourceFile {
    pub file: TextFile,
    pub kind: FileKind,
    pub test: TestCode,
}

/// Whether a source file's code can end up in its package's library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// Reached from a library target through `mod` declarations none of which requires `test`.
    Library,
    /// Reached only through `mod` declarations that require `test`, or only from other targets:
    /// binaries, tests, benches, examples, the build script.
    NotLibrary,
}

impl Reach {
    /// What the module a declaration of reach `declaration` names is, in a file of reach `self`.
    const fn through(self, declaration: Self) -> Self {
        match (self, declaration) {
            (Self::Library, Self::Library) => Self::Library,
            (Self::Library, Self::NotLibrary)
            | (Self::NotLibrary, Self::Library | Self::NotLibrary) => Self::NotLibrary,
        }
    }

    /// A file two routes reach is library code when either route is.
    const fn either(self, other: Self) -> Self {
        match (self, other) {
            (Self::Library, Self::Library | Self::NotLibrary)
            | (Self::NotLibrary, Self::Library) => Self::Library,
            (Self::NotLibrary, Self::NotLibrary) => Self::NotLibrary,
        }
    }
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
    /// The crate it names after any `package = "..."` rename.
    pub crate_name: String,
    /// A `[target.<platform>]` table's dependency is in the section of the table's name.
    pub section: Section,
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
    targets: Vec<Target>,
}

/// One cargo target as discovery walks it.
struct Target {
    root: PathBuf,
    /// What the target's own code is to the library.
    reach: Reach,
    /// `Test` for a test target: its root file is test code whole-file.
    test: TestCode,
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

/// Source files read so far, by the route that reached each, with the reach of that route.
type Sources = BTreeMap<Route, Source>;

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
    sources: Vec<Source>,
}

/// One package once ownership is decided, before it becomes a `Package`.
struct Owned {
    metadata: PackageMetadata,
    /// As `Reached.manifest`.
    manifest: Looked,
    /// The reached source files it owns.
    sources: Vec<SourceFile>,
    /// As `Package.library_code`.
    library_code: BTreeSet<PathBuf>,
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

pub fn load(manifest_path: Option<&Path>, directory: Option<&Path>) -> Result<Workspace> {
    let directory = directory.map(starting_directory).transpose()?;
    let metadata = metadata(manifest_path, directory.as_deref())?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let packages = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package_metadata(&lexical(workspace_root), package))
        .collect::<Result<Vec<_>>>()?;
    assemble(&Disk, workspace_root, packages)
}

/// - `cargo metadata` decides the workspace root, so the run's root moves by running it in
///   `directory`: a relative `--manifest-path` resolves under it, and with no manifest the
///   workspace is found from it, as `cargo` does from the working directory.
fn metadata(manifest_path: Option<&Path>, directory: Option<&Path>) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();
    if let Some(path) = manifest_path {
        command.manifest_path(path);
    }
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    command.exec().context("`cargo metadata` failed")
}

/// - The directory the run starts in, as an absolute path for `cargo metadata`'s cwd.
/// - A relative DIR resolves against the process cwd, the only place the cwd is consulted.
/// - DIR is taken as given, like the workspace root: `fs::metadata` follows a symlink, so a
///   link to a directory starts the run and a link to a file is not a directory.
fn starting_directory(directory: &Path) -> Result<PathBuf> {
    let absolute = if directory.is_relative() {
        std::env::current_dir()
            .with_context(|| format!("cannot resolve the directory {}", directory.display()))?
            .join(directory)
    } else {
        directory.to_path_buf()
    };
    match std::fs::metadata(&absolute) {
        Ok(metadata) if metadata.is_dir() => Ok(absolute),
        Ok(_) => bail!("{}: not a directory", directory.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            bail!("{}: no such file or directory", directory.display())
        }
        Err(error) => bail!("{}: {error}", directory.display()),
    }
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
        targets: package
            .targets
            .iter()
            .map(|target| Target {
                root: target.src_path.as_std_path().to_path_buf(),
                reach: target_reach(&target.kind),
                test: target_test(&target.kind),
            })
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
                .targets
                .iter()
                .try_fold(Tree::default(), |tree, target| {
                    target_root(files, root_dir, package.edition, excluded, tree, target)
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

/// `tree` plus every file the target root cargo reports for `target` reaches.
fn target_root(
    files: &impl Files,
    root_dir: &Path,
    edition: Edition,
    excluded: &Excluded,
    tree: Tree,
    target: &Target,
) -> Result<Tree> {
    let written = &target.root;
    match look(files, root_dir, excluded, written)? {
        Looked::File(file) => module_tree(
            files,
            root_dir,
            edition,
            excluded,
            tree,
            Child {
                file,
                kind: FileKind::ModRs,
                code: RouteCode {
                    reach: target.reach,
                    test: target.test,
                },
            },
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
        library_code,
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
        library_code,
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

/// A file several routes reach is linted once, ordered by its path, as library code when any
/// route reaches it as such, by the shape any route gives it, and test-silent when every
/// route reaches it as test code.
fn one_per_file(sources: Sources) -> Vec<Source> {
    let by_path =
        sources
            .into_values()
            .fold(BTreeMap::<PathBuf, Source>::new(), |mut by_path, source| {
                match by_path.entry(source.file.path.clone()) {
                    Entry::Vacant(slot) => {
                        slot.insert(source);
                    }
                    Entry::Occupied(mut slot) => {
                        let kept = slot.get_mut();
                        kept.kind = kept.kind.either(source.kind);
                        kept.code = kept.code.either(source.code);
                    }
                }
                by_path
            });
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

/// - `tree` plus the `child` file and every file its `mod` declarations reach along this route.
/// - A route followed only as `Reach::NotLibrary` so far is followed again as library code
///   when a library route reaches it, so the files below it become library code too; a file
///   followed only as `TestCode::Test` is followed again as `TestCode::NotTest` the same way.
fn module_tree(
    files: &impl Files,
    root_dir: &Path,
    edition: Edition,
    excluded: &Excluded,
    tree: Tree,
    child: Child,
) -> Result<Tree> {
    let Tree {
        mut sources,
        warnings,
    } = tree;
    let Child { file, kind, code } = child;
    let children = match sources.entry(Route::of(root_dir, file.path(), kind)?) {
        Entry::Occupied(mut slot) => {
            let walked = slot.get().code;
            // The first walk read this file and followed its declarations along its own route.
            // Follow them again only when this route reaches the files below as something that
            // walk did not: library code, or linted instead of test code. Reading the trigger
            // off the merged route keeps it from drifting from what the merge decides.
            let merged = walked.either(code);
            let follows = merged != walked;
            slot.get_mut().code = merged;
            if follows {
                let route = slot.key();
                child_files(
                    files,
                    root_dir,
                    excluded,
                    route,
                    &slot.get().file,
                    edition,
                    code,
                )?
            } else {
                Children::default()
            }
        }
        Entry::Vacant(slot) => {
            let source = read_file(files, root_dir, file)?;
            let children = child_files(
                files,
                root_dir,
                excluded,
                slot.key(),
                &source,
                edition,
                code,
            )?;
            slot.insert(Source {
                file: source,
                kind,
                code,
            });
            children
        }
    };

    let tree = Tree {
        sources,
        warnings: warnings.into_iter().chain(children.warnings).collect(),
    };
    children.found.into_iter().try_fold(tree, |tree, child| {
        module_tree(files, root_dir, edition, excluded, tree, child)
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

/// - The files the `mod` declarations in `source` name, every existing candidate of each.
/// - `code` is what the route says about them; each child's is that through its declaration.
fn child_files(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    route: &Route,
    source: &TextFile,
    edition: Edition,
    code: RouteCode,
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
            let resolution = look_up(files, root_dir, excluded, route, declaration, code)?
                .into_iter()
                .fold(Resolution::Missing(Vec::new()), Resolution::with);
            children.with(root_dir, shown, declaration, resolution)
        })
}

/// A file a target or a `mod` declaration names, found by `look`.
struct Child {
    file: FileAt,
    kind: FileKind,
    /// What the route through it says about the file.
    code: RouteCode,
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

/// Each candidate file of `declaration`, found along `route` whose code is `code`, looked up.
fn look_up(
    files: &impl Files,
    root_dir: &Path,
    excluded: &Excluded,
    route: &Route,
    declaration: &Declaration,
    code: RouteCode,
) -> Result<Vec<Lookup>> {
    let code = code.through(declaration.test);
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
        .map(|candidate| {
            let Candidate {
                path,
                kind,
                test: location,
            } = candidate;
            Ok(match look(files, root_dir, excluded, &path)? {
                Looked::File(file) => Lookup::Found(Child {
                    file,
                    kind,
                    code: code.through(location),
                }),
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
        crate_name: dependency.name.clone(),
        section,
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

/// A library target's code is library code; any other target's is not.
fn target_reach(kinds: &[TargetKind]) -> Reach {
    if kinds.iter().any(is_library_kind) {
        Reach::Library
    } else {
        Reach::NotLibrary
    }
}

/// A test target's root file is test code whole-file; any other target's is not.
fn target_test(kinds: &[TargetKind]) -> TestCode {
    if kinds.iter().any(is_test_kind) {
        TestCode::Test
    } else {
        TestCode::NotTest
    }
}

const fn is_library_kind(kind: &TargetKind) -> bool {
    match kind {
        TargetKind::Bin
        | TargetKind::Example
        | TargetKind::Test
        | TargetKind::Bench
        | TargetKind::CustomBuild => false,
        // `#[non_exhaustive]`: the wildcard absorbs kinds newer than this cargo_metadata, read
        // as library code like `Unknown`, so a new kind never hides a finding.
        TargetKind::Lib
        | TargetKind::RLib
        | TargetKind::DyLib
        | TargetKind::CDyLib
        | TargetKind::StaticLib
        | TargetKind::ProcMacro
        | TargetKind::Unknown(_)
        | _ => true,
    }
}

const fn is_test_kind(kind: &TargetKind) -> bool {
    match kind {
        TargetKind::Test => true,
        // `#[non_exhaustive]`: the wildcard absorbs kinds newer than this cargo_metadata, read
        // as not test code, so a new kind never hides a finding.
        TargetKind::Bin
        | TargetKind::Example
        | TargetKind::Bench
        | TargetKind::CustomBuild
        | TargetKind::Lib
        | TargetKind::RLib
        | TargetKind::DyLib
        | TargetKind::CDyLib
        | TargetKind::StaticLib
        | TargetKind::ProcMacro
        | TargetKind::Unknown(_)
        | _ => false,
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
