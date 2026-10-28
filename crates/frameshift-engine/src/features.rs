//! Feature occurrences of the V1 catalog, detected from source-model facts.
//!
//! Detection maps the facts that the project model establishes to catalog
//! features (`D-099`). It reads nothing beyond the model, the snapshot
//! inventory, and Next.js detection, evaluates nothing, and executes nothing.
//! An occurrence carries the status of its feature only when the matrix
//! allows exactly one; every other choice belongs to later assessment rules.
//! Constructs that no entry describes, or that analysis cannot establish,
//! are occurrences of [`catalog::UNKNOWN`] with their cause, so nothing
//! that detection sees disappears silently. Occurrences are not yet
//! assigned to units, and their absence never establishes compatibility:
//! the matrix entries that detection does not yet cover are listed in
//! IMPLEMENTATION.md section 50.

use std::collections::BTreeSet;

use frameshift_bindings::{ImportedName, Meaning, Target};
use frameshift_catalog::{self as catalog, Feature, Status};
use frameshift_core::{SourcePath, Span};
use frameshift_evaluate::{Object, Value};
use frameshift_input::Snapshot;
use frameshift_modules::{FileKind, ModuleState, Resolution, UnresolvedReason};
use frameshift_nextjs::{
    ContextIssue, Convention, Detection, FacetKind, FrameworkModuleFacts, ImportedUse,
    ImportedUseKind, PageExtensions, Rendered, RouteIssue, RouteKind, Segment, SegmentKind,
    UnknownReason, VariableAccess,
};

use crate::project::{FileState, Project, inside};

/// Where an occurrence is, in original root coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// The project as a whole.
    Project,
    /// A file or directory without a text range.
    File(SourcePath),
    /// A text range of a file.
    Text {
        /// The file.
        path: SourcePath,
        /// The exact source range.
        span: Span,
    },
}

impl Location {
    /// The file, absent for project scope.
    pub fn path(&self) -> Option<&SourcePath> {
        match self {
            Self::Project => None,
            Self::File(path) | Self::Text { path, .. } => Some(path),
        }
    }

    // Project scope first, then path bytes, a file before its ranges, and
    // ranges by offsets, as findings are ordered (`D-057`).
    fn key(&self) -> (bool, &str, Option<Span>) {
        match self {
            Self::Project => (false, "", None),
            Self::File(path) => (true, path.as_str(), None),
            Self::Text { path, span } => (true, path.as_str(), Some(*span)),
        }
    }
}

/// Why a construct is an occurrence of [`catalog::UNKNOWN`] or of
/// `nextjs.config.unknown`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// A module without a strict parse; its constructs are not analyzed.
    Unparsed,
    /// A runtime reference without an established target.
    Unresolved(UnresolvedReason),
    /// A render site whose value is not established.
    Render(UnknownReason),
    /// An export of `next`, `react`, or `react-dom` that the catalog does
    /// not list, or a namespace used without a literal member.
    Api {
        /// The import specifier.
        specifier: String,
        /// The export, absent when no literal name selects one.
        export: Option<String>,
    },
    /// A configuration property that is not recognized, not evaluated,
    /// or whose value the mapping needs but the evaluator cannot establish.
    /// `None` stands for the configuration or an unknown property set.
    Configuration {
        /// The dotted property path.
        key: Option<String>,
    },
    /// A route segment option that the catalog does not list.
    SegmentOption {
        /// The export name.
        name: String,
    },
    /// A `dynamic` segment option other than a literal `force-dynamic`.
    SegmentValue,
    /// Whether a parameter segment is enumerated is not established: a
    /// candidate file has `export *` or no strict parse.
    Enumeration,
    /// A whole-object or computed `process.env` use, which may read any variable.
    Environment(VariableAccess),
    /// A convention file that the catalog does not list.
    Convention(Convention),
    /// Route tree evidence that Next.js rejects or Frameshift cannot see.
    Route(RouteIssue),
    /// Render-context evidence that contradicts Next.js's directives.
    Context(ContextIssue),
}

/// What an occurrence is about, beyond its feature and location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    /// Nothing further.
    None,
    /// A route, by its URL pattern.
    Route {
        /// The original pattern.
        pattern: String,
    },
    /// One segment of a route.
    Segment {
        /// The route's pattern.
        pattern: String,
        /// The segment's directory name.
        name: String,
    },
    /// A configuration property, by its dotted path.
    Configuration {
        /// The property path.
        key: String,
    },
    /// An imported framework API.
    Api {
        /// The import specifier.
        specifier: String,
        /// The selected export.
        export: String,
    },
    /// A convention export.
    Export {
        /// The export name.
        name: String,
    },
    /// A metadata field outside the supported subset.
    Field {
        /// `metadata` or `viewport`.
        export: String,
        /// The dotted field path; empty when the known value is not an
        /// object, so no field of it is supported.
        path: String,
    },
    /// An environment variable, by its decoded name.
    Variable {
        /// The name.
        name: String,
    },
    /// The HTTP methods that a route handler exports, in source order.
    Methods {
        /// The method names.
        methods: Vec<String>,
    },
    /// A component definition, by its static name.
    Component {
        /// The ECMAScript `name`, absent when it is not static.
        name: Option<String>,
    },
    /// A construct that is not recognized or not analyzable.
    Unknown(Cause),
}

