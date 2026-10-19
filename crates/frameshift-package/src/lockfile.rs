//! Lockfile models of npm (`package-lock.json` and `npm-shrinkwrap.json`,
//! versions 2 and 3) and Bun (`bun.lock`, versions 1 and 2) (`D-069`).
//!
//! Both formats become one model: the workspaces with the dependencies
//! they declare, and the locked packages with the versions they resolve to.
//! A lockfile whose structure differs from its format is rejected as a
//! whole, because a partly read lockfile could resolve a dependency to the
//! wrong entry.

mod bun;
mod npm;
mod pnpm;

use std::fmt;

use frameshift_core::Span;
use frameshift_syntax::{JsonDocument, YamlStream};

use crate::manager::LockfileKind;
use crate::manifest::{DependencyKind, PackageManifest};
use crate::version::Version;

/// A dependency name and specifier text, as a lockfile records them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Declared {
    /// The dependency name.
    pub name: Box<str>,
    /// The specifier text.
    pub spec: Box<str>,
}

/// A workspace and what it declares: the root (path `""`) or a member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedWorkspace {
    /// The workspace directory relative to the lockfile, `""` for the root.
    pub path: Box<str>,
    /// The package name, if recorded.
    pub name: Option<Box<str>>,
    /// The package version, if recorded.
    pub version: Option<Box<str>>,
    /// The declared dependencies by kind, sorted by name.
    pub declared: [Vec<Declared>; 4],
}

impl LockedWorkspace {
    /// Returns the declared dependencies of one kind, sorted by name.
    pub fn declared(&self, kind: DependencyKind) -> &[Declared] {
        &self.declared[kind as usize]
    }
}

/// How a locked package was resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// A registry package with this exact version.
    Registry(Version),
    /// npm: a link to another location of the lockfile, such as a workspace.
    Link(Box<str>),
    /// Bun: a workspace package at this path.
    Workspace(Box<str>),
    /// Anything else, such as a Git, file, or tarball source, or a version
    /// that is not exact SemVer; the text as recorded.
    Other(Box<str>),
}

/// A package that the lockfile installs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedPackage {
    /// The key: an install path for npm, a path of package names for Bun.
    pub key: Box<str>,
    /// The span of the entry's value.
    pub span: Span,
    /// The installed package's own name, which differs from the key's last
    /// name for an alias.
    pub name: Box<str>,
    /// How the package was resolved.
    pub resolution: Resolution,
    /// Production and optional dependencies, sorted by name.
    pub dependencies: Vec<Declared>,
    /// Peer dependencies, sorted by name.
    pub peer_dependencies: Vec<Declared>,
    /// The names of optional peer dependencies, sorted.
    pub optional_peers: Vec<Box<str>>,
    /// npm: installed only for development.
    pub dev: bool,
    /// npm: an optional dependency.
    pub optional: bool,
}

impl LockedPackage {
    /// Returns the exact version of a registry package.
    pub fn version(&self) -> Option<&Version> {
        match &self.resolution {
            Resolution::Registry(version) => Some(version),
            Resolution::Link(_) | Resolution::Workspace(_) | Resolution::Other(_) => None,
        }
    }
}

/// What is wrong with a lockfile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockfileErrorKind {
    /// `lockfileVersion` names a format version that is not read: npm 1,
    /// Bun 0, or an unknown version.
    UnsupportedVersion,
    /// The document does not have the structure of its format.
    Structure,
}

/// A rejected lockfile, with the span of the offending value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockfileError {
    /// What is wrong.
    pub kind: LockfileErrorKind,
    /// The span of the offending value.
    pub span: Span,
}

impl fmt::Display for LockfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            LockfileErrorKind::UnsupportedVersion => {
                write!(
                    f,
                    "lockfile format version at byte {} is not supported",
                    self.span.start()
                )
            }
            LockfileErrorKind::Structure => {
                write!(
                    f,
                    "lockfile structure is invalid at byte {}",
                    self.span.start()
                )
            }
        }
    }
}

impl std::error::Error for LockfileError {}

/// A parsed npm, Bun, or pnpm 9.0 lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lockfile {
    kind: LockfileKind,
    version: u64,
    workspaces: Vec<LockedWorkspace>,
    packages: Vec<LockedPackage>,
    pnpm_references: pnpm::References,
}

impl Lockfile {
    /// Interprets a parsed lockfile of an npm or Bun text kind.
    ///
    /// The caller parses `package-lock.json` and `npm-shrinkwrap.json` as
    /// JSON and `bun.lock` as JSONC. Other kinds are never parsed and fail
    /// with [`LockfileErrorKind::UnsupportedVersion`] at the document.
    pub fn from_json(
        kind: LockfileKind,
        document: &JsonDocument<'_>,
    ) -> Result<Self, LockfileError> {
        let (version, mut workspaces, mut packages) = match kind {
            LockfileKind::NpmShrinkwrap | LockfileKind::NpmPackageLock => npm::read(document)?,
            LockfileKind::BunText => bun::read(document)?,
            LockfileKind::Pnpm | LockfileKind::Yarn | LockfileKind::BunBinary => {
                return Err(LockfileError {
                    kind: LockfileErrorKind::UnsupportedVersion,
                    span: crate::manifest::span(document, document.root()),
                });
            }
        };
        workspaces.sort_by(|left, right| left.path.cmp(&right.path));
        packages.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(Self {
            kind,
            version,
            workspaces,
            packages,
            pnpm_references: Default::default(),
        })
    }

