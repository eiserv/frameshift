//! Workspace detection: the declarations of a workspace root, the member
//! directories that the package manager selects, and the package that a
//! run analyzes (`D-022`, `D-073`).
//!
//! The root of the run is the workspace root. Its `package.json`
//! `workspaces` field (npm, Yarn, Bun) and its `pnpm-workspace.yaml`
//! (pnpm) are evaluated with the pattern semantics of each package manager
//! release that can install the workspace: the one that `packageManager`
//! names, or every modeled release of the detected manager, or every
//! release whose declaration is present when no manager is detected. A
//! directory is a member only when all evaluated releases select it;
//! nothing below the root is guessed.

mod matcher;
mod pattern;
mod semantics;

#[cfg(test)]
mod tests;

pub use pattern::PatternRejection;
pub use semantics::WorkspaceSemantics;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use frameshift_core::{
    FindingLocation, FindingText, Severity, SourcePath, SourceText, Span, TextLocation,
};
use frameshift_input::{EntryKind, Inventory, Snapshot};
use frameshift_syntax::{YamlScalarStyle, YamlStream, YamlValue};

use crate::load::{
    LoadError, LoadLimits, LockfileState, ManifestState, PackageDirectory, Reporter, decode,
    load_directory, parse_yaml_file, read_manifest, report_stale,
};
use crate::lockfile::{LockedPackage, Lockfile};
use crate::manager::PackageManagerKind;
use crate::manifest::{ManifestField, PackageManifest};
use crate::range::VersionRange;
use crate::version::Version;
use semantics::{
    Bounds, Declaration, Facts, FailureKind, Files, Node, Outcome, PatternInput, Presence, Read,
    Tree, Undetermined, evaluate,
};

/// The workspace model's limits (`D-073`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceLimits {
    /// The most patterns that one declaration may have, default 1,024.
    pub max_patterns: usize,
    /// The longest pattern in bytes, default 4,096.
    pub max_pattern_bytes: usize,
    /// The most brace alternatives of one pattern segment, default 256.
    pub max_alternatives: usize,
    /// The matching steps that evaluating one release may take, default
    /// 100,000,000.
    pub max_match_work: u64,
}

impl Default for WorkspaceLimits {
    fn default() -> Self {
        Self {
            max_patterns: 1024,
            max_pattern_bytes: 4096,
            max_alternatives: 256,
            max_match_work: 100_000_000,
        }
    }
}

/// Where workspace patterns are declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationFile {
    /// The `workspaces` field of the root `package.json`.
    PackageJson,
    /// The `packages` field of `pnpm-workspace.yaml`.
    PnpmWorkspace,
}

/// A workspace member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The member directory.
    pub directory: SourcePath,
    /// Its `package.json`.
    pub manifest: ManifestState,
}

impl Member {
    /// Returns the `name` of the member's analyzed `package.json`.
    pub fn name(&self) -> Option<&str> {
        match &self.manifest {
            ManifestState::Analyzed(manifest) => manifest.name.as_ref().map(|name| &*name.value),
            ManifestState::Absent | ManifestState::NotAnalyzed => None,
        }
    }
}

/// The workspace rooted at the run's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    /// The root package directory with its lockfile.
    pub root: PackageDirectory,
    /// The declarations that the evaluated releases read, in
    /// [`DeclarationFile`] order.
    pub declarations: Vec<DeclarationFile>,
    /// The releases whose semantics were evaluated, empty when the root
    /// declares no workspace for the package manager that installs it.
    pub semantics: Vec<WorkspaceSemantics>,
    /// The members that every evaluated release selects, sorted by path.
    pub members: Vec<Member>,
    /// Directories whose membership is not known, sorted by path.
    pub undetermined: Vec<SourcePath>,
    /// Whether the members are known exactly; findings say why not.
    pub complete: bool,
}

/// Why a package cannot be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// The directory is not the root and not a known member.
    NotAMember,
    /// The directory's membership is not known.
    Undetermined,
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAMember => "the directory is not the root or a member of its workspace",
            Self::Undetermined => "the directory's workspace membership is not known",
        })
    }
}

impl std::error::Error for SelectionError {}

/// The package that a run analyzes: the root or one member, resolved
/// through the root's lockfile (`D-022`).
#[derive(Clone, Copy, Debug)]
pub struct SelectedPackage<'w> {
    workspace: &'w Workspace,
    member: Option<&'w Member>,
}