/// One detected instance of a catalog feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// The catalog entry.
    pub feature: &'static Feature,
    /// Where it occurs.
    pub location: Location,
    /// The feature's status when the matrix allows exactly one; `None`
    /// awaits an assessment rule, or the feature is out of scope.
    pub status: Option<Status>,
    /// What it is about.
    pub detail: Detail,
}

/// Detects every catalog feature that the model's facts establish, in
/// location order. `detection` must describe the model's package; it
/// supplies the Pages Router evidence that the App Router model lacks.
pub fn detect(snapshot: &Snapshot, project: &Project, detection: &Detection) -> Vec<Occurrence> {
    let mut out = Out(Vec::new());
    routers(&mut out, project, detection);
    configuration(&mut out, project);
    files(&mut out, snapshot, project);
    routes(&mut out, project);
    conventions(&mut out, project);
    modules(&mut out, project);
    contexts(&mut out, project);
    components(&mut out, project);
    environment(&mut out, project);
    graph(&mut out, project);
    let mut occurrences = out.0;
    // Stable, so equal keys keep the deterministic detection order.
    occurrences.sort_by(|a, b| {
        a.location
            .key()
            .cmp(&b.location.key())
            .then_with(|| a.feature.id.cmp(b.feature.id))
    });
    occurrences
}

struct Out(Vec<Occurrence>);

impl Out {
    fn push(&mut self, id: &str, location: Location, detail: Detail) {
        let feature = catalog::feature(id).expect("detected features are catalog entries");
        self.0.push(Occurrence {
            feature,
            location,
            status: feature.v1.fixed(),
            detail,
        });
    }

    fn unknown(&mut self, location: Location, cause: Cause) {
        self.push(catalog::UNKNOWN, location, Detail::Unknown(cause));
    }
}

fn text(path: &SourcePath, span: Span) -> Location {
    Location::Text {
        path: path.clone(),
        span,
    }
}

fn routers(out: &mut Out, project: &Project, detection: &Detection) {
    let routes = &project.nextjs().routes;
    if !routes.routes.is_empty() {
        out.push(
            "nextjs.project.app-router",
            Location::File(routes.app.clone()),
            Detail::None,
        );
    }
    if let Some(pages) = detection.pages.as_ref().filter(|pages| pages.routes() > 0) {
        out.push(
            "nextjs.project.pages-router",
            Location::File(pages.path.clone()),
            Detail::None,
        );
    }
}

fn configuration(out: &mut Out, project: &Project) {
    let facts = project.configuration();
    // No file: detection established that no configuration exists.
    let Some(file) = &facts.file else {
        return;
    };
    let at = || Location::File(file.path.clone());
    let unknown = |out: &mut Out, key: Option<String>| {
        out.push(
            "nextjs.config.unknown",
            at(),
            Detail::Unknown(Cause::Configuration { key }),
        )
    };
    let object = match &facts.value {
        Some(Value::Object(object)) => object,
        None | Some(_) => return unknown(out, None),
    };
    for _ in &object.unknown_properties {
        unknown(out, None);
    }
    for (key, value) in &object.entries {
        let member = |out: &mut Out, id: &str| {
            out.push(id, at(), Detail::Configuration { key: key.clone() })
        };
        let known = |out: &mut Out, id: &str| {
            if value.is_known() {
                member(out, id);
            } else {
                unknown(out, Some(key.clone()));
            }
        };
        match key.as_str() {
            "output" if *value == Value::String("export".into()) => {
                member(out, "nextjs.config.output")
            }
            "trailingSlash" if matches!(value, Value::Bool(_)) => {
                member(out, "nextjs.config.trailing-slash")
            }
            "basePath" if matches!(value, Value::String(_)) => {
                member(out, "nextjs.config.base-path")
            }
            "env" => known(out, "nextjs.config.env"),
            "redirects" => known(out, "nextjs.config.redirects"),
            "transpilePackages" => known(out, "nextjs.config.transpile-packages"),
            "rewrites" => member(out, "nextjs.config.rewrites"),
            "headers" => member(out, "nextjs.config.headers"),
            "reactStrictMode" => member(out, "nextjs.config.react-strict-mode"),
            "poweredByHeader" => member(out, "nextjs.config.powered-by-header"),
            "compress" => member(out, "nextjs.config.compress"),
            "typescript" => member(out, "nextjs.config.typescript"),
            "eslint" => member(out, "nextjs.config.eslint"),
            "webpack" => member(out, "nextjs.config.webpack"),
            "turbopack" => member(out, "nextjs.config.turbopack"),
            "i18n" => member(out, "nextjs.config.i18n"),
            "pageExtensions"
                if matches!(facts.page_extensions(), PageExtensions::Known(list)
                    if list.iter().any(|extension| extension == "mdx")) =>
            {
                member(out, "nextjs.config.page-extensions")
            }
            "images" => nested(out, file, key, value, |name, value| match name {
                "unoptimized" if matches!(value, Value::Bool(_)) => {
                    Some("nextjs.config.images.unoptimized")
                }
                "remotePatterns" if value.is_known() => {
                    Some("nextjs.config.images.remote-patterns")
                }
                "domains" if value.is_known() => Some("nextjs.config.images.domains"),
                "loader" | "loaderFile" => Some("nextjs.config.images.loader"),
                _ => None,
            }),
            "experimental" => nested(out, file, key, value, |name, _| {
                (name == "turbo").then_some("nextjs.config.turbopack")
            }),
            _ => unknown(out, Some(key.clone())),
        }
    }
}

