//! Package-based configuration inheritance through linked workspace members
//! (`D-098`).
//!
//! TypeScript 6.0.3 resolves an `extends` string that is neither rooted nor
//! relative with `nodeNextJsonConfigResolver`: a lookup for JSON through
//! `node_modules` directories, followed to the real path of a package
//! manager link. Frameshift never reads `node_modules`. A name resolves only
//! when the package that holds the extending configuration declares it with
//! the `workspace:` protocol, which pnpm, Yarn Berry, and Bun link to the one
//! workspace member of that name; the lookup then runs over the member's
//! admitted files with the reference's configuration-lookup rules.

use frameshift_core::{FindingCollector, SourcePath, SourceText};
use frameshift_input::{EntryKind, FileRead, Snapshot};
use frameshift_package::{
    DependencySpec, LoadError, ManifestState, VersionRange, Workspace, load_workspace,
};

use crate::package::{
    CONFIG, Target, child, components, field_candidates, from_file, normalize, replace_stars,
    rooted, select,
};
use crate::{ConfigProblem, GraphLimits, ModuleError, PackageManifest, Work, below, parent};

/// The workspace model, loaded through the snapshot on first use for one
/// configuration chain.
pub(crate) struct Links {
    workspace: Option<Option<Workspace>>,
}

impl Links {
    pub(crate) fn new() -> Self {
        Self { workspace: None }
    }

    fn workspace(
        &mut self,
        snapshot: &Snapshot,
        limits: GraphLimits,
    ) -> Result<Option<&Workspace>, ModuleError> {
        if self.workspace.is_none() {
            // `inspect` reports the workspace findings; the graph needs only
            // the members.
            let mut findings = FindingCollector::new(limits.finding);
            let loaded = match load_workspace(snapshot, limits.package, &mut findings) {
                Ok(workspace) => Some(workspace),
                Err(LoadError::Snapshot(error)) => return Err(ModuleError::Snapshot(error)),
                Err(LoadError::AllocationFailed) => return Err(ModuleError::AllocationFailed),
                // Without a complete model, no link is established.
                Err(LoadError::Findings(_) | LoadError::Internal) => None,
            };
            self.workspace = Some(loaded);
        }
        Ok(self.workspace.as_ref().and_then(Option::as_ref))
    }
}

/// What a package-based `extends` string establishes.
pub(crate) enum Extended {
    /// The admitted configuration file it inherits.
    Found(SourcePath),
    /// Why it establishes nothing, at the path that caused it.
    Issue(SourcePath, ConfigProblem),
}

fn analyzed(state: &ManifestState) -> Option<&frameshift_package::PackageManifest> {
    match state {
        ManifestState::Analyzed(manifest) => Some(manifest),
        ManifestState::Absent | ManifestState::NotAnalyzed => None,
    }
}

/// The reference's `parsePackageName`: one leading component, two for a
/// scoped name, and the rest.
fn package_name(reference: &str) -> (&str, &str) {
    let first = reference.find('/');
    let split = if reference.starts_with('@') {
        first.and_then(|i| reference[i + 1..].find('/').map(|j| i + 1 + j))
    } else {
        first
    };
    match split {
        Some(i) => (&reference[..i], &reference[i + 1..]),
        None => (reference, ""),
    }
}