    /// Interprets exactly one YAML document as a pnpm lockfile of format
    /// 9.0. Duplicate, complex, or non-string mapping keys and incompatible
    /// tags fail closed. Anchors and aliases are read without expansion.
    pub fn from_yaml(stream: &YamlStream) -> Result<Self, LockfileError> {
        let mut read = pnpm::read(stream)?;
        read.workspaces.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            kind: LockfileKind::Pnpm,
            version: 9,
            workspaces: read.workspaces,
            packages: read.packages,
            pnpm_references: read.references,
        })
    }

    /// Returns a pnpm importer's exact recorded dependency reference,
    /// including aliases, peer contexts, and `link:` references. Other
    /// lockfile kinds return `None`. The root workspace is `""`.
    pub fn pnpm_reference(&self, workspace: &str, name: &str) -> Option<&str> {
        self.pnpm_references
            .get(&(workspace.into(), name.into()))
            .map(AsRef::as_ref)
    }

    /// Returns the lockfile kind.
    pub const fn kind(&self) -> LockfileKind {
        self.kind
    }

    /// Returns the format major: npm/Bun's integer or 9 for pnpm 9.0.
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Returns the workspaces sorted by path; the root comes first.
    pub fn workspaces(&self) -> &[LockedWorkspace] {
        &self.workspaces
    }

    /// Finds a workspace by its path, `""` for the root.
    pub fn workspace(&self, path: &str) -> Option<&LockedWorkspace> {
        self.workspaces
            .binary_search_by(|workspace| (*workspace.path).cmp(path))
            .ok()
            .map(|index| &self.workspaces[index])
    }

    /// Returns the locked packages sorted by key.
    pub fn packages(&self) -> &[LockedPackage] {
        &self.packages
    }

    /// Finds a locked package by its key.
    pub fn package(&self, key: &str) -> Option<&LockedPackage> {
        self.packages
            .binary_search_by(|package| (*package.key).cmp(key))
            .ok()
            .map(|index| &self.packages[index])
    }

    /// Resolves a dependency of a workspace, `""` for the root, as the
    /// package manager installs it.
    ///
    /// npm follows Node.js: `<dir>/node_modules/<name>` from the workspace
    /// directory up to the root, skipping `node_modules` directories, then
    /// one link to its target. Bun looks for `<prefix>/<name>` from the
    /// workspace's package name down to the bare name.
    /// pnpm follows the importer's exact snapshot reference, preserving
    /// peer contexts and aliases; local `link:` references return `None`.
    pub fn resolve(&self, workspace: &str, name: &str) -> Option<&LockedPackage> {
        match self.kind {
            LockfileKind::Pnpm => {
                pnpm::find(&self.packages, name, self.pnpm_reference(workspace, name)?)
            }
            LockfileKind::BunText => {
                let prefix = if workspace.is_empty() {
                    ""
                } else {
                    self.workspace(workspace)?.name.as_deref()?
                };
                self.resolve_bun(prefix, name)
            }
            LockfileKind::NpmShrinkwrap
            | LockfileKind::NpmPackageLock
            | LockfileKind::Yarn
            | LockfileKind::BunBinary => self.resolve_npm(workspace, name),
        }
    }

    /// Resolves a dependency of a locked package.
    pub fn resolve_from(&self, package: &LockedPackage, name: &str) -> Option<&LockedPackage> {
        match self.kind {
            LockfileKind::Pnpm => {
                let index = package
                    .dependencies
                    .binary_search_by(|dependency| dependency.name.as_ref().cmp(name))
                    .ok()?;
                pnpm::find(&self.packages, name, &package.dependencies[index].spec)
            }
            LockfileKind::BunText => self.resolve_bun(&package.key, name),
            LockfileKind::NpmShrinkwrap
            | LockfileKind::NpmPackageLock
            | LockfileKind::Yarn
            | LockfileKind::BunBinary => self.resolve_npm(&package.key, name),
        }
    }

    fn resolve_npm(&self, directory: &str, name: &str) -> Option<&LockedPackage> {
        let mut current = Some(directory);
        while let Some(directory) = current {
            if directory != "node_modules" && !directory.ends_with("/node_modules") {
                let key = if directory.is_empty() {
                    format!("node_modules/{name}")
                } else {
                    format!("{directory}/node_modules/{name}")
                };
                if let Some(package) = self.package(&key) {
                    return match &package.resolution {
                        Resolution::Link(target) => self.package(target).or(Some(package)),
                        Resolution::Registry(_)
                        | Resolution::Workspace(_)
                        | Resolution::Other(_) => Some(package),
                    };
                }
            }
            current = (!directory.is_empty())
                .then(|| directory.rsplit_once('/').map_or("", |(parent, _)| parent));
        }
        None
    }

    fn resolve_bun(&self, prefix: &str, name: &str) -> Option<&LockedPackage> {
        let mut names = bun::split_names(prefix)?;
        loop {
            let key = if names.is_empty() {
                name.to_owned()
            } else {
                format!("{}/{name}", names.join("/"))
            };
            if let Some(package) = self.package(&key) {
                return Some(package);
            }
            names.pop()?;
        }
    }

    /// Lists the dependency kinds whose declarations in the manifest differ
    /// from the root workspace's declarations in the lockfile, by name and
    /// specifier text. A difference means that the lockfile may be stale.
    pub fn out_of_sync(&self, manifest: &PackageManifest) -> Vec<DependencyKind> {
        let empty = LockedWorkspace {
            path: "".into(),
            name: None,
            version: None,
            declared: Default::default(),
        };
        let root = self.workspace("").unwrap_or(&empty);
        DependencyKind::ALL
            .into_iter()
            .filter(|kind| {
                // pnpm importers record installed direct dependencies, not
                // the manifest's peer declarations.
                if self.kind == LockfileKind::Pnpm && *kind == DependencyKind::Peer {
                    return false;
                }
                let mut declared: Vec<Declared> = manifest
                    .dependencies_of(*kind)
                    .iter()
                    .map(|entry| Declared {
                        name: entry.key.clone(),
                        spec: entry.value.text.clone(),
                    })
                    .collect();
                declared.sort();
                declared != root.declared(*kind)
            })
            .collect()
    }
}

