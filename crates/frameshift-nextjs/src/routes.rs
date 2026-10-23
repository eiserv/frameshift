//! The App Router route tree from the inventory alone, as Next.js 16.4.2
//! discovers it: `collectAppFiles`, `normalizeAppPath`, `getSegmentParam`,
//! the segment file conventions of its app loader, and `isMetadataRouteFile`.
//! No file content is read.

use std::collections::BTreeMap;

use frameshift_core::{PathLimits, SourcePath};
use frameshift_input::{EntryKind, Inventory};

/// A file convention of an App Router segment or of the package root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Convention {
    /// `page`: a routable segment's UI.
    Page,
    /// `route`: a route handler.
    Route,
    /// `layout`.
    Layout,
    /// `template`.
    Template,
    /// `loading`.
    Loading,
    /// `error`.
    Error,
    /// `not-found`.
    NotFound,
    /// `forbidden`.
    Forbidden,
    /// `unauthorized`.
    Unauthorized,
    /// `default`, the fallback of a parallel slot.
    Default,
    /// `global-error`, honored only in the app root.
    GlobalError,
    /// `global-not-found`, honored only in the app root.
    GlobalNotFound,
    /// A metadata file, such as `icon.png` or `sitemap.ts`.
    Metadata(Metadata),
    /// `middleware` in the package or `src` directory.
    Middleware,
    /// `proxy`, the Next.js 16 name of middleware.
    Proxy,
    /// `instrumentation`.
    Instrumentation,
}

/// The kind of a metadata file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metadata {
    /// `favicon.ico`, root only.
    Favicon,
    /// `icon`, with an optional digit variant.
    Icon,
    /// `apple-icon`, with an optional digit variant.
    AppleIcon,
    /// `opengraph-image`, with an optional digit variant.
    OpenGraphImage,
    /// `twitter-image`, with an optional digit variant.
    TwitterImage,
    /// `sitemap`.
    Sitemap,
    /// `robots`, root only.
    Robots,
    /// `manifest`, root only.
    Manifest,
}

/// What a directory segment means for routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    /// A literal URL segment.
    Static,
    /// `(name)`: no URL segment.
    Group,
    /// `@name`: a parallel slot, no URL segment; `@children` is the
    /// implicit children slot.
    Parallel,
    /// `[name]`.
    Dynamic(String),
    /// `[...name]`.
    CatchAll(String),
    /// `[[...name]]`.
    OptionalCatchAll(String),
}

/// An interception marker that a segment starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interception {
    /// `(.)`: the same level.
    SameLevel,
    /// `(..)`: one level up.
    OneLevelUp,
    /// `(..)(..)`: two levels up.
    TwoLevelsUp,
    /// `(...)`: from the app root.
    Root,
}

/// One directory segment below the app directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// The folder name.
    pub name: String,
    /// Its routing meaning, after any interception marker.
    pub kind: SegmentKind,
    /// The interception marker, if any.
    pub interception: Option<Interception>,
}

/// One convention file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConventionFile {
    /// The convention.
    pub convention: Convention,
    /// The file.
    pub path: SourcePath,
    /// The directory segments from the app directory to the file, empty for
    /// the app root and for package-level files.
    pub segments: Vec<Segment>,
}

/// What a route serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    /// A page, rendered within its layouts.
    Page,
    /// A route handler.
    Handler,
    /// A metadata route.
    Metadata(Metadata),
}

/// One route: a page, route handler, or metadata file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRoute {
    /// What it serves.
    pub kind: RouteKind,
    /// The defining file.
    pub file: SourcePath,
    /// The URL pattern as `normalizeAppPath` derives it, such as
    /// `/blog/[slug]`; for metadata files the directory pattern followed by
    /// the file name without a code extension.
    pub pattern: String,
    /// The directory segments from the app directory.
    pub segments: Vec<Segment>,
    /// For pages, the segment convention files of every enclosing
    /// directory from the app root down, such as layouts, templates, and
    /// boundaries, in that order.
    pub chain: Vec<ConventionFile>,
}

impl AppRoute {
    /// The layouts that wrap a page, outermost first.
    pub fn layouts(&self) -> impl Iterator<Item = &SourcePath> {
        self.chain
            .iter()
            .filter(|f| f.convention == Convention::Layout)
            .map(|f| &f.path)
    }

    /// Whether a segment of the route is a parallel slot other than
    /// `@children`.
    pub fn in_slot(&self) -> bool {
        self.segments
            .iter()
            .any(|s| s.kind == SegmentKind::Parallel && s.name != "@children")
    }
}