/// Maps the properties of a nested configuration object; every property that
/// `map` does not recognize, and an unknown or non-object value, is unknown.
fn nested(
    out: &mut Out,
    file: &frameshift_nextjs::ConfigFile,
    key: &str,
    value: &Value,
    map: impl Fn(&str, &Value) -> Option<&'static str>,
) {
    let at = || Location::File(file.path.clone());
    let unknown = |out: &mut Out, key: String| {
        out.push(
            "nextjs.config.unknown",
            at(),
            Detail::Unknown(Cause::Configuration { key: Some(key) }),
        )
    };
    let Value::Object(Object {
        entries,
        unknown_properties,
        ..
    }) = value
    else {
        return unknown(out, key.to_owned());
    };
    for _ in unknown_properties {
        unknown(out, key.to_owned());
    }
    for (name, value) in entries {
        let path = format!("{key}.{name}");
        match map(name, value) {
            Some(id) => out.push(id, at(), Detail::Configuration { key: path }),
            None => unknown(out, path),
        }
    }
}

fn files(out: &mut Out, snapshot: &Snapshot, project: &Project) {
    let package = project.package();
    let prefix = package.map_or(String::new(), |p| format!("{}/", p.as_str()));
    if let Some(package) = package {
        out.push(
            "npm.workspace",
            Location::File(package.clone()),
            Detail::None,
        );
    }
    // Inherited files of other workspace members are configuration input
    // of this package, not files of it (`D-098`).
    for path in project.modules().configuration().files() {
        if inside(path, package) {
            out.push("ts.config", Location::File(path.clone()), Detail::None);
        }
    }
    // Lockfiles and CI workflows live at the run root, which is the
    // workspace root (`D-073`), never above it.
    for path in snapshot.inventory().files().as_slice() {
        let name = path.as_str();
        match name {
            "package-lock.json"
            | "npm-shrinkwrap.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lock" => out.push("npm.lockfile", Location::File(path.clone()), Detail::None),
            "bun.lockb" => out.push(
                "npm.lockfile.bun-binary",
                Location::File(path.clone()),
                Detail::None,
            ),
            _ if name.starts_with(".github/workflows/") => out.push(
                "tooling.workflow",
                Location::File(path.clone()),
                Detail::None,
            ),
            _ => {}
        }
    }
    let app = &project.nextjs().routes.app;
    let mut private = BTreeSet::new();
    for file in project.files() {
        if file.state != FileState::Admitted {
            continue;
        }
        let path = file.path.as_str();
        let Some(local) = path.strip_prefix(prefix.as_str()) else {
            continue;
        };
        let at = || Location::File(file.path.clone());
        let config = local.rsplit_once('.').is_some_and(|(_, extension)| {
            matches!(extension, "js" | "cjs" | "mjs" | "ts" | "cts" | "mts")
        }) && !local.contains('/');
        if local == "package.json" {
            out.push("npm.manifest", at(), Detail::None);
        } else if local == "vercel.json" || local == "netlify.toml" {
            out.push("deploy.config", at(), Detail::None);
        } else if config && local.starts_with("tailwind.config.") {
            out.push("css.tailwind", at(), Detail::None);
        } else if config
            && ["jest", "vitest", "playwright", "cypress"]
                .iter()
                .any(|tool| local.starts_with(&format!("{tool}.config.")))
        {
            out.push("test.setup", at(), Detail::None);
        }
        if local.starts_with("public/") {
            out.push("asset.public", at(), Detail::None);
        } else if file.kind == Some(FileKind::Stylesheet) {
            let name = local.rsplit('/').next().unwrap_or(local);
            let module = [".module.css", ".module.scss", ".module.sass"]
                .iter()
                .any(|suffix| name.ends_with(suffix));
            if module {
                out.push("css.module", at(), Detail::None);
            }
            if name.ends_with(".scss") || name.ends_with(".sass") {
                out.push("css.sass", at(), Detail::None);
            } else if !module {
                out.push("css.global", at(), Detail::None);
            }
        }
        // The shallowest private component below the app directory; Next.js
        // excludes it and everything below from routing.
        if let Some(rest) = path
            .strip_prefix(app.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
        {
            let mut directory = app.as_str().to_owned();
            for component in rest.split('/').take(rest.split('/').count() - 1) {
                directory.push('/');
                directory.push_str(component);
                if component.starts_with('_') {
                    private.insert(directory);
                    break;
                }
            }
        }
    }
    for directory in private {
        let path = SourcePath::new(&directory, frameshift_core::PathLimits::default())
            .expect("a prefix of an admitted path is a valid path");
        out.push("nextjs.route.private", Location::File(path), Detail::None);
    }
}

fn facts<'a>(project: &'a Project, path: &SourcePath) -> Option<&'a FrameworkModuleFacts> {
    let modules = project.framework();
    modules
        .binary_search_by(|module| module.path.cmp(path))
        .ok()
        .map(|index| &modules[index])
}

