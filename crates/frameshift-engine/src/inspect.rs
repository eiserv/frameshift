//! Read-only identification of a project: `frameshift inspect` (`D-075`).
//!
//! [`inspect`] takes a snapshot of the root, loads its workspace, detects
//! Next.js for the root and every member, or for one selected package, and
//! returns the facts with the finalized findings. It reads only what
//! identification needs: directory listings, `.gitignore` files,
//! `package.json` files, the selected lockfile, and `pnpm-workspace.yaml`.
//! It computes no snapshot identity, because that would read every file.
//! [`Inspection::document`] renders the versioned `frameshift.inspection`
//! JSON document; human-readable text is the CLI's concern.

use std::fmt;
use std::path::Path;

use frameshift_core::{
    FindingCollector, FindingError, FindingList, FindingLocation, Position, SourcePath,
};
use frameshift_input::{DiscoveryError, EntryKind, IgnoreReason, Inventory, SnapshotError, Stop};
use frameshift_nextjs::{DetectError, Detection, RouterDirectory, RouterState, VersionEvidence};
use frameshift_package::{
    DeclarationFile, LoadError, LockfileState, ManifestState, SelectionError, Workspace,
    load_workspace,
};

use crate::document::{self, Value};
use crate::limits::{Limits, LimitsError};

/// The `kind` of the inspection document.
pub const DOCUMENT_KIND: &str = "frameshift.inspection";

/// The schema version of the inspection document.
pub const DOCUMENT_VERSION: u64 = 1;

/// Which packages of the workspace to inspect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PackageScope {
    /// The root and every member, in path order.
    #[default]
    All,
    /// The root package only.
    Root,
    /// One member, given by its directory relative to the root.
    Member(SourcePath),
}

/// The options of one inspection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InspectOptions {
    /// The resource limits of the run.
    pub limits: Limits,
    /// The packages to inspect.
    pub packages: PackageScope,
}

/// What discovery accounted for, counted by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSummary {
    /// Whether every entry below the root is accounted for.
    pub complete: bool,
    /// The cumulative limit that stopped discovery, if any.
    pub stop: Option<Stop>,
    /// Admitted regular files.
    pub admitted: usize,
    /// The total size of the admitted files in bytes.
    pub admitted_bytes: u64,
    /// Traversed directories, excluding the root.
    pub directories: usize,
    /// Ignored entries per reason, in [`IGNORE_REASONS`] order.
    pub ignored: [usize; 5],
    /// Files over the per-file size limit.
    pub oversized: usize,
    /// Links, which are never followed.
    pub links: usize,
    /// Windows reparse points that are not links.
    pub reparse_points: usize,
    /// Device files, FIFOs, and sockets.
    pub special: usize,
    /// Directories whose entries were not discovered.
    pub not_traversed: usize,
    /// Entries whose metadata could not be read.
    pub uninspectable: usize,
    /// Entries whose names cannot be represented as paths.
    pub unrepresentable: usize,
}

/// The ignore reasons in the order of [`FileSummary::ignored`].
pub const IGNORE_REASONS: [IgnoreReason; 5] = [
    IgnoreReason::Vcs,
    IgnoreReason::Dependencies,
    IgnoreReason::BuildOutput,
    IgnoreReason::Environment,
    IgnoreReason::Gitignore,
];

impl FileSummary {
    fn new(inventory: &Inventory) -> Self {
        let files = inventory.files();
        let mut summary = Self {
            complete: inventory.is_complete(),
            stop: inventory.stop(),
            admitted: files.len(),
            admitted_bytes: files
                .iter()
                .filter_map(|(id, _)| inventory.file_size(id))
                .sum(),
            directories: 0,
            ignored: [0; 5],
            oversized: 0,
            links: 0,
            reparse_points: 0,
            special: 0,
            not_traversed: 0,
            uninspectable: 0,
            unrepresentable: inventory.unrepresentable(),
        };
        for entry in inventory.entries() {
            match entry.kind() {
                EntryKind::Directory => summary.directories += 1,
                EntryKind::Ignored { reason, .. } => {
                    let index = match reason {
                        IgnoreReason::Vcs => 0,
                        IgnoreReason::Dependencies => 1,
                        IgnoreReason::BuildOutput => 2,
                        IgnoreReason::Environment => 3,
                        IgnoreReason::Gitignore => 4,
                    };
                    summary.ignored[index] += 1;
                }
                EntryKind::Oversized { .. } => summary.oversized += 1,
                EntryKind::Link(_) => summary.links += 1,
                EntryKind::ReparsePoint { .. } => summary.reparse_points += 1,
                EntryKind::Special => summary.special += 1,
                EntryKind::NotTraversed(_) => summary.not_traversed += 1,
                EntryKind::Uninspectable => summary.uninspectable += 1,
            }
        }
        summary
    }

