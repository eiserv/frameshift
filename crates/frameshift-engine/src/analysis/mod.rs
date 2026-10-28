//! Read-only V1 assessment and the versioned analysis report.
//!
//! Assessment is evidence about the specified migration path, never an
//! execution or equivalence claim. Source I/O is confined to the snapshot.

mod assess;
pub mod knowledge;
pub mod metrics;
mod model;
mod report;
mod route;
pub mod stored;

use crate::{features, limits::Limits, project};
use frameshift_core::{FindingCollector, FindingList, SourcePath};
use frameshift_nextjs::{PageExtensions, detect};
use frameshift_package::{ManifestState, load_workspace};
use std::path::Path;

pub use assess::{AssessedOccurrence, Reason};
pub use knowledge::{Compatibility, Dependency};
pub use model::{Assessment, File, FileClass, Route, Strategy, Unit};

/// Version of the analysis JSON contract.
pub const SCHEMA_VERSION: u64 = 1;
/// Version of the embedded assessment rules.
pub const RULESET: &str = "nextjs-to-astro-preact/assessment-v1";
/// Pinned migration target profile.
pub const TARGET_PROFILE: &str = "astro7-preact10";

/// Inclusive bounds for assessment and stored analysis documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisLimits {
    /// Maximum counting units, default 100,000.
    pub max_units: usize,
    /// Maximum assessed occurrences, default 100,000.
    pub max_occurrences: usize,
    /// Cumulative dependency work across all routes, default 16,777,216.
    pub max_dependency_work: u64,
    /// Maximum stored/generated JSON bytes, default 64 MiB.
    pub max_report_bytes: usize,
}
impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            max_units: 100_000,
            max_occurrences: 100_000,
            max_dependency_work: 16_777_216,
            max_report_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Explicit inputs to one read-only analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalyzeOptions {
    /// One selected workspace member; `None` selects the root.
    pub package: Option<SourcePath>,
    /// All source and assessment resource bounds.
    pub limits: Limits,
}

/// Failure category, separate from a successful assessment with findings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzeErrorKind {
    /// The selected input or stored document cannot be analyzed.
    Input,
    /// An explicit resource boundary prevents a complete result.
    Limit,
    /// A violated engine invariant or invalid API limit relation.
    Internal,
}
/// A failure returns no partial report. Messages never contain source paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyzeError {
    kind: AnalyzeErrorKind,
    message: String,
}
impl AnalyzeError {
    /// The operational failure category, never a feature assessment status.
    pub const fn kind(&self) -> AnalyzeErrorKind {
        self.kind
    }
}
impl std::fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for AnalyzeError {}
pub(crate) fn failure(error: impl std::fmt::Display) -> AnalyzeError {
    AnalyzeError {
        kind: AnalyzeErrorKind::Input,
        message: error.to_string(),
    }
}
pub(crate) fn limit(error: impl std::fmt::Display) -> AnalyzeError {
    AnalyzeError {
        kind: AnalyzeErrorKind::Limit,
        message: error.to_string(),
    }
}
pub(crate) fn internal(error: impl std::fmt::Display) -> AnalyzeError {
    AnalyzeError {
        kind: AnalyzeErrorKind::Internal,
        message: error.to_string(),
    }
}

/// One immutable evidence ledger. Every renderer projects this same result.
#[derive(Debug)]
pub struct Analysis {
    pub(crate) max_report_bytes: usize,
    pub(crate) limits: Limits,
    /// Package directory in root coordinates, `None` for the run root.
    pub package: Option<SourcePath>,
    /// Established source version, or `None` when no version is known.
    pub source_version: Option<String>,
    /// Units, occurrences, files, and route closures in canonical order.
    pub assessment: Assessment,
    /// Direct dependencies, with compatibility evidence and actual uses.
    pub dependencies: Vec<Dependency>,
    /// All discovery, framework, and assessment findings in canonical order.
    pub findings: FindingList,
}

/// Analyzes one package, without executing source, writing files, or using
/// the network. A root with several workspace members requires explicit
/// package selection when its own manifest has no App Router application.
pub fn analyze(root: &Path, options: &AnalyzeOptions) -> Result<Analysis, AnalyzeError> {
    let limits = &options.limits;
    limits.validate().map_err(internal)?;
    let mut findings = FindingCollector::new(limits.finding);
    let snapshot =
        frameshift_input::snapshot(root, limits.snapshot(), &mut findings).map_err(failure)?;
    let workspace = load_workspace(&snapshot, limits.package(), &mut findings).map_err(failure)?;
    let selected = workspace
        .select(options.package.as_ref())
        .map_err(failure)?;
    let detection = detect(snapshot.inventory(), &selected, &mut findings).map_err(failure)?;
    let app = detection
        .app
        .as_ref()
        .ok_or_else(|| failure("the selected package has no App Router directory"))?;
    let mut syntax = assess::Syntax::default();
    // Default extensions are provisional only if configuration cannot establish
    // them; that uncertainty is retained as a project-wide assessment finding.
    let defaults = ["tsx", "ts", "jsx", "js"];
    let mut project = project::build_with(
        &snapshot,
        options.package.as_ref(),
        &app.path,
        &defaults,
        detection.config.as_ref(),
        limits,
        &mut |path, tree, bindings| syntax.collect(path, tree, bindings),
    )
    .map_err(failure)?;
    if let PageExtensions::Known(extensions) = project.configuration().page_extensions()
        && extensions.iter().map(String::as_str).collect::<Vec<_>>() != defaults
    {
        // Configuration was evaluated without executing it. Rebuild only
        // when the route language actually differs from the default.
        syntax = assess::Syntax::default();
        let refs: Vec<_> = extensions.iter().map(String::as_str).collect();
        project = project::build_with(
            &snapshot,
            options.package.as_ref(),
            &app.path,
            &refs,
            detection.config.as_ref(),
            limits,
            &mut |path, tree, bindings| syntax.collect(path, tree, bindings),
        )
        .map_err(failure)?;
    }
    let occurrences = features::detect(&snapshot, &project, &detection);
    if occurrences.len() > limits.analysis.max_occurrences {
        return Err(limit("analysis occurrence limit reached"));
    }
    let dependencies = knowledge::dependencies(&selected, &workspace, &project);
    syntax.correlate_routes(&project);
    syntax.correlate_images(&project);
    let manifest_automatic = matches!(selected.manifest(),ManifestState::Analyzed(manifest) if manifest.scripts.iter().all(|entry|matches!(entry.value.as_ref(),"next dev"|"next build")));
    let assessment = model::assess(
        &snapshot,
        &project,
        &detection,
        occurrences,
        &mut syntax,
        &dependencies,
        manifest_automatic,
        limits,
        &mut findings,
    )?;
    snapshot.report_unreadable(&mut findings).map_err(failure)?;
    let source_version = match &detection.version {
        frameshift_nextjs::VersionEvidence::Locked(v)
        | frameshift_nextjs::VersionEvidence::LowerBound(v) => Some(v.to_string()),
        frameshift_nextjs::VersionEvidence::Unknown => None,
    };
    if !matches!(selected.manifest(), ManifestState::Analyzed(_)) {
        return Err(failure("the selected package manifest cannot be analyzed"));
    }
    Ok(Analysis {
        max_report_bytes: limits.analysis.max_report_bytes,
        limits: *limits,
        package: options.package.clone(),
        source_version,
        assessment,
        dependencies,
        findings: findings.finish(),
    })
}