fn convention(project: &Project, path: &SourcePath) -> Option<Convention> {
    let files = &project.nextjs().routes.files;
    files
        .binary_search_by(|file| file.path.cmp(path))
        .ok()
        .map(|index| files[index].convention)
}

fn routes(out: &mut Out, project: &Project) {
    for route in &project.nextjs().routes.routes {
        let at = || Location::File(route.file.clone());
        let pattern = || route.pattern.clone();
        match route.kind {
            RouteKind::Metadata(_) => continue,
            RouteKind::Page => {
                if !route.segments.iter().any(parameter) {
                    out.push(
                        "nextjs.route.page",
                        at(),
                        Detail::Route { pattern: pattern() },
                    );
                }
            }
            RouteKind::Handler => match facts(project, &route.file) {
                Some(module) if module.star_exports.is_empty() => {
                    let methods: Vec<String> = module
                        .exports
                        .iter()
                        .filter(|export| export.kind == FacetKind::Handler)
                        .map(|export| export_name(&export.name))
                        .collect();
                    let id = if methods == ["GET"] {
                        "nextjs.file.route-handler-static"
                    } else {
                        "nextjs.file.route-handler"
                    };
                    out.push(id, at(), Detail::Methods { methods });
                }
                // Without a strict parse the methods are unknown; the
                // graph reports the file itself.
                Some(_) => out.unknown(at(), Cause::Enumeration),
                None => {}
            },
        }
        if route
            .segments
            .iter()
            .any(|segment| segment.kind == SegmentKind::Static)
        {
            out.push(
                "nextjs.route.segment",
                at(),
                Detail::Route { pattern: pattern() },
            );
        }
        for (index, segment) in route.segments.iter().enumerate() {
            let detail = || Detail::Segment {
                pattern: pattern(),
                name: segment.name.clone(),
            };
            if segment.interception.is_some() {
                out.push("nextjs.route.intercepting", at(), detail());
            }
            let id = match &segment.kind {
                SegmentKind::Static => continue,
                SegmentKind::Group => "nextjs.route.group",
                SegmentKind::Parallel => "nextjs.route.parallel",
                SegmentKind::Dynamic(_) => "nextjs.route.dynamic",
                SegmentKind::CatchAll(_) => "nextjs.route.catch-all",
                SegmentKind::OptionalCatchAll(_) => "nextjs.route.optional-catch-all",
            };
            let enumerated = parameter(segment).then(|| enumerated(project, route, index));
            match enumerated {
                Some(Some(false)) if id == "nextjs.route.dynamic" => {
                    out.push("nextjs.route.dynamic-unenumerable", at(), detail())
                }
                Some(Some(false)) => {
                    out.push(id, at(), detail());
                    out.push("nextjs.route.dynamic-unenumerable", at(), detail());
                }
                Some(None) => {
                    out.push(id, at(), detail());
                    out.unknown(at(), Cause::Enumeration);
                }
                Some(Some(true)) | None => out.push(id, at(), detail()),
            }
        }
    }
}

fn parameter(segment: &Segment) -> bool {
    matches!(
        segment.kind,
        SegmentKind::Dynamic(_) | SegmentKind::CatchAll(_) | SegmentKind::OptionalCatchAll(_)
    )
}

