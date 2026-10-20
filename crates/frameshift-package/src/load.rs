//! Reading one package directory through the snapshot store (`D-069`).
//!
//! The loader reads `package.json` and the lockfile that detection selects,
//! at most once each through [`Snapshot::read`], and turns every problem
//! into a finding (MIGRATION_REPORTING.md section 8.2). Files that are not
//! admitted by discovery, such as ignored or oversized ones, are absent.

use std::fmt;

use frameshift_core::{
    Finding, FindingCode, FindingCollector, FindingContext, FindingError, FindingLimits,
    FindingLocation, FindingText, PathLimits, Severity, SourcePath, SourceText, Span, TextError,
    TextLimits, TextLocation,
};
use frameshift_input::{FileRead, Snapshot, SnapshotError};
use frameshift_syntax::{
    JsonDialect, JsonDocument, JsonLimits, YamlLimits, YamlStream, YamlValue, YarnDocument,
    YarnErrorKind, YarnLimits, parse_json, parse_yaml, parse_yarn_lockfile,
};

use crate::lockfile::{Lockfile, LockfileErrorKind, has_classic_header};
use crate::manager::{
    Detection, IgnoredReason, LockfileKind, PackageManagerKind, PackageManagerSpec, detect,
};
use crate::manifest::{IssueKind, ManifestError, PackageManifest};

/// The package model's own limit (`D-069`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageLimits {
    /// The largest lockfile that is decoded and parsed, default 64 MiB.
    /// Lockfiles use this bound instead of `text.max-source-bytes`.
    pub max_lockfile_bytes: u64,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_lockfile_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Every limit that loading a package directory applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadLimits {
    /// The decoded size of `package.json`.
    pub text: TextLimits,
    /// Each JSON and JSONC parse.
    pub json: JsonLimits,
    /// Each YAML stream.
    pub yaml: YamlLimits,
    /// Each Yarn classic lockfile.
    pub yarn: YarnLimits,
    /// The paths of the files in the directory.
    pub path: PathLimits,
    /// The lockfile size.
    pub package: PackageLimits,
}

/// The state of a directory's `package.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestState {
    /// No admitted `package.json`.
    Absent,
    /// The file exists but is not analyzed; a finding says why.
    NotAnalyzed,
    /// The interpreted manifest.
    Analyzed(Box<PackageManifest>),
}

/// The state of the lockfile that detection selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockfileState {
    /// No lockfile is selected.
    None,
    /// The lockfile is not read or not usable; a finding says why.
    /// `bun.lockb` and unsupported formats or format versions are here.
    NotParsed(LockfileKind),
    /// The parsed lockfile.
    Parsed(Box<Lockfile>),
}

/// What one package directory declares and locks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageDirectory {
    /// The directory, `None` for the project root.
    pub directory: Option<SourcePath>,
    /// `package.json`.
    pub manifest: ManifestState,
    /// The valid `packageManager` field, if any.
    pub package_manager: Option<PackageManagerSpec>,
    /// The package manager and the lockfile selection.
    pub detection: Detection,
    /// The selected lockfile.
    pub lockfile: LockfileState,
}