impl<'w> SelectedPackage<'w> {
    /// The package directory, `None` for the root.
    pub fn directory(&self) -> Option<&'w SourcePath> {
        self.member.map(|member| &member.directory)
    }

    /// The package's `package.json`.
    pub fn manifest(&self) -> &'w ManifestState {
        match self.member {
            Some(member) => &member.manifest,
            None => &self.workspace.root.manifest,
        }
    }

    /// The root's parsed lockfile, if any.
    pub fn lockfile(&self) -> Option<&'w Lockfile> {
        match &self.workspace.root.lockfile {
            LockfileState::Parsed(lockfile) => Some(lockfile),
            LockfileState::None | LockfileState::NotParsed(_) => None,
        }
    }

    /// The package's workspace path in the lockfile, `""` for the root.
    pub fn lockfile_workspace(&self) -> &'w str {
        self.directory().map_or("", SourcePath::as_str)
    }

    /// Resolves a dependency that the package declares with `spec`
    /// through the root's lockfile.
    pub fn resolve_declared(&self, name: &str, spec: &str) -> Option<&'w LockedPackage> {
        self.lockfile()?
            .resolve_declared(self.lockfile_workspace(), name, spec)
    }

    /// Finds another member of the workspace, or the root, whose
    /// `package.json` has this name. Such siblings are external
    /// dependencies with unknown compatibility (`D-022`).
    pub fn sibling(&self, name: &str) -> Option<Option<&'w SourcePath>> {
        let directory = self.directory();
        let root = &self.workspace.root.manifest;
        if directory.is_some() && manifest_name(root) == Some(name) {
            return Some(None);
        }
        self.workspace
            .members
            .iter()
            .find(|member| Some(&member.directory) != directory && member.name() == Some(name))
            .map(|member| Some(&member.directory))
    }
}

impl Workspace {
    /// Whether the root declares a workspace for an evaluated release.
    pub fn is_workspace(&self) -> bool {
        !self.semantics.is_empty()
    }

    /// Finds a member by directory.
    pub fn member(&self, directory: &SourcePath) -> Option<&Member> {
        self.members
            .binary_search_by(|member| member.directory.cmp(directory))
            .ok()
            .map(|index| &self.members[index])
    }

    /// Selects the root (`None`) or a member as the analyzed package.
    pub fn select(
        &self,
        directory: Option<&SourcePath>,
    ) -> Result<SelectedPackage<'_>, SelectionError> {
        let Some(directory) = directory else {
            return Ok(SelectedPackage {
                workspace: self,
                member: None,
            });
        };
        if let Some(member) = self.member(directory) {
            return Ok(SelectedPackage {
                workspace: self,
                member: Some(member),
            });
        }
        if self.undetermined.binary_search(directory).is_ok() {
            return Err(SelectionError::Undetermined);
        }
        Err(SelectionError::NotAMember)
    }
}

fn manifest_name(manifest: &ManifestState) -> Option<&str> {
    match manifest {
        ManifestState::Analyzed(manifest) => manifest.name.as_ref().map(|name| &*name.value),
        ManifestState::Absent | ManifestState::NotAnalyzed => None,
    }
}

/// Loads the workspace at the snapshot's root: the root package
/// directory, its workspace declarations, and its members, each read at
/// most once through [`Snapshot::read`].
pub fn load_workspace(
    snapshot: &Snapshot,
    limits: LoadLimits,
    findings: &mut frameshift_core::FindingCollector,
) -> Result<Workspace, LoadError> {
    let mut report = Reporter { findings };
    let (root, root_source) = load_directory(snapshot, None, limits, &mut report, false)?;
    let root_facts = match (&root.manifest, &root_source) {
        (ManifestState::Analyzed(manifest), Some(source)) => {
            facts(manifest, &path("package.json")?, source)?
        }
        _ => Facts::default(),
    };
    let pnpm = read_pnpm_workspace(snapshot, limits, &mut report)?;
    let package_declared = root_facts.workspaces.is_some();
    let (releases, unmodeled) = releases(&root, package_declared, pnpm.declared());
    let mut workspace = Workspace {
        root,
        declarations: Vec::new(),
        semantics: Vec::new(),
        members: Vec::new(),
        undetermined: Vec::new(),
        complete: true,
    };
    ignored_declarations(&workspace.root, package_declared, &pnpm, &mut report)?;
    let selected: Vec<WorkspaceSemantics> = releases
        .into_iter()
        .filter(|release| {
            if release.reads_pnpm_workspace() {
                pnpm.declared()
            } else {
                package_declared
            }
        })
        .collect();
    if selected.is_empty() {
        check_stale(&workspace, &mut report)?;
        return Ok(workspace);
    }
    if unmodeled {
        report.push(
            FindingLocation::File(path("package.json")?),
            FS1037,
            Severity::Review,
            TEXT_FS1037,
        )?;
    }
    if selected
        .iter()
        .any(|release| !release.reads_pnpm_workspace())
    {
        workspace.declarations.push(DeclarationFile::PackageJson);
    }
    if selected
        .iter()
        .any(|release| release.reads_pnpm_workspace())
    {
        workspace.declarations.push(DeclarationFile::PnpmWorkspace);
    }
    workspace.semantics.clone_from(&selected);
    let tree = tree(snapshot.inventory())?;
    let mut files = SnapshotFiles {
        snapshot,
        limits,
        report,
        cache: BTreeMap::new(),
    };
    let bounds = Bounds {
        parse: pattern::ParseBounds {
            max_bytes: limits.workspace.max_pattern_bytes,
            max_alternatives: limits.workspace.max_alternatives,
        },
        max_match_work: limits.workspace.max_match_work,
    };
    let empty = Declaration {
        patterns: Vec::new(),
        complete: true,
    };
    let mut outcomes = Vec::new();
    let mut case_dependent = false;
    for release in &selected {
        let declaration = if release.reads_pnpm_workspace() {
            pnpm.declaration.as_ref().unwrap_or(&empty)
        } else {
            root_facts.workspaces.as_ref().unwrap_or(&empty)
        };
        if declaration.patterns.len() > limits.workspace.max_patterns {
            let explanation = format!(
                "The workspace declaration has more patterns than workspace.max-patterns ({}), \
                 so its members are not determined.",
                limits.workspace.max_patterns
            );
            files.report.push(
                FindingLocation::File(declaration_path(*release)?),
                FS1038,
                Severity::Unknown,
                FindingText {
                    message: "Workspace limit reached",
                    explanation: &explanation,
                    remediation: TEXT_LIMIT_REMEDIATION,
                },
            )?;
            outcomes.push((*release, None));
            continue;
        }
        if pnpm.invalid && release.reads_pnpm_workspace() {
            outcomes.push((*release, None));
            continue;
        }
        let outcome = evaluate(
            *release,
            &tree,
            &mut files,
            &root_facts,
            declaration,
            bounds,
            false,
        )?;
        if outcome.is_exact() {
            let mut presence = PresenceOnly { snapshot, limits };
            let sensitive = evaluate(
                *release,
                &tree,
                &mut presence,
                &root_facts,
                declaration,
                bounds,
                false,
            )?;
            let folded = evaluate(
                *release,
                &tree,
                &mut presence,
                &root_facts,
                declaration,
                bounds,
                true,
            )?;
            if !folded.exhausted && candidates(&sensitive) != candidates(&folded) {
                case_dependent = true;
            }
        }
        outcomes.push((*release, Some(outcome)));
    }
    let SnapshotFiles { report, cache, .. } = files;
    let mut report = report;
    if case_dependent {
        let location = FindingLocation::File(declaration_path(selected[0])?);
        report.push(location, FS1033, Severity::Review, TEXT_FS1033)?;
    }
    aggregate(&mut workspace, &outcomes, cache, &mut report)?;
    if !snapshot.inventory().is_complete() {
        workspace.complete = false;
        report.push(
            FindingLocation::Project,
            FS1031,
            Severity::Unknown,
            TEXT_FS1031_INCOMPLETE,
        )?;
    }
    check_stale(&workspace, &mut report)?;
    Ok(workspace)
}