/// Whether a page or a layout at or below the segment exports
/// `generateStaticParams`, which can generate that segment's values.
/// `None` when a candidate without it has `export *` or no strict parse.
fn enumerated(
    project: &Project,
    route: &frameshift_nextjs::AppRoute,
    index: usize,
) -> Option<bool> {
    let candidates = std::iter::once(&route.file).chain(
        route
            .chain
            .iter()
            .filter(|file| file.convention == Convention::Layout && file.segments.len() > index)
            .map(|file| &file.path),
    );
    let mut certain = true;
    for path in candidates {
        match facts(project, path) {
            Some(module) => {
                if module.exports.iter().any(|export| {
                    export.kind == FacetKind::Generator
                        && export_name(&export.name) == "generateStaticParams"
                }) {
                    return Some(true);
                }
                certain &= module.star_exports.is_empty();
            }
            None => certain = false,
        }
    }
    certain.then_some(false)
}

fn export_name(name: &frameshift_bindings::ExportName) -> String {
    match name {
        frameshift_bindings::ExportName::Named(name) => name.clone(),
        frameshift_bindings::ExportName::Default => "default".to_owned(),
        frameshift_bindings::ExportName::Star => "*".to_owned(),
        frameshift_bindings::ExportName::Assignment => "=".to_owned(),
    }
}

fn conventions(out: &mut Out, project: &Project) {
    let routes = &project.nextjs().routes;
    for file in &routes.files {
        let at = || Location::File(file.path.clone());
        let id = match file.convention {
            Convention::Page | Convention::Route => continue,
            Convention::Layout => "nextjs.file.layout",
            Convention::Template => "nextjs.file.template",
            Convention::Loading => "nextjs.file.loading",
            Convention::Error | Convention::GlobalError => "nextjs.file.error",
            Convention::NotFound => "nextjs.file.not-found",
            Convention::Middleware | Convention::Proxy => "nextjs.file.proxy",
            Convention::Default => "nextjs.route.parallel",
            Convention::Metadata(_) => {
                if FileKind::of(&file.path) == FileKind::Module {
                    "nextjs.metadata.route"
                } else {
                    "nextjs.metadata.file"
                }
            }
            Convention::Forbidden
            | Convention::Unauthorized
            | Convention::GlobalNotFound
            | Convention::Instrumentation => {
                out.unknown(at(), Cause::Convention(file.convention));
                continue;
            }
        };
        out.push(id, at(), Detail::None);
    }
    for issue in &routes.issues {
        let path = match issue {
            RouteIssue::PageAndRoute { directory } => Some(directory),
            RouteIssue::DuplicatePattern { files, .. }
            | RouteIssue::AmbiguousConvention { files } => files.first(),
            RouteIssue::MissingRootLayout { page } => Some(page),
            RouteIssue::Incomplete { path } => Some(path),
        };
        let location = path.map_or(Location::Project, |path| Location::File(path.clone()));
        out.unknown(location, Cause::Route(issue.clone()));
    }
}

fn modules(out: &mut Out, project: &Project) {
    let contexts = &project.nextjs().contexts;
    let browser = &project.components().browser_imports;
    for module in project.framework() {
        let convention = convention(project, &module.path);
        let path = &module.path;
        for export in &module.exports {
            let name = export_name(&export.name);
            let at = || text(path, export.span);
            let detail = || Detail::Export { name: name.clone() };
            match (export.kind, name.as_str()) {
                (FacetKind::Segment, "revalidate") => {
                    out.push("nextjs.segment.revalidate", at(), detail())
                }
                (FacetKind::Segment, "runtime") => {
                    out.push("nextjs.segment.runtime", at(), detail())
                }
                (FacetKind::Segment, "dynamic")
                    if export.value == Value::String("force-dynamic".into()) =>
                {
                    out.push("nextjs.segment.dynamic", at(), detail())
                }
                // These modes render at build time or fail the build, so
                // static output preserves them; other values stay unknown.
                (FacetKind::Segment, "dynamic") if matches!(&export.value, Value::String(mode) if ["auto", "force-static", "error"].contains(&mode.as_str())) => {
                    out.push("nextjs.segment.static", at(), detail())
                }
                (FacetKind::Segment, "dynamic") => out.unknown(at(), Cause::SegmentValue),
                // `true` serves unenumerated parameters on demand, which
                // static output cannot; only `false` matches its 404s.
                (FacetKind::Segment, "dynamicParams") if export.value == Value::Bool(false) => {
                    out.push("nextjs.segment.dynamic-params", at(), detail())
                }
                (FacetKind::Segment, _) => out.unknown(at(), Cause::SegmentOption { name }),
                (FacetKind::Metadata, _) if export.value.is_known() => {
                    out.push("nextjs.metadata.static", at(), detail());
                    for field in unsupported_fields(&name, &export.value) {
                        out.push(
                            "nextjs.metadata.unsupported-field",
                            at(),
                            Detail::Field {
                                export: name.clone(),
                                path: field,
                            },
                        );
                    }
                }
                (FacetKind::Metadata, _) => out.push("nextjs.metadata.dynamic", at(), detail()),
                (FacetKind::Generator, "generateMetadata" | "generateViewport") => {
                    out.push("nextjs.metadata.dynamic", at(), detail())
                }
                // Parameters, sitemaps, image metadata, handlers, and
                // metadata routes belong to their route and file features.
                (FacetKind::Generator | FacetKind::Handler | FacetKind::MetadataRoute, _) => {}
            }
        }
        let context = contexts.module(path);
        for usage in &module.import_uses {
            let Some(Resolution::External { package }) = &usage.resolution else {
                continue;
            };
            match package.as_str() {
                "next" if usage.specifier.starts_with("next/") || usage.specifier == "next" => {
                    let in_browser_call = usage.kind == ImportedUseKind::Call
                        && browser.iter().any(|import| {
                            import.path == *path
                                && usage.span.start() <= import.span.start()
                                && import.span.end() <= usage.span.end()
                        });
                    next_api(out, path, usage, convention, in_browser_call)
                }
                "react" | "react-dom" => {
                    let (server, client) = context.map_or((false, false), |c| (c.server, c.client));
                    react_api(out, path, usage, server, client)
                }
                _ => {}
            }
        }
        if convention == Some(Convention::Page)
            && context.is_some_and(|context| context.server && !context.use_client)
            && let Some(semantics) = semantics(project, path)
        {
            for &span in &semantics.search_params {
                out.push(
                    "nextjs.api.search-params-server",
                    text(path, span),
                    Detail::None,
                );
            }
        }
        if let Some(semantics) = semantics(project, path) {
            for &span in &semantics.styled_jsx {
                out.push("css.styled-jsx", text(path, span), Detail::None);
            }
        }
    }
}

