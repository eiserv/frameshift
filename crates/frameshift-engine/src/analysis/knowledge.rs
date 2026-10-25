//! Embedded compatibility evidence. Missing evidence is never compatibility.

use crate::project::Project;
use frameshift_core::SourcePath;
use frameshift_modules::Resolution;
use frameshift_package::{DependencyKind, ManifestState, SelectedPackage, Workspace};
use std::collections::{BTreeMap, BTreeSet};

/// Embedded knowledge version; changes require reviewed evidence.
pub const VERSION: &str = "2026.10.1";
/// Knowledge-base compatibility classes of MIGRATION_SPEC.md section 14.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compatibility {
    /// Replaced source-framework package.
    Framework,
    /// Verified framework-independent package.
    Compatible,
    /// Verified through the pinned compat runtime.
    Compat,
    /// Expected compat behavior with no executable fixture evidence.
    CompatUnverified,
    /// Known incompatible runtime requirements.
    Incompatible,
    /// No applicable versioned evidence.
    Unknown,
}
impl Compatibility {
    /// Stable reporting spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Framework => "framework",
            Self::Compatible => "compatible",
            Self::Compat => "compat",
            Self::CompatUnverified => "compat-unverified",
            Self::Incompatible => "incompatible",
            Self::Unknown => "unknown",
        }
    }
}
/// One direct dependency and the runtime modules that actually import it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Original package name; escape before presentation.
    pub name: String,
    /// Manifest dependency fields, in declaration-kind order.
    pub fields: Vec<String>,
    /// Declared dependency specifiers; duplicates are retained by field.
    pub declared: Vec<String>,
    /// Established exact lockfile version, never guessed from a range.
    pub resolved: Option<String>,
    /// Version-applicable knowledge entry, or explicit unknown.
    pub class: Compatibility,
    /// Why this entry applies, including the evidence boundary.
    pub evidence: String,
    /// Runtime import sites by module, sorted and deduplicated.
    pub used_in: Vec<SourcePath>,
}
/// Explains an assessment finding code. No source-derived prose is included.
pub fn explain(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match code {
        "FS3001" => (
            "Migration behavior is not established",
            "The construct or its required preconditions cannot be established from the available static facts.",
            "Inspect the named feature and source location. Simplify it or migrate it manually; do not assume compatibility.",
        ),
        "FS3002" => (
            "A runtime dependency cannot be resolved",
            "The snapshot graph cannot establish the target of a runtime reference.",
            "Provide a literal import and admitted source target, or review the reference manually.",
        ),
        "FS3003" => (
            "Configuration is not statically known",
            "An exported configuration value or property set cannot be evaluated without running source.",
            "Use a literal configuration or review each unknown property; Frameshift never executes configuration.",
        ),
        "FS3100" => (
            "Feature is unsupported in static output",
            "The V1 compatibility matrix identifies this feature as a definite blocker.",
            "Remove the feature, implement it outside the static site, or retain a server deployment.",
        ),
        "FS3101" => (
            "Request-time API requires a server",
            "A request-time Next.js API cannot run in static output.",
            "Replace request-time data with build-time data or a separately hosted API.",
        ),
        "FS3110" => (
            "Dynamic route parameters cannot be enumerated",
            "No analyzable parameter generator establishes the static route set.",
            "Supply generateStaticParams for every required parameter segment.",
        ),
        "FS3120" => (
            "Server Action cannot run in static output",
            "Static output has no Server Action runtime.",
            "Move the action to a separately hosted API and update its callers.",
        ),
        "FS3130" => (
            "Proxy or middleware requires hosting behavior",
            "The source request pipeline cannot be reproduced by static files alone.",
            "Configure equivalent hosting behavior or remove the request-time dependency.",
        ),
        "FS3201" => (
            "Context scope cannot cross independent islands",
            "A shared context or provider requires proof that all consumers remain within one Preact root.",
            "Keep provider and consumers in one island, or use explicitly shared module state.",
        ),
        "FS3210" => (
            "Navigation changes to full page loads",
            "The target static site uses browser navigation rather than the Next.js client router.",
            "Review state persistence, scroll behavior, and navigation-dependent interactions.",
        ),
        "FS3301" => (
            "Dependency is incompatible",
            "The versioned knowledge base establishes incompatible runtime requirements.",
            "Replace the package or move the feature outside this migration.",
        ),
        "FS3302" => (
            "Dependency compatibility is unknown",
            "No verified knowledge entry covers this dependency and version. Declaration alone does not affect units; runtime usage does.",
            "Verify the dependency with the pinned target profile or replace it.",
        ),
        "FS3500" => (
            "Mapping requires review",
            "The documented mapping depends on runtime values or changes behavior.",
            "Review the feature's mapping, generated behavior, and deployment settings before relying on the migration.",
        ),
        "FS3501" => (
            "JSX conversion has value-dependent assumptions",
            "The printer identified rendering differences whose absence cannot be proved statically.",
            "Review the converter assumptions or retain static Preact rendering.",
        ),
        "FS3502" => (
            "Source version is outside the validated range",
            "The source version is unknown or is outside Next.js 14 through 16.",
            "Validate the project version before relying on version-dependent mappings.",
        ),
        "FS9001" => (
            "Internal accounting invariant failed",
            "The report cannot account for every admitted source file or has contradictory identities.",
            "Do not rely on this report; report the reproducible input to Frameshift maintainers.",
        ),
        _ => return input_explanation(code),
    })
}