/// A structural problem that Next.js reports or that limits the evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteIssue {
    /// A directory has both `page` and `route`; Next.js fails the build.
    PageAndRoute {
        /// The directory.
        directory: SourcePath,
    },
    /// Several pages or handlers outside parallel slots resolve to one
    /// pattern; Next.js fails the build.
    DuplicatePattern {
        /// The pattern.
        pattern: String,
        /// The conflicting files, in path order.
        files: Vec<SourcePath>,
    },
    /// A page has no layout in its chain; Next.js requires a root layout.
    MissingRootLayout {
        /// The page.
        page: SourcePath,
    },
    /// A segment has several files of one convention with different page
    /// extensions; Next.js picks one by its extension order.
    AmbiguousConvention {
        /// The files, in path order.
        files: Vec<SourcePath>,
    },
    /// An entry below the app directory was not read as a file or
    /// directory: ignored, oversized, a link, or untraversed. Next.js may
    /// read files there that the analysis cannot see.
    Incomplete {
        /// The entry.
        path: SourcePath,
    },
}

/// The App Router facts of one app directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRoutes {
    /// The app directory.
    pub app: SourcePath,
    /// Every convention file, in path order, including package-level
    /// middleware, proxy, and instrumentation files.
    pub files: Vec<ConventionFile>,
    /// Pages, handlers, and metadata routes, in path order.
    pub routes: Vec<AppRoute>,
    /// Structural problems, in path order of their first file.
    pub issues: Vec<RouteIssue>,
}

/// A failure that ends route discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteError {
    /// The app directory is not a traversed directory of the inventory.
    NotADirectory,
    /// A path could not be represented.
    Path,
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotADirectory => "the app directory is not a traversed directory",
            Self::Path => "an app route path could not be represented",
        })
    }
}

impl std::error::Error for RouteError {}

/// Classifies a folder name as `normalizeAppPath` and `getSegmentParam` do:
/// a group first, then an interception marker, then a parameter.
pub fn segment(name: &str) -> Segment {
    if name.starts_with('(') && name.ends_with(')') {
        return Segment {
            name: name.to_owned(),
            kind: SegmentKind::Group,
            interception: None,
        };
    }
    if name.starts_with('@') {
        return Segment {
            name: name.to_owned(),
            kind: SegmentKind::Parallel,
            interception: None,
        };
    }
    let markers = [
        ("(..)(..)", Interception::TwoLevelsUp),
        ("(.)", Interception::SameLevel),
        ("(..)", Interception::OneLevelUp),
        ("(...)", Interception::Root),
    ];
    let (rest, interception) = markers
        .iter()
        .find(|(m, _)| name.starts_with(m))
        .map_or((name, None), |(m, i)| (&name[m.len()..], Some(*i)));
    let kind = if let Some(param) = rest
        .strip_prefix("[[...")
        .and_then(|r| r.strip_suffix("]]"))
    {
        SegmentKind::OptionalCatchAll(param.to_owned())
    } else if let Some(param) = rest.strip_prefix("[...").and_then(|r| r.strip_suffix(']')) {
        SegmentKind::CatchAll(param.to_owned())
    } else if let Some(param) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        SegmentKind::Dynamic(param.to_owned())
    } else {
        SegmentKind::Static
    };
    Segment {
        name: name.to_owned(),
        kind,
        interception,
    }
}

/// The `normalizeAppPath` pattern of app-relative directory segments.
pub fn pattern(segments: &[Segment]) -> String {
    let mut out = String::new();
    for s in segments {
        if matches!(s.kind, SegmentKind::Group | SegmentKind::Parallel) || s.name.is_empty() {
            continue;
        }
        out.push('/');
        out.push_str(&s.name);
    }
    if out.is_empty() { "/".to_owned() } else { out }
}

/// The convention of a file name inside an app directory, as Next.js's
/// valid-file matcher and app loader classify it. `root` says whether the
/// file is directly in the app directory.
pub fn convention(name: &str, root: bool, extensions: &[&str]) -> Option<Convention> {
    let code = |stem: &str| {
        name.strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|ext| extensions.contains(&ext))
    };
    for (stem, convention) in [
        ("page", Convention::Page),
        ("route", Convention::Route),
        ("layout", Convention::Layout),
        ("template", Convention::Template),
        ("loading", Convention::Loading),
        ("error", Convention::Error),
        ("not-found", Convention::NotFound),
        ("forbidden", Convention::Forbidden),
        ("unauthorized", Convention::Unauthorized),
        ("default", Convention::Default),
    ] {
        if code(stem) {
            return Some(convention);
        }
    }
    if root && code("global-error") {
        return Some(Convention::GlobalError);
    }
    if root && code("global-not-found") {
        return Some(Convention::GlobalNotFound);
    }
    metadata(name, root, extensions).map(Convention::Metadata)
}