/// A failure that ends the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The snapshot store failed, for example because a file changed.
    Snapshot(SnapshotError),
    /// The finding collector rejected a finding.
    Findings(FindingError),
    /// An internal invariant failed.
    Internal,
    /// Storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => write!(f, "package files could not be read: {error}"),
            Self::Findings(error) => write!(f, "package findings could not be stored: {error}"),
            Self::Internal => f.write_str("internal package model invariant failed"),
            Self::AllocationFailed => f.write_str("package model allocation failed"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Reads and interprets the package files of one directory of a snapshot:
/// `package.json`, the `packageManager` field, the lockfiles present, and the
/// selected lockfile if it is an npm, Bun text, pnpm, or Yarn lockfile.
pub fn load_package(
    snapshot: &Snapshot,
    directory: Option<&SourcePath>,
    limits: LoadLimits,
    findings: &mut FindingCollector,
) -> Result<PackageDirectory, LoadError> {
    let mut report = Reporter { findings };
    let manifest_path = child(directory, "package.json", limits.path);
    let (manifest, package_manager) = match &manifest_path {
        Some(path) => read_manifest(snapshot, path, limits, &mut report)?,
        None => (ManifestState::Absent, None),
    };
    let mut present = Vec::new();
    let mut paths = Vec::new();
    for kind in LockfileKind::ALL {
        if let Some(path) = child(directory, kind.file_name(), limits.path)
            && snapshot.inventory().files().find(&path).is_some()
        {
            present.push(kind);
            paths.push((kind, path));
        }
    }
    let path_of = |kind: LockfileKind| {
        paths
            .iter()
            .find(|(candidate, _)| *candidate == kind)
            .map(|(_, path)| path.clone())
            .ok_or(LoadError::Internal)
    };
    let detection = detect(package_manager.as_ref(), &present);
    for ignored in &detection.ignored {
        let path = path_of(ignored.kind)?;
        match ignored.reason {
            IgnoredReason::OtherManager => {
                report.file(&path, FS1023, Severity::Review, TEXT_FS1023)?;
            }
            IgnoredReason::Superseded => report.file(&path, FS1025, Severity::Info, TEXT_FS1025)?,
            IgnoredReason::Ambiguous => {}
        }
    }
    if detection
        .ignored
        .iter()
        .any(|ignored| ignored.reason == IgnoredReason::Ambiguous)
    {
        let location = directory.map_or(FindingLocation::Project, |path| {
            FindingLocation::File(path.clone())
        });
        report.push(location, FS1024, Severity::Unknown, TEXT_FS1024)?;
    }
    let lockfile = match detection.lockfile {
        None => LockfileState::None,
        Some(kind) => {
            let path = path_of(kind)?;
            let lockfile = read_lockfile(snapshot, kind, &path, &manifest, limits, &mut report)?;
            if let (LockfileState::Parsed(parsed), Some(spec)) = (&lockfile, &package_manager) {
                yarn_generation(parsed, spec, &path, &mut report)?;
            }
            lockfile
        }
    };
    Ok(PackageDirectory {
        directory: directory.cloned(),
        manifest,
        package_manager,
        detection,
        lockfile,
    })
}

fn child(directory: Option<&SourcePath>, name: &str, limits: PathLimits) -> Option<SourcePath> {
    let text = match directory {
        Some(directory) => format!("{}/{name}", directory.as_str()),
        None => name.to_owned(),
    };
    SourcePath::new(&text, limits).ok()
}

/// Decoded file content, or `None` when a finding already explains why
/// there is none.
fn decode(
    snapshot: &Snapshot,
    path: &SourcePath,
    max_bytes: usize,
    report: &mut Reporter<'_>,
) -> Result<Option<SourceText>, LoadError> {
    let id = snapshot
        .inventory()
        .files()
        .find(path)
        .ok_or(LoadError::Internal)?;
    let file = match snapshot.read(id).map_err(LoadError::Snapshot)? {
        FileRead::Content(file) => file,
        // The snapshot reports unreadable files itself (FS1019).
        FileRead::Unreadable(_) => return Ok(None),
    };
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(file.bytes().len())
        .map_err(|_| LoadError::AllocationFailed)?;
    bytes.extend_from_slice(file.bytes());
    let limits = TextLimits {
        max_source_bytes: max_bytes,
    };
    match SourceText::from_bytes(bytes, limits) {
        Ok(source) => Ok(Some(source)),
        Err(TextError::InvalidUtf8 { .. }) => {
            report.file(path, FS2002, Severity::Unknown, TEXT_FS2002)?;
            Ok(None)
        }
        Err(TextError::SourceTooLarge { .. }) => {
            let explanation = format!(
                "The file is larger than the text limit of {max_bytes} bytes, so it is not \
                 decoded or analyzed."
            );
            report.file(
                path,
                FS2003,
                Severity::Unknown,
                FindingText {
                    message: "File exceeds the text size limit",
                    explanation: &explanation,
                    remediation: TEXT_FS2003_REMEDIATION,
                },
            )?;
            Ok(None)
        }
        Err(TextError::AllocationFailed) => Err(LoadError::AllocationFailed),
        Err(
            TextError::ReversedSpan { .. }
            | TextError::OutOfBounds { .. }
            | TextError::NotCharBoundary { .. },
        ) => Err(LoadError::Internal),
    }
}

/// Parses decoded JSON, reporting a syntax error and every duplicate key.
fn parse<'s>(
    source: &'s SourceText,
    path: &SourcePath,
    dialect: JsonDialect,
    limits: JsonLimits,
    report: &mut Reporter<'_>,
) -> Result<Option<JsonDocument<'s>>, LoadError> {
    let document = match parse_json(source, dialect, limits) {
        Ok(document) => document,
        Err(error) => {
            let explanation = format!(
                "The file is not valid {} ({error}), so it is not analyzed.",
                match dialect {
                    JsonDialect::Json => "JSON",
                    JsonDialect::Jsonc => "JSONC",
                }
            );
            report.text_with(
                path,
                source,
                error.span(),
                FS2001,
                Severity::Unknown,
                FindingText {
                    message: "Syntax error, file not analyzed",
                    explanation: &explanation,
                    remediation: "Fix the syntax error, then run Frameshift again.",
                },
            )?;
            return Ok(None);
        }
    };
    for member in document.duplicate_keys() {
        report.text_with(
            path,
            source,
            member.key_span(),
            FS1021,
            Severity::Review,
            TEXT_FS1021,
        )?;
    }
    Ok(Some(document))
}