/// Resolves an `extends` string of `config` that is neither rooted nor
/// relative. Every outcome other than a linked member's file is an issue.
pub(crate) fn package_extends(
    snapshot: &Snapshot,
    config: &SourcePath,
    reference: &str,
    limits: GraphLimits,
    links: &mut Links,
    work: &mut Work,
) -> Result<Extended, ModuleError> {
    let unsupported = || Ok(Extended::Issue(config.clone(), ConfigProblem::Unsupported));
    work.charge(reference.len() + 1)?;
    let (name, rest) = package_name(reference);
    // `#` imports, rooted paths, and dot or empty components are not modeled;
    // the latter would leave the linked directory inside `node_modules`.
    if reference.starts_with('#')
        || rooted(reference)
        || name.is_empty()
        || name
            .split('/')
            .chain(rest.split('/').filter(|_| !rest.is_empty()))
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return unsupported();
    }
    let inventory = snapshot.inventory();
    let mut directory = parent(config.as_str());
    let scope = loop {
        let manifest = child(directory, "package.json");
        work.charge(manifest.len() + 1)?;
        let Ok(manifest) = SourcePath::new(&manifest, limits.path) else {
            return Ok(Extended::Issue(config.clone(), ConfigProblem::Invalid));
        };
        if inventory.files().find(&manifest).is_some() {
            break directory;
        }
        if inventory
            .entry(&manifest)
            .is_some_and(|entry| !matches!(entry.kind(), EntryKind::Directory))
        {
            return Ok(Extended::Issue(manifest, ConfigProblem::Unreadable));
        }
        if directory.is_empty() {
            return unsupported();
        }
        directory = parent(directory);
    };
    let Some(workspace) = links.workspace(snapshot, limits)? else {
        return unsupported();
    };
    if !workspace.is_workspace() || !workspace.complete {
        return unsupported();
    }
    let declaring = if scope.is_empty() {
        analyzed(&workspace.root.manifest)
    } else {
        let Ok(scope) = SourcePath::new(scope, limits.path) else {
            return unsupported();
        };
        workspace
            .member(&scope)
            .and_then(|member| analyzed(&member.manifest))
    };
    let Some(declaring) = declaring else {
        return unsupported();
    };
    // A self-reference resolves through the package's own `exports`, and pnpm
    // also links the workspace root by its name; neither is modeled.
    let named = |manifest: Option<&frameshift_package::PackageManifest>| {
        manifest
            .and_then(|manifest| manifest.name.as_ref())
            .is_some_and(|field| &*field.value == name)
    };
    if named(Some(declaring)) || named(analyzed(&workspace.root.manifest)) {
        return unsupported();
    }
    let mut members = workspace
        .members
        .iter()
        .filter(|member| member.name() == Some(name));
    let (Some(member), None) = (members.next(), members.next()) else {
        return unsupported();
    };
    let version = analyzed(&member.manifest).and_then(|manifest| manifest.parsed_version());
    let mut declared = false;
    for entry in declaring
        .dependencies
        .iter()
        .chain(&declaring.dev_dependencies)
        .chain(&declaring.optional_dependencies)
        .filter(|entry| &*entry.key == name)
    {
        work.charge(entry.value.text.len() + 1)?;
        let DependencySpec::Workspace(text) = &entry.value.spec else {
            return unsupported();
        };
        // Aliases and `workspace:` paths fail to parse as ranges.
        let linked = matches!(&**text, "*" | "^" | "~")
            || !text.is_empty()
                && VersionRange::parse(text)
                    .is_ok_and(|range| version.as_ref().is_some_and(|v| range.satisfies(v)));
        if !linked {
            return unsupported();
        }
        declared = true;
    }
    if !declared {
        return unsupported();
    }
    let mut lookup = Lookup {
        snapshot,
        limits,
        work,
        config,
        member: member.directory.as_str(),
    };
    Ok(lookup
        .package(rest)?
        .unwrap_or_else(|| Extended::Issue(config.clone(), ConfigProblem::Missing)))
}

/// A manifest inside the member.
enum Manifest {
    Absent,
    Found(PackageManifest),
    Stop(Extended),
}

/// The outcome of one `exports` step, as in package-map resolution.
enum Step {
    Continue,
    Null,
    Done(Extended),
}

/// The configuration lookup inside one linked member. Paths stand for the
/// member's link in `node_modules`, whose real path the reference reports.
struct Lookup<'a> {
    snapshot: &'a Snapshot,
    limits: GraphLimits,
    work: &'a mut Work,
    config: &'a SourcePath,
    member: &'a str,
}

