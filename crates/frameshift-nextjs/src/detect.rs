//! Next.js framework, version, configuration, and router detection
//! (`D-023`, `D-074`).
//!
//! The rules follow Next.js 14.2 to 16.4: `findPagesDir` prefers `pages`
//! and `app` in the package directory over `src/`, and Next.js 15 and
//! later reject the two in different directories; `loadConfig` loads the
//! nearest `next.config.*` of the names that the version supports, searching
//! upward from the package directory.

use std::fmt;

use frameshift_core::{
    Finding, FindingCode, FindingCollector, FindingContext, FindingError, FindingLimits,
    FindingLocation, FindingText, PathLimits, Severity, SourcePath,
};
use frameshift_input::{EntryKind, Inventory};
use frameshift_package::{DependencyKind, DependencySpec, ManifestState, SelectedPackage, Version};

/// The Next.js majors that V1 validates with fixtures (`D-023`).
pub const VALIDATED_MAJORS: std::ops::RangeInclusive<u64> = 14..=16;

/// Whether the package is a Next.js project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// `next` is a dependency and a configuration file or route files
    /// confirm it.
    NextJs,
    /// Some Next.js evidence exists, but not enough; a finding says what
    /// is missing.
    Inconclusive,
    /// No Next.js evidence.
    Absent,
}

/// The package's `next` declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextDependency {
    /// The dependency field that declares it.
    pub kind: DependencyKind,
    /// The specifier text.
    pub spec: Box<str>,
}

/// What is known about the Next.js version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionEvidence {
    /// The exact version that the root lockfile resolves.
    Locked(Version),
    /// The lowest version of the declared range; the installed version may
    /// be higher.
    LowerBound(Version),
    /// No version: no `next` dependency, a range without a lower bound, or
    /// a source other than the registry.
    Unknown,
}

impl VersionEvidence {
    /// The exact version or the lower bound.
    pub fn version(&self) -> Option<&Version> {
        match self {
            Self::Locked(version) | Self::LowerBound(version) => Some(version),
            Self::Unknown => None,
        }
    }
}

/// Whether Next.js loads a configuration file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigLoad {
    /// The version loads it.
    Loaded,
    /// Next.js 16 loads `next.config.mts` only when Node.js strips
    /// TypeScript types, which depends on the Node.js version.
    DependsOnNode,
    /// Next.js ignores it (`next.config.ts` before Next.js 15) or fails on
    /// it (`next.config.cjs`, `.cts`, `.json`, `.jsx`, `.tsx`).
    Unsupported,
}

/// The `next.config.*` file nearest to the package directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigFile {
    /// The file.
    pub path: SourcePath,
    /// Whether Next.js loads it.
    pub load: ConfigLoad,
}

/// What a router directory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouterState {
    /// A traversed directory.
    Directory {
        /// Route files below it, counted with the default page extensions.
        routes: usize,
        /// Whether every entry below it was traversed.
        complete: bool,
    },
    /// An entry that is not a traversed directory: a file, a link, or an
    /// ignored or unread directory.
    Other,
}

/// The `app` or `pages` directory that Next.js uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouterDirectory {
    /// The directory.
    pub path: SourcePath,
    /// Whether it lies below `src/`.
    pub in_src: bool,
    /// What it is.
    pub state: RouterState,
}

impl RouterDirectory {
    /// The number of route files, zero for an entry that is not a
    /// traversed directory.
    pub const fn routes(&self) -> usize {
        match self.state {
            RouterState::Directory { routes, .. } => routes,
            RouterState::Other => 0,
        }
    }
}

/// Which routers have route files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Router {
    /// Route files only below `app`.
    App,
    /// Route files only below `pages`.
    Pages,
    /// Route files below both; V1 migrates the App Router part.
    Hybrid,
    /// No route files.
    None,
}