/// The members that one release selects at the snapshot's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseMembers {
    /// The member directories, sorted by path.
    pub members: Vec<SourcePath>,
    /// Whether the members are known exactly: every pattern was
    /// evaluated, and no manifest or untraversed entry left one open.
    pub exact: bool,
    /// Whether the release would reject the workspace.
    pub rejects: bool,
}

/// Evaluates the root's workspace declaration with one release's
/// semantics, whichever package manager the root uses. This explains
/// [`Workspace::undetermined`] directories and serves differential tests;
/// [`load_workspace`] decides which releases apply.
pub fn release_members(
    snapshot: &Snapshot,
    limits: LoadLimits,
    release: WorkspaceSemantics,
    findings: &mut frameshift_core::FindingCollector,
) -> Result<ReleaseMembers, LoadError> {
    let mut report = Reporter { findings };
    let (root, root_source) = load_directory(snapshot, None, limits, &mut report, false)?;
    let root_facts = match (&root.manifest, &root_source) {
        (ManifestState::Analyzed(manifest), Some(source)) => {
            facts(manifest, &path("package.json")?, source)?
        }
        _ => Facts::default(),
    };
    let pnpm = read_pnpm_workspace(snapshot, limits, &mut report)?;
    let empty = Declaration {
        patterns: Vec::new(),
        complete: true,
    };
    let declaration = if release.reads_pnpm_workspace() {
        if pnpm.invalid {
            return Ok(ReleaseMembers {
                members: Vec::new(),
                exact: false,
                rejects: true,
            });
        }
        pnpm.declaration.as_ref().unwrap_or(&empty)
    } else {
        root_facts.workspaces.as_ref().unwrap_or(&empty)
    };
    let tree = tree(snapshot.inventory())?;
    let mut files = SnapshotFiles {
        snapshot,
        limits,
        report,
        cache: BTreeMap::new(),
    };
    let bounds = Bounds {
        parse: pattern::ParseBounds {
            max_bytes: limits.workspace.max_pattern_bytes,
            max_alternatives: limits.workspace.max_alternatives,
        },
        max_match_work: limits.workspace.max_match_work,
    };
    let outcome = evaluate(
        release,
        &tree,
        &mut files,
        &root_facts,
        declaration,
        bounds,
        false,
    )?;
    Ok(ReleaseMembers {
        exact: outcome.is_exact() && snapshot.inventory().is_complete(),
        rejects: !outcome.failures.is_empty(),
        members: outcome.members.into_iter().collect(),
    })
}

/// The directories that an outcome selects or cannot decide.
fn candidates(outcome: &Outcome) -> BTreeSet<SourcePath> {
    outcome
        .members
        .iter()
        .chain(outcome.undetermined.keys())
        .cloned()
        .collect()
}

