//! Lockfile models of npm (`package-lock.json` and `npm-shrinkwrap.json`,
//! versions 2 and 3) and Bun (`bun.lock`, versions 1 and 2) (`D-069`), pnpm
//! (`pnpm-lock.yaml`, version 9.0) (`D-071`), and Yarn (`yarn.lock`, classic
//! version 1 and Berry versions 4 to 10) (`D-072`).
//!
//! Every format becomes one model: the workspaces with the dependencies
//! they declare, and the locked packages with the versions they resolve to.
//! A lockfile whose structure differs from its format is rejected as a
//! whole, because a partly read lockfile could resolve a dependency to the
//! wrong entry.

mod bun;
mod npm;
pub(crate) mod pnpm;
mod yarn_berry;
mod yarn_classic;

pub(crate) use yarn_classic::has_classic_header;

use std::collections::BTreeMap;
use std::fmt;

use frameshift_core::Span;
use frameshift_syntax::{JsonDocument, YamlStream, YarnDocument};

use crate::manager::LockfileKind;
use crate::manifest::{DependencyKind, PackageManifest};
use crate::range::VersionRange;
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
    /// Bun and Yarn Berry: a workspace package at this path, empty for the
    /// root.
    Workspace(Box<str>),
    /// Anything else, such as a Git, file, or tarball source, or a version
    /// that is not exact SemVer; the text as recorded.
    Other(Box<str>),
}

/// A package that the lockfile installs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedPackage {
    /// The key: an install path for npm, a path of package names for Bun,
    /// the snapshot key with peer contexts for pnpm, the patterns joined
    /// with `, ` for Yarn classic, and the resolution locator for Yarn Berry.
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
    /// The format or format version is not read: npm 1, Bun 0, pnpm other
    /// than 9.0, a Yarn classic version above 1, a Yarn Berry version that
    /// no stable release writes, a `yarn.lock` with neither `__metadata` nor
    /// the classic header, or an unknown version.
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