/// A name a lockfile installs a package under: `@scope/name` or one
/// segment, without whitespace, control characters, or backslashes, and not
/// `.` or `..`. Unlike new npm packages, old names may have uppercase letters.
pub(crate) fn is_installed_name(name: &str) -> bool {
    let segment = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '/' | '\\'))
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| segment(scope) && segment(rest)),
        None => segment(name),
    }
}

/// Reads a string map into declarations sorted by name.
pub(crate) fn declared(
    document: &JsonDocument<'_>,
    id: Option<frameshift_syntax::JsonId>,
) -> Result<Vec<Declared>, LockfileError> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let object = object(document, id)?;
    let mut declared = Vec::new();
    for (name, _, value) in crate::manifest::members(object) {
        declared.push(Declared {
            name: name.into(),
            spec: string(document, value)?.into(),
        });
    }
    declared.sort();
    Ok(declared)
}

pub(crate) fn structure(
    document: &JsonDocument<'_>,
    id: frameshift_syntax::JsonId,
) -> LockfileError {
    LockfileError {
        kind: LockfileErrorKind::Structure,
        span: crate::manifest::span(document, id),
    }
}

pub(crate) fn object<'d>(
    document: &'d JsonDocument<'_>,
    id: frameshift_syntax::JsonId,
) -> Result<frameshift_syntax::JsonObject<'d>, LockfileError> {
    match document.value(id) {
        Some(frameshift_syntax::JsonValue::Object(object)) => Ok(object),
        _ => Err(structure(document, id)),
    }
}

pub(crate) fn string<'d>(
    document: &'d JsonDocument<'_>,
    id: frameshift_syntax::JsonId,
) -> Result<&'d str, LockfileError> {
    match document.value(id) {
        Some(frameshift_syntax::JsonValue::String(text)) => Ok(text),
        _ => Err(structure(document, id)),
    }
}

pub(crate) fn optional_string(
    document: &JsonDocument<'_>,
    id: Option<frameshift_syntax::JsonId>,
) -> Result<Option<Box<str>>, LockfileError> {
    id.map(|id| string(document, id).map(Box::from)).transpose()
}

pub(crate) fn flag(
    document: &JsonDocument<'_>,
    id: Option<frameshift_syntax::JsonId>,
) -> Result<bool, LockfileError> {
    match id.map(|id| (id, document.value(id))) {
        None => Ok(false),
        Some((_, Some(frameshift_syntax::JsonValue::Bool(value)))) => Ok(value),
        Some((id, _)) => Err(structure(document, id)),
    }
}

/// Reads `lockfileVersion` as a nonnegative integer.
pub(crate) fn format_version(
    document: &JsonDocument<'_>,
    root: frameshift_syntax::JsonObject<'_>,
) -> Result<(u64, frameshift_syntax::JsonId), LockfileError> {
    let id = root
        .get("lockfileVersion")
        .ok_or_else(|| structure(document, document.root()))?;
    match document.value(id) {
        Some(frameshift_syntax::JsonValue::Number(number)) => number
            .as_u64()
            .map(|version| (version, id))
            .ok_or_else(|| structure(document, id)),
        _ => Err(structure(document, id)),
    }
}

pub(crate) fn unsupported(
    document: &JsonDocument<'_>,
    id: frameshift_syntax::JsonId,
) -> LockfileError {
    LockfileError {
        kind: LockfileErrorKind::UnsupportedVersion,
        span: crate::manifest::span(document, id),
    }
}

#[cfg(test)]
mod tests;