/// The detection result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// The verdict.
    pub verdict: Verdict,
    /// The `next` declaration, if any.
    pub dependency: Option<NextDependency>,
    /// The version evidence.
    pub version: VersionEvidence,
    /// The configuration file nearest to the package directory, if any.
    pub config: Option<ConfigFile>,
    /// The `app` directory that Next.js uses, if any.
    pub app: Option<RouterDirectory>,
    /// The `pages` directory that Next.js uses, if any.
    pub pages: Option<RouterDirectory>,
}

impl Detection {
    /// Which routers have route files.
    pub fn router(&self) -> Router {
        let routes = |directory: &Option<RouterDirectory>| {
            directory
                .as_ref()
                .is_some_and(|directory| directory.routes() > 0)
        };
        match (routes(&self.app), routes(&self.pages)) {
            (true, true) => Router::Hybrid,
            (true, false) => Router::App,
            (false, true) => Router::Pages,
            (false, false) => Router::None,
        }
    }
}

/// A failure that ends detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectError {
    /// The finding collector rejected a finding.
    Findings(FindingError),
    /// An internal invariant failed.
    Internal,
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Findings(error) => write!(f, "detection findings could not be stored: {error}"),
            Self::Internal => f.write_str("internal Next.js detection invariant failed"),
        }
    }
}

impl std::error::Error for DetectError {}

/// The default `pageExtensions`; a configuration may change them.
const PAGE_EXTENSIONS: [&str; 4] = ["tsx", "ts", "jsx", "js"];