/// The releases that can install the workspace, and whether
/// `packageManager` names a release whose semantics are not modeled.
fn releases(
    root: &PackageDirectory,
    package_declared: bool,
    pnpm_declared: bool,
) -> (Vec<WorkspaceSemantics>, bool) {
    use WorkspaceSemantics as S;
    let spec = root.package_manager.as_ref();
    let Some(manager) = root.detection.manager else {
        let mut all = Vec::new();
        if package_declared {
            all.extend([
                S::Npm,
                S::YarnClassic,
                S::YarnBerryV2,
                S::YarnBerryV4,
                S::Bun,
            ]);
        }
        if pnpm_declared {
            all.extend([S::PnpmV10, S::PnpmV11, S::PnpmV12]);
        }
        return (all, false);
    };
    let version = spec.map(|spec| &spec.version);
    let major = version.map(Version::major);
    match manager {
        PackageManagerKind::Npm => (vec![S::Npm], major.is_some_and(|major| major != 11)),
        PackageManagerKind::Pnpm => match major {
            Some(10) => (vec![S::PnpmV10], false),
            Some(11) => (vec![S::PnpmV11], false),
            Some(12) => (vec![S::PnpmV12], false),
            Some(_) => (vec![S::PnpmV10, S::PnpmV11, S::PnpmV12], true),
            None => (vec![S::PnpmV10, S::PnpmV11, S::PnpmV12], false),
        },
        PackageManagerKind::Yarn => match major {
            Some(1) => (vec![S::YarnClassic], false),
            Some(2 | 3) => (vec![S::YarnBerryV2], false),
            Some(4) => (vec![S::YarnBerryV4], false),
            Some(_) => (vec![S::YarnBerryV2, S::YarnBerryV4], true),
            None => match &root.lockfile {
                LockfileState::Parsed(lockfile) => match lockfile.version() {
                    1 => (vec![S::YarnClassic], false),
                    4..=6 => (vec![S::YarnBerryV2], false),
                    _ => (vec![S::YarnBerryV4], false),
                },
                LockfileState::None | LockfileState::NotParsed(_) => {
                    (vec![S::YarnClassic, S::YarnBerryV2, S::YarnBerryV4], false)
                }
            },
        },
        PackageManagerKind::Bun => (
            vec![S::Bun],
            version.is_some_and(|version| version.major() != 1 || version.minor() != 4),
        ),
    }
}

/// Reports a declaration that the detected package manager does not read.
fn ignored_declarations(
    root: &PackageDirectory,
    package_declared: bool,
    pnpm: &PnpmWorkspace,
    report: &mut Reporter<'_>,
) -> Result<(), LoadError> {
    let Some(manager) = root.detection.manager else {
        return Ok(());
    };
    let pnpm_manager = manager == PackageManagerKind::Pnpm;
    if pnpm_manager && package_declared {
        report.file(
            &path("package.json")?,
            FS1034,
            Severity::Review,
            TEXT_FS1034,
        )?;
    }
    if !pnpm_manager && pnpm.present {
        report.file(
            &path("pnpm-workspace.yaml")?,
            FS1034,
            Severity::Review,
            TEXT_FS1034,
        )?;
    }
    Ok(())
}

fn declaration_path(release: WorkspaceSemantics) -> Result<SourcePath, LoadError> {
    path(if release.reads_pnpm_workspace() {
        "pnpm-workspace.yaml"
    } else {
        "package.json"
    })
}

fn path(text: &str) -> Result<SourcePath, LoadError> {
    SourcePath::new(text, frameshift_core::PathLimits::default()).map_err(|_| LoadError::Internal)
}

