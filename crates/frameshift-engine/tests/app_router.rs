//! App Router route trees, convention exports, and render contexts through
//! the engine facade, on fictional projects created at test time.

use std::fs;
use std::path::PathBuf;

use frameshift_engine::bindings::ExportName;
use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::GraphLimits;
use frameshift_engine::nextjs::{
    AppAnalysis, ContextIssue, Convention, Interception, Metadata, RouteIssue, RouteKind,
    SegmentKind, analyze_app, app_routes, convention, pattern, segment,
};

mod support;

const EXTENSIONS: [&str; 4] = ["tsx", "ts", "jsx", "js"];

fn path(s: &str) -> SourcePath {
    SourcePath::new(s, PathLimits::default()).unwrap()
}

fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = support::temporary().join("app-router").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    for (name, text) in files {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    root
}

fn open(name: &str, files: &[(&str, &str)]) -> Snapshot {
    let root = scratch(name, files);
    let mut findings = FindingCollector::new(FindingLimits::default());
    snapshot(&root, Limits::default().snapshot(), &mut findings).unwrap()
}

const PROJECT: &[(&str, &str)] = &[
    (
        "app/layout.tsx",
        "export default function RootLayout({ children }) { return <html><body>{children}</body></html>; }\nexport const metadata = { title: 'x' };\n",
    ),
    (
        "app/page.tsx",
        "import Counter from '../components/Counter';\nimport { shared } from '../lib/shared';\nimport type { T } from '../lib/types';\nexport default function Home() { return <Counter value={shared} />; }\n",
    ),
    (
        "app/(marketing)/layout.tsx",
        "export default function M({ children }) { return children; }\n",
    ),
    (
        "app/(marketing)/about/page.tsx",
        "export default function About() { return null; }\nexport const dynamic = 'force-static';\n",
    ),
    (
        "app/blog/[slug]/page.tsx",
        "export async function generateStaticParams() { return []; }\nexport default function Post() { return null; }\n",
    ),
    (
        "app/blog/[slug]/loading.tsx",
        "export default function L() { return null; }\n",
    ),
    ("app/blog/opengraph-image.png", ""),
    (
        "app/docs/[...path]/page.tsx",
        "export default () => null;\n",
    ),
    (
        "app/shop/[[...rest]]/page.tsx",
        "export * from '../../../lib/shared';\nexport { default } from '../../../lib/page';\n",
    ),
    (
        "app/@modal/(.)photo/[id]/page.tsx",
        "export default function Photo() { return null; }\n",
    ),
    (
        "app/@modal/default.tsx",
        "export default function D() { return null; }\n",
    ),
    (
        "app/api/feed/route.ts",
        "export async function GET() { return new Response('x'); }\n",
    ),
    (
        "app/_lib/page.tsx",
        "export default function Private() {}\n",
    ),
    ("app/favicon.ico", ""),
    (
        "app/robots.ts",
        "export default function robots() { return {}; }\n",
    ),
    ("app/icon1.png", ""),
    ("app/sitemap.xml", ""),
    ("app/nested/robots.txt", ""),
    (
        "app/error.tsx",
        "'use client';\nexport default function E() { return null; }\n",
    ),
    ("app/conflict/page.tsx", "export default function P() {}\n"),
    ("app/conflict/route.ts", "export function GET() {}\n"),
    ("app/(a)/dup/page.tsx", "export default function A() {}\n"),
    ("app/(b)/dup/page.js", "export default function B() {}\n"),
    (
        "components/Counter.tsx",
        "'use client';\nimport { shared } from '../lib/shared';\nimport { save } from '../lib/actions';\nimport 'server-only';\nexport default function Counter() { return <button onClick={() => save(shared)} />; }\n",
    ),
    (
        "lib/shared.ts",
        "import { helper } from './helper';\nexport const shared = helper();\n",
    ),
    (
        "lib/helper.ts",
        "import 'client-only';\nexport function helper() { return 1; }\nexport async function inline() { 'use server'; }\n",
    ),
    (
        "lib/actions.ts",
        "'use server';\nimport { db } from './db';\nexport async function save() { return db; }\n",
    ),
    ("lib/db.ts", "export const db = import(name);\n"),
    ("lib/types.ts", "export type T = number;\n"),
    (
        "lib/page.tsx",
        "export default function Shop() { return null; }\n",
    ),
    ("middleware.ts", "export function middleware() {}\n"),
];