/// Detects Next.js for the selected package of a workspace.
pub fn detect(
    inventory: &Inventory,
    package: &SelectedPackage<'_>,
    findings: &mut FindingCollector,
) -> Result<Detection, DetectError> {
    let mut report = Reporter { findings };
    let directory = package.directory();
    let dependency = dependency(package.manifest());
    let mut version = VersionEvidence::Unknown;
    let mut version_problem = None;
    if let Some(dependency) = &dependency {
        (version, version_problem) = resolve(package, dependency);
    }
    let major = version.version().map(Version::major);
    let config = config(inventory, directory, major)?;
    let mut detection = Detection {
        verdict: Verdict::Absent,
        dependency,
        version,
        config,
        app: router_directory(inventory, directory, "app", Route::App)?,
        pages: router_directory(inventory, directory, "pages", Route::Pages)?,
    };
    let declared = detection
        .dependency
        .as_ref()
        .is_some_and(|dependency| dependency.kind != DependencyKind::Peer);
    let structural = detection.config.is_some() || detection.router() != Router::None;
    detection.verdict = if declared && structural {
        Verdict::NextJs
    } else if declared || detection.config.is_some() {
        Verdict::Inconclusive
    } else {
        Verdict::Absent
    };
    let manifest = manifest_path(directory)?;
    match detection.verdict {
        Verdict::Absent => return Ok(detection),
        Verdict::Inconclusive => {
            let explanation = if declared {
                "The package declares next, but has neither a next.config file nor route files \
                 in app or pages, so it is not classified as a Next.js project."
            } else {
                "A next.config file exists, but the package does not declare next as a \
                 dependency, so it is not classified as a Next.js project."
            };
            report.push(
                FindingLocation::File(manifest),
                FS1040,
                Severity::Review,
                FindingText {
                    message: "Next.js evidence incomplete",
                    explanation,
                    remediation: "Check that the selected package is the Next.js application.",
                },
            )?;
            return Ok(detection);
        }
        Verdict::NextJs => {}
    }
    if let Some(problem) = version_problem {
        let lockfile = package.lockfile().is_some();
        let explanation = match (problem, lockfile) {
            (VersionProblem::Unresolved, false) => {
                "No lockfile was read, so the Next.js version is the lowest version of the \
                 declared range, or unknown. If this package is a member of a workspace, run \
                 Frameshift on the workspace root and select the package."
            }
            (VersionProblem::Unresolved, true) => {
                "The lockfile does not resolve the next declaration, so the Next.js version is \
                 the lowest version of the declared range, or unknown."
            }
            (VersionProblem::NotRegistry, _) => {
                "next resolves to a source other than the npm registry, so its version is not \
                 known."
            }
        };
        report.push(
            FindingLocation::File(manifest.clone()),
            FS1042,
            Severity::Review,
            FindingText {
                message: "Next.js version imprecise",
                explanation,
                remediation: "Install the dependencies so that the lockfile resolves next, \
                              then run Frameshift again.",
            },
        )?;
    }
    if let Some(version) = detection.version.version()
        && !VALIDATED_MAJORS.contains(&version.major())
    {
        report.push(
            FindingLocation::File(manifest.clone()),
            FS1041,
            Severity::Review,
            FindingText {
                message: "Next.js version outside the validated range",
                explanation: "Frameshift V1 validates Next.js 14 to 16. This version is analyzed, \
                              and version-dependent rules behave conservatively.",
                remediation: "Upgrade to a validated Next.js version, or review the results \
                              with extra care.",
            },
        )?;
    }
    report_config(&detection, directory, &mut report)?;
    report_routers(&detection, major, &mut report)?;
    Ok(detection)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VersionProblem {
    Unresolved,
    NotRegistry,
}

/// The first `next` declaration in the order production, development,
/// optional, and peer, unless it is an alias of another package.
fn dependency(manifest: &ManifestState) -> Option<NextDependency> {
    let ManifestState::Analyzed(manifest) = manifest else {
        return None;
    };
    for kind in [
        DependencyKind::Production,
        DependencyKind::Development,
        DependencyKind::Optional,
        DependencyKind::Peer,
    ] {
        let Some(entry) = manifest
            .dependencies_of(kind)
            .iter()
            .find(|entry| &*entry.key == "next")
        else {
            continue;
        };
        if let DependencySpec::Alias { name, .. } = &entry.value.spec
            && &**name != "next"
        {
            return None;
        }
        return Some(NextDependency {
            kind,
            spec: entry.value.text.clone(),
        });
    }
    None
}

/// Resolves the version through the root lockfile, or falls back to the
/// declared range's lower bound.
fn resolve(
    package: &SelectedPackage<'_>,
    dependency: &NextDependency,
) -> (VersionEvidence, Option<VersionProblem>) {
    if let Some(locked) = package.resolve_declared("next", &dependency.spec) {
        return match locked.version() {
            Some(version) if &*locked.name == "next" => {
                (VersionEvidence::Locked(version.clone()), None)
            }
            Some(_) | None => (VersionEvidence::Unknown, Some(VersionProblem::NotRegistry)),
        };
    }
    let spec = DependencySpec::parse(&dependency.spec);
    let lower = spec
        .range()
        .and_then(|range| range.min_version())
        .filter(|version| Version::new(0, 0, 0).as_ref() != Some(version));
    match lower {
        Some(version) => (
            VersionEvidence::LowerBound(version),
            Some(VersionProblem::Unresolved),
        ),
        None => (VersionEvidence::Unknown, Some(VersionProblem::Unresolved)),
    }
}

fn child(directory: Option<&SourcePath>, name: &str) -> Result<SourcePath, DetectError> {
    let text = match directory {
        Some(directory) => format!("{}/{name}", directory.as_str()),
        None => name.to_owned(),
    };
    SourcePath::new(&text, PathLimits::default()).map_err(|_| DetectError::Internal)
}

fn manifest_path(directory: Option<&SourcePath>) -> Result<SourcePath, DetectError> {
    child(directory, "package.json")
}

/// Whether discovery recorded any entry at the path, as `fs.existsSync`
/// would find something there.
fn exists(inventory: &Inventory, path: &SourcePath) -> bool {
    inventory.files().find(path).is_some() || inventory.entry(path).is_some()
}

const SUPPORTED: [&str; 4] = [
    "next.config.js",
    "next.config.mjs",
    "next.config.ts",
    "next.config.mts",
];
const UNSUPPORTED: [&str; 5] = [
    "next.config.cjs",
    "next.config.cts",
    "next.config.json",
    "next.config.jsx",
    "next.config.tsx",
];

/// How a version loads a configuration file name.
fn load(name: &str, major: Option<u64>) -> ConfigLoad {
    // An unknown version is read as the newest one.
    let major = major.unwrap_or(16);
    match name {
        "next.config.js" | "next.config.mjs" => ConfigLoad::Loaded,
        "next.config.ts" if major >= 15 => ConfigLoad::Loaded,
        "next.config.mts" if major >= 16 => ConfigLoad::DependsOnNode,
        _ => ConfigLoad::Unsupported,
    }
}

/// The nearest configuration file from the package directory up to the
/// root, as Next.js's `findUp` searches: the first directory with a name
/// that the version loads, else the first with a name it rejects.
fn config(
    inventory: &Inventory,
    directory: Option<&SourcePath>,
    major: Option<u64>,
) -> Result<Option<ConfigFile>, DetectError> {
    let mut directories = Vec::new();
    let mut current = directory.map(|directory| directory.as_str().to_owned());
    loop {
        directories.push(current.clone());
        match current {
            None => break,
            Some(path) => {
                current = path.rsplit_once('/').map(|(parent, _)| parent.to_owned());
            }
        }
    }
    let find = |names: &[&str]| -> Result<Option<SourcePath>, DetectError> {
        for directory in &directories {
            let base = directory
                .as_deref()
                .map(|text| SourcePath::new(text, PathLimits::default()))
                .transpose()
                .map_err(|_| DetectError::Internal)?;
            for name in names {
                let path = child(base.as_ref(), name)?;
                if exists(inventory, &path) {
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    };
    let loadable: Vec<&str> = SUPPORTED
        .iter()
        .copied()
        .filter(|name| load(name, major) != ConfigLoad::Unsupported)
        .collect();
    if let Some(path) = find(&loadable)? {
        let name = path.as_str().rsplit('/').next().unwrap_or_default();
        return Ok(Some(ConfigFile {
            load: load(name, major),
            path,
        }));
    }
    let rejected: Vec<&str> = SUPPORTED
        .iter()
        .chain(UNSUPPORTED.iter())
        .copied()
        .filter(|name| load(name, major) == ConfigLoad::Unsupported)
        .collect();
    Ok(find(&rejected)?.map(|path| ConfigFile {
        path,
        load: ConfigLoad::Unsupported,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    App,
    Pages,
}

/// Finds `name` or `src/name` as `findPagesDir` does and counts its route
/// files with the default page extensions.
fn router_directory(
    inventory: &Inventory,
    directory: Option<&SourcePath>,
    name: &str,
    route: Route,
) -> Result<Option<RouterDirectory>, DetectError> {
    let direct = child(directory, name)?;
    let (path, in_src) = if exists(inventory, &direct) {
        (direct, false)
    } else {
        let src = child(directory, &format!("src/{name}"))?;
        if !exists(inventory, &src) {
            return Ok(None);
        }
        (src, true)
    };
    let state = match inventory.entry(&path).map(|entry| entry.kind()) {
        Some(EntryKind::Directory) => {
            let prefix = format!("{}/", path.as_str());
            let files = inventory.files().as_slice();
            let start = files.partition_point(|file| file.as_str() < prefix.as_str());
            let routes = files[start..]
                .iter()
                .take_while(|file| file.as_str().starts_with(&prefix))
                .filter(|file| is_route(&file.as_str()[prefix.len()..], route))
                .count();
            let entries = inventory.entries();
            let start = entries.partition_point(|entry| entry.path().as_str() < prefix.as_str());
            let complete = entries[start..]
                .iter()
                .take_while(|entry| entry.path().as_str().starts_with(&prefix))
                .all(|entry| matches!(entry.kind(), EntryKind::Directory | EntryKind::Special));
            RouterState::Directory { routes, complete }
        }
        Some(_) | None => RouterState::Other,
    };
    Ok(Some(RouterDirectory {
        path,
        in_src,
        state,
    }))
}

/// Whether a file below a router directory is a route file: `page` or
/// `route` outside private `_` folders for the App Router, and any page
/// file except `_app`, `_document`, `_error`, and declaration files for
/// the Pages Router.
fn is_route(relative: &str, route: Route) -> bool {
    let (folders, name) = relative.rsplit_once('/').unwrap_or(("", relative));
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if !PAGE_EXTENSIONS.contains(&extension) || stem.ends_with(".d") {
        return false;
    }
    match route {
        Route::App => {
            matches!(stem, "page" | "route")
                && !folders.split('/').any(|folder| folder.starts_with('_'))
        }
        Route::Pages => !matches!(stem, "_app" | "_document" | "_error"),
    }
}

fn report_config(
    detection: &Detection,
    directory: Option<&SourcePath>,
    report: &mut Reporter<'_>,
) -> Result<(), DetectError> {
    let (location, severity, explanation) = match &detection.config {
        None => (
            FindingLocation::File(manifest_path(directory)?),
            Severity::Info,
            "No next.config file exists in the package directory or above it inside the root. \
             Next.js uses its defaults unless a directory above the analyzed root has one.",
        ),
        Some(ConfigFile {
            load: ConfigLoad::Loaded,
            ..
        }) => return Ok(()),
        Some(ConfigFile {
            path,
            load: ConfigLoad::DependsOnNode,
        }) => (
            FindingLocation::File(path.clone()),
            Severity::Review,
            "Next.js 16 loads next.config.mts only when Node.js strips TypeScript types, which \
             depends on the Node.js version that builds the project.",
        ),
        Some(ConfigFile {
            path,
            load: ConfigLoad::Unsupported,
        }) => (
            FindingLocation::File(path.clone()),
            Severity::Review,
            "This Next.js version does not load a configuration file with this name: it ignores \
             next.config.ts before Next.js 15 and fails on the other names.",
        ),
    };
    report.push(
        location,
        FS1043,
        severity,
        FindingText {
            message: "Next.js configuration file not loaded as expected",
            explanation,
            remediation: "Use next.config.js, next.config.mjs, or, from Next.js 15, \
                          next.config.ts.",
        },
    )
}

fn report_routers(
    detection: &Detection,
    major: Option<u64>,
    report: &mut Reporter<'_>,
) -> Result<(), DetectError> {
    for directory in [&detection.app, &detection.pages].into_iter().flatten() {
        let explanation = match directory.state {
            RouterState::Other => {
                "The router directory is not a directory that discovery traversed, such as a \
                 file, a link, or an ignored directory, so its routes are not known."
            }
            RouterState::Directory {
                complete: false, ..
            } => {
                "Discovery did not traverse every entry below the router directory, so its \
                 routes may be incomplete."
            }
            RouterState::Directory { complete: true, .. } => continue,
        };
        report.push(
            FindingLocation::File(directory.path.clone()),
            FS1044,
            Severity::Review,
            FindingText {
                message: "Next.js router directory not fully known",
                explanation,
                remediation: "Make the router directory an ordinary traversed directory, then \
                              run Frameshift again.",
            },
        )?;
    }
    if let (Some(app), Some(pages)) = (&detection.app, &detection.pages)
        && app.in_src != pages.in_src
        && major.is_none_or(|major| major >= 15)
    {
        report.push(
            FindingLocation::File(app.path.clone()),
            FS1044,
            Severity::Review,
            FindingText {
                message: "Next.js router directory not fully known",
                explanation: "app and pages lie in different directories, which Next.js 15 and \
                              later reject.",
                remediation: "Move app and pages into the same directory.",
            },
        )?;
    }
    Ok(())
}

const FS1040: &str = "FS1040";
const FS1041: &str = "FS1041";
const FS1042: &str = "FS1042";
const FS1043: &str = "FS1043";
const FS1044: &str = "FS1044";

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
    ) -> Result<(), DetectError> {
        let code = FindingCode::new(code).map_err(|_| DetectError::Internal)?;
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
        .map_err(DetectError::Findings)?;
        self.findings.push(finding).map_err(DetectError::Findings)
    }
}