/// Combines the outcomes of the evaluated releases into the workspace's
/// members and reports what is not known.
fn aggregate(
    workspace: &mut Workspace,
    outcomes: &[(WorkspaceSemantics, Option<Outcome>)],
    mut cache: BTreeMap<SourcePath, Cached>,
    report: &mut Reporter<'_>,
) -> Result<(), LoadError> {
    let mut rejected = BTreeSet::new();
    let mut hidden = BTreeSet::new();
    let mut failures = BTreeSet::new();
    let mut undetermined: BTreeMap<SourcePath, Undetermined> = BTreeMap::new();
    let mut indeterminate = false;
    let mut exhausted = false;
    for (release, outcome) in outcomes {
        let Some(outcome) = outcome else {
            indeterminate = true;
            continue;
        };
        for (location, rejection) in &outcome.rejected {
            rejected.insert((location.clone(), *rejection));
        }
        hidden.extend(outcome.hidden.iter().cloned());
        for failure in &outcome.failures {
            failures.insert((failure.clone(), *release));
        }
        for (directory, reason) in &outcome.undetermined {
            undetermined.entry(directory.clone()).or_insert(*reason);
        }
        indeterminate |= !outcome.rejected.is_empty() || outcome.incomplete_declaration;
        exhausted |= outcome.exhausted;
    }
    for (location, rejection) in &rejected {
        let explanation = format!(
            "A workspace pattern is not evaluated because {rejection}, so the workspace members \
             are not determined."
        );
        report.push(
            FindingLocation::Text(location.clone()),
            FS1030,
            Severity::Unknown,
            FindingText {
                message: "Workspace pattern not evaluated",
                explanation: &explanation,
                remediation: "Rewrite the pattern with literal names, `*`, `**`, `?`, simple \
                              classes, or brace alternatives.",
            },
        )?;
    }
    if exhausted {
        let explanation = "Evaluating the workspace patterns reached workspace.max-match-work, \
                           so the workspace members are not determined.";
        report.push(
            FindingLocation::Project,
            FS1038,
            Severity::Unknown,
            FindingText {
                message: "Workspace limit reached",
                explanation,
                remediation: TEXT_LIMIT_REMEDIATION,
            },
        )?;
    }
    for entry in &hidden {
        report.file(entry, FS1031, Severity::Unknown, TEXT_FS1031)?;
    }
    for (failure, release) in &failures {
        let (message, explanation) = match failure.kind {
            FailureKind::RootNotPrivate => (
                "Package manager rejects the workspace",
                "Yarn 1 requires \"private\": true in a workspace root and fails on this one.",
            ),
            FailureKind::DuplicateName => (
                "Package manager rejects the workspace",
                "Two workspace members have the same name, which this package manager rejects.",
            ),
            FailureKind::MissingListed => (
                "Package manager rejects the workspace",
                "Bun requires a package.json in every directory that the workspaces field lists \
                 without wildcards, and this one has none.",
            ),
            FailureKind::MissingName => (
                "Package manager rejects the workspace",
                "Bun requires a name in the package.json of every workspace member, and this one \
                 has none.",
            ),
            FailureKind::InvalidManifest => (
                "Package manager rejects the workspace",
                "The package.json of a workspace member is not valid, so the package manager \
                 fails to read the workspace.",
            ),
        };
        let explanation = format!("{explanation} This holds for {}.", release.releases());
        let location = match &failure.directory {
            Some(directory) => FindingLocation::File(directory.clone()),
            None => FindingLocation::File(path("package.json")?),
        };
        report.push(
            location,
            FS1035,
            Severity::Review,
            FindingText {
                message,
                explanation: &explanation,
                remediation: "Correct the workspace so that the package manager installs it, \
                              then run Frameshift again.",
            },
        )?;
    }
    for (directory, reason) in &undetermined {
        let explanation = match reason {
            Undetermined::HiddenManifest => {
                "The directory's package.json is not an admitted file, for example because it \
                 is ignored or too large, so its workspace membership is not known."
            }
            Undetermined::UnreadManifest => {
                "The directory has only a manifest format that Frameshift does not read \
                 (yarn.json, package.yaml, or package.json5), so its workspace membership is not \
                 known."
            }
            Undetermined::InvalidManifest => continue,
            Undetermined::NestedUnknown => {
                "The member's own workspaces field is not usable, so the nested workspace \
                 members that Yarn 2 and later read from it are not known."
            }
        };
        report.push(
            FindingLocation::File(directory.clone()),
            FS1036,
            Severity::Unknown,
            FindingText {
                message: "Workspace membership not known",
                explanation,
                remediation: "Make the manifest a package.json that discovery admits, then run \
                              Frameshift again.",
            },
        )?;
    }
    let evaluated: Vec<(WorkspaceSemantics, &Outcome)> = outcomes
        .iter()
        .filter_map(|(release, outcome)| outcome.as_ref().map(|outcome| (*release, outcome)))
        .collect();
    let mut members = BTreeSet::new();
    let mut disputed = BTreeMap::new();
    if !indeterminate && !exhausted {
        let all: BTreeSet<&SourcePath> = evaluated
            .iter()
            .flat_map(|(_, outcome)| outcome.members.iter())
            .collect();
        for directory in all {
            let selecting: Vec<WorkspaceSemantics> = evaluated
                .iter()
                .filter(|(_, outcome)| outcome.members.contains(directory))
                .map(|(release, _)| *release)
                .collect();
            if undetermined.contains_key(directory) {
                continue;
            }
            if selecting.len() == evaluated.len() {
                members.insert(directory.clone());
            } else {
                disputed.insert(directory.clone(), selecting);
            }
        }
    }
    for (directory, selecting) in &disputed {
        let names: Vec<&str> = selecting.iter().map(|release| release.releases()).collect();
        let explanation = format!(
            "The package manager release is not determined, and only {} select this directory as \
             a workspace member, so its membership is not known.",
            names.join(", ")
        );
        report.push(
            FindingLocation::File(directory.clone()),
            FS1032,
            Severity::Unknown,
            FindingText {
                message: "Workspace membership differs between package managers",
                explanation: &explanation,
                remediation: "Add a packageManager field to the root package.json, then run \
                              Frameshift again.",
            },
        )?;
    }
    workspace.complete = !indeterminate
        && !exhausted
        && hidden.is_empty()
        && undetermined.is_empty()
        && disputed.is_empty();
    let mut unknown: BTreeSet<SourcePath> = undetermined.keys().cloned().collect();
    unknown.extend(disputed.keys().cloned());
    workspace.undetermined = unknown.into_iter().collect();
    for directory in members {
        let manifest = cache
            .remove(&directory)
            .map_or(ManifestState::Absent, |cached| cached.state);
        workspace.members.push(Member {
            directory,
            manifest,
        });
    }
    Ok(())
}