/// A parsed npm, Bun, pnpm 9.0, or Yarn lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lockfile {
    kind: LockfileKind,
    version: u64,
    workspaces: Vec<LockedWorkspace>,
    packages: Vec<LockedPackage>,
    pnpm_references: pnpm::References,
    yarn_patterns: yarn_classic::Patterns,
    yarn_index: yarn_berry::Index,
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
            yarn_patterns: BTreeMap::new(),
            yarn_index: Default::default(),
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
            yarn_patterns: BTreeMap::new(),
            yarn_index: Default::default(),
        })
    }

    /// Interprets a Yarn classic lockfile, format version 1. Duplicate
    /// keys, patterns of one entry that name different packages, values of
    /// unexpected types, and dependencies without their own entry fail
    /// closed. The model records no workspaces, because Yarn 1 does not.
    pub fn from_yarn_classic(document: &YarnDocument<'_>) -> Result<Self, LockfileError> {
        let read = yarn_classic::read(document)?;
        Ok(Self {
            kind: LockfileKind::Yarn,
            version: 1,
            workspaces: Vec::new(),
            packages: read.packages,
            pnpm_references: Default::default(),
            yarn_patterns: read.patterns,
            yarn_index: Default::default(),
        })
    }

    /// Interprets exactly one YAML document as a Yarn Berry lockfile of a
    /// format version that a stable Yarn release writes (4, 5, 6, 8, 9, or
    /// 10). Scalars are strings, as under Yarn's failsafe schema. Duplicate
    /// or complex keys, ambiguous descriptors, and values of unexpected
    /// types fail closed. A dependency whose descriptor the root's
    /// `resolutions` or a catalog replaced has no entry and resolves to
    /// nothing.
    /// A document without `__metadata` is not a Berry lockfile and fails
    /// with [`LockfileErrorKind::UnsupportedVersion`].
    pub fn from_yarn_berry(stream: &YamlStream) -> Result<Self, LockfileError> {
        let read = yarn_berry::read(stream)?;
        Ok(Self {
            kind: LockfileKind::Yarn,
            version: read.version,
            workspaces: read.workspaces,
            packages: read.packages,
            pnpm_references: Default::default(),
            yarn_patterns: BTreeMap::new(),
            yarn_index: read.index,
        })
    }

    /// Returns whether this is a Yarn classic lockfile.
    const fn is_yarn_classic(&self) -> bool {
        matches!(self.kind, LockfileKind::Yarn) && self.version == 1
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

    /// Returns the format major: npm's or Bun's integer, 9 for pnpm 9.0, 1
    /// for Yarn classic, and `__metadata.version` for Yarn Berry.
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
    /// Yarn Berry looks up the descriptor that the workspace's entry
    /// records, normalized and bound as Yarn does. Yarn classic records no
    /// workspaces and returns `None`; use [`Lockfile::resolve_declared`].
    pub fn resolve(&self, workspace: &str, name: &str) -> Option<&LockedPackage> {
        if self.kind == LockfileKind::Yarn {
            if self.is_yarn_classic() {
                return None;
            }
            let declared = self
                .workspace(workspace)?
                .declared(DependencyKind::Production);
            let index = declared
                .binary_search_by(|dependency| dependency.name.as_ref().cmp(name))
                .ok()?;
            let parent = self.yarn_index.workspaces.get(workspace);
            return self.resolve_berry(name, &declared[index].spec, parent.map(AsRef::as_ref));
        }
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

    /// Resolves a dependency that a workspace's `package.json` declares with
    /// the specifier text `spec`.
    ///
    /// Yarn looks up declarations by name and range: classic finds the
    /// pattern `<name>@<spec>`, or the bare name when the lockfile has it,
    /// and Berry the descriptor of `spec`, normalized and bound to the
    /// workspace. A specifier that changed after the last install therefore
    /// resolves to nothing, as for Yarn. The other formats record installs
    /// by name and resolve as [`Lockfile::resolve`] does.
    pub fn resolve_declared(
        &self,
        workspace: &str,
        name: &str,
        spec: &str,
    ) -> Option<&LockedPackage> {
        if self.kind != LockfileKind::Yarn {
            return self.resolve(workspace, name);
        }
        if self.is_yarn_classic() {
            let key = yarn_classic::find(&self.yarn_patterns, name, spec)?;
            return self.package(key);
        }
        let parent = self.yarn_index.workspaces.get(workspace);
        self.resolve_berry(name, spec, parent.map(AsRef::as_ref))
    }

    fn resolve_berry(
        &self,
        name: &str,
        spec: &str,
        parent: Option<&str>,
    ) -> Option<&LockedPackage> {
        let descriptor = yarn_berry::descriptor(name, spec, parent);
        self.package(self.yarn_index.descriptors.get(descriptor.as_str())?)
    }

    /// Resolves a dependency of a locked package.
    pub fn resolve_from(&self, package: &LockedPackage, name: &str) -> Option<&LockedPackage> {
        if self.kind == LockfileKind::Yarn {
            let index = package
                .dependencies
                .binary_search_by(|dependency| dependency.name.as_ref().cmp(name))
                .ok()?;
            let spec = &package.dependencies[index].spec;
            if self.is_yarn_classic() {
                let key = self.yarn_patterns.get(format!("{name}@{spec}").as_str())?;
                return self.package(key);
            }
            return self.resolve_berry(name, spec, Some(&package.key));
        }
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
    ///
    /// Yarn classic records no workspaces, so a kind is listed when one of
    /// its declarations has no pattern in the lockfile; a declaration that a
    /// workspace package satisfies has none either. Yarn Berry merges a
    /// workspace's production, optional, and development dependencies,
    /// development last, so their effective declarations are compared with
    /// normalized ranges, and peer ranges as Yarn records them.
    pub fn out_of_sync(&self, manifest: &PackageManifest) -> Vec<DependencyKind> {
        self.out_of_sync_in("", manifest, &|_, _| false)
    }

    /// Lists the dependency kinds whose declarations in the manifest of the
    /// workspace at `workspace` (`""` for the root) differ from the
    /// lockfile, as [`Lockfile::out_of_sync`] does for the root.
    ///
    /// `linked(name, spec)` tells whether a workspace package satisfies a
    /// declaration. Yarn classic links such a package instead of locking
    /// it, so its declaration is not stale (`D-073`).
    pub fn out_of_sync_in(
        &self,
        workspace: &str,
        manifest: &PackageManifest,
        linked: &dyn Fn(&str, &str) -> bool,
    ) -> Vec<DependencyKind> {
        if self.kind == LockfileKind::Yarn {
            return if self.is_yarn_classic() {
                self.out_of_sync_classic(manifest, linked)
            } else {
                self.out_of_sync_berry(workspace, manifest)
            };
        }
        let empty = LockedWorkspace {
            path: workspace.into(),
            name: None,
            version: None,
            declared: Default::default(),
        };
        let root = self.workspace(workspace).unwrap_or(&empty);
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

impl Lockfile {
    fn out_of_sync_classic(
        &self,
        manifest: &PackageManifest,
        linked: &dyn Fn(&str, &str) -> bool,
    ) -> Vec<DependencyKind> {
        // Yarn 1 does not install peer dependencies.
        [
            DependencyKind::Production,
            DependencyKind::Development,
            DependencyKind::Optional,
        ]
        .into_iter()
        .filter(|kind| {
            manifest.dependencies_of(*kind).iter().any(|entry| {
                yarn_classic::find(&self.yarn_patterns, &entry.key, &entry.value.text).is_none()
                    && !linked(&entry.key, &entry.value.text)
            })
        })
        .collect()
    }

    fn out_of_sync_berry(
        &self,
        workspace: &str,
        manifest: &PackageManifest,
    ) -> Vec<DependencyKind> {
        let empty = Vec::new();
        let root = self.workspace(workspace);
        let recorded =
            |kind: DependencyKind| root.map_or(&empty, |root| &root.declared[kind as usize]);
        // The effective declarations: optional over production, development
        // over both.
        let mut effective: BTreeMap<&str, (DependencyKind, String)> = BTreeMap::new();
        for kind in [
            DependencyKind::Production,
            DependencyKind::Optional,
            DependencyKind::Development,
        ] {
            for entry in manifest.dependencies_of(kind) {
                effective.insert(&entry.key, (kind, yarn_berry::normalize(&entry.value.text)));
            }
        }
        let locked: BTreeMap<&str, String> = recorded(DependencyKind::Production)
            .iter()
            .map(|declared| {
                (
                    declared.name.as_ref(),
                    yarn_berry::normalize(&declared.spec),
                )
            })
            .collect();
        let mut stale = Vec::new();
        for (name, (kind, spec)) in &effective {
            if locked.get(name) != Some(spec) {
                stale.push(*kind);
            }
        }
        if locked.keys().any(|name| !effective.contains_key(name)) {
            stale.push(DependencyKind::Production);
        }
        // Yarn records an invalid peer range as `*`.
        let mut peers: Vec<Declared> = manifest
            .dependencies_of(DependencyKind::Peer)
            .iter()
            .map(|entry| {
                let text = &entry.value.text;
                let valid = text.starts_with("workspace:")
                    || (!text.contains(':') && VersionRange::parse(text).is_ok());
                Declared {
                    name: entry.key.clone(),
                    spec: if valid { text.clone() } else { "*".into() },
                }
            })
            .collect();
        peers.sort();
        if peers != recorded(DependencyKind::Peer).as_slice() {
            stale.push(DependencyKind::Peer);
        }
        DependencyKind::ALL
            .into_iter()
            .filter(|kind| stale.contains(kind))
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