fn semantics<'a>(
    project: &'a Project,
    path: &SourcePath,
) -> Option<&'a crate::project::ModuleFacts> {
    let modules = project.semantics();
    modules
        .binary_search_by(|module| module.path.cmp(path))
        .ok()
        .map(|index| &modules[index])
}

/// The export that a use selects: a named import, or the first literal
/// member of a default or namespace import for packages whose default export
/// is a namespace object.
fn selected(usage: &ImportedUse, namespace_default: bool) -> Option<String> {
    match &usage.imported {
        ImportedName::Named(name) => Some(name.clone()),
        ImportedName::Default if !namespace_default => Some("default".to_owned()),
        ImportedName::Default | ImportedName::Namespace => usage.members.first().cloned().flatten(),
        ImportedName::Require => None,
    }
}

fn next_api(
    out: &mut Out,
    path: &SourcePath,
    usage: &ImportedUse,
    convention: Option<Convention>,
    browser_only: bool,
) {
    let specifier = usage.specifier.as_str();
    let export = selected(usage, false);
    let id = match (specifier, export.as_deref()) {
        ("next/link", Some("default")) => Some("nextjs.api.link"),
        ("next/image" | "next/legacy/image", Some(_)) => Some("nextjs.api.image"),
        ("next/font/google" | "next/font/local", Some(_)) => Some("nextjs.api.font"),
        ("next/script", Some("default")) => Some("nextjs.api.script"),
        (
            "next/navigation",
            Some("useRouter" | "usePathname" | "useSearchParams" | "useParams"),
        ) => Some("nextjs.api.navigation"),
        ("next/navigation", Some("redirect" | "permanentRedirect")) => Some("nextjs.api.redirect"),
        ("next/navigation", Some("notFound")) => Some("nextjs.api.not-found"),
        ("next/dynamic", Some("default")) if usage.kind == ImportedUseKind::Call => {
            Some(if browser_only {
                "nextjs.api.dynamic-no-ssr"
            } else {
                "nextjs.api.dynamic"
            })
        }
        ("next/headers", Some("cookies" | "headers" | "draftMode")) => Some("nextjs.api.request"),
        ("next/cache", Some("unstable_cache")) => Some("react.cache"),
        ("next/headers" | "next/cache" | "next/og" | "next/form", Some(_)) => {
            Some("nextjs.api.server-only")
        }
        // Static route handlers map `next/server` responses (MIGRATION_SPEC.md
        // section 6.3); the handler feature carries their assessment.
        ("next/server", Some(_)) if convention == Some(Convention::Route) => return,
        ("next/server", Some(_)) => Some("nextjs.api.server-only"),
        ("next/head" | "next/router" | "next/document" | "next/app", Some(_)) => {
            Some("nextjs.api.pages-router")
        }
        ("next/config", Some(_)) => Some("nextjs.api.config"),
        _ => None,
    };
    let at = text(path, usage.span);
    match (id, export) {
        (Some(id), Some(export)) => out.push(
            id,
            at,
            Detail::Api {
                specifier: specifier.to_owned(),
                export,
            },
        ),
        (_, export) => out.unknown(
            at,
            Cause::Api {
                specifier: specifier.to_owned(),
                export,
            },
        ),
    }
}