/// Reports the lockfile as stale for the root and every member whose
/// declarations it does not record (FS1027).
fn check_stale(workspace: &Workspace, report: &mut Reporter<'_>) -> Result<(), LoadError> {
    let LockfileState::Parsed(lockfile) = &workspace.root.lockfile else {
        return Ok(());
    };
    let Some(kind) = workspace.root.detection.lockfile else {
        return Ok(());
    };
    let lockfile_path = path(kind.file_name())?;
    let siblings = Siblings { workspace };
    if let ManifestState::Analyzed(manifest) = &workspace.root.manifest {
        let stale = lockfile.out_of_sync_in("", manifest, &|name, spec| {
            siblings.satisfies(None, name, spec)
        });
        report_stale(report, &lockfile_path, lockfile, &stale)?;
    }
    for member in &workspace.members {
        let ManifestState::Analyzed(manifest) = &member.manifest else {
            continue;
        };
        let stale = lockfile.out_of_sync_in(member.directory.as_str(), manifest, &|name, spec| {
            siblings.satisfies(Some(&member.directory), name, spec)
        });
        if !stale.is_empty() {
            let location = SourcePath::new(
                &format!("{}/package.json", member.directory.as_str()),
                frameshift_core::PathLimits::default(),
            )
            .map_err(|_| LoadError::Internal)?;
            report_stale(report, &location, lockfile, &stale)?;
        }
    }
    Ok(())
}

/// Whether a declaration is satisfied by a workspace package, as Yarn 1
/// links one whose version satisfies the range instead of locking it.
struct Siblings<'a> {
    workspace: &'a Workspace,
}

impl Siblings<'_> {
    fn satisfies(&self, from: Option<&SourcePath>, name: &str, spec: &str) -> bool {
        let candidates = std::iter::once((None, &self.workspace.root.manifest)).chain(
            self.workspace
                .members
                .iter()
                .map(|member| (Some(&member.directory), &member.manifest)),
        );
        for (directory, manifest) in candidates {
            if directory == from {
                continue;
            }
            let ManifestState::Analyzed(manifest) = manifest else {
                continue;
            };
            if manifest.name.as_ref().map(|field| &*field.value) != Some(name) {
                continue;
            }
            let Some(version) = manifest.parsed_version() else {
                continue;
            };
            if VersionRange::parse(spec).is_ok_and(|range| range.satisfies(&version)) {
                return true;
            }
        }
        false
    }
}

/// The cached reading of one `package.json`.
struct Cached {
    state: ManifestState,
    read: Read,
}

struct SnapshotFiles<'s, 'f> {
    snapshot: &'s Snapshot,
    limits: LoadLimits,
    report: Reporter<'f>,
    cache: BTreeMap<SourcePath, Cached>,
}

impl Files for SnapshotFiles<'_, '_> {
    fn presence(&self, directory: &SourcePath, name: &str) -> Presence {
        presence(self.snapshot.inventory(), directory, name, self.limits)
    }

    fn manifest(&mut self, directory: &SourcePath) -> Result<Read, LoadError> {
        if let Some(cached) = self.cache.get(directory) {
            return Ok(cached.read.clone());
        }
        let file = child(directory, "package.json", self.limits)?;
        let (state, _, source) =
            read_manifest(self.snapshot, &file, self.limits, &mut self.report)?;
        let read = match (&state, &source) {
            (ManifestState::Analyzed(manifest), Some(source)) => {
                Read::Analyzed(facts(manifest, &file, source)?)
            }
            _ => Read::NotAnalyzed,
        };
        self.cache.insert(
            directory.clone(),
            Cached {
                state,
                read: read.clone(),
            },
        );
        Ok(read)
    }
}

/// Files that answer from discovery alone, as the case sensitivity check
/// compares candidate directories without reading manifests.
struct PresenceOnly<'s> {
    snapshot: &'s Snapshot,
    limits: LoadLimits,
}

impl Files for PresenceOnly<'_> {
    fn presence(&self, directory: &SourcePath, name: &str) -> Presence {
        presence(self.snapshot.inventory(), directory, name, self.limits)
    }

    fn manifest(&mut self, _: &SourcePath) -> Result<Read, LoadError> {
        Ok(Read::Analyzed(Facts {
            name: Some("-".into()),
            version: Some("0.0.0".into()),
            private: true,
            workspaces: None,
        }))
    }
}

fn presence(
    inventory: &Inventory,
    directory: &SourcePath,
    name: &str,
    limits: LoadLimits,
) -> Presence {
    let Ok(file) = child(directory, name, limits) else {
        return Presence::Hidden;
    };
    if inventory.files().find(&file).is_some() {
        return Presence::Admitted;
    }
    match inventory.entry(&file) {
        Some(_) => Presence::Hidden,
        None => Presence::Absent,
    }
}

fn child(directory: &SourcePath, name: &str, limits: LoadLimits) -> Result<SourcePath, LoadError> {
    SourcePath::new(&format!("{}/{name}", directory.as_str()), limits.path)
        .map_err(|_| LoadError::Internal)
}

/// Extracts the facts that membership depends on from a manifest.
fn facts(
    manifest: &PackageManifest,
    file: &SourcePath,
    source: &SourceText,
) -> Result<Facts, LoadError> {
    let issue = manifest
        .issues
        .iter()
        .any(|issue| issue.field == ManifestField::Workspaces);
    let workspaces = match &manifest.workspaces {
        Some(field) => {
            let mut patterns = Vec::new();
            for element in &field.value {
                patterns.push(PatternInput {
                    text: element.value.clone(),
                    location: location(file, source, element.span)?,
                });
            }
            Some(Declaration {
                patterns,
                complete: !issue,
            })
        }
        None if issue => Some(Declaration {
            patterns: Vec::new(),
            complete: false,
        }),
        None => None,
    };
    Ok(Facts {
        name: manifest.name.as_ref().map(|field| field.value.clone()),
        version: manifest.version.as_ref().map(|field| field.value.clone()),
        private: manifest.private.as_ref().is_some_and(|field| field.value),
        workspaces,
    })
}