/// Reads `package.json` and its valid `packageManager` field.
fn read_manifest(
    snapshot: &Snapshot,
    path: &SourcePath,
    limits: LoadLimits,
    report: &mut Reporter<'_>,
) -> Result<(ManifestState, Option<PackageManagerSpec>), LoadError> {
    if snapshot.inventory().files().find(path).is_none() {
        return Ok((ManifestState::Absent, None));
    }
    let Some(source) = decode(snapshot, path, limits.text.max_source_bytes, report)? else {
        return Ok((ManifestState::NotAnalyzed, None));
    };
    let Some(document) = parse(&source, path, JsonDialect::Json, limits.json, report)? else {
        return Ok((ManifestState::NotAnalyzed, None));
    };
    let manifest = match PackageManifest::from_json(&document) {
        Ok(manifest) => manifest,
        Err(ManifestError::NotAnObject { span }) => {
            report.text_with(
                path,
                &source,
                span,
                FS1020,
                Severity::Unknown,
                TEXT_NOT_OBJECT,
            )?;
            return Ok((ManifestState::NotAnalyzed, None));
        }
    };
    for issue in &manifest.issues {
        let explanation = format!(
            "The `{}` field of package.json {}, so this value is not used.",
            issue.field,
            match issue.kind {
                IssueKind::UnexpectedType => "has a value of an unexpected JSON type",
                IssueKind::InvalidValue => "has a value that is not valid for the field",
            }
        );
        report.text_with(
            path,
            &source,
            issue.span,
            FS1020,
            Severity::Unknown,
            FindingText {
                message: "package.json field has an unexpected type or value",
                explanation: &explanation,
                remediation: "Correct the value, or accept that Frameshift treats it as unknown.",
            },
        )?;
    }
    let mut package_manager = None;
    if let Some(field) = &manifest.package_manager {
        match PackageManagerSpec::parse(&field.value) {
            Ok(spec) => package_manager = Some(spec),
            Err(_) => {
                report.text_with(
                    path,
                    &source,
                    field.span,
                    FS1022,
                    Severity::Unknown,
                    TEXT_FS1022,
                )?;
            }
        }
    }
    Ok((ManifestState::Analyzed(Box::new(manifest)), package_manager))
}