    /// All ignored entries.
    pub fn ignored_total(&self) -> usize {
        self.ignored.iter().sum()
    }

    /// Accounted entries that are neither admitted files, directories, nor
    /// ignored: oversized files, links, reparse points, special files,
    /// untraversed directories, and uninspectable entries.
    pub fn other_total(&self) -> usize {
        self.oversized
            + self.links
            + self.reparse_points
            + self.special
            + self.not_traversed
            + self.uninspectable
    }
}

/// Whether a package's `package.json` was analyzed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestStatus {
    /// No admitted `package.json`.
    Absent,
    /// The file exists but is not analyzed; a finding says why.
    NotAnalyzed,
    /// The manifest was interpreted.
    Analyzed,
}

impl ManifestStatus {
    fn new(state: &ManifestState) -> Self {
        match state {
            ManifestState::Absent => Self::Absent,
            ManifestState::NotAnalyzed => Self::NotAnalyzed,
            ManifestState::Analyzed(_) => Self::Analyzed,
        }
    }

    /// Returns the stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::NotAnalyzed => "not-analyzed",
            Self::Analyzed => "analyzed",
        }
    }
}

/// The identification of one package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageInspection {
    /// The package directory, `None` for the root.
    pub directory: Option<SourcePath>,
    /// The `name` of its analyzed `package.json`. Escape it before rendering.
    pub name: Option<Box<str>>,
    /// Whether its `package.json` was analyzed.
    pub manifest: ManifestStatus,
    /// Next.js detection (`D-074`).
    pub nextjs: Detection,
}

/// The result of one inspection.
#[derive(Debug)]
pub struct Inspection {
    /// What discovery accounted for.
    pub files: FileSummary,
    /// The workspace at the root, with the root's package manager and
    /// lockfile.
    pub workspace: Workspace,
    /// The inspected packages: the root first, then members in path order.
    pub packages: Vec<PackageInspection>,
    /// Every finding of the run, in canonical order.
    pub findings: FindingList,
}

/// A failure that ends an inspection without results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectError {
    /// The limits violate a relation.
    Limits(LimitsError),
    /// Discovery failed, for example because the root cannot be read.
    Discovery(DiscoveryError),
    /// The snapshot store failed, for example because a file changed.
    Snapshot(SnapshotError),
    /// The package model failed.
    Package(LoadError),
    /// Next.js detection failed.
    Detection(DetectError),
    /// The selected directory is not the root or a known member.
    Selection(SelectionError),
}

impl InspectError {
    /// Whether the failure is an engine defect rather than a property of the
    /// input: a violated invariant, invalid finding prose, or invalid limits.
    /// Input failures, such as an unreadable root, a changed file, a full
    /// finding collector, or exhausted storage, return `false`.
    pub const fn is_internal(&self) -> bool {
        match self {
            Self::Limits(_) => true,
            Self::Discovery(error) => match error {
                DiscoveryError::Internal => true,
                DiscoveryError::Findings(error) => internal_finding(*error),
                DiscoveryError::Root(_)
                | DiscoveryError::RootNotDirectory
                | DiscoveryError::SourceChanged
                | DiscoveryError::AllocationFailed => false,
            },
            Self::Snapshot(error) => internal_snapshot(*error),
            Self::Package(error) => match error {
                LoadError::Snapshot(error) => internal_snapshot(*error),
                LoadError::Findings(error) => internal_finding(*error),
                LoadError::Internal => true,
                LoadError::AllocationFailed => false,
            },
            Self::Detection(error) => match error {
                DetectError::Findings(error) => internal_finding(*error),
                DetectError::Internal => true,
            },
            Self::Selection(_) => false,
        }
    }
}