fn location(file: &SourcePath, source: &SourceText, span: Span) -> Result<TextLocation, LoadError> {
    TextLocation::new(file.clone(), source, span).map_err(|_| LoadError::Internal)
}

/// Discovery's directories as the semantics module reads them.
fn tree(inventory: &Inventory) -> Result<Tree, LoadError> {
    let mut tree = Tree::default();
    for entry in inventory.entries() {
        let hidden = match entry.kind() {
            EntryKind::Directory => false,
            EntryKind::Ignored { directory, .. } => {
                if !directory {
                    continue;
                }
                true
            }
            EntryKind::Link(_)
            | EntryKind::ReparsePoint { .. }
            | EntryKind::NotTraversed(_)
            | EntryKind::Uninspectable => true,
            EntryKind::Oversized { .. } | EntryKind::Special => continue,
        };
        let node = Node {
            path: entry.path().clone(),
            segments: entry.path().as_str().split('/').map(Box::from).collect(),
        };
        if hidden {
            tree.hidden.push(node);
        } else {
            tree.dirs.push(node);
        }
    }
    Ok(tree)
}

/// The `pnpm-workspace.yaml` of the root.
struct PnpmWorkspace {
    /// Whether the file exists in any form.
    present: bool,
    /// Its `packages`, when it is analyzed and declares them.
    declaration: Option<Declaration>,
    /// Whether pnpm would reject the file, or it is not analyzed.
    invalid: bool,
}

impl PnpmWorkspace {
    /// Whether the file is present, so pnpm treats the root as a workspace.
    fn declared(&self) -> bool {
        self.present
    }
}

fn read_pnpm_workspace(
    snapshot: &Snapshot,
    limits: LoadLimits,
    report: &mut Reporter<'_>,
) -> Result<PnpmWorkspace, LoadError> {
    let file = path("pnpm-workspace.yaml")?;
    let inventory = snapshot.inventory();
    let absent = PnpmWorkspace {
        present: false,
        declaration: None,
        invalid: false,
    };
    if inventory.files().find(&file).is_none() {
        if inventory.entry(&file).is_some() {
            report.file(&file, FS1031, Severity::Unknown, TEXT_FS1031)?;
            return Ok(PnpmWorkspace {
                present: true,
                declaration: None,
                invalid: true,
            });
        }
        return Ok(absent);
    }
    let not_analyzed = PnpmWorkspace {
        present: true,
        declaration: None,
        invalid: true,
    };
    let Some(source) = decode(snapshot, &file, limits.text.max_source_bytes, report)? else {
        return Ok(not_analyzed);
    };
    let Some(stream) = parse_yaml_file(&source, &file, limits.yaml, report, TEXT_FS1021_PNPM)?
    else {
        return Ok(not_analyzed);
    };
    match pnpm_packages(&stream, &file, &source)? {
        Ok(declaration) => Ok(PnpmWorkspace {
            present: true,
            declaration,
            invalid: false,
        }),
        Err(span) => {
            report.text_with(
                &file,
                &source,
                span,
                FS1035,
                Severity::Review,
                TEXT_FS1035_PNPM,
            )?;
            Ok(not_analyzed)
        }
    }
}

/// Reads `packages` as pnpm validates it: a missing, empty, or null
/// document or field declares no members; anything but a sequence of
/// non-empty strings is rejected.
fn pnpm_packages(
    stream: &YamlStream,
    file: &SourcePath,
    source: &SourceText,
) -> Result<Result<Option<Declaration>, Span>, LoadError> {
    let whole = Span::new(0, source.len()).map_err(|_| LoadError::Internal)?;
    let [document] = stream.documents() else {
        return Ok(if stream.documents().is_empty() {
            Ok(None)
        } else {
            Err(whole)
        });
    };
    let node = |id| stream.node(id).ok_or(LoadError::Internal);
    let root = node(document.root)?;
    if is_null(root) {
        return Ok(Ok(None));
    }
    let YamlValue::Mapping { entries, .. } = &root.value else {
        return Ok(Err(root.span));
    };
    if root
        .tag
        .as_deref()
        .is_some_and(|tag| tag != "tag:yaml.org,2002:map")
        || root.anchor.is_some()
    {
        return Ok(Err(root.span));
    }
    let mut packages = None;
    for (key, value) in entries {
        let key = node(*key)?;
        match &key.value {
            YamlValue::Scalar { text, .. } if key.tag.is_none() => {
                if &**text == "packages" {
                    packages = Some(*value);
                }
            }
            YamlValue::Scalar { .. }
            | YamlValue::Sequence { .. }
            | YamlValue::Mapping { .. }
            | YamlValue::Alias(_) => return Ok(Err(key.span)),
        }
    }
    let Some(packages) = packages else {
        return Ok(Ok(None));
    };
    let packages = node(packages)?;
    if is_null(packages) {
        return Ok(Ok(None));
    }
    let YamlValue::Sequence { elements, .. } = &packages.value else {
        return Ok(Err(packages.span));
    };
    if packages.tag.is_some() || packages.anchor.is_some() {
        return Ok(Err(packages.span));
    }
    let mut patterns = Vec::new();
    for element in elements {
        let element = node(*element)?;
        let text = match &element.value {
            YamlValue::Scalar { text, style }
                if element.tag.is_none() && element.anchor.is_none() && is_string(text, *style) =>
            {
                text.clone()
            }
            YamlValue::Scalar { .. }
            | YamlValue::Sequence { .. }
            | YamlValue::Mapping { .. }
            | YamlValue::Alias(_) => return Ok(Err(element.span)),
        };
        patterns.push(PatternInput {
            text,
            location: location(file, source, element.span)?,
        });
    }
    if patterns.is_empty() {
        return Ok(Ok(None));
    }
    Ok(Ok(Some(Declaration {
        patterns,
        complete: true,
    })))
}