fn read_lockfile(
    snapshot: &Snapshot,
    kind: LockfileKind,
    path: &SourcePath,
    manifest: &ManifestState,
    limits: LoadLimits,
    report: &mut Reporter<'_>,
) -> Result<LockfileState, LoadError> {
    let dialect = match kind {
        LockfileKind::NpmShrinkwrap | LockfileKind::NpmPackageLock => Some(JsonDialect::Json),
        LockfileKind::BunText => Some(JsonDialect::Jsonc),
        LockfileKind::Pnpm | LockfileKind::Yarn => None,
        LockfileKind::BunBinary => {
            report.file(path, FS1010, Severity::Unknown, TEXT_FS1010)?;
            return Ok(LockfileState::NotParsed(kind));
        }
    };
    let id = snapshot
        .inventory()
        .files()
        .find(path)
        .ok_or(LoadError::Internal)?;
    let size = snapshot
        .inventory()
        .file_size(id)
        .ok_or(LoadError::Internal)?;
    let max = limits.package.max_lockfile_bytes;
    if size > max {
        let explanation = format!(
            "The lockfile has {size} bytes, more than package.max-lockfile-bytes ({max}), so it \
             is not parsed. Versions fall back to the declared ranges."
        );
        report.file(
            path,
            FS1028,
            Severity::Unknown,
            FindingText {
                message: "Lockfile exceeds the lockfile size limit",
                explanation: &explanation,
                remediation: "Raise the limit if the lockfile is trusted, then run Frameshift again.",
            },
        )?;
        return Ok(LockfileState::NotParsed(kind));
    }
    let max = usize::try_from(max).unwrap_or(usize::MAX);
    let Some(source) = decode(snapshot, path, max, report)? else {
        return Ok(LockfileState::NotParsed(kind));
    };
    let parsed = if let Some(dialect) = dialect {
        let Some(document) = parse(&source, path, dialect, limits.json, report)? else {
            return Ok(LockfileState::NotParsed(kind));
        };
        Lockfile::from_json(kind, &document)
    } else if kind == LockfileKind::Yarn && has_classic_header(source.as_str()) {
        let Some(document) = parse_classic(&source, path, limits.yarn, report)? else {
            return Ok(LockfileState::NotParsed(kind));
        };
        Lockfile::from_yarn_classic(&document)
    } else {
        let Some(stream) = parse_yaml_lockfile(&source, path, limits.yaml, report)? else {
            return Ok(LockfileState::NotParsed(kind));
        };
        if kind == LockfileKind::Yarn {
            Lockfile::from_yarn_berry(&stream)
        } else {
            Lockfile::from_yaml(&stream)
        }
    };
    let lockfile = match parsed {
        Ok(lockfile) => lockfile,
        Err(error) => {
            let (code, text) = match error.kind {
                LockfileErrorKind::UnsupportedVersion => (FS1010, TEXT_FS1010),
                LockfileErrorKind::Structure => (FS1026, TEXT_FS1026),
            };
            report.text_with(path, &source, error.span, code, Severity::Unknown, text)?;
            return Ok(LockfileState::NotParsed(kind));
        }
    };
    if let ManifestState::Analyzed(manifest) = manifest {
        let stale = lockfile.out_of_sync(manifest);
        if !stale.is_empty() {
            let kinds: Vec<&str> = stale.iter().map(|kind| kind.key()).collect();
            let explanation = if kind == LockfileKind::Yarn && lockfile.version() == 1 {
                format!(
                    "The Yarn classic lockfile has no entry for declarations of package.json in \
                     {}, so resolved versions may not match what an install would produce. Yarn \
                     1 records no entry for a declaration that a workspace package satisfies.",
                    kinds.join(", ")
                )
            } else {
                format!(
                    "The lockfile's root declarations differ from package.json in {}, so \
                     resolved versions may not match what an install would produce.",
                    kinds.join(", ")
                )
            };
            report.file(
                path,
                FS1027,
                Severity::Review,
                FindingText {
                    message: "Lockfile is out of sync with package.json",
                    explanation: &explanation,
                    remediation: "Run an install with the package manager to update the lockfile, \
                                  then run Frameshift again.",
                },
            )?;
        }
    }
    Ok(LockfileState::Parsed(Box::new(lockfile)))
}

/// Parses a Yarn classic lockfile, reporting a syntax error or a newer
/// format version and every duplicate key.
fn parse_classic<'s>(
    source: &'s SourceText,
    path: &SourcePath,
    limits: YarnLimits,
    report: &mut Reporter<'_>,
) -> Result<Option<YarnDocument<'s>>, LoadError> {
    let document = match parse_yarn_lockfile(source, limits) {
        Ok(document) => document,
        Err(error) if error.kind() == YarnErrorKind::UnsupportedVersion => {
            report.text_with(
                path,
                source,
                error.span(),
                FS1010,
                Severity::Unknown,
                TEXT_FS1010,
            )?;
            return Ok(None);
        }
        Err(error) => {
            let explanation = format!(
                "The file is not a Yarn classic lockfile that Yarn 1 reads as written ({error}), \
                 so it is not analyzed."
            );
            report.text_with(
                path,
                source,
                error.span(),
                FS2001,
                Severity::Unknown,
                FindingText {
                    message: "Syntax error, file not analyzed",
                    explanation: &explanation,
                    remediation: "Fix the syntax error or resolve merge conflicts, for example \
                                  by running an install, or raise the reported limit, then run \
                                  Frameshift again.",
                },
            )?;
            return Ok(None);
        }
    };
    for key in document.duplicate_keys() {
        report.text_with(
            path,
            source,
            key.span(),
            FS1021,
            Severity::Review,
            TEXT_FS1021_CLASSIC,
        )?;
    }
    Ok(Some(document))
}