/// `isMetadataRouteFile` with strictly matched extensions.
fn metadata(name: &str, root: bool, extensions: &[&str]) -> Option<Metadata> {
    let with = |stem: &str, statics: &[&str], dynamic: bool, variants: bool| {
        let Some(rest) = name.strip_prefix(stem) else {
            return false;
        };
        let rest = if variants {
            rest.strip_prefix(|c: char| c.is_ascii_digit())
                .unwrap_or(rest)
        } else {
            rest
        };
        rest.strip_prefix('.')
            .is_some_and(|ext| statics.contains(&ext) || dynamic && extensions.contains(&ext))
    };
    if root && name == "favicon.ico" {
        return Some(Metadata::Favicon);
    }
    if root && with("robots", &["txt"], true, false) {
        return Some(Metadata::Robots);
    }
    if root && with("manifest", &["webmanifest", "json"], true, false) {
        return Some(Metadata::Manifest);
    }
    let images = [
        (
            "apple-icon",
            Metadata::AppleIcon,
            &["jpg", "jpeg", "png"][..],
        ),
        (
            "icon",
            Metadata::Icon,
            &["ico", "jpg", "jpeg", "png", "svg"][..],
        ),
        (
            "opengraph-image",
            Metadata::OpenGraphImage,
            &["jpg", "jpeg", "png", "gif"][..],
        ),
        (
            "twitter-image",
            Metadata::TwitterImage,
            &["jpg", "jpeg", "png", "gif"][..],
        ),
    ];
    if with("sitemap", &["xml"], true, false) {
        return Some(Metadata::Sitemap);
    }
    images
        .into_iter()
        .find(|(stem, _, statics)| with(stem, statics, true, true))
        .map(|(_, kind, _)| kind)
}

fn path(text: &str) -> Result<SourcePath, RouteError> {
    SourcePath::new(text, PathLimits::default()).map_err(|_| RouteError::Path)
}