/// One exact, fixture-verified package compatibility entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Registry package name.
    pub package: &'static str,
    /// Only this exact version is covered; ranges are never guessed.
    pub version: &'static str,
    /// Established compatibility class.
    pub class: Compatibility,
    /// Reproducible evidence identifier.
    pub fixture: &'static str,
}
/// Curated V1 evidence. No other version inherits these claims.
pub const ENTRIES: &[Entry] = &[
    Entry {
        package: "classnames",
        version: "2.5.1",
        class: Compatibility::Compatible,
        fixture: "external_knowledge_fixtures/classnames-2.5.1",
    },
    Entry {
        package: "clsx",
        version: "2.1.1",
        class: Compatibility::Compatible,
        fixture: "external_knowledge_fixtures/clsx-2.1.1",
    },
];

pub(crate) fn dependencies(
    selected: &SelectedPackage<'_>,
    _: &Workspace,
    project: &Project,
) -> Vec<Dependency> {
    let mut uses: BTreeMap<String, BTreeSet<SourcePath>> = BTreeMap::new();
    for node in project.modules().nodes() {
        for edge in &node.edges {
            if !edge.type_only
                && let Resolution::External { package } = &edge.resolution
                && !project.environment().module(&node.path).is_some_and(|m| {
                    m.apis.iter().any(|api| {
                        api.kind == frameshift_nextjs::ApiKind::NodeModule && api.span == edge.span
                    })
                })
            {
                uses.entry(package.clone())
                    .or_default()
                    .insert(node.path.clone());
            }
        }
    }
    let mut result: BTreeMap<String, Dependency> = BTreeMap::new();
    if let ManifestState::Analyzed(manifest) = selected.manifest() {
        for kind in DependencyKind::ALL {
            for entry in manifest.dependencies_of(kind) {
                let locked = selected.resolve_declared(&entry.key, &entry.value.text);
                let ordinary = matches!(
                    entry.value.spec,
                    frameshift_package::DependencySpec::Range(_)
                );
                let exact = locked
                    .and_then(|p| p.version())
                    .map(ToString::to_string)
                    .or_else(|| {
                        frameshift_package::Version::parse(&entry.value.text)
                            .ok()
                            .map(|v| v.to_string())
                    });
                let framework = ordinary
                    && matches!(
                        entry.key.as_ref(),
                        "next" | "react" | "react-dom" | "@types/react" | "@types/react-dom"
                    );
                let verified = ENTRIES.iter().find(|e| {
                    ordinary
                        && e.package == entry.key.as_ref()
                        && exact.as_deref() == Some(e.version)
                });
                let class = if framework {
                    Compatibility::Framework
                } else {
                    verified.map_or(Compatibility::Unknown, |e| e.class)
                };
                let evidence = if framework {
                    "MIGRATION_SPEC.md section 5.4: source-framework packages are replaced or removed.".to_owned()
                } else if let Some(e) = verified {
                    format!(
                        "{}: framework-independent class aggregation and Preact 10.29.8 SSR, reference preact-render-to-string 6.6.1.",
                        e.fixture
                    )
                } else {
                    "No verified versioned package fixture covers this dependency.".to_owned()
                };
                let item = result
                    .entry(entry.key.to_string())
                    .or_insert_with(|| Dependency {
                        name: entry.key.to_string(),
                        fields: Vec::new(),
                        declared: Vec::new(),
                        resolved: locked.and_then(|p| p.version()).map(ToString::to_string),
                        class,
                        evidence,
                        used_in: uses
                            .remove(entry.key.as_ref())
                            .unwrap_or_default()
                            .into_iter()
                            .collect(),
                    });
                if item.class != class {
                    item.class = Compatibility::Unknown;
                    item.evidence =
                        "Conflicting declarations do not establish one compatibility entry.".into();
                }
                item.fields.push(kind.key().into());
                item.declared.push(entry.value.text.to_string());
            }
        }
    }
    for (name, used_in) in uses {
        result.insert(
            name.clone(),
            Dependency {
                name,
                fields: Vec::new(),
                declared: Vec::new(),
                resolved: None,
                class: Compatibility::Unknown,
                evidence: "Runtime import has no declaration in the selected package.".into(),
                used_in: used_in.into_iter().collect(),
            },
        );
    }
    result.into_values().collect()
}

