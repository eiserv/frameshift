//! Feature catalog and detection contracts (`D-099`): the catalog agrees
//! with the compatibility matrix, and detection maps model facts to catalog
//! features on fictional projects created at test time. Manifests naming
//! the real `next` package exist only during the test (`Q-014`).

use std::fs;
use std::path::PathBuf;

use frameshift_engine::catalog::{self, FAMILIES, FEATURES, Status, V1};
use frameshift_engine::features::{self, Cause, Detail, Location, Occurrence};
use frameshift_engine::foundation::{
    FeatureId, FindingCollector, FindingLimits, IdentifierLimits, PathLimits, SourcePath,
};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::UnresolvedReason;
use frameshift_engine::nextjs::{Detection, UnknownReason, detect};
use frameshift_engine::package::load_workspace;
use frameshift_engine::project::{self, Project};

mod support;

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];

fn statuses(cell: &str) -> V1 {
    match cell {
        "Out of scope" => V1::OutOfScope,
        _ => {
            let mut statuses: Vec<Status> = cell
                .split([',', ' '])
                .filter(|word| !word.is_empty() && *word != "or")
                .map(|word| match word {
                    "A" => Status::Auto,
                    "R" => Status::Review,
                    "U" => Status::Unsupported,
                    "Unknown" => Status::Unknown,
                    other => panic!("unexpected V1 cell word {other:?}"),
                })
                .collect();
            statuses.sort();
            V1::Assessed(Box::leak(statuses.into_boxed_slice()))
        }
    }
}

/// Every row of every table with the matrix header, as (ID, V1) pairs.
fn matrix() -> Vec<(String, V1)> {
    let mut rows = Vec::new();
    let mut table = false;
    for line in include_str!("../../../docs/MIGRATION_SPEC.md").lines() {
        if line == "| Feature | Feature ID | V1 | Notes |" {
            table = true;
            continue;
        }
        if !line.starts_with('|') {
            table = false;
        }
        if table && !line.starts_with("| ---") {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for id in cells[2].split('`').skip(1).step_by(2) {
                rows.push((id.to_owned(), statuses(cells[3])));
            }
        }
    }
    rows
}

