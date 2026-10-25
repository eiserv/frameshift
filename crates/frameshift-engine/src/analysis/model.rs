//! Non-overlapping counting ownership, assessment propagation, route readiness.

use super::{
    AnalyzeError,
    assess::{self, AssessedOccurrence, Reason, Syntax},
    failure,
    knowledge::{self, Compatibility, Dependency},
    metrics,
};
use crate::{
    features::{Detail, Location, Occurrence},
    limits::Limits,
    project::{FileState, Project},
};
use frameshift_catalog::Status;
use frameshift_core::{
    FeatureId, Finding, FindingCode, FindingCollector, FindingContext, FindingLocation,
    FindingText, RuleId, Severity, SourcePath, SourceText, Span, TextLocation, UnitId, UnitKind,
};
use frameshift_input::{EntryKind, FileRead, Snapshot};
use frameshift_modules::{FileKind, ModuleState, Resolution};
use frameshift_nextjs::{
    ApiKind, Convention, Detection, PageExtensions, Rendered, RouteKind, VersionEvidence,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Intended component rendering strategy. No output has been generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Astro template, after the conversion fixed point.
    Astro,
    /// Synchronous static Preact rendering.
    PreactStatic,
    /// Preact within a client tree, with a directive at its outer boundary.
    Island,
    /// No established transformation.
    Preserve,
    /// Not a component.
    NotApplicable,
}
impl Strategy {
    /// Stable reporting spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Astro => "astro",
            Self::PreactStatic => "preact-static",
            Self::Island => "island",
            Self::Preserve => "preserve",
            Self::NotApplicable => "not-applicable",
        }
    }
}
/// A separately assessed counting element. Source ranges within one file
/// are disjoint; configuration concerns own properties rather than code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub(crate) definition: Option<Span>,
    /// Stable original-source identity.
    pub id: UnitId,
    /// Published counting kind.
    pub kind: UnitKind,
    /// Owning source file, absent for project configuration concerns.
    pub path: Option<SourcePath>,
    /// Disjoint code ranges. Empty for whole-file or logical config ownership.
    pub ranges: Vec<Span>,
    /// Whether this component participates in a browser context.
    pub client: bool,
    /// The fixed integer weight.
    pub weight: u64,
    /// Most severe occurrence or unit-level finding.
    pub status: Status,
    /// Intended rendering strategy, withheld for blocked or unknown units.
    pub strategy: Strategy,
}
/// Discovery/file disposition; excluded entries stay visible as ignored
/// inventory with their exclusion reason and an unknown project finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileClass {
    /// At least one counting unit owns the file.
    Units,
    /// A public asset or non-code data file.
    Verbatim,
    /// Explicitly outside V1, for example tests and repository tooling.
    OutOfScope,
    /// Discovery ignored or excluded this entry; never copied implicitly.
    Ignored,
}
impl FileClass {
    /// Stable reporting spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Units => "units",
            Self::Verbatim => "verbatim",
            Self::OutOfScope => "out-of-scope",
            Self::Ignored => "ignored",
        }
    }
}
/// One inventory entry and its exact disposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// Canonical root-relative path.
    pub path: SourcePath,
    /// Exactly one accounting group.
    pub class: FileClass,
    /// Owning unit IDs, sorted, when the class is `units`.
    pub units: Vec<String>,
    /// Why the disposition follows. No output/copy permission is implied.
    pub reason: String,
}
/// Route readiness including the complete transitive runtime closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// Original URL pattern.
    pub pattern: String,
    /// Route or endpoint unit.
    pub unit: String,
    /// Most severe closure status; auto/review/unknown/unsupported map to
    /// ready-automatic/ready-with-review/uncertain/blocked respectively.
    pub status: Status,
    /// Every unit in the closure, sorted and distinct.
    pub closure: Vec<String>,
    /// Deterministic witness unit for unknown/unsupported readiness.
    pub blocker: Option<String>,
    /// Shortest local import path to that witness, starting at a convention
    /// root. Configuration witnesses are project-wide and have no path.
    pub dependency_path: Vec<SourcePath>,
}
/// Pure assessment and accounting results used by every report projection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assessment {
    /// Units sorted by their serialized identities.
    pub units: Vec<Unit>,
    /// Every detected occurrence, in detection order.
    pub occurrences: Vec<AssessedOccurrence>,
    /// Accounted files and ignored entries, sorted by exact path bytes.
    pub files: Vec<File>,
    /// Routes in framework route order.
    pub routes: Vec<Route>,
    /// Discovery entries without representable paths.
    pub unrepresentable: usize,
}
fn client(project: &Project, path: &SourcePath) -> bool {
    project
        .nextjs()
        .contexts
        .modules
        .iter()
        .any(|m| &m.path == path && m.client)
}
fn add(
    units: &mut Vec<Unit>,
    id: UnitId,
    kind: UnitKind,
    path: Option<SourcePath>,
    ranges: Vec<Span>,
    is_client: bool,
    strategy: Strategy,
) {
    let definition = if kind == UnitKind::Component {
        ranges.first().copied()
    } else {
        None
    };
    units.push(Unit {
        definition,
        id,
        kind,
        path,
        ranges,
        client: is_client,
        weight: metrics::weight(kind, is_client),
        status: Status::Auto,
        strategy,
    });
}
fn file_id(
    kind: UnitKind,
    path: &SourcePath,
    name: Option<&str>,
    limits: &Limits,
) -> Result<UnitId, AnalyzeError> {
    UnitId::for_file(kind, path, name, limits.identifier).map_err(failure)
}
struct Ownership {
    ranges: BTreeMap<SourcePath, Vec<(Span, usize)>>,
    definitions: BTreeMap<(SourcePath, Span), usize>,
    fallback: BTreeMap<SourcePath, usize>,
}
impl Ownership {
    fn new(units: &[Unit]) -> Self {
        let mut result = Self {
            ranges: BTreeMap::new(),
            definitions: BTreeMap::new(),
            fallback: BTreeMap::new(),
        };
        for (i, u) in units.iter().enumerate() {
            if let Some(path) = &u.path {
                if let Some(span) = u.definition {
                    result.definitions.insert((path.clone(), span), i);
                }
                result
                    .ranges
                    .entry(path.clone())
                    .or_default()
                    .extend(u.ranges.iter().map(|&s| (s, i)));
                if u.ranges.is_empty() || u.kind == UnitKind::Module {
                    result.fallback.insert(path.clone(), i);
                } else {
                    result.fallback.entry(path.clone()).or_insert(i);
                }
            }
        }
        for ranges in result.ranges.values_mut() {
            ranges.sort_by_key(|(span, _)| *span);
        }
        result
    }
    fn owner(&self, _units: &[Unit], location: &Location) -> Option<usize> {
        let path = location.path()?;
        if let Location::Text { span, .. } = location {
            if let Some(&unit) = self.definitions.get(&(path.clone(), *span)) {
                return Some(unit);
            }
            if let Some(ranges) = self.ranges.get(path) {
                let end = ranges.partition_point(|(r, _)| r.start() <= span.start());
                if let Some((range, unit)) = end.checked_sub(1).and_then(|i| ranges.get(i))
                    && span.end() <= range.end()
                {
                    return Some(*unit);
                }
            }
        }
        self.fallback.get(path).copied()
    }
}
fn config_id(id: &str, limits: &Limits) -> Result<UnitId, AnalyzeError> {
    UnitId::for_config(id, limits.identifier).map_err(failure)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assess(
    snapshot: &Snapshot,
    project: &Project,
    detection: &Detection,
    occurrences: Vec<Occurrence>,
    syntax: &Syntax,
    dependencies: &[Dependency],
    manifest_automatic: bool,
    limits: &Limits,
    findings: &mut FindingCollector,
) -> Result<Assessment, AnalyzeError> {
    let mut conversion_work = limits.analysis.max_dependency_work;
    let runtime_files = runtime_files(project, &mut conversion_work)?;
    let mut units = Vec::new();
    let mut main_files = BTreeSet::new();
    for route in &project.nextjs().routes.routes {
        let kind = if route.kind == RouteKind::Page {
            UnitKind::Route
        } else {
            UnitKind::Endpoint
        };
        let id = UnitId::for_route(kind, &route.pattern, limits.identifier).map_err(failure)?;
        if let Some(existing) = units.iter_mut().find(|u: &&mut Unit| u.id == id) {
            existing.status = Status::Unknown;
            emit(
                findings,
                limits,
                "FS3001",
                Status::Unknown,
                FindingLocation::File(route.file.clone()),
                Some(&existing.id),
                None,
                None,
            )?;
            let module = file_id(UnitKind::Module, &route.file, None, limits)?;
            add(
                &mut units,
                module,
                UnitKind::Module,
                Some(route.file.clone()),
                Vec::new(),
                false,
                Strategy::NotApplicable,
            );
            main_files.insert(route.file.clone());
            continue;
        }

        add(
            &mut units,
            id,
            kind,
            Some(route.file.clone()),
            Vec::new(),
            client(project, &route.file),
            Strategy::NotApplicable,
        );
        main_files.insert(route.file.clone());
    }
    for file in &project.nextjs().routes.files {
        if main_files.contains(&file.path) {
            continue;
        }
        let kind = if file.convention == Convention::Layout {
            UnitKind::Layout
        } else {
            UnitKind::RouteFile
        };
        add(
            &mut units,
            file_id(kind, &file.path, None, limits)?,
            kind,
            Some(file.path.clone()),
            Vec::new(),
            client(project, &file.path),
            Strategy::NotApplicable,
        );
        main_files.insert(file.path.clone());
    }
    // Candidate component strategies are a greatest fixed point. Each
    // removal visits its incoming component edges only once.
    let graph = project.components();
    let mut candidates: Vec<bool> = graph
        .definitions
        .iter()
        .map(|d| {
            !client(project, &d.path) && syntax.function(&d.path, d.span).is_some_and(|f| f.astro)
        })
        .collect();
    let mut incoming = vec![Vec::new(); candidates.len()];
    let mut queue = VecDeque::new();
    for (i, &candidate) in candidates.iter().enumerate() {
        if !candidate {
            queue.push_back(i)
        }
    }
    for site in &graph.sites {
        if let Rendered::Definition(target) = site.resolved.rendered {
            if let Some(parent) = site.owner {
                incoming[parent].push(target)
            } else if client(project, &site.path) {
                candidates[target] = false;
                queue.push_back(target)
            }
        }
    }
    // Convention composition also renders a child, even though it has no
    // explicit JSX import site in the layout's source.
    let entries: BTreeMap<_, _> = graph
        .entries
        .iter()
        .filter_map(|entry| {
            if let Rendered::Definition(id) = entry.resolved.rendered {
                Some((&entry.path, id))
            } else {
                None
            }
        })
        .collect();
    for route in &project.nextjs().routes.routes {
        if route.kind != RouteKind::Page {
            continue;
        }
        let chain: Vec<_> = route
            .chain
            .iter()
            .filter(|f| matches!(f.convention, Convention::Layout | Convention::Template))
            .filter_map(|f| entries.get(&f.path).copied())
            .chain(entries.get(&route.file).copied())
            .collect();
        for pair in chain.windows(2) {
            conversion_work = conversion_work
                .checked_sub(1)
                .ok_or_else(|| super::limit("assessment dependency work limit reached"))?;
            incoming[pair[0]].push(pair[1]);
        }
    }
    let mapped: BTreeSet<_> = occurrences
        .iter()
        .filter(|o| {
            matches!(
                o.feature.id,
                "nextjs.api.link" | "nextjs.api.image" | "nextjs.api.script"
            )
        })
        .filter(|o| {
            matches!(
                assess::classify((*o).clone(), project, syntax).status,
                Some(Status::Auto | Status::Review)
            )
        })
        .filter_map(|o| {
            if let Location::Text { path, span } = &o.location {
                Some((path.clone(), *span))
            } else {
                None
            }
        })
        .collect();
    // Third-party, intrinsic-unknown, and unsupported render values keep the
    // containing definition in Preact. Intrinsic elements are already checked
    // against the printer while the strict tree was available.
    for site in &graph.sites {
        if let Some(parent) = site.owner
            && matches!(
                site.resolved.rendered,
                Rendered::External { .. } | Rendered::Unknown { .. }
            )
            && !mapped.contains(&(site.path.clone(), site.span))
            && candidates[parent]
        {
            candidates[parent] = false;
            queue.push_back(parent)
        }
    }
    while let Some(parent) = queue.pop_front() {
        for &target in &incoming[parent] {
            if candidates[target] {
                candidates[target] = false;
                queue.push_back(target)
            }
        }
    }
    let mut definition_names = BTreeMap::new();
    for definition in &graph.definitions {
        if let Some(name) = &definition.name {
            *definition_names
                .entry((&definition.path, name))
                .or_insert(0usize) += 1;
        }
    }
    let mut component_strategies = BTreeMap::new();
    for (i, definition) in graph.definitions.iter().enumerate() {
        let strategy = if client(project, &definition.path) {
            Strategy::Island
        } else if candidates[i] {
            Strategy::Astro
        } else if syntax
            .function(&definition.path, definition.span)
            .is_some_and(|f| !f.asynchronous)
            || matches!(definition.kind, frameshift_nextjs::DefinitionKind::Class)
        {
            Strategy::PreactStatic
        } else {
            Strategy::Preserve
        };
        component_strategies.insert((definition.path.clone(), definition.span), strategy);
        if main_files.contains(&definition.path) {
            continue;
        }
        // Anonymous identities use their original byte offset, not a counter.
        let name = definition
            .name
            .clone()
            .unwrap_or_else(|| format!("anonymous@{}", definition.span.start()));
        let name = if definition.name.as_ref().is_some_and(|n| {
            definition_names
                .get(&(&definition.path, n))
                .copied()
                .unwrap_or(0)
                > 1
        }) {
            format!("{name}@{}", definition.span.start())
        } else {
            name
        };
        add(
            &mut units,
            file_id(UnitKind::Component, &definition.path, Some(&name), limits)?,
            UnitKind::Component,
            Some(definition.path.clone()),
            vec![definition.span],
            client(project, &definition.path),
            strategy,
        );
    }
    // Definition intervals can nest. Assign every byte to the innermost
    // definition, and all remaining bytes to a single module unit.
    let mut per_file: BTreeMap<SourcePath, Vec<(usize, Span)>> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        if u.kind == UnitKind::Component {
            per_file
                .entry(u.path.clone().unwrap())
                .or_default()
                .push((i, u.ranges[0]));
        }
    }
    for (path, intervals) in &per_file {
        let Some(source) = read_text(snapshot, path, limits)? else {
            continue;
        };
        let mut events: BTreeMap<usize, Vec<(bool, usize, Span)>> = BTreeMap::new();
        events.entry(0).or_default();
        events.entry(source.len()).or_default();
        for &(i, span) in intervals {
            units[i].ranges.clear();
            events
                .entry(span.start())
                .or_default()
                .push((true, i, span));
            events.entry(span.end()).or_default().push((false, i, span));
        }
        let mut active: BTreeSet<(usize, usize, usize)> = BTreeSet::new();
        let mut previous = 0;
        let mut rest = Vec::new();
        for (offset, events) in events {
            if offset > previous {
                let range = Span::new(previous, offset).map_err(failure)?;
                if let Some(&(_, _, unit)) = active.first() {
                    units[unit].ranges.push(range)
                } else {
                    rest.push(range)
                }
            }
            for (start, unit, span) in events {
                let key = (span.len(), span.start(), unit);
                if start {
                    active.insert(key);
                } else {
                    active.remove(&key);
                }
            }
            previous = offset;
        }
        if !rest.is_empty() {
            add(
                &mut units,
                file_id(UnitKind::Module, path, None, limits)?,
                UnitKind::Module,
                Some(path.clone()),
                rest,
                client(project, path),
                Strategy::NotApplicable,
            );
        }
    }
    let mut files = Vec::new();
    let prefix = project
        .package()
        .map_or(String::new(), |p| format!("{}/", p.as_str()));
    for file in project.files() {
        match &file.state {
            FileState::Other(EntryKind::Directory) => continue,
            FileState::Other(kind) => {
                files.push(File {
                    path: file.path.clone(),
                    class: FileClass::Ignored,
                    units: Vec::new(),
                    reason: format!("{kind:?}"),
                });
                if !matches!(kind, EntryKind::Ignored { .. }) {
                    emit(
                        findings,
                        limits,
                        "FS3001",
                        Status::Unknown,
                        FindingLocation::File(file.path.clone()),
                        None,
                        None,
                        None,
                    )?;
                }
            }
            FileState::Admitted => {
                let local = file
                    .path
                    .as_str()
                    .strip_prefix(&prefix)
                    .unwrap_or(file.path.as_str());
                let existing = units.iter().any(|u| u.path.as_ref() == Some(&file.path));
                let class = if existing {
                    FileClass::Units
                } else if local.starts_with("public/") {
                    FileClass::Verbatim
                } else if out_of_scope(local) && !runtime_files.contains(&file.path) {
                    FileClass::OutOfScope
                } else {
                    match file.kind.unwrap_or(FileKind::Asset) {
                        FileKind::Stylesheet => {
                            add(
                                &mut units,
                                file_id(UnitKind::Stylesheet, &file.path, None, limits)?,
                                UnitKind::Stylesheet,
                                Some(file.path.clone()),
                                Vec::new(),
                                false,
                                Strategy::NotApplicable,
                            );
                            FileClass::Units
                        }
                        FileKind::Module | FileKind::Declaration => {
                            add(
                                &mut units,
                                file_id(UnitKind::Module, &file.path, None, limits)?,
                                UnitKind::Module,
                                Some(file.path.clone()),
                                Vec::new(),
                                client(project, &file.path),
                                Strategy::NotApplicable,
                            );
                            FileClass::Units
                        }
                        FileKind::Asset | FileKind::Json => FileClass::Verbatim,
                    }
                };
                files.push(File {
                    path: file.path.clone(),
                    class,
                    units: Vec::new(),
                    reason: match class {
                        FileClass::Units => "Owned by counting units.",
                        FileClass::Verbatim => {
                            "Public asset or non-code data; excluded from headline weights."
                        }
                        FileClass::OutOfScope => "Repository tooling or tests are outside V1.",
                        FileClass::Ignored => "Discovery exclusion.",
                    }
                    .into(),
                });
            }
        }
    }
    let owners = Ownership::new(&units);
    let mut assessed: Vec<_> = occurrences
        .into_iter()
        .map(|mut o| {
            if o.feature.id=="nextjs.file.route-handler-static" {
                let request=o.location.path().and_then(|p|syntax.modules.get(p)).and_then(|m|m.handler_request);
                if request==Some(true){o.feature=crate::catalog::feature("nextjs.file.route-handler").unwrap();o.status=o.feature.v1.fixed();}
                let mut assessed=assess::classify(o,project,syntax);
                if request.is_none(){assessed.status=Some(Status::Unknown);assessed.rule=None;assessed.reason=Reason::Missing("The GET export's implementation and request independence are not established.");}
                assessed
            }else{assess::classify(o, project, syntax)}
        })
        .collect();
    for entry in &graph.entries {
        if let Some(unit) = owners.owner(&units, &Location::File(entry.path.clone())) {
            units[unit].strategy = if let Rendered::Definition(id) = entry.resolved.rendered {
                let definition = &graph.definitions[id];
                component_strategies[&(definition.path.clone(), definition.span)]
            } else {
                Strategy::Preserve
            };
        }
    }
    for (key, strategy) in &component_strategies {
        if let Some(unit) = owners.owner(
            &units,
            &Location::Text {
                path: key.0.clone(),
                span: key.1,
            },
        ) && (!main_files.contains(&key.0) || *strategy == Strategy::Preserve)
        {
            // Astro entries can embed synchronous Preact helpers. A helper
            // without a mapping withholds the whole owning unit's strategy.
            units[unit].strategy = *strategy;
        }
        let id = match strategy {
            Strategy::Astro => Some("component.astro-convertible"),
            Strategy::PreactStatic => Some("component.preact-static"),
            Strategy::Island | Strategy::Preserve | Strategy::NotApplicable => None,
        };
        if let Some(id) = id {
            let feature = crate::catalog::feature(id).unwrap();
            assessed.push(assess::classify(
                Occurrence {
                    feature,
                    location: Location::Text {
                        path: key.0.clone(),
                        span: key.1,
                    },
                    status: feature.v1.fixed(),
                    detail: Detail::None,
                },
                project,
                syntax,
            ));
        }
    }
    for site in &graph.sites {
        if !project
            .nextjs()
            .contexts
            .module(&site.path)
            .is_some_and(|m| m.server)
        {
            continue;
        }
        let Rendered::Definition(target) = site.resolved.rendered else {
            continue;
        };
        let definition = &graph.definitions[target];
        if !project
            .nextjs()
            .contexts
            .module(&definition.path)
            .is_some_and(|m| m.use_client)
        {
            continue;
        }
        let location = Location::Text {
            path: site.path.clone(),
            span: site.span,
        };
        let Some(tag) = syntax.tag(&location) else {
            continue;
        };
        let mut ids = vec!["boundary.props"];
        if tag.jsx_props {
            ids.push("boundary.jsx-props")
        }
        if tag.dynamic_props || tag.spread {
            ids.push("boundary.dynamic-props")
        }
        if tag.promise_props {
            ids.push("boundary.promise-props")
        }
        for id in ids {
            let feature = crate::catalog::feature(id).unwrap();
            assessed.push(assess::classify(
                Occurrence {
                    feature,
                    location: location.clone(),
                    status: feature.v1.fixed(),
                    detail: Detail::None,
                },
                project,
                syntax,
            ));
        }
    }
    if assessed.len() > limits.analysis.max_occurrences {
        return Err(super::limit("analysis occurrence limit reached"));
    }
    let excluded_files: BTreeSet<_> = files
        .iter()
        .filter(|f| f.class == FileClass::OutOfScope)
        .map(|f| &f.path)
        .collect();
    for occurrence in &mut assessed {
        if occurrence
            .occurrence
            .location
            .path()
            .is_some_and(|p| excluded_files.contains(p))
        {
            occurrence.status = None;
            occurrence.rule = None;
            occurrence.reason = Reason::Proven(
                "Repository tooling and tests are outside the V1 application assessment.",
            );
        }
        if occurrence.status.is_none() {
            continue;
        }
        let id = occurrence.occurrence.feature.id;
        if id == "npm.manifest" && manifest_automatic {
            occurrence.status = Some(Status::Auto);
            occurrence.reason = Reason::Proven(
                "The manifest has no custom scripts; standard Next.js script commands have deterministic Astro mappings.",
            );
        }

        if id == "asset.public" {
            continue;
        }
        let is_config = id.starts_with("nextjs.config.")
            || matches!(
                id,
                "npm.manifest"
                    | "npm.lockfile"
                    | "npm.lockfile.bun-binary"
                    | "npm.workspace"
                    | "ts.config"
                    | "deploy.config"
                    | "css.tailwind"
                    | "nextjs.project.app-router"
                    | "nextjs.project.pages-router"
            );
        let index = if is_config {
            let concern = id;
            let identity = config_id(concern, limits)?;
            if let Some(index) = units.iter().position(|u| u.id == identity) {
                index
            } else {
                let i = units.len();
                add(
                    &mut units,
                    identity,
                    UnitKind::Config,
                    occurrence.occurrence.location.path().cloned(),
                    Vec::new(),
                    false,
                    Strategy::NotApplicable,
                );
                i
            }
        } else if let Some(index) = owners.owner(&units, &occurrence.occurrence.location) {
            index
        } else {
            let identity = config_id("source.analysis", limits)?;
            if let Some(index) = units.iter().position(|u| u.id == identity) {
                index
            } else {
                let i = units.len();
                add(
                    &mut units,
                    identity,
                    UnitKind::Config,
                    None,
                    Vec::new(),
                    false,
                    Strategy::NotApplicable,
                );
                i
            }
        };
        if id == "react.server-component"
            && let Location::Text { path, span } = &occurrence.occurrence.location
            && component_strategies.get(&(path.clone(), *span)) == Some(&Strategy::Preserve)
        {
            occurrence.status = Some(Status::Unsupported);
            occurrence.reason = Reason::Proven(
                "An async component demoted by the conversion fixed point has no static Preact equivalent.",
            );
        }
        let status = occurrence.status.unwrap();
        units[index].status = units[index].status.max(status);
        occurrence.unit = Some(units[index].id.as_str().into());
        if status != Status::Auto {
            let code = code(occurrence);
            emit(
                findings,
                limits,
                code,
                status,
                location(snapshot, &occurrence.occurrence.location, limits)?,
                Some(&units[index].id),
                Some(id),
                occurrence.rule.as_deref(),
            )?;
        }
    }
    // Every source unit needs established parsing/analysis, even when no
    // detector emitted an occurrence. Absence is never compatibility.
    for unit in &mut units {
        if matches!(
            unit.kind,
            UnitKind::Module
                | UnitKind::Component
                | UnitKind::Route
                | UnitKind::Endpoint
                | UnitKind::Layout
                | UnitKind::RouteFile
        ) && let Some(path) = &unit.path
            && !project
                .modules()
                .node(path)
                .is_some_and(|n| n.state == ModuleState::Parsed)
            && !matches!(frameshift_modules::FileKind::of(path), FileKind::Asset)
        {
            unit.status = unit.status.max(Status::Unknown);
            emit(
                findings,
                limits,
                "FS3001",
                Status::Unknown,
                FindingLocation::File(path.clone()),
                Some(&unit.id),
                None,
                None,
            )?;
        }
        if unit.strategy == Strategy::Preserve {
            unit.status = unit.status.max(Status::Unknown);
            emit(
                findings,
                limits,
                "FS3001",
                Status::Unknown,
                unit.path.as_ref().map_or(FindingLocation::Project, |p| {
                    FindingLocation::File(p.clone())
                }),
                Some(&unit.id),
                None,
                None,
            )?;
        }
    }
    for (key, strategy) in &component_strategies {
        if *strategy == Strategy::Astro
            && syntax
                .function(&key.0, key.1)
                .is_some_and(|f| f.assumptions)
            && let Some(i) = owners.owner(
                &units,
                &Location::Text {
                    path: key.0.clone(),
                    span: key.1,
                },
            )
        {
            units[i].status = units[i].status.max(Status::Review);
            emit(
                findings,
                limits,
                "FS3501",
                Status::Review,
                location(
                    snapshot,
                    &Location::Text {
                        path: key.0.clone(),
                        span: key.1,
                    },
                    limits,
                )?,
                Some(&units[i].id),
                None,
                None,
            )?;
        }
    }
    for dependency in dependencies {
        if !matches!(
            dependency.class,
            Compatibility::Framework | Compatibility::Compatible | Compatibility::Compat
        ) {
            let status = match dependency.class {
                Compatibility::Incompatible => Status::Unsupported,
                Compatibility::CompatUnverified => Status::Review,
                Compatibility::Unknown => Status::Unknown,
                Compatibility::Framework | Compatibility::Compatible | Compatibility::Compat => {
                    continue;
                }
            };
            for path in &dependency.used_in {
                for unit in units.iter_mut().filter(|u| u.path.as_ref() == Some(path)) {
                    unit.status = unit.status.max(status);
                    emit(
                        findings,
                        limits,
                        if status == Status::Unsupported {
                            "FS3301"
                        } else {
                            "FS3302"
                        },
                        status,
                        FindingLocation::File(path.clone()),
                        Some(&unit.id),
                        None,
                        None,
                    )?;
                }
            }
        }
    }
    for module in &project.environment().modules {
        for api in &module.apis {
            let uncertain = match api.kind {
                ApiKind::BrowserGlobal => module.build && !api.deferred && !api.typeof_guard,
                ApiKind::NodeGlobal | ApiKind::NodeModule => module.browser && !api.typeof_guard,
            };
            if uncertain
                && let Some(i) = owners.owner(
                    &units,
                    &Location::Text {
                        path: module.path.clone(),
                        span: api.span,
                    },
                )
            {
                units[i].status = units[i].status.max(Status::Unknown);
                emit(
                    findings,
                    limits,
                    "FS3001",
                    Status::Unknown,
                    location(
                        snapshot,
                        &Location::Text {
                            path: module.path.clone(),
                            span: api.span,
                        },
                        limits,
                    )?,
                    Some(&units[i].id),
                    None,
                    None,
                )?;
            }
        }
    }
    let version_ok = match &detection.version {
        VersionEvidence::Locked(v) | VersionEvidence::LowerBound(v) => matches!(v.major(), 14..=16),
        VersionEvidence::Unknown => false,
    };
    if !version_ok
        || matches!(
            project.configuration().page_extensions(),
            PageExtensions::Unknown | PageExtensions::Invalid
        )
        || snapshot.inventory().unrepresentable() > 0
    {
        let id = config_id("source.analysis", limits)?;
        let index = if let Some(i) = units.iter().position(|u| u.id == id) {
            i
        } else {
            let i = units.len();
            add(
                &mut units,
                id,
                UnitKind::Config,
                None,
                Vec::new(),
                false,
                Strategy::NotApplicable,
            );
            i
        };
        units[index].status = Status::Unknown;
        emit(
            findings,
            limits,
            if version_ok { "FS3001" } else { "FS3502" },
            Status::Unknown,
            FindingLocation::Project,
            Some(&units[index].id),
            None,
            None,
        )?;
    }
    if units.len() > limits.analysis.max_units {
        return Err(super::limit("analysis unit limit reached"));
    }
    units.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    if units.windows(2).any(|p| p[0].id == p[1].id) {
        return Err(super::internal("FS9001: duplicate counting identity"));
    }
    for unit in &mut units {
        if matches!(unit.status, Status::Unknown | Status::Unsupported)
            && unit.strategy != Strategy::NotApplicable
        {
            unit.strategy = Strategy::Preserve
        }
    }
    // A configuration concern can have evidence in several inherited files.
    let mut file_units: BTreeMap<SourcePath, BTreeSet<String>> = BTreeMap::new();
    for unit in &units {
        if let Some(path) = &unit.path {
            file_units
                .entry(path.clone())
                .or_default()
                .insert(unit.id.as_str().to_owned());
        }
    }
    for occurrence in &assessed {
        if let (Some(unit), Some(path)) = (&occurrence.unit, occurrence.occurrence.location.path())
        {
            file_units
                .entry(path.clone())
                .or_default()
                .insert(unit.clone());
        }
    }
    for file in &mut files {
        file.units = file_units
            .remove(&file.path)
            .unwrap_or_default()
            .into_iter()
            .collect();
        if !file.units.is_empty() {
            file.class = FileClass::Units;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut routes = Vec::new();
    let mut work = conversion_work;
    for (index, route) in project.nextjs().routes.routes.iter().enumerate() {
        let dependencies = project.route_dependencies(index).map_err(failure)?;
        work = work
            .checked_sub(dependencies.files.len() as u64 + dependencies.edges.len() as u64)
            .ok_or_else(|| super::limit("assessment dependency work limit reached"))?;
        let files: BTreeSet<_> = dependencies.files.into_iter().collect();
        let kind = if route.kind == RouteKind::Page {
            UnitKind::Route
        } else {
            UnitKind::Endpoint
        };
        let route_id =
            UnitId::for_route(kind, &route.pattern, limits.identifier).map_err(failure)?;
        let closure: Vec<_> = units
            .iter()
            .filter(|u| {
                u.path.as_ref().is_some_and(|p| files.contains(p))
                    || u.kind == UnitKind::Config
                    || u.id == route_id
            })
            .collect();
        let status = closure.iter().fold(Status::Auto, |s, u| s.max(u.status));
        let witness = closure.iter().find(|u| {
            u.status == status && matches!(status, Status::Unknown | Status::Unsupported)
        });
        let path = witness
            .and_then(|u| u.path.as_ref())
            .map_or(Vec::new(), |target| witness_path(project, route, target));
        let kind = if route.kind == RouteKind::Page {
            UnitKind::Route
        } else {
            UnitKind::Endpoint
        };
        routes.push(Route {
            pattern: route.pattern.clone(),
            unit: UnitId::for_route(kind, &route.pattern, limits.identifier)
                .map_err(failure)?
                .as_str()
                .into(),
            status,
            closure: closure.iter().map(|u| u.id.as_str().into()).collect(),
            blocker: witness.map(|u| u.id.as_str().into()),
            dependency_path: path,
        });
    }
    let by_id: BTreeMap<_, _> = units.iter().map(|u| (u.id.as_str(), u)).collect();
    let mut unique: BTreeMap<String, Route> = BTreeMap::new();
    for route in routes {
        if let Some(existing) = unique.get_mut(&route.unit) {
            let mut closure: BTreeSet<_> = existing.closure.iter().cloned().collect();
            closure.extend(route.closure);
            existing.closure = closure.into_iter().collect();
            existing.status = existing.status.max(route.status);
            let witness = existing
                .closure
                .iter()
                .find(|id| {
                    by_id[id.as_str()].status == existing.status
                        && matches!(existing.status, Status::Unknown | Status::Unsupported)
                })
                .cloned();
            if witness == route.blocker {
                existing.dependency_path = route.dependency_path;
            }
            existing.blocker = witness;
        } else {
            unique.insert(route.unit.clone(), route);
        }
    }
    let mut routes: Vec<_> = unique.into_values().collect();
    routes.sort_by(|a, b| (&a.pattern, &a.unit).cmp(&(&b.pattern, &b.unit)));
    Ok(Assessment {
        units,
        occurrences: assessed,
        files,
        routes,
        unrepresentable: snapshot.inventory().unrepresentable(),
    })
}
fn runtime_files(project: &Project, work: &mut u64) -> Result<BTreeSet<SourcePath>, AnalyzeError> {
    let mut pending: BTreeSet<_> = project
        .nextjs()
        .routes
        .routes
        .iter()
        .flat_map(|r| std::iter::once(&r.file).chain(r.chain.iter().map(|f| &f.path)))
        .collect();
    let mut reached = BTreeSet::new();
    while let Some(path) = pending.pop_first() {
        if reached.contains(path) {
            continue;
        }
        *work = work
            .checked_sub(1)
            .ok_or_else(|| super::limit("assessment dependency work limit reached"))?;
        reached.insert(path.clone());
        if let Some(node) = project.nextjs().graph.node(path)
            && node.kind != FileKind::Declaration
        {
            for edge in &node.edges {
                *work = work
                    .checked_sub(1)
                    .ok_or_else(|| super::limit("assessment dependency work limit reached"))?;
                if !edge.type_only
                    && let Resolution::Local { path, .. } = &edge.resolution
                    && !reached.contains(path)
                {
                    pending.insert(path);
                }
            }
        }
    }
    Ok(reached)
}
fn out_of_scope(path: &str) -> bool {
    path.starts_with(".github/")
        || path.starts_with(".vscode/")
        || path.starts_with("tests/")
        || path.starts_with("test/")
        || path.contains("/__tests__/")
        || path.contains(".test.")
        || path.contains(".spec.")
        || [
            "eslint",
            "jest",
            "vitest",
            "playwright",
            "cypress",
            "prettier",
        ]
        .iter()
        .any(|p| path.starts_with(p) || path.starts_with(&format!(".{p}")))
}
fn code(o: &AssessedOccurrence) -> &'static str {
    match o.occurrence.feature.id {
        "nextjs.api.request" => {
            if let Detail::Api { export, .. } = &o.occurrence.detail {
                match export.as_str() {
                    "cookies" => "FS3101",
                    "headers" => "FS3102",
                    _ => "FS3100",
                }
            } else {
                "FS3100"
            }
        }
        "nextjs.api.server-action" => "FS3120",
        "nextjs.route.dynamic-unenumerable" => "FS3110",
        "nextjs.file.proxy" => "FS3130",
        "nextjs.navigation.client" => "FS3210",
        "nextjs.config.unknown" => "FS3003",
        _ => match o.status {
            Some(Status::Unsupported) => "FS3100",
            Some(Status::Review) => "FS3500",
            Some(Status::Auto | Status::Unknown) | None => "FS3001",
        },
    }
}
fn read_text(
    snapshot: &Snapshot,
    path: &SourcePath,
    limits: &Limits,
) -> Result<Option<SourceText>, AnalyzeError> {
    let Some(id) = snapshot.inventory().files().find(path) else {
        return Ok(None);
    };
    match snapshot.read(id).map_err(failure)? {
        FileRead::Content(file) => {
            Ok(SourceText::from_bytes(file.bytes().to_vec(), limits.text).ok())
        }
        FileRead::Unreadable(_) => Ok(None),
    }
}
fn location(
    snapshot: &Snapshot,
    at: &Location,
    limits: &Limits,
) -> Result<FindingLocation, AnalyzeError> {
    Ok(match at {
        Location::Project => FindingLocation::Project,
        Location::File(p) => FindingLocation::File(p.clone()),
        Location::Text { path, span } => match read_text(snapshot, path, limits)? {
            Some(source) => FindingLocation::Text(
                TextLocation::new(path.clone(), &source, *span).map_err(failure)?,
            ),
            None => FindingLocation::File(path.clone()),
        },
    })
}
#[allow(clippy::too_many_arguments)]
fn emit(
    findings: &mut FindingCollector,
    limits: &Limits,
    code: &str,
    status: Status,
    at: FindingLocation,
    unit: Option<&UnitId>,
    feature: Option<&str>,
    rule: Option<&str>,
) -> Result<(), AnalyzeError> {
    let (message, explanation, remediation) =
        knowledge::explain(code).ok_or_else(|| failure("FS9001: missing finding documentation"))?;
    let severity = match status {
        Status::Auto => Severity::Info,
        Status::Review => Severity::Review,
        Status::Unknown => Severity::Unknown,
        Status::Unsupported => Severity::Blocker,
    };
    let context = FindingContext {
        unit: unit.cloned(),
        feature: feature
            .map(|s| FeatureId::new(s, limits.identifier))
            .transpose()
            .map_err(failure)?,
        rule: rule
            .map(|s| RuleId::new(s, limits.identifier))
            .transpose()
            .map_err(failure)?,
    };
    findings
        .push(
            Finding::new(
                FindingCode::new(code).map_err(failure)?,
                severity,
                at,
                context,
                FindingText {
                    message,
                    explanation,
                    remediation,
                },
                limits.finding,
            )
            .map_err(failure)?,
        )
        .map_err(failure)
}
fn witness_path(
    project: &Project,
    route: &frameshift_nextjs::AppRoute,
    target: &SourcePath,
) -> Vec<SourcePath> {
    let mut queue = VecDeque::new();
    let mut previous: BTreeMap<SourcePath, Option<SourcePath>> = BTreeMap::new();
    for root in std::iter::once(&route.file).chain(route.chain.iter().map(|f| &f.path)) {
        previous.entry(root.clone()).or_insert(None);
        queue.push_back(root.clone());
    }
    while let Some(path) = queue.pop_front() {
        if &path == target {
            let mut result = vec![path.clone()];
            let mut current = path;
            while let Some(Some(parent)) = previous.get(&current) {
                result.push(parent.clone());
                current = parent.clone();
            }
            result.reverse();
            return result;
        }
        if let Some(node) = project.modules().node(&path) {
            for edge in &node.edges {
                if !edge.type_only
                    && let Resolution::Local { path: next, .. } = &edge.resolution
                    && !previous.contains_key(next)
                {
                    previous.insert(next.clone(), Some(path.clone()));
                    queue.push_back(next.clone())
                }
            }
        }
    }
    Vec::new()
}