fn react_api(out: &mut Out, path: &SourcePath, usage: &ImportedUse, server: bool, client: bool) {
    let specifier = usage.specifier.as_str();
    let export = selected(usage, true);
    let ids: &[&str] = match (specifier, export.as_deref()) {
        ("react", Some(name)) => match name {
            "useState" => &["react.hook.use-state"],
            "useEffect" => &["react.hook.use-effect"],
            "useLayoutEffect" => &["react.hook.use-layout-effect"],
            "useRef" => &["react.hook.use-ref"],
            "useMemo" => &["react.hook.use-memo"],
            "useCallback" => &["react.hook.use-callback"],
            "useReducer" => &["react.hook.use-reducer"],
            "useContext" => &["react.hook.use-context"],
            "useImperativeHandle" => &["react.hook.use-imperative-handle"],
            "useTransition" => &["react.hook.use-transition"],
            "useDeferredValue" => &["react.hook.use-deferred-value"],
            "useSyncExternalStore" => &["react.hook.use-sync-external-store"],
            "useInsertionEffect" => &["react.hook.use-insertion-effect"],
            "useId" => &["react.hook.use-id"],
            "use" | "useActionState" | "useOptimistic" | "useEffectEvent" => {
                &["react.hook.react19"]
            }
            "createContext" => &["react.api.create-context"],
            "memo" => &["react.api.memo"],
            "forwardRef" => &["react.api.forward-ref"],
            "lazy" => &["react.api.lazy"],
            "Component" | "PureComponent" => &["react.api.component"],
            "StrictMode" => &["react.api.strict-mode"],
            "Fragment" => &["react.api.fragment"],
            "Children" => &["react.api.children"],
            "cloneElement" => &["react.api.clone-element"],
            "isValidElement" => &["react.api.is-valid-element"],
            "createElement" => &["react.api.create-element"],
            "startTransition" => &["react.api.start-transition"],
            "cache" => &["react.cache"],
            // Suspense is unwrapped in server-rendered trees and kept in
            // islands; a module in both contexts has both uses.
            "Suspense" => match (server, client) {
                (true, true) => &["react.suspense-server", "react.api.suspense"],
                (true, false) => &["react.suspense-server"],
                (false, true | false) => &["react.api.suspense"],
            },
            _ => &[],
        },
        ("react-dom", Some("createPortal")) => &["react.api.create-portal"],
        ("react-dom", Some("useFormStatus")) => &["react.hook.react19"],
        _ => &[],
    };
    let at = || text(path, usage.span);
    match export {
        Some(export) if !ids.is_empty() => {
            for id in ids {
                out.push(
                    id,
                    at(),
                    Detail::Api {
                        specifier: specifier.to_owned(),
                        export: export.clone(),
                    },
                );
            }
        }
        export => out.unknown(
            at(),
            Cause::Api {
                specifier: specifier.to_owned(),
                export,
            },
        ),
    }
}

/// Field paths outside the V1 subset of MIGRATION_SPEC.md section 13, in
/// source order. Only known objects reach this check.
fn unsupported_fields(export: &str, value: &Value) -> Vec<String> {
    let Value::Object(object) = value else {
        return vec![String::new()];
    };
    let mut fields = Vec::new();
    for (key, value) in &object.entries {
        let nested: Option<&[&str]> = match (export, key.as_str()) {
            (
                "metadata",
                "title" | "description" | "keywords" | "authors" | "creator" | "publisher"
                | "robots" | "icons" | "metadataBase",
            )
            | ("viewport", "width" | "initialScale" | "themeColor") => None,
            ("metadata", "alternates") => Some(&["canonical"]),
            ("metadata", "openGraph") => Some(&[
                "title",
                "description",
                "url",
                "siteName",
                "images",
                "locale",
                "type",
            ]),
            ("metadata", "twitter") => {
                Some(&["card", "title", "description", "images", "creator", "site"])
            }
            _ => {
                fields.push(key.clone());
                continue;
            }
        };
        let (Some(allowed), Value::Object(inner)) = (nested, value) else {
            continue;
        };
        for (name, _) in &inner.entries {
            if !allowed.contains(&name.as_str()) {
                fields.push(format!("{key}.{name}"));
            }
        }
    }
    fields
}