#[test]
fn catalog_and_matrix_agree() {
    let rows = matrix();
    let mut concrete = Vec::new();
    let mut families = Vec::new();
    for (id, v1) in &rows {
        if let Some(prefix) = id.strip_suffix(".*") {
            let family = FAMILIES
                .iter()
                .find(|family| family.prefix == prefix)
                .unwrap_or_else(|| panic!("family {id} is not in the catalog"));
            assert_eq!(family.v1, *v1, "{id}");
            families.push(prefix.to_owned());
        } else {
            let feature =
                catalog::feature(id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(feature.v1, *v1, "{id}");
            concrete.push(id.clone());
        }
    }
    concrete.sort();
    let before = concrete.len();
    concrete.dedup();
    assert_eq!(before, concrete.len(), "a feature ID is listed twice");
    let catalog: Vec<&str> = FEATURES.iter().map(|feature| feature.id).collect();
    assert_eq!(
        concrete, catalog,
        "the matrix and the catalog list other IDs"
    );
    families.sort();
    let catalog: Vec<&str> = FAMILIES.iter().map(|family| family.prefix).collect();
    assert_eq!(families, catalog);
    for feature in FEATURES {
        assert_eq!(
            FeatureId::new(feature.id, IdentifierLimits::default())
                .unwrap()
                .as_str(),
            feature.id
        );
    }
    assert_eq!(catalog::PATH, "nextjs-to-astro-preact");
}

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn create(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = support::temporary().join("features").join(name);
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

/// Detection, the model, and the occurrences of the package `package`.
fn analyze(
    name: &str,
    files: &[(&str, &str)],
    package: Option<&str>,
) -> (Snapshot, Project, Detection, Vec<Occurrence>) {
    let root = create(name, files);
    let limits = Limits::default();
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(&root, limits.snapshot(), &mut findings).unwrap();
    let workspace = load_workspace(&snapshot, limits.package(), &mut findings).unwrap();
    let package = package.map(path);
    let selected = workspace.select(package.as_ref()).unwrap();
    let detection = detect(snapshot.inventory(), &selected, &mut findings).unwrap();
    let app = detection.app.as_ref().unwrap().path.clone();
    let model = project::build(
        &snapshot,
        package.as_ref(),
        &app,
        EXTENSIONS,
        detection.config.as_ref(),
        &limits,
    )
    .unwrap();
    let occurrences = features::detect(&snapshot, &model, &detection);
    (snapshot, model, detection, occurrences)
}

const MANIFEST: &str = "{\"name\":\"fixture-site\",\"private\":true,\
    \"dependencies\":{\"next\":\"16.2.0\",\"react\":\"19.2.0\",\"react-dom\":\"19.2.0\"}}\n";

const SOURCE: &[(&str, &str)] = &[
    ("package.json", MANIFEST),
    ("bun.lockb", "binary"),
    ("vercel.json", "{}\n"),
    (".github/workflows/ci.yml", "on: push\n"),
    (
        "next.config.mjs",
        "const base = '/docs';\n\
         export default {\n\
         output: 'export',\n\
         trailingSlash: true,\n\
         basePath: base,\n\
         images: { unoptimized: true, loader: 'custom', deviceSizes: [640] },\n\
         reactStrictMode: true,\n\
         webpack(config) { return config; },\n\
         experimental: { turbo: {}, ppr: true },\n\
         rewrites: async () => [],\n\
         redirects: async () => [],\n\
         env: { SITE: process.env.SITE },\n\
         pageExtensions: ['tsx', 'mdx'],\n\
         futureKey: 1,\n\
         };\n",
    ),
    (
        "tsconfig.json",
        "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"@/*\": [\"./*\"] } } }\n",
    ),
    ("tailwind.config.ts", "export default {};\n"),
    ("jest.config.js", "module.exports = {};\n"),
    ("middleware.ts", "export function middleware() {}\n"),
    ("public/logo.svg", "<svg/>\n"),
    ("public/site.css", "body {}\n"),
    (
        "pages/legacy.tsx",
        "export default function Legacy() { return null; }\n",
    ),
    (
        "app/layout.tsx",
        "import './globals.css';\nimport 'server-only';\n\
         export const metadata = { title: 'Site', openGraph: { title: 'x', videos: [] }, category: 'tech' };\n\
         export default function RootLayout({ children }) {\n\
         return <html><body>{children}</body></html>;\n}\n",
    ),
    ("app/globals.css", "body { margin: 0 }\n"),
    ("app/theme.module.scss", ".a { color: red }\n"),
    ("app/data.json", "{\"x\": 1}\n"),
    (
        "app/page.tsx",
        "import Link from 'next/link';\nimport Image from 'next/image';\n\
         import logo from '../public/logo.svg';\nimport data from './data.json';\n\
         import { Suspense } from 'react';\nimport Counter from '@/components/Counter';\n\
         import { Widget } from 'ui-kit';\nimport missing from './missing';\n\
         import { make } from '../lib/make';\n\
         export const revalidate = 60;\nexport const runtime = 'nodejs';\n\
         export const fetchCache = 'force-cache';\n\
         const Dyn = make();\n\
         export default async function Home() {\n\
         const name = 'x';\nawait import(name);\n\
         const res = await fetch('https://example.com');\n\
         return <main><title>Home</title><Suspense><Counter /></Suspense>\
         <Link href=\"/about\">About</Link><Image src={logo} alt=\"\" /><Widget /><Dyn />{data.x}</main>;\n}\n",
    ),
    (
        "app/about/page.tsx",
        "'use client';\nimport { useState, useId } from 'react';\n\
         import { useRouter } from 'next/navigation';\n\
         export default function About() {\n\
         const [n] = useState(0);\nconst id = useId();\nuseRouter();\n\
         return <p id={id}>{n}</p>;\n}\n",
    ),
    (
        "app/blog/[slug]/page.tsx",
        "import { notFound } from 'next/navigation';\n\
         export async function generateStaticParams() { return [{ slug: 'a' }]; }\n\
         export async function generateMetadata() { return {}; }\n\
         export default function Post({ params, searchParams }) {\n\
         if (!params) notFound();\nreturn <article>{searchParams.q}</article>;\n}\n",
    ),
    (
        "app/docs/[...path]/page.tsx",
        "export const dynamic = 'force-dynamic';\n\
         export default function Docs() { return null; }\n",
    ),
    (
        "app/(marketing)/pricing/page.tsx",
        "export const dynamic = 'force-static';\n\
         export default function Pricing() { return null; }\n",
    ),
    (
        "app/shop/layout.tsx",
        "export async function generateStaticParams() { return []; }\n\
         export default function Shop({ children }) { return children; }\n",
    ),
    (
        "app/shop/[[...filter]]/page.tsx",
        "export * from './params';\nexport default function Filter() { return null; }\n",
    ),
    ("app/shop/[[...filter]]/params.ts", "export const x = 1;\n"),
    (
        "app/@modal/default.tsx",
        "export default function Modal() { return null; }\n",
    ),
    (
        "app/feed.xml/route.ts",
        "export function GET() { return new Response('x'); }\n",
    ),
    (
        "app/api/submit/route.ts",
        "import { NextResponse } from 'next/server';\n\
         export async function POST() { return NextResponse.json({}); }\n",
    ),
    (
        "app/loading.tsx",
        "export default function Loading() { return null; }\n",
    ),
    (
        "app/error.tsx",
        "'use client';\nexport default function Error() { return null; }\n",
    ),
    (
        "app/not-found.tsx",
        "export default function NotFound() { return null; }\n",
    ),
    (
        "app/template.tsx",
        "export default function T({ children }) { return children; }\n",
    ),
    (
        "app/forbidden.tsx",
        "export default function F() { return null; }\n",
    ),
    ("app/icon.png", "png"),
    (
        "app/sitemap.ts",
        "export default function sitemap() { return []; }\n",
    ),
    ("app/_lib/util.ts", "export const u = 1;\n"),
    (
        "components/Counter.tsx",
        "'use client';\nimport { useEffect, useActionState } from 'react';\n\
         import * as React from 'react';\nimport dynamic from 'next/dynamic';\n\
         const Chart = dynamic(() => import('./Chart'), { ssr: false });\n\
         const Map = dynamic(() => import('./Map'));\n\
         export default function Counter() {\n\
         useEffect(() => {}, []);\nReact.useMemo(() => 1, []);\n\
         useActionState(async () => 1, 0);\n\
         const v = process.env.NEXT_PUBLIC_API;\n\
         JSON.stringify(process.env);\n\
         return <div><style jsx>{`div{}`}</style><Chart /><Map /><meta name=\"x\" /></div>;\n}\n",
    ),
    (
        "components/Chart.tsx",
        "export default function Chart() { return <canvas />; }\n",
    ),
    (
        "components/Map.tsx",
        "export default function Map() { return <svg />; }\n",
    ),
    ("components/Broken.tsx", "export default function (\n"),
    (
        "lib/make.ts",
        "export function make() { return () => null; }\n",
    ),
    (
        "lib/actions.ts",
        "'use server';\nimport { cookies } from 'next/headers';\n\
         import { revalidatePath, unstable_cache } from 'next/cache';\n\
         export async function save() { cookies(); revalidatePath('/'); unstable_cache(() => 1); }\n",
    ),
    (
        "lib/legacy.ts",
        "import Head from 'next/head';\nimport getConfig from 'next/config';\n\
         import { userAgent } from 'next/server';\n\
         import { useSelectedLayoutSegment } from 'next/navigation';\n\
         export const h = Head;\nexport const c = getConfig();\n\
         export const u = userAgent;\nexport const s = useSelectedLayoutSegment;\n",
    ),
];

fn render(occurrences: &[Occurrence]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for occurrence in occurrences {
        let location = match &occurrence.location {
            Location::Project => "project".to_owned(),
            Location::File(path) => path.as_str().to_owned(),
            Location::Text { path, span } => {
                let text = SOURCE
                    .iter()
                    .find(|(name, _)| *name == path.as_str())
                    .map(|(_, text)| *text)
                    .unwrap();
                let excerpt: String = text[span.start()..span.end()].chars().take(32).collect();
                format!(
                    "{}@{}..{} {:?}",
                    path.as_str(),
                    span.start(),
                    span.end(),
                    excerpt.replace('\n', " ")
                )
            }
        };
        let status = occurrence.status.map_or("-", Status::as_str);
        writeln!(
            out,
            "{} {} {} {:?}",
            location, occurrence.feature.id, status, occurrence.detail
        )
        .unwrap();
    }
    out
}

fn has(occurrences: &[Occurrence], id: &str, location: &str) -> bool {
    occurrences.iter().any(|occurrence| {
        occurrence.feature.id == id
            && occurrence
                .location
                .path()
                .is_some_and(|path| path.as_str() == location)
    })
}

#[test]
fn detection_maps_facts_to_catalog_features() {
    let (_, model, detection, occurrences) = analyze("site", SOURCE, None);
    assert_eq!(
        detection.config.as_ref().unwrap().path,
        path("next.config.mjs")
    );
    assert!(model.configuration().value.is_some());
    // Fixed statuses come only from single-status entries.
    for occurrence in &occurrences {
        assert_eq!(occurrence.status, occurrence.feature.v1.fixed());
    }
    for (id, file) in [
        ("nextjs.project.app-router", "app"),
        ("nextjs.project.pages-router", "pages"),
        ("npm.lockfile.bun-binary", "bun.lockb"),
        ("deploy.config", "vercel.json"),
        ("deploy.config", ".github/workflows/ci.yml"),
        ("test.setup", "jest.config.js"),
        ("css.tailwind", "tailwind.config.ts"),
        ("ts.config", "tsconfig.json"),
        ("asset.public", "public/logo.svg"),
        ("asset.public", "public/site.css"),
        ("css.global", "app/globals.css"),
        ("css.module", "app/theme.module.scss"),
        ("css.sass", "app/theme.module.scss"),
        ("nextjs.route.private", "app/_lib"),
        ("nextjs.route.page", "app/page.tsx"),
        ("nextjs.route.dynamic", "app/blog/[slug]/page.tsx"),
        (
            "nextjs.api.search-params-server",
            "app/blog/[slug]/page.tsx",
        ),
        ("nextjs.route.catch-all", "app/docs/[...path]/page.tsx"),
        (
            "nextjs.route.dynamic-unenumerable",
            "app/docs/[...path]/page.tsx",
        ),
        ("nextjs.segment.dynamic", "app/docs/[...path]/page.tsx"),
        ("nextjs.route.group", "app/(marketing)/pricing/page.tsx"),
        (
            "nextjs.route.optional-catch-all",
            "app/shop/[[...filter]]/page.tsx",
        ),
        ("nextjs.route.parallel", "app/@modal/default.tsx"),
        ("nextjs.file.route-handler-static", "app/feed.xml/route.ts"),
        ("nextjs.file.route-handler", "app/api/submit/route.ts"),
        ("nextjs.file.proxy", "middleware.ts"),
        ("nextjs.metadata.file", "app/icon.png"),
        ("nextjs.metadata.route", "app/sitemap.ts"),
        ("component.client-page", "app/about/page.tsx"),
        ("component.island", "components/Counter.tsx"),
        ("nextjs.api.dynamic-no-ssr", "components/Counter.tsx"),
        ("nextjs.api.dynamic", "components/Counter.tsx"),
        ("css.styled-jsx", "components/Counter.tsx"),
        ("react.hook.react19", "components/Counter.tsx"),
        ("react.hook.use-memo", "components/Counter.tsx"),
        ("react.head-hoisting", "components/Counter.tsx"),
        ("react.head-hoisting", "app/page.tsx"),
        ("nextjs.api.server-action", "lib/actions.ts"),
        ("nextjs.api.request", "lib/actions.ts"),
        ("nextjs.api.server-only", "lib/actions.ts"),
        ("react.cache", "lib/actions.ts"),
        ("nextjs.api.pages-router", "lib/legacy.ts"),
        ("nextjs.api.config", "lib/legacy.ts"),
        ("nextjs.api.server-only", "lib/legacy.ts"),
        ("nextjs.guard", "app/layout.tsx"),
        ("nextjs.metadata.static", "app/layout.tsx"),
        ("nextjs.metadata.unsupported-field", "app/layout.tsx"),
        ("nextjs.metadata.dynamic", "app/blog/[slug]/page.tsx"),
        ("asset.import", "app/page.tsx"),
        ("asset.json-import", "app/page.tsx"),
        ("nextjs.fetch", "app/page.tsx"),
        ("react.suspense-server", "app/page.tsx"),
        ("env.reference", "components/Counter.tsx"),
        ("nextjs.config.output", "next.config.mjs"),
        ("nextjs.config.webpack", "next.config.mjs"),
        ("nextjs.config.turbopack", "next.config.mjs"),
        ("nextjs.config.page-extensions", "next.config.mjs"),
        ("nextjs.config.images.loader", "next.config.mjs"),
        ("nextjs.config.unknown", "next.config.mjs"),
    ] {
        assert!(has(&occurrences, id, file), "{id} at {file}");
    }
    // Route handlers keep `next/server` with the handler; the generator
    // in the page establishes the dynamic segment.
    assert!(!has(
        &occurrences,
        "nextjs.api.server-only",
        "app/api/submit/route.ts"
    ));
    assert!(!has(
        &occurrences,
        "nextjs.route.dynamic-unenumerable",
        "app/blog/[slug]/page.tsx"
    ));
    assert!(!has(
        &occurrences,
        "nextjs.route.page",
        "app/blog/[slug]/page.tsx"
    ));
    // Public stylesheets are verbatim assets, not CSS features.
    assert!(!has(&occurrences, "css.global", "public/site.css"));
    // Static Response values in a page are not request-time APIs, and the
    // client page's searchParams evidence would be ignored.
    assert!(!has(
        &occurrences,
        "nextjs.api.search-params-server",
        "app/about/page.tsx"
    ));
    let unknown = |cause: &Cause| {
        occurrences
            .iter()
            .any(|o| o.detail == Detail::Unknown(cause.clone()))
    };
    for cause in [
        Cause::Unparsed,
        Cause::Unresolved(UnresolvedReason::NotFound),
        Cause::Unresolved(UnresolvedReason::Computed),
        Cause::Render(UnknownReason::Call(Some("make".to_owned()))),
        Cause::Enumeration,
        Cause::SegmentValue,
        Cause::SegmentOption {
            name: "fetchCache".to_owned(),
        },
        Cause::Environment(frameshift_engine::nextjs::VariableAccess::Whole),
        Cause::Api {
            specifier: "next/navigation".to_owned(),
            export: Some("useSelectedLayoutSegment".to_owned()),
        },
        Cause::Configuration {
            key: Some("futureKey".to_owned()),
        },
        Cause::Configuration {
            key: Some("experimental.ppr".to_owned()),
        },
        Cause::Configuration {
            key: Some("images.deviceSizes".to_owned()),
        },
        Cause::Configuration {
            key: Some("redirects".to_owned()),
        },
    ] {
        assert!(unknown(&cause), "{cause:?}");
    }
    assert!(
        occurrences
            .iter()
            .any(|o| matches!(&o.detail, Detail::Unknown(Cause::Convention(_))))
    );
    assert!(
        occurrences
            .iter()
            .any(|o| o.feature.id == "nextjs.navigation.client" && o.location == Location::Project)
    );
}

#[test]
fn absent_and_unloaded_configuration() {
    let (_, _, detection, occurrences) = analyze(
        "no-config",
        &[
            ("package.json", MANIFEST),
            (
                "app/layout.tsx",
                "export default function L({ children }) { return children; }\n",
            ),
            (
                "app/page.tsx",
                "export default function P() { return null; }\n",
            ),
        ],
        None,
    );
    assert!(detection.config.is_none());
    assert!(
        !occurrences
            .iter()
            .any(|o| o.feature.id.starts_with("nextjs.config."))
    );
    // A function configuration is evaluated as unknown, never as defaults.
    let (_, _, _, occurrences) = analyze(
        "function-config",
        &[
            ("package.json", MANIFEST),
            (
                "next.config.js",
                "module.exports = () => ({ output: 'export' });\n",
            ),
            (
                "app/layout.tsx",
                "export default function L({ children }) { return children; }\n",
            ),
            (
                "app/page.tsx",
                "export default function P() { return null; }\n",
            ),
        ],
        None,
    );
    let config: Vec<_> = occurrences
        .iter()
        .filter(|o| o.feature.id.starts_with("nextjs.config."))
        .collect();
    assert_eq!(config.len(), 1);
    assert_eq!(config[0].feature.id, "nextjs.config.unknown");
    assert_eq!(config[0].status, Some(Status::Unknown));
}

#[test]
fn workspace_members_and_root_lockfiles() {
    let (_, model, _, occurrences) = analyze(
        "workspace",
        &[
            (
                "package.json",
                "{\"name\":\"fixture-root\",\"private\":true,\"workspaces\":[\"apps/*\"]}\n",
            ),
            ("package-lock.json", "{}\n"),
            ("apps/web/package.json", MANIFEST),
            (
                "apps/web/app/layout.tsx",
                "export default function L({ children }) { return children; }\n",
            ),
            (
                "apps/web/app/page.tsx",
                "export default function P() { return null; }\n",
            ),
            ("apps/web/vercel.json", "{}\n"),
            ("apps/other/vercel.json", "{}\n"),
        ],
        Some("apps/web"),
    );
    assert_eq!(model.package(), Some(&path("apps/web")));
    assert!(has(&occurrences, "npm.workspace", "apps/web"));
    assert!(has(&occurrences, "npm.lockfile", "package-lock.json"));
    assert!(has(&occurrences, "npm.manifest", "apps/web/package.json"));
    assert!(!has(&occurrences, "npm.manifest", "package.json"));
    assert!(has(&occurrences, "deploy.config", "apps/web/vercel.json"));
    assert!(!has(
        &occurrences,
        "deploy.config",
        "apps/other/vercel.json"
    ));
}

#[test]
fn shadowed_and_aliased_names_are_not_framework_apis() {
    let (_, _, _, occurrences) = analyze(
        "shadow",
        &[
            ("package.json", MANIFEST),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"paths\": { \"next/link\": [\"./local-link.tsx\"] } } }\n",
            ),
            (
                "local-link.tsx",
                "export default function Link() { return null; }\n",
            ),
            (
                "app/layout.tsx",
                "export default function L({ children }) { return children; }\n",
            ),
            (
                "app/page.tsx",
                "import Link from 'next/link';\n\
                 export default function P() { const fetch = () => 1; fetch(); return <Link />; }\n",
            ),
        ],
        None,
    );
    assert!(
        !occurrences
            .iter()
            .any(|o| o.feature.id == "nextjs.api.link")
    );
    assert!(!occurrences.iter().any(|o| o.feature.id == "nextjs.fetch"));
}

#[test]
fn detection_is_deterministic_and_ordered() {
    let (_, _, _, first) = analyze("repeat-a", SOURCE, None);
    let (_, _, _, second) = analyze("repeat-b", SOURCE, None);
    assert_eq!(first, second);
    let keys: Vec<_> = first
        .iter()
        .map(|o| match &o.location {
            Location::Project => (false, String::new(), None),
            Location::File(path) => (true, path.as_str().to_owned(), None),
            Location::Text { path, span } => (true, path.as_str().to_owned(), Some(*span)),
        })
        .collect();
    assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn features_golden() {
    let (_, _, _, occurrences) = analyze("golden", SOURCE, None);
    assert_eq!(render(&occurrences), include_str!("golden/features.txt"));
}

#[test]
#[ignore = "explicit golden update"]
fn update_features_golden() {
    let (_, _, _, occurrences) = analyze("golden", SOURCE, None);
    fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/features.txt"),
        render(&occurrences),
    )
    .unwrap();
}