/// Each test uses its own directory, because tests run in parallel.
fn analysis(name: &str) -> AppAnalysis {
    let snapshot = open(name, PROJECT);
    analyze_app(
        &snapshot,
        None,
        &path("app"),
        &EXTENSIONS,
        GraphLimits::default(),
    )
    .unwrap()
}

#[test]
fn segments_patterns_and_conventions_follow_next_js() {
    assert_eq!(segment("(shop)").kind, SegmentKind::Group);
    assert_eq!(segment("@modal").kind, SegmentKind::Parallel);
    assert_eq!(segment("[id]").kind, SegmentKind::Dynamic("id".into()));
    assert_eq!(segment("[...p]").kind, SegmentKind::CatchAll("p".into()));
    assert_eq!(
        segment("[[...p]]").kind,
        SegmentKind::OptionalCatchAll("p".into())
    );
    let intercepted = segment("(..)(..)[id]");
    assert_eq!(intercepted.interception, Some(Interception::TwoLevelsUp));
    assert_eq!(intercepted.kind, SegmentKind::Dynamic("id".into()));
    assert_eq!(segment("(.)").kind, SegmentKind::Group);
    let segments: Vec<_> = ["(g)", "@s", "a", "[b]"].into_iter().map(segment).collect();
    assert_eq!(pattern(&segments), "/a/[b]");
    assert_eq!(pattern(&[]), "/");
    assert_eq!(
        convention("page.tsx", false, &EXTENSIONS),
        Some(Convention::Page)
    );
    assert_eq!(convention("page.mdx", false, &EXTENSIONS), None);
    assert_eq!(
        convention("page.mdx", false, &["mdx"]),
        Some(Convention::Page)
    );
    assert_eq!(
        convention("global-error.tsx", true, &EXTENSIONS),
        Some(Convention::GlobalError)
    );
    assert_eq!(convention("global-error.tsx", false, &EXTENSIONS), None);
    assert_eq!(
        convention("robots.txt", true, &EXTENSIONS),
        Some(Convention::Metadata(Metadata::Robots))
    );
    assert_eq!(convention("robots.txt", false, &EXTENSIONS), None);
    assert_eq!(
        convention("icon2.tsx", false, &EXTENSIONS),
        Some(Convention::Metadata(Metadata::Icon))
    );
    assert_eq!(convention("icon12.png", false, &EXTENSIONS), None);
    assert_eq!(
        convention("sitemap.xml", false, &EXTENSIONS),
        Some(Convention::Metadata(Metadata::Sitemap))
    );
}

#[test]
fn route_tree_lists_pages_handlers_metadata_and_chains() {
    let a = analysis("tree");
    let routes: Vec<_> = a
        .routes
        .routes
        .iter()
        .map(|r| (r.file.as_str(), r.kind, r.pattern.as_str()))
        .collect();
    assert_eq!(
        routes,
        [
            ("app/(a)/dup/page.tsx", RouteKind::Page, "/dup"),
            ("app/(b)/dup/page.js", RouteKind::Page, "/dup"),
            ("app/(marketing)/about/page.tsx", RouteKind::Page, "/about"),
            (
                "app/@modal/(.)photo/[id]/page.tsx",
                RouteKind::Page,
                "/(.)photo/[id]"
            ),
            ("app/api/feed/route.ts", RouteKind::Handler, "/api/feed"),
            ("app/blog/[slug]/page.tsx", RouteKind::Page, "/blog/[slug]"),
            (
                "app/blog/opengraph-image.png",
                RouteKind::Metadata(Metadata::OpenGraphImage),
                "/blog/opengraph-image.png"
            ),
            ("app/conflict/page.tsx", RouteKind::Page, "/conflict"),
            ("app/conflict/route.ts", RouteKind::Handler, "/conflict"),
            (
                "app/docs/[...path]/page.tsx",
                RouteKind::Page,
                "/docs/[...path]"
            ),
            (
                "app/favicon.ico",
                RouteKind::Metadata(Metadata::Favicon),
                "/favicon.ico"
            ),
            (
                "app/icon1.png",
                RouteKind::Metadata(Metadata::Icon),
                "/icon1.png"
            ),
            ("app/page.tsx", RouteKind::Page, "/"),
            (
                "app/robots.ts",
                RouteKind::Metadata(Metadata::Robots),
                "/robots"
            ),
            (
                "app/shop/[[...rest]]/page.tsx",
                RouteKind::Page,
                "/shop/[[...rest]]"
            ),
            (
                "app/sitemap.xml",
                RouteKind::Metadata(Metadata::Sitemap),
                "/sitemap.xml"
            ),
        ]
    );
    let about = a
        .routes
        .routes
        .iter()
        .find(|r| r.pattern == "/about")
        .unwrap();
    let layouts: Vec<_> = about.layouts().map(SourcePath::as_str).collect();
    assert_eq!(layouts, ["app/layout.tsx", "app/(marketing)/layout.tsx"]);
    let post = a
        .routes
        .routes
        .iter()
        .find(|r| r.pattern == "/blog/[slug]")
        .unwrap();
    let chain: Vec<_> = post.chain.iter().map(|f| f.convention).collect();
    assert_eq!(
        chain,
        [Convention::Layout, Convention::Error, Convention::Loading]
    );
    assert!(
        a.routes
            .routes
            .iter()
            .find(|r| r.pattern.starts_with("/(.)"))
            .unwrap()
            .in_slot()
    );
    assert!(
        a.routes
            .files
            .iter()
            .any(|f| f.convention == Convention::Middleware)
    );
    assert!(
        a.routes
            .files
            .iter()
            .all(|f| !f.path.as_str().contains("_lib"))
    );
    assert_eq!(
        a.routes.issues,
        [
            RouteIssue::PageAndRoute {
                directory: path("app/conflict")
            },
            RouteIssue::DuplicatePattern {
                pattern: "/conflict".into(),
                files: vec![path("app/conflict/page.tsx"), path("app/conflict/route.ts")]
            },
            RouteIssue::DuplicatePattern {
                pattern: "/dup".into(),
                files: vec![path("app/(a)/dup/page.tsx"), path("app/(b)/dup/page.js")]
            },
        ]
    );
}