fn input_explanation(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let title = match code {
        "FS1001" => "Source path cannot be represented canonically",
        "FS1002" => "Special file skipped",
        "FS1003" => "Symbolic link leaves the project root",
        "FS1004" => "File exceeds the per-file size limit",
        "FS1005" => "Symbolic link is not followed",
        "FS1006" => "Symbolic link target cannot be established",
        "FS1007" => "Symbolic link resolves to an ignored path",
        "FS1008" => "Windows reparse point is not followed",
        "FS1009" => "Directory exceeds the depth limit",
        "FS1010" => "Lockfile format is not parsed",
        "FS1011" => "File count limit reached",
        "FS1012" => "Total byte limit reached",
        "FS1013" => "Discovery entry limit reached",
        "FS1014" => "Ignore-file byte limit reached",
        "FS1015" => "Ignore matching work limit reached",
        "FS1016" => "Directory could not be read",
        "FS1017" => "Entry could not be inspected",
        "FS1018" => "Ignore file could not be read",
        "FS1019" => "Admitted file could not be read",
        "FS1020" => "Manifest field has an unexpected type or value",
        "FS1021" => "Duplicate package-file key",
        "FS1022" => "Invalid packageManager declaration",
        "FS1023" => "Another package manager's lockfile is ignored",
        "FS1024" => "Package manager is ambiguous",
        "FS1025" => "Lockfile is superseded by another lockfile",
        "FS1026" => "Lockfile structure is invalid",
        "FS1027" => "Lockfile and manifest are out of sync",
        "FS1028" => "Lockfile exceeds the size limit",
        "FS1029" => "Lockfile format disagrees with the Yarn generation",
        "FS1030" => "Workspace pattern cannot be evaluated",
        "FS1031" => "Workspace pattern reaches an untraversed entry",
        "FS1032" => "Workspace membership differs between package managers",
        "FS1033" => "Workspace membership depends on case sensitivity",
        "FS1034" => "Another package manager's workspace declaration is ignored",
        "FS1035" => "Package manager rejects the workspace",
        "FS1036" => "Workspace membership cannot be established",
        "FS1037" => "Package manager release semantics are not modeled",
        "FS1038" => "Workspace resource limit reached",
        "FS1040" => "Next.js evidence is incomplete",
        "FS1041" => "Next.js version is outside the validated range",
        "FS1042" => "Next.js version is imprecise",
        "FS1043" => "Next.js configuration is not loaded as expected",
        "FS1044" => "Router directory is not fully known",
        "FS1045" => "Next.js ignores this file or directory",
        "FS2001" => "Source syntax error",
        "FS2002" => "Unsupported source text encoding",
        "FS2003" => "Source exceeds the decoded text limit",
        _ => return None,
    };
    let (explanation, remediation) = match code {
        c if ("FS1001"..="FS1019").contains(&c) => (
            "Discovery and snapshot loading are read-only. Entries rejected by a safety or resource boundary are retained as evidence; links and special files are never followed or executed.",
            "Provide regular readable files with canonical names inside the selected root. Address the report's specific exclusion or limit before relying on complete analysis.",
        ),
        c if ("FS1020"..="FS1029").contains(&c) => (
            "Package metadata and lockfiles are parsed without installing dependencies. Invalid or ambiguous evidence is never used to guess an installed version or package-manager layout.",
            "Correct the manifest or package-manager declaration and regenerate the lockfile with the project's chosen package manager, then analyze again.",
        ),
        c if ("FS1030"..="FS1038").contains(&c) => (
            "Workspace membership is established using the modeled package-manager releases and the admitted inventory. An unestablished membership is never treated as an included package.",
            "Use an explicitly supported package-manager declaration and workspace pattern, keep members within the root, and select a proven member with --package.",
        ),
        c if ("FS1040"..="FS1045").contains(&c) => (
            "Next.js identity, version, configuration naming, and router directories are detected without running Next.js. The validated source range is Next.js 14 through 16; a declared lower bound is not a locked version.",
            "Check the selected package, its Next.js dependency and lockfile, its configuration name, and its app or src/app directory against the reported evidence.",
        ),
        c if ("FS2001"..="FS2003").contains(&c) => (
            "A file without valid source text and a strict syntax tree cannot establish migration preconditions. Diagnostic recovery never makes an erroneous file eligible for transformation.",
            "Correct the syntax, provide valid UTF-8, or reduce the file below the documented limits, then analyze again.",
        ),
        _ => return None,
    };
    Some((title, explanation, remediation))
}