/// Discovers the App Router facts of `app`, a traversed directory of the
/// inventory, with the given page extensions (the default is `tsx`, `ts`,
/// `jsx`, `js`; a configuration may change them). Package-level middleware,
/// proxy, and instrumentation files are looked up in `package` and, when
/// `app` lies below `src`, in `src`.
pub fn app_routes(
    inventory: &Inventory,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
) -> Result<AppRoutes, RouteError> {
    if !inventory
        .entry(app)
        .is_some_and(|e| matches!(e.kind(), EntryKind::Directory))
    {
        return Err(RouteError::NotADirectory);
    }
    let prefix = format!("{}/", app.as_str());
    let mut issues = Vec::new();
    let entries = inventory.entries();
    let start = entries.partition_point(|e| e.path().as_str() < prefix.as_str());
    for entry in entries[start..]
        .iter()
        .take_while(|e| e.path().as_str().starts_with(&prefix))
    {
        let relative = &entry.path().as_str()[prefix.len()..];
        if relative.split('/').any(|part| part.starts_with('_')) {
            continue;
        }
        if !matches!(entry.kind(), EntryKind::Directory | EntryKind::Special) {
            issues.push(RouteIssue::Incomplete {
                path: entry.path().clone(),
            });
        }
    }
    let mut files = Vec::new();
    let all = inventory.files().as_slice();
    let start = all.partition_point(|f| f.as_str() < prefix.as_str());
    for file in all[start..]
        .iter()
        .take_while(|f| f.as_str().starts_with(&prefix))
    {
        let relative = &file.as_str()[prefix.len()..];
        // `collectAppFiles` skips every entry whose name starts with `_`.
        if relative.split('/').any(|part| part.starts_with('_')) {
            continue;
        }
        let (directory, name) = relative.rsplit_once('/').unwrap_or(("", relative));
        let Some(convention) = convention(name, directory.is_empty(), extensions) else {
            continue;
        };
        let segments = if directory.is_empty() {
            Vec::new()
        } else {
            directory.split('/').map(segment).collect()
        };
        files.push(ConventionFile {
            convention,
            path: file.clone(),
            segments,
        });
    }
    // Package-level files beside `app`, or beside `src/app` in `src`.
    let base = package.map_or(String::new(), |p| format!("{}/", p.as_str()));
    let in_src = app.as_str() == format!("{base}src/app");
    let level = if in_src { format!("{base}src/") } else { base };
    for (stem, convention) in [
        ("instrumentation", Convention::Instrumentation),
        ("middleware", Convention::Middleware),
        ("proxy", Convention::Proxy),
    ] {
        for ext in extensions {
            let candidate = path(&format!("{level}{stem}.{ext}"))?;
            if inventory.files().find(&candidate).is_some() {
                files.push(ConventionFile {
                    convention,
                    path: candidate,
                    segments: Vec::new(),
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    // One file per convention and directory: several extensions are ambiguous.
    let mut by_directory: BTreeMap<(String, Convention), Vec<&ConventionFile>> = BTreeMap::new();
    for file in &files {
        if matches!(file.convention, Convention::Metadata(_)) {
            continue;
        }
        let directory = file.path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        by_directory
            .entry((directory.to_owned(), file.convention))
            .or_default()
            .push(file);
    }
    for list in by_directory.values().filter(|l| l.len() > 1) {
        issues.push(RouteIssue::AmbiguousConvention {
            files: list.iter().map(|f| f.path.clone()).collect(),
        });
    }
    for (directory, convention) in by_directory.keys() {
        if *convention == Convention::Page
            && by_directory.contains_key(&(directory.clone(), Convention::Route))
        {
            issues.push(RouteIssue::PageAndRoute {
                directory: path(directory)?,
            });
        }
    }
    let mut routes = Vec::new();
    for file in &files {
        let kind = match file.convention {
            Convention::Page => RouteKind::Page,
            Convention::Route => RouteKind::Handler,
            Convention::Metadata(kind) => RouteKind::Metadata(kind),
            Convention::Layout
            | Convention::Template
            | Convention::Loading
            | Convention::Error
            | Convention::NotFound
            | Convention::Forbidden
            | Convention::Unauthorized
            | Convention::Default
            | Convention::GlobalError
            | Convention::GlobalNotFound
            | Convention::Middleware
            | Convention::Proxy
            | Convention::Instrumentation => continue,
        };
        let mut pattern = pattern(&file.segments);
        if let RouteKind::Metadata(_) = kind {
            let name = file.path.as_str().rsplit('/').next().unwrap_or("");
            let shown = name
                .rsplit_once('.')
                .filter(|(_, ext)| extensions.contains(ext))
                .map_or(name, |(stem, _)| stem);
            if !pattern.ends_with('/') {
                pattern.push('/');
            }
            pattern.push_str(shown);
        }
        let mut chain = Vec::new();
        if kind == RouteKind::Page {
            for depth in 0..=file.segments.len() {
                let directory =
                    file.segments[..depth]
                        .iter()
                        .fold(app.as_str().to_owned(), |mut d, s| {
                            d.push('/');
                            d.push_str(&s.name);
                            d
                        });
                for convention in [
                    Convention::Layout,
                    Convention::Template,
                    Convention::Error,
                    Convention::Loading,
                    Convention::NotFound,
                    Convention::Forbidden,
                    Convention::Unauthorized,
                ] {
                    if let Some(list) = by_directory.get(&(directory.clone(), convention)) {
                        chain.push((*list[0]).clone());
                    }
                }
            }
            if !chain.iter().any(|f| f.convention == Convention::Layout) {
                issues.push(RouteIssue::MissingRootLayout {
                    page: file.path.clone(),
                });
            }
        }
        routes.push(AppRoute {
            kind,
            file: file.path.clone(),
            pattern,
            segments: file.segments.clone(),
            chain,
        });
    }
    let mut patterns: BTreeMap<&str, Vec<SourcePath>> = BTreeMap::new();
    for route in &routes {
        if matches!(route.kind, RouteKind::Page | RouteKind::Handler) && !route.in_slot() {
            patterns
                .entry(route.pattern.as_str())
                .or_default()
                .push(route.file.clone());
        }
    }
    for (pattern, list) in patterns.into_iter().filter(|(_, l)| l.len() > 1) {
        issues.push(RouteIssue::DuplicatePattern {
            pattern: pattern.to_owned(),
            files: list,
        });
    }
    Ok(AppRoutes {
        app: app.clone(),
        files,
        routes,
        issues,
    })
}