#[test]
fn convention_exports_name_next_js_facets() {
    let a = analysis("exports");
    let exports = |file: &str| a.exports.iter().find(|e| e.path.as_str() == file).unwrap();
    assert!(exports("app/blog/[slug]/page.tsx").exports("generateStaticParams"));
    assert!(exports("app/layout.tsx").exports("metadata"));
    assert!(exports("app/(marketing)/about/page.tsx").exports("dynamic"));
    assert!(exports("app/api/feed/route.ts").exports("GET"));
    let docs = exports("app/docs/[...path]/page.tsx");
    assert!(docs.has_default() && docs.default_expression);
    let shop = exports("app/shop/[[...rest]]/page.tsx");
    assert!(shop.star && shop.names == [ExportName::Default]);
    assert!(exports("middleware.ts").exports("middleware"));
}

#[test]
fn render_contexts_cross_client_boundaries_over_runtime_edges() {
    let a = analysis("contexts");
    let c = &a.contexts;
    let context = |file: &str| {
        let m = c.module(&path(file)).unwrap_or_else(|| panic!("{file}"));
        (m.server, m.client)
    };
    assert_eq!(context("app/page.tsx"), (true, false));
    assert_eq!(context("components/Counter.tsx"), (false, true));
    assert_eq!(context("lib/shared.ts"), (true, true));
    assert_eq!(context("lib/helper.ts"), (true, true));
    assert_eq!(context("lib/actions.ts"), (true, false));
    assert_eq!(context("lib/db.ts"), (true, false));
    assert_eq!(context("app/error.tsx"), (false, true));
    assert!(c.module(&path("lib/types.ts")).is_none(), "type-only edge");
    let boundaries: Vec<_> = c
        .boundaries
        .iter()
        .map(|b| {
            (
                b.importer.as_ref().map(SourcePath::as_str),
                b.client.as_str(),
            )
        })
        .collect();
    assert_eq!(
        boundaries,
        [
            (None, "app/error.tsx"),
            (Some("app/page.tsx"), "components/Counter.tsx")
        ]
    );
    assert!(
        c.client_packages
            .contains(&(path("components/Counter.tsx"), "server-only".into()))
    );
    let kinds: Vec<_> = c
        .issues
        .iter()
        .map(|i| match i {
            ContextIssue::Unparsed { .. } => "unparsed",
            ContextIssue::Unresolved { .. } => "unresolved",
            ContextIssue::EscapedDirective { .. } => "escaped",
            ContextIssue::ServerOnlyInClient { .. } => "server-only",
            ContextIssue::ClientOnlyInServer { .. } => "client-only",
            ContextIssue::InlineServerAction { .. } => "inline-action",
        })
        .collect();
    assert_eq!(
        kinds,
        ["server-only", "unresolved", "client-only", "inline-action"]
    );
}