fn is_null(node: &frameshift_syntax::YamlNode) -> bool {
    matches!(
        &node.value,
        YamlValue::Scalar { text, style: YamlScalarStyle::Plain }
            if node.tag.is_none() && matches!(&**text, "" | "~" | "null" | "Null" | "NULL")
    )
}

/// Whether a scalar reads as a non-empty string in the YAML schemas of
/// pnpm's readers; plain scalars that could read as another type are not
/// taken as strings.
fn is_string(text: &str, style: YamlScalarStyle) -> bool {
    if text.is_empty() {
        return false;
    }
    match style {
        YamlScalarStyle::SingleQuoted | YamlScalarStyle::DoubleQuoted => true,
        YamlScalarStyle::Literal | YamlScalarStyle::Folded => false,
        YamlScalarStyle::Plain => {
            // YAML 1.1 and 1.2 keywords, numbers, and timestamps.
            let keyword = matches!(
                text.to_ascii_lowercase().as_str(),
                "~" | "null" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n"
            );
            let date = text.len() >= 8
                && text.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                && text.as_bytes()[4] == b'-';
            !keyword && !date && !crate::lockfile::pnpm::core_number(text)
        }
    }
}

const FS1030: &str = "FS1030";
const FS1031: &str = "FS1031";
const FS1032: &str = "FS1032";
const FS1033: &str = "FS1033";
const FS1034: &str = "FS1034";
const FS1035: &str = "FS1035";
const FS1036: &str = "FS1036";
const FS1037: &str = "FS1037";
const FS1038: &str = "FS1038";

const TEXT_FS1031: FindingText<'static> = FindingText {
    message: "Workspace pattern reaches an entry that discovery did not traverse",
    explanation: "A workspace pattern could select this entry or a directory below it, but \
                  discovery did not traverse it (it is ignored, a link, or not read), while the \
                  package manager may. The workspace members may be incomplete.",
    remediation: "Make the entry an ordinary traversed directory, or narrow the pattern, then \
                  run Frameshift again.",
};
const TEXT_FS1031_INCOMPLETE: FindingText<'static> = FindingText {
    message: "Workspace pattern reaches an entry that discovery did not traverse",
    explanation: "Discovery is incomplete, so workspace patterns may select directories that \
                  are not in the inventory. The workspace members may be incomplete.",
    remediation: "Resolve the discovery findings, then run Frameshift again.",
};
const TEXT_FS1033: FindingText<'static> = FindingText {
    message: "Workspace members depend on case sensitivity",
    explanation: "The workspace patterns select other directories when names are compared \
                  without case, as on the default file systems of Windows and macOS. Frameshift \
                  compares names case-sensitively.",
    remediation: "Spell the patterns with the exact case of the directory names.",
};
const TEXT_FS1034: FindingText<'static> = FindingText {
    message: "Workspace declaration of another package manager ignored",
    explanation: "The detected package manager does not read this workspace declaration \
                  (pnpm reads only pnpm-workspace.yaml; npm, Yarn, and Bun read only the \
                  workspaces field of package.json), so it is not evaluated.",
    remediation: "Remove the stale declaration, or correct the packageManager field.",
};
const TEXT_FS1035_PNPM: FindingText<'static> = FindingText {
    message: "Package manager rejects the workspace",
    explanation: "pnpm requires pnpm-workspace.yaml to be a mapping whose packages field is a \
                  sequence of non-empty strings, and rejects this one, so its members are not \
                  determined.",
    remediation: "Correct pnpm-workspace.yaml, then run Frameshift again.",
};
const TEXT_FS1037: FindingText<'static> = FindingText {
    message: "Workspace semantics of the package manager release not modeled",
    explanation: "packageManager names a release whose workspace pattern semantics Frameshift \
                  does not model, so every modeled release of that package manager is \
                  evaluated, and directories on which they differ are not known.",
    remediation: "Verify the workspace members, or use a modeled release.",
};
const TEXT_FS1021_PNPM: FindingText<'static> = FindingText {
    message: "Duplicate key in a package file",
    explanation: "A YAML mapping repeats a decoded key, which pnpm rejects, so the workspace \
                  declaration is not used.",
    remediation: "Remove the duplicate key.",
};
const TEXT_LIMIT_REMEDIATION: &str = "Raise the limit if the project is trusted, then run \
                                      Frameshift again.";