const fn internal_finding(error: FindingError) -> bool {
    match error {
        FindingError::TextTooLong | FindingError::InvalidText => true,
        FindingError::TooManyFindings | FindingError::AllocationFailed => false,
    }
}

const fn internal_snapshot(error: SnapshotError) -> bool {
    match error {
        SnapshotError::UnknownFile | SnapshotError::Hash(_) | SnapshotError::Internal => true,
        SnapshotError::Findings(error) => internal_finding(error),
        SnapshotError::SourceChanged | SnapshotError::AllocationFailed => false,
    }
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limits(error) => write!(f, "invalid limits: {error}"),
            Self::Discovery(error) => error.fmt(f),
            Self::Snapshot(error) => error.fmt(f),
            Self::Package(error) => error.fmt(f),
            Self::Detection(error) => error.fmt(f),
            Self::Selection(error) => {
                write!(f, "the selected package cannot be inspected: {error}")
            }
        }
    }
}

impl std::error::Error for InspectError {}

impl From<DiscoveryError> for InspectError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<SnapshotError> for InspectError {
    fn from(error: SnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<LoadError> for InspectError {
    fn from(error: LoadError) -> Self {
        Self::Package(error)
    }
}

impl From<DetectError> for InspectError {
    fn from(error: DetectError) -> Self {
        Self::Detection(error)
    }
}

/// Identifies the project at `root` read-only.
///
/// The root is the workspace root; nothing above it is read (`D-073`).
/// Findings never stop the inspection: a project without a supported
/// framework, an incomplete inventory, or unusable package files are
/// results. Only the failures of [`InspectError`] end it without results.
pub fn inspect(root: &Path, options: &InspectOptions) -> Result<Inspection, InspectError> {
    let limits = options.limits;
    limits.validate().map_err(InspectError::Limits)?;
    let mut findings = FindingCollector::new(limits.finding);
    let snapshot = frameshift_input::snapshot(root, limits.snapshot(), &mut findings)?;
    let workspace = load_workspace(&snapshot, limits.package(), &mut findings)?;
    let directories: Vec<Option<&SourcePath>> = match &options.packages {
        PackageScope::All => std::iter::once(None)
            .chain(
                workspace
                    .members
                    .iter()
                    .map(|member| Some(&member.directory)),
            )
            .collect(),
        PackageScope::Root => vec![None],
        PackageScope::Member(directory) => vec![Some(directory)],
    };
    let mut packages = Vec::with_capacity(directories.len());
    for directory in directories {
        let selected = workspace
            .select(directory)
            .map_err(InspectError::Selection)?;
        let nextjs = frameshift_nextjs::detect(snapshot.inventory(), &selected, &mut findings)?;
        let manifest = selected.manifest();
        packages.push(PackageInspection {
            directory: directory.cloned(),
            name: match manifest {
                ManifestState::Analyzed(manifest) => {
                    manifest.name.as_ref().map(|name| name.value.clone())
                }
                ManifestState::Absent | ManifestState::NotAnalyzed => None,
            },
            manifest: ManifestStatus::new(manifest),
            nextjs,
        });
    }
    snapshot.report_unreadable(&mut findings)?;
    Ok(Inspection {
        files: FileSummary::new(snapshot.inventory()),
        workspace,
        packages,
        findings: findings.finish(),
    })
}

impl Inspection {
    /// Renders the `frameshift.inspection` document, version 1: UTF-8 JSON
    /// with two-space indentation, LF line ends, and a final LF. Equal
    /// inspections render equal bytes; the document has no `run` section.
    /// The schema is `schemas/frameshift.inspection.schema.json`.
    pub fn document(&self) -> String {
        document::print(&Value::Object(vec![
            ("kind", Value::string(DOCUMENT_KIND)),
            ("schemaVersion", Value::Number(DOCUMENT_VERSION)),
            (
                "engineVersion",
                Value::string(crate::version::info().engine_version),
            ),
            ("files", files(&self.files)),
            ("packageManager", package_manager(&self.workspace)),
            ("workspace", workspace(&self.workspace)),
            (
                "packages",
                Value::Array(self.packages.iter().map(package).collect()),
            ),
            ("migrationPaths", Value::Array(Vec::new())),
            ("findings", findings(&self.findings)),
        ]))
    }
}

fn path(directory: Option<&SourcePath>) -> Value {
    Value::string(directory.map_or(".", SourcePath::as_str))
}

fn files(summary: &FileSummary) -> Value {
    Value::Object(vec![
        ("complete", Value::Bool(summary.complete)),
        (
            "stop",
            Value::optional(summary.stop, |stop| Value::string(stop.limit_name())),
        ),
        ("admitted", Value::count(summary.admitted)),
        ("admittedBytes", Value::Number(summary.admitted_bytes)),
        ("directories", Value::count(summary.directories)),
        (
            "ignored",
            Value::Object(
                IGNORE_REASONS
                    .iter()
                    .zip(summary.ignored)
                    .map(|(reason, count)| (reason.as_str(), Value::count(count)))
                    .collect(),
            ),
        ),
        ("oversized", Value::count(summary.oversized)),
        ("links", Value::count(summary.links)),
        ("reparsePoints", Value::count(summary.reparse_points)),
        ("special", Value::count(summary.special)),
        ("notTraversed", Value::count(summary.not_traversed)),
        ("uninspectable", Value::count(summary.uninspectable)),
        ("unrepresentable", Value::count(summary.unrepresentable)),
    ])
}

fn package_manager(workspace: &Workspace) -> Value {
    let root = &workspace.root;
    let detection = &root.detection;
    let lockfile = detection.lockfile.map(|kind| {
        let (status, version) = match &root.lockfile {
            LockfileState::Parsed(lockfile) => ("parsed", Value::Number(lockfile.version())),
            LockfileState::NotParsed(_) | LockfileState::None => ("not-parsed", Value::Null),
        };
        Value::Object(vec![
            ("path", Value::string(kind.file_name())),
            ("status", Value::string(status)),
            ("formatVersion", version),
        ])
    });
    Value::Object(vec![
        (
            "name",
            Value::optional(detection.manager, |kind| Value::string(kind.name())),
        ),
        (
            "source",
            Value::optional(detection.source, |source| Value::string(source.as_str())),
        ),
        (
            "declared",
            Value::optional(root.package_manager.as_ref(), |spec| {
                Value::Object(vec![
                    ("name", Value::string(spec.kind.name())),
                    ("version", Value::String(spec.version.to_string())),
                ])
            }),
        ),
        ("lockfile", lockfile.unwrap_or(Value::Null)),
        (
            "ignoredLockfiles",
            Value::Array(
                detection
                    .ignored
                    .iter()
                    .map(|ignored| {
                        Value::Object(vec![
                            ("path", Value::string(ignored.kind.file_name())),
                            ("reason", Value::string(ignored.reason.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// The spelling of a declaration file in reports.
pub const fn declaration_path(declaration: DeclarationFile) -> &'static str {
    match declaration {
        DeclarationFile::PackageJson => "package.json",
        DeclarationFile::PnpmWorkspace => "pnpm-workspace.yaml",
    }
}

fn workspace(workspace: &Workspace) -> Value {
    let paths = |paths: &mut dyn Iterator<Item = &SourcePath>| {
        Value::Array(paths.map(|path| Value::string(path.as_str())).collect())
    };
    Value::Object(vec![
        ("isWorkspace", Value::Bool(workspace.is_workspace())),
        (
            "declarations",
            Value::Array(
                workspace
                    .declarations
                    .iter()
                    .map(|declaration| Value::string(declaration_path(*declaration)))
                    .collect(),
            ),
        ),
        (
            "releases",
            Value::Array(
                workspace
                    .semantics
                    .iter()
                    .map(|release| Value::string(release.as_str()))
                    .collect(),
            ),
        ),
        ("complete", Value::Bool(workspace.complete)),
        (
            "members",
            paths(&mut workspace.members.iter().map(|member| &member.directory)),
        ),
        ("undetermined", paths(&mut workspace.undetermined.iter())),
    ])
}

fn package(package: &PackageInspection) -> Value {
    let detection = &package.nextjs;
    let version = match &detection.version {
        VersionEvidence::Locked(version) => Some((version, "locked")),
        VersionEvidence::LowerBound(version) => Some((version, "lower-bound")),
        VersionEvidence::Unknown => None,
    };
    Value::Object(vec![
        ("directory", path(package.directory.as_ref())),
        (
            "name",
            Value::optional(package.name.as_deref(), Value::string),
        ),
        ("manifest", Value::string(package.manifest.as_str())),
        (
            "nextjs",
            Value::Object(vec![
                ("verdict", Value::string(detection.verdict.as_str())),
                (
                    "dependency",
                    Value::optional(detection.dependency.as_ref(), |dependency| {
                        Value::Object(vec![
                            ("field", Value::string(dependency.kind.key())),
                            ("specifier", Value::string(&dependency.spec)),
                        ])
                    }),
                ),
                (
                    "version",
                    Value::optional(version, |(version, evidence)| {
                        Value::Object(vec![
                            ("value", Value::String(version.to_string())),
                            ("evidence", Value::string(evidence)),
                        ])
                    }),
                ),
                (
                    "config",
                    Value::optional(detection.config.as_ref(), |config| {
                        Value::Object(vec![
                            ("path", Value::string(config.path.as_str())),
                            ("load", Value::string(config.load.as_str())),
                        ])
                    }),
                ),
                ("router", Value::string(detection.router().as_str())),
                ("app", router_directory(detection.app.as_ref())),
                ("pages", router_directory(detection.pages.as_ref())),
            ]),
        ),
    ])
}

fn router_directory(directory: Option<&RouterDirectory>) -> Value {
    Value::optional(directory, |directory| {
        let (state, complete) = match directory.state {
            RouterState::Directory { complete, .. } => ("directory", Value::Bool(complete)),
            RouterState::Other => ("other", Value::Null),
        };
        Value::Object(vec![
            ("path", Value::string(directory.path.as_str())),
            ("inSrc", Value::Bool(directory.in_src)),
            ("state", Value::string(state)),
            ("routeFiles", Value::count(directory.routes())),
            ("complete", complete),
        ])
    })
}

fn position(position: Position) -> Value {
    Value::Object(vec![
        ("offset", Value::count(position.offset)),
        ("line", Value::Number(u64::from(position.line))),
        ("column", Value::Number(u64::from(position.column))),
    ])
}

fn findings(list: &FindingList) -> Value {
    Value::Array(
        list.iter()
            .map(|(id, finding)| {
                let location = match finding.location() {
                    FindingLocation::Project => {
                        Value::Object(vec![("scope", Value::string("project"))])
                    }
                    FindingLocation::File(path) => Value::Object(vec![
                        ("scope", Value::string("file")),
                        ("path", Value::string(path.as_str())),
                    ]),
                    FindingLocation::Text(text) => Value::Object(vec![
                        ("scope", Value::string("text")),
                        ("path", Value::string(text.file().as_str())),
                        ("start", position(text.start())),
                        ("end", position(text.end())),
                    ]),
                };
                let mut members = vec![
                    ("id", Value::String(id.to_string())),
                    ("code", Value::string(finding.code().as_str())),
                    ("severity", Value::string(finding.severity().as_str())),
                    ("location", location),
                ];
                let context = finding.context();
                if let Some(unit) = &context.unit {
                    members.push(("unit", Value::string(unit.as_str())));
                }
                if let Some(feature) = &context.feature {
                    members.push(("feature", Value::string(feature.as_str())));
                }
                if let Some(rule) = &context.rule {
                    members.push(("rule", Value::string(rule.as_str())));
                }
                members.extend([
                    ("message", Value::string(finding.message())),
                    ("explanation", Value::string(finding.explanation())),
                    ("remediation", Value::string(finding.remediation())),
                ]);
                Value::Object(members)
            })
            .collect(),
    )
}