#[test]
fn routes_without_layouts_and_unreadable_entries_are_reported() {
    let snapshot = open(
        "no-layout",
        &[
            ("app/page.tsx", "export default function P() {}\n"),
            ("app/.gitignore", "generated/\n"),
            ("app/generated/page.tsx", "export default function G() {}\n"),
        ],
    );
    let routes = app_routes(snapshot.inventory(), None, &path("app"), &EXTENSIONS).unwrap();
    assert_eq!(
        routes.issues,
        [
            RouteIssue::Incomplete {
                path: path("app/generated")
            },
            RouteIssue::MissingRootLayout {
                page: path("app/page.tsx")
            },
        ]
    );
    assert!(app_routes(snapshot.inventory(), None, &path("missing"), &EXTENSIONS).is_err());
}

#[test]
#[ignore = "requires an external Next.js 16.4.2 installation"]
fn external_next_js_route_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let module = std::env::var("FRAMESHIFT_NEXT_MODULE")
        .expect("set FRAMESHIFT_NEXT_MODULE to an external next package directory");
    // Next.js classifies leaf, layout, default, and metadata files with its
    // valid-file matcher and derives patterns with `normalizeAppPath`.
    let script = r#"
const next = process.argv[1];
if (require(next + '/package.json').version !== '16.4.2') throw Error('expected Next.js 16.4.2');
const { createValidFileMatcher } = require(next + '/dist/server/lib/find-page-file.js');
const { normalizeAppPath } = require(next + '/dist/shared/lib/router/utils/app-paths.js');
const [extensions, ...paths] = require('fs').readFileSync(0, 'utf8').split('\n');
const v = createValidFileMatcher(extensions.split(','), '/p/app');
for (const rel of paths) {
 const file = '/p/app/' + rel;
 const kind = v.isMetadataFile(file) ? 'metadata' : v.isAppRouterRoute(file) ? 'route'
  : v.isAppRouterPage(file) ? 'page' : v.isAppLayoutPage(file) ? 'layout' : v.isAppDefaultPage(file) ? 'default' : '-';
 const stem = rel.slice(0, rel.lastIndexOf('.'));
 process.stdout.write(kind + ' ' + (kind === 'page' || kind === 'route' ? normalizeAppPath('/' + stem) : '') + '\n');
}
"#;
    let directories = [
        "",
        "(g)/",
        "@s/",
        "a/",
        "a/(g)/b/",
        "[id]/",
        "[...p]/",
        "[[...p]]/",
        "(.)x/",
        "(..)(..)[y]/",
        "@m/(...)z/",
        "(..)/",
        "x(y)/",
    ];
    let names = [
        "page",
        "route",
        "layout",
        "default",
        "template",
        "icon",
        "icon1",
        "icon12",
        "apple-icon",
        "opengraph-image",
        "twitter-image2",
        "sitemap",
        "robots",
        "manifest",
        "favicon",
        "pages",
        "xpage",
    ];
    let mut paths = Vec::new();
    for directory in directories {
        for name in names {
            for ext in [
                "tsx",
                "ts",
                "js",
                "mdx",
                "png",
                "jpg",
                "ico",
                "svg",
                "xml",
                "txt",
                "json",
                "webmanifest",
                "gif",
            ] {
                paths.push(format!("{directory}{name}.{ext}"));
            }
        }
    }
    let mut total = 0;
    let mut differences = Vec::new();
    for extensions in [&EXTENSIONS[..], &["tsx", "mdx"][..]] {
        let mut child = Command::new("node")
            .args(["-e", script, &module])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(
            child.stdin.take().unwrap(),
            "{}\n{}",
            extensions.join(","),
            paths.join("\n")
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let answers = String::from_utf8(output.stdout).unwrap();
        for (rel, expected) in paths.iter().zip(answers.lines()) {
            let (directory, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            let ours = convention(name, directory.is_empty(), extensions);
            let segments: Vec<_> = if directory.is_empty() {
                Vec::new()
            } else {
                directory.split('/').map(segment).collect()
            };
            let actual = match ours {
                Some(Convention::Metadata(_)) => "metadata ".to_owned(),
                Some(Convention::Route) => format!("route {}", pattern(&segments)),
                Some(Convention::Page) => format!("page {}", pattern(&segments)),
                Some(Convention::Layout) => "layout ".to_owned(),
                Some(Convention::Default) => "default ".to_owned(),
                // The app loader resolves these by exact name, not by the matcher.
                Some(_) | None => "- ".to_owned(),
            };
            if actual != expected {
                differences.push(format!("{extensions:?} {rel}: {actual:?} != {expected:?}"));
            }
            total += 1;
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences:\n{}",
        differences.len(),
        differences
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    eprintln!("Next.js 16.4.2 route oracle: {total} cases");
}