impl Lookup<'_> {
    fn unsupported(&self) -> Extended {
        Extended::Issue(self.config.clone(), ConfigProblem::Unsupported)
    }

    /// `loadModuleFromSpecificNodeModulesDirectory` for JSON, with the member
    /// as the linked package directory. `None` is a request that the member
    /// cannot satisfy.
    fn package(&mut self, rest: &str) -> Result<Option<Extended>, ModuleError> {
        let member = self.member;
        let root = match self.manifest(member)? {
            Manifest::Found(manifest) => manifest,
            Manifest::Absent => return Ok(Some(self.unsupported())),
            Manifest::Stop(issue) => return Ok(Some(issue)),
        };
        let candidate = if rest.is_empty() {
            member.to_owned()
        } else {
            child(member, rest)
        };
        if !rest.is_empty() {
            match self.manifest(&candidate)? {
                Manifest::Stop(issue) => return Ok(Some(issue)),
                // A subdirectory's own manifest applies unless the package has
                // an `exports` key, even a null one.
                Manifest::Found(nested) if root.exports.is_none() => {
                    if let Some(found) = self.file(&candidate)? {
                        return Ok(Some(found));
                    }
                    return self.directory(&candidate, Some(&nested));
                }
                Manifest::Found(_) | Manifest::Absent => {}
            }
        }
        if let Some(exports) = &root.exports
            && exports.truthy()
        {
            let subpath = if rest.is_empty() {
                ".".to_owned()
            } else {
                format!("./{rest}")
            };
            return self.exports(exports, &subpath);
        }
        if root.types_versions {
            return Ok(Some(self.unsupported()));
        }
        if let Some(found) = self.file(&candidate)? {
            return Ok(Some(found));
        }
        self.directory(&candidate, rest.is_empty().then_some(&root))
    }

    /// One file probe. Only `<name>.json` beside the link lies outside the
    /// member; no package manager creates it, so it is absent.
    fn probe(&mut self, path: &str) -> Result<Option<Extended>, ModuleError> {
        self.work.charge(path.len() + 1)?;
        if !below(path, self.member) {
            return Ok(None);
        }
        let Ok(exact) = SourcePath::new(path, self.limits.path) else {
            return Ok(Some(Extended::Issue(
                self.config.clone(),
                ConfigProblem::Invalid,
            )));
        };
        let inventory = self.snapshot.inventory();
        if inventory.files().find(&exact).is_some() {
            return Ok(Some(Extended::Found(exact)));
        }
        // A link, ignored, or excluded entry on the path may hide the file.
        let mut prefix = path;
        while prefix != self.member {
            if let Ok(entry) = SourcePath::new(prefix, self.limits.path)
                && inventory
                    .entry(&entry)
                    .is_some_and(|entry| !matches!(entry.kind(), EntryKind::Directory))
            {
                return Ok(Some(Extended::Issue(entry, ConfigProblem::Unreadable)));
            }
            prefix = parent(prefix);
        }
        Ok(None)
    }

    fn first(&mut self, candidates: &[String]) -> Result<Option<Extended>, ModuleError> {
        for candidate in candidates {
            if let Some(outcome) = self.probe(candidate)? {
                return Ok(Some(outcome));
            }
        }
        Ok(None)
    }

    /// `loadModuleFromFile` for JSON in a configuration lookup.
    fn file(&mut self, candidate: &str) -> Result<Option<Extended>, ModuleError> {
        let mut candidates = Vec::new();
        from_file(candidate, CONFIG, &mut candidates);
        self.first(&candidates)
    }

    fn manifest(&mut self, directory: &str) -> Result<Manifest, ModuleError> {
        let path = child(directory, "package.json");
        let path = match self.probe(&path)? {
            None => return Ok(Manifest::Absent),
            Some(Extended::Found(path)) => path,
            Some(issue @ Extended::Issue(..)) => return Ok(Manifest::Stop(issue)),
        };
        let inventory = self.snapshot.inventory();
        let id = inventory
            .files()
            .find(&path)
            .expect("a found probe is an admitted file");
        let unreadable =
            |path: SourcePath| Manifest::Stop(Extended::Issue(path, ConfigProblem::Unreadable));
        let FileRead::Content(file) = self.snapshot.read(id).map_err(ModuleError::Snapshot)? else {
            return Ok(unreadable(path));
        };
        self.work.charge(file.bytes().len())?;
        let Ok(source) = SourceText::from_bytes(file.bytes().to_vec(), self.limits.text) else {
            return Ok(unreadable(path));
        };
        Ok(match PackageManifest::parse(&source, self.limits.json) {
            Some(manifest) => Manifest::Found(manifest),
            None => Manifest::Stop(Extended::Issue(path, ConfigProblem::Invalid)),
        })
    }

    /// `loadNodeModuleFromDirectoryWorker` for JSON in a configuration
    /// lookup: the `tsconfig` field of the directory's own manifest, then the
    /// `tsconfig.json` index.
    fn directory(
        &mut self,
        candidate: &str,
        manifest: Option<&PackageManifest>,
    ) -> Result<Option<Extended>, ModuleError> {
        if let Some(manifest) = manifest {
            if manifest.types_versions {
                return Ok(Some(self.unsupported()));
            }
            if let Some(field) = &manifest.tsconfig {
                // Outside the member, the field names a path relative to the
                // link in `node_modules`, which is not read.
                let Some((path, trailing)) = normalize(candidate, field)
                    .filter(|(path, _)| path == self.member || below(path, self.member))
                else {
                    return Ok(Some(self.unsupported()));
                };
                let mut candidates = Vec::new();
                if trailing {
                    field_candidates(&format!("{path}/"), CONFIG, &mut candidates);
                } else {
                    field_candidates(&path, CONFIG, &mut candidates);
                    from_file(&path, CONFIG, &mut candidates);
                }
                from_file(&child(&path, "tsconfig"), CONFIG, &mut candidates);
                if let Some(found) = self.first(&candidates)? {
                    return Ok(Some(found));
                }
            }
        }
        let mut candidates = Vec::new();
        from_file(&child(candidate, "tsconfig"), CONFIG, &mut candidates);
        self.first(&candidates)
    }

    /// `loadModuleFromExports` with the configuration conditions `require`,
    /// `types`, `node`, and `default`.
    fn exports(
        &mut self,
        exports: &Target,
        subpath: &str,
    ) -> Result<Option<Extended>, ModuleError> {
        let step = if subpath == "." {
            let main = match exports {
                Target::String(_) | Target::Array(_) => Some(exports),
                Target::Object(members) if !members.iter().any(|(k, _)| k.starts_with('.')) => {
                    Some(exports)
                }
                Target::Object(members) => members.iter().find(|(k, _)| k == ".").map(|(_, t)| t),
                Target::Null | Target::Scalar(_) => None,
            };
            match main {
                Some(main) if main.truthy() => self.target(main, "", false)?,
                Some(_) | None => Step::Continue,
            }
        } else {
            match exports {
                Target::Object(table) if table.iter().all(|(k, _)| k.starts_with('.')) => {
                    for (key, _) in table {
                        self.work.charge(key.len() + subpath.len() + 1)?;
                    }
                    match select(table, subpath) {
                        None => Step::Continue,
                        Some((target, rest, pattern)) => self.target(target, &rest, pattern)?,
                    }
                }
                Target::Object(_)
                | Target::String(_)
                | Target::Array(_)
                | Target::Null
                | Target::Scalar(_) => Step::Continue,
            }
        };
        Ok(match step {
            Step::Done(outcome) => Some(outcome),
            // The reference then continues in ancestor `node_modules`
            // directories, which are not read.
            Step::Continue | Step::Null => None,
        })
    }

    /// `loadModuleFromTargetExportOrImport` for `exports`.
    fn target(
        &mut self,
        target: &Target,
        subpath: &str,
        pattern: bool,
    ) -> Result<Step, ModuleError> {
        self.work.charge(subpath.len() + 1)?;
        match target {
            Target::String(text) => self.string_target(text, subpath, pattern),
            Target::Null => Ok(Step::Null),
            Target::Scalar(_) => Ok(Step::Continue),
            Target::Array(items) => {
                for item in items {
                    let step = self.target(item, subpath, pattern)?;
                    if !matches!(step, Step::Continue) {
                        return Ok(step);
                    }
                }
                Ok(Step::Continue)
            }
            Target::Object(conditions) => {
                for (condition, item) in conditions {
                    self.work.charge(condition.len() + 1)?;
                    // Versioned conditions use TypeScript's own range grammar.
                    if condition.starts_with("types@") {
                        return Ok(Step::Done(self.unsupported()));
                    }
                    if matches!(condition.as_str(), "default" | "require" | "types" | "node") {
                        let step = self.target(item, subpath, pattern)?;
                        if !matches!(step, Step::Continue) {
                            return Ok(step);
                        }
                    }
                }
                Ok(Step::Continue)
            }
        }
    }

    fn string_target(
        &mut self,
        text: &str,
        subpath: &str,
        pattern: bool,
    ) -> Result<Step, ModuleError> {
        if !pattern && !subpath.is_empty() && !text.ends_with('/') {
            return Ok(Step::Continue);
        }
        if !text.starts_with("./") {
            return Ok(Step::Continue);
        }
        let text = text.replace('\\', "/");
        let invalid = |part: &&str| matches!(*part, "." | ".." | "node_modules");
        if components(&text).iter().skip(1).any(invalid)
            || components(&subpath.replace('\\', "/")).iter().any(invalid)
        {
            return Ok(Step::Continue);
        }
        let Some(joined) = (if pattern {
            replace_stars(&text, subpath, None)
        } else {
            Some(format!("{text}{subpath}"))
        }) else {
            return Ok(Step::Done(self.unsupported()));
        };
        let Some((path, _)) = normalize(self.member, &joined) else {
            return Ok(Step::Done(self.unsupported()));
        };
        let mut candidates = Vec::new();
        field_candidates(&path, CONFIG, &mut candidates);
        Ok(match self.first(&candidates)? {
            Some(outcome) => Step::Done(outcome),
            None => Step::Continue,
        })
    }
}