/// Parses a YAML lockfile, reporting a syntax error and every duplicate
/// key of a mapping.
fn parse_yaml_lockfile(
    source: &SourceText,
    path: &SourcePath,
    limits: YamlLimits,
    report: &mut Reporter<'_>,
) -> Result<Option<YamlStream>, LoadError> {
    let stream = match parse_yaml(source, limits) {
        Ok(stream) => stream,
        Err(error) => {
            let explanation =
                format!("The file is not usable YAML ({error}), so it is not analyzed.");
            report.text_with(
                path,
                source,
                error.span,
                FS2001,
                Severity::Unknown,
                FindingText {
                    message: "Syntax error, file not analyzed",
                    explanation: &explanation,
                    remediation: "Fix the syntax error or raise the reported YAML limit, then \
                                  run Frameshift again.",
                },
            )?;
            return Ok(None);
        }
    };
    for node in stream.nodes() {
        if let YamlValue::Mapping { entries, .. } = &node.value {
            let mut seen = std::collections::BTreeMap::new();
            for (key, _) in entries {
                let key_node = stream.node(*key).ok_or(LoadError::Internal)?;
                let target = match key_node.value {
                    YamlValue::Alias(target) => target,
                    YamlValue::Scalar { .. }
                    | YamlValue::Sequence { .. }
                    | YamlValue::Mapping { .. } => *key,
                };
                if let Some(text) = stream.scalar(target)
                    && let Some(previous) = seen.insert(text, key_node.span)
                {
                    report.text_with(
                        path,
                        source,
                        previous,
                        FS1021,
                        Severity::Review,
                        TEXT_FS1021_YAML,
                    )?;
                }
            }
        }
    }
    Ok(Some(stream))
}

/// Reports a Yarn lockfile whose format belongs to another Yarn generation
/// than the one `packageManager` names (FS1029).
fn yarn_generation(
    lockfile: &Lockfile,
    spec: &PackageManagerSpec,
    path: &SourcePath,
    report: &mut Reporter<'_>,
) -> Result<(), LoadError> {
    if lockfile.kind() != LockfileKind::Yarn || spec.kind != PackageManagerKind::Yarn {
        return Ok(());
    }
    let classic = lockfile.version() == 1;
    let explanation = match (spec.version.major() == 1, classic) {
        (true, false) => {
            "packageManager names Yarn 1, which ignores a Yarn Berry lockfile, so the next \
             install resolves every version anew."
        }
        (false, true) => {
            "packageManager names Yarn 2 or later, which migrates a Yarn classic lockfile on \
             the next install, so locked versions may change."
        }
        (true, true) | (false, false) => return Ok(()),
    };
    report.file(
        path,
        FS1029,
        Severity::Review,
        FindingText {
            message: "Lockfile format does not match the Yarn version",
            explanation,
            remediation: "Run an install with the Yarn version that packageManager names, then \
                          run Frameshift again.",
        },
    )
}

const FS1010: &str = "FS1010";
const FS1020: &str = "FS1020";
const FS1021: &str = "FS1021";
const FS1022: &str = "FS1022";
const FS1023: &str = "FS1023";
const FS1024: &str = "FS1024";
const FS1025: &str = "FS1025";
const FS1026: &str = "FS1026";
const FS1027: &str = "FS1027";
const FS1028: &str = "FS1028";
const FS1029: &str = "FS1029";
const FS2001: &str = "FS2001";
const FS2002: &str = "FS2002";
const FS2003: &str = "FS2003";