fn contexts(out: &mut Out, project: &Project) {
    let contexts = &project.nextjs().contexts;
    let graph = project.modules();
    let mut clients = BTreeSet::new();
    for boundary in &contexts.boundaries {
        if !clients.insert(&boundary.client) {
            continue;
        }
        let id = if convention(project, &boundary.client) == Some(Convention::Page) {
            "component.client-page"
        } else {
            "component.island"
        };
        let location = graph
            .node(&boundary.client)
            .and_then(|node| {
                node.directives
                    .iter()
                    .find(|d| !d.escaped && d.value.as_deref() == Some("use client"))
            })
            .map_or(Location::File(boundary.client.clone()), |directive| {
                text(&boundary.client, directive.span)
            });
        out.push(id, location, Detail::None);
    }
    if !contexts.boundaries.is_empty() {
        out.push("nextjs.navigation.client", Location::Project, Detail::None);
    }
    for issue in &contexts.issues {
        let (module, span) = match issue {
            // The graph reports unparsed modules, unresolved references, and
            // server actions itself.
            ContextIssue::Unparsed { .. }
            | ContextIssue::Unresolved { .. }
            | ContextIssue::InlineServerAction { .. } => continue,
            ContextIssue::EscapedDirective { module, span }
            | ContextIssue::ServerOnlyInClient { module, span }
            | ContextIssue::ClientOnlyInServer { module, span } => (module, *span),
        };
        out.unknown(text(module, span), Cause::Context(issue.clone()));
    }
}

fn components(out: &mut Out, project: &Project) {
    let graph = project.components();
    let contexts = &project.nextjs().contexts;
    for definition in &graph.definitions {
        if contexts
            .module(&definition.path)
            .is_some_and(|context| context.server)
        {
            out.push(
                "react.server-component",
                text(&definition.path, definition.span),
                Detail::Component {
                    name: definition.name.clone(),
                },
            );
        }
    }
    for site in &graph.sites {
        match &site.resolved.rendered {
            Rendered::Intrinsic => {
                let spelling = semantics(project, &site.path).and_then(|module| {
                    module
                        .renders
                        .binary_search_by_key(&site.span.start(), |render| render.span.start())
                        .ok()
                        .map(|index| module.renders[index].spelling.as_str())
                });
                if matches!(spelling, Some("title" | "meta" | "link")) {
                    out.push(
                        "react.head-hoisting",
                        text(&site.path, site.span),
                        Detail::None,
                    );
                }
            }
            // The graph reports unresolved references and unparsed modules.
            Rendered::Unknown {
                reason: UnknownReason::Unresolved(_) | UnknownReason::Unparsed,
                ..
            }
            | Rendered::Definition(_)
            | Rendered::External { .. } => {}
            Rendered::Unknown { reason, .. } => {
                out.unknown(text(&site.path, site.span), Cause::Render(reason.clone()))
            }
        }
    }
}

fn environment(out: &mut Out, project: &Project) {
    for module in &project.environment().modules {
        for variable in &module.variables {
            let at = text(&module.path, variable.span);
            match (&variable.name, variable.access) {
                (
                    Some(name),
                    VariableAccess::Direct | VariableAccess::Alias | VariableAccess::Destructured,
                ) => out.push("env.reference", at, Detail::Variable { name: name.clone() }),
                (_, access) => out.unknown(at, Cause::Environment(access)),
            }
        }
    }
    // Build-time `fetch` runs in server components; prerendered client
    // components fetch in the browser after hydration as well.
    for module in project.semantics() {
        if !project
            .nextjs()
            .contexts
            .module(&module.path)
            .is_some_and(|context| context.server)
        {
            continue;
        }
        for reference in module.bindings.references() {
            if reference.name == "fetch"
                && reference.target == Target::Global
                && reference.read
                && reference.meaning.overlaps(Meaning::VALUE)
            {
                out.push(
                    "nextjs.fetch",
                    text(&module.path, module.spans[reference.node.index()]),
                    Detail::None,
                );
            }
        }
    }
}

fn graph(out: &mut Out, project: &Project) {
    let package = project.package();
    for node in project.modules().nodes() {
        if !inside(&node.path, package) {
            continue;
        }
        if node.state != ModuleState::Parsed {
            out.unknown(Location::File(node.path.clone()), Cause::Unparsed);
        }
        for edge in node.edges.iter().filter(|edge| !edge.type_only) {
            let at = || text(&node.path, edge.span);
            match &edge.resolution {
                Resolution::Unresolved(reason) => out.unknown(at(), Cause::Unresolved(*reason)),
                Resolution::Local {
                    kind: FileKind::Asset,
                    ..
                } => out.push("asset.import", at(), Detail::None),
                Resolution::Local {
                    kind: FileKind::Json,
                    ..
                } => out.push("asset.json-import", at(), Detail::None),
                Resolution::External { package }
                    if matches!(package.as_str(), "server-only" | "client-only")
                        && edge.specifier.as_deref() == Some(package.as_str()) =>
                {
                    out.push("nextjs.guard", at(), Detail::None)
                }
                Resolution::Local { .. }
                | Resolution::External { .. }
                | Resolution::NodeBuiltin => {}
            }
        }
        for directive in node.directives.iter().chain(&node.function_directives) {
            if !directive.escaped && directive.value.as_deref() == Some("use server") {
                out.push(
                    "nextjs.api.server-action",
                    text(&node.path, directive.span),
                    Detail::None,
                );
            }
        }
    }
}