const TEXT_FS1010: FindingText<'static> = FindingText {
    message: "Lockfile format not parsed",
    explanation: "Frameshift does not read this lockfile format or format version, so resolved \
                  versions fall back to the ranges declared in package.json and are imprecise.",
    remediation: "Use a supported lockfile, or accept imprecise versions.",
};
const TEXT_NOT_OBJECT: FindingText<'static> = FindingText {
    message: "package.json is not a JSON object",
    explanation: "The top-level value of package.json must be an object, so the file is not \
                  analyzed.",
    remediation: "Correct package.json, then run Frameshift again.",
};
const TEXT_FS1021: FindingText<'static> = FindingText {
    message: "Duplicate key in a package file",
    explanation: "The key occurs again later in the same object. Frameshift uses the last \
                  occurrence, as npm does, but other tools may read the file differently.",
    remediation: "Remove the duplicate key.",
};
const TEXT_FS1021_YAML: FindingText<'static> = FindingText {
    message: "Duplicate key in a package file",
    explanation: "A YAML mapping repeats a decoded key. The lockfile is ambiguous and is rejected \
                  as a whole.",
    remediation: "Remove the duplicate key.",
};
const TEXT_FS1021_CLASSIC: FindingText<'static> = FindingText {
    message: "Duplicate key in a package file",
    explanation: "A key repeats in one object of the Yarn classic lockfile, where Yarn uses the \
                  last occurrence. The lockfile is ambiguous and is rejected as a whole.",
    remediation: "Remove the duplicate key.",
};
const TEXT_FS1022: FindingText<'static> = FindingText {
    message: "packageManager is not a valid specification",
    explanation: "The packageManager field must be <name>@<version>[+<hash>] with npm, pnpm, \
                  yarn, or bun and an exact version, so it is ignored and lockfiles decide.",
    remediation: "Correct the field, for example \"pnpm@10.14.0\".",
};
const TEXT_FS1023: FindingText<'static> = FindingText {
    message: "Lockfile of another package manager ignored",
    explanation: "The packageManager field names a different package manager, so this lockfile \
                  is not used.",
    remediation: "Remove the stale lockfile, or correct the packageManager field.",
};
const TEXT_FS1024: FindingText<'static> = FindingText {
    message: "Package manager ambiguous",
    explanation: "Lockfiles of several package managers are present and no valid \
                  packageManager field decides, so no lockfile is used and versions fall back \
                  to the declared ranges.",
    remediation: "Remove stale lockfiles, or add a packageManager field.",
};
const TEXT_FS1025: FindingText<'static> = FindingText {
    message: "Lockfile superseded",
    explanation: "A lockfile of the same package manager takes precedence: npm-shrinkwrap.json \
                  over package-lock.json, and bun.lock over bun.lockb.",
    remediation: "Remove the superseded lockfile if it is stale.",
};
const TEXT_FS1026: FindingText<'static> = FindingText {
    message: "Lockfile structure not valid",
    explanation: "The lockfile does not have the structure of its format, so it is not used and \
                  versions fall back to the declared ranges.",
    remediation: "Regenerate the lockfile with the package manager.",
};
const TEXT_FS2002: FindingText<'static> = FindingText {
    message: "Unsupported text encoding",
    explanation: "The file is not valid UTF-8, so it is not decoded or analyzed.",
    remediation: "Convert the file to UTF-8.",
};
const TEXT_FS2003_REMEDIATION: &str = "Reduce the file size, or raise the limit if the file is \
                                       trusted.";

struct Reporter<'a> {
    findings: &'a mut FindingCollector,
}

impl Reporter<'_> {
    fn push(
        &mut self,
        location: FindingLocation,
        code: &str,
        severity: Severity,
        text: FindingText<'_>,
    ) -> Result<(), LoadError> {
        let code = FindingCode::new(code).map_err(|_| LoadError::Internal)?;
        let construction = FindingLimits {
            max_text_bytes: usize::MAX,
            max_findings: usize::MAX,
        };
        let finding = Finding::new(
            code,
            severity,
            location,
            FindingContext::default(),
            text,
            construction,
        )
        .map_err(LoadError::Findings)?;
        self.findings.push(finding).map_err(LoadError::Findings)
    }

    fn file(
        &mut self,
        path: &SourcePath,
        code: &str,
        severity: Severity,
        text: FindingText<'_>,
    ) -> Result<(), LoadError> {
        self.push(FindingLocation::File(path.clone()), code, severity, text)
    }

    fn text_with(
        &mut self,
        path: &SourcePath,
        source: &SourceText,
        span: Span,
        code: &str,
        severity: Severity,
        text: FindingText<'_>,
    ) -> Result<(), LoadError> {
        let location =
            TextLocation::new(path.clone(), source, span).map_err(|_| LoadError::Internal)?;
        self.push(FindingLocation::Text(location), code, severity, text)
    }
}
