//! Next.js facts through the public facade, without executing source.

use std::fs;

use frameshift_engine::bindings::{ExportName, ImportedName};
use frameshift_engine::evaluate::{Reason, Value};
use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{ModuleError, ModuleState};
use frameshift_engine::nextjs::{
    AppError, AppFacts, BundlerSetting, ConfigFile, ConfigLoad, ConfigProperty, FacetKind,
    FactError, FactLimits, FrameworkModuleFacts, ImportedUseKind, PageExtensions,
    analyze_app_facts,
};

mod support;

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn open(name: &str, files: &[(&str, &str)]) -> Snapshot {
    let root = support::temporary().join("nextjs-facts").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    for (name, text) in files {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    let mut findings = FindingCollector::new(FindingLimits::default());
    snapshot(&root, Limits::default().snapshot(), &mut findings).unwrap()
}

fn analyze(name: &str, files: &[(&str, &str)], config: Option<ConfigFile>) -> AppFacts {
    analyze_app_facts(
        &open(name, files),
        None,
        &path("app"),
        EXTENSIONS,
        config.as_ref(),
        FactLimits::default(),
    )
    .unwrap()
}

fn module<'a>(facts: &'a AppFacts, name: &str) -> &'a FrameworkModuleFacts {
    facts.modules.iter().find(|m| m.path == path(name)).unwrap()
}

fn config_file(name: &str) -> ConfigFile {
    ConfigFile {
        path: path(name),
        load: ConfigLoad::Loaded,
    }
}

fn named(facts: &FrameworkModuleFacts, name: &str) -> Value {
    facts
        .exports
        .iter()
        .find(|e| e.name == ExportName::Named(name.into()))
        .unwrap()
        .value
        .clone()
}

const PAGE: &str = r#"import Link from 'next/link';
import { cookies as jar, headers } from 'next/headers';
import * as navigation from 'next/navigation';
import type { NextRequest } from 'next/server';
import { headers as fake } from 'fictional-framework';
export const dynamic = 'force-static';
export const dynamicParams = false;
export const revalidate = 60 * 2;
export const fetchCache = 'force-cache';
export const runtime = 'nodejs';
export const preferredRegion = ['home', 'fictional-region'];
export const maxDuration = 30;
export const metadata = { title: 'Fictional', description: process.env.DESCRIPTION };
export const viewport = { width: 'device-width' };
export async function generateStaticParams() { throw new Error('must never execute'); }
export function generateMetadata() { return { title: 'dynamic' }; }
export function generateViewport() { return {}; }
function shadow(jar, navigation, Link) { jar(); navigation.useRouter(); return <Link />; }
export default function Page() {
  const alias = jar;
  jar();
  (headers)();
  navigation.usePathname();
  navigation['useRouter']?.();
  navigation[key]();
  fake();
  return <Link href='/'><Link>Text</Link></Link>;
}
"#;

#[test]
fn convention_values_and_api_uses_keep_names_scopes_and_exact_spans() {
    let facts = analyze("convention-and-apis", &[("app/page.tsx", PAGE)], None);
    let page = module(&facts, "app/page.tsx");
    assert_eq!(page.exports.len(), 12);
    assert_eq!(named(page, "dynamic"), Value::String("force-static".into()));
    assert_eq!(
        named(page, "revalidate"),
        Value::Number(frameshift_engine::evaluate::Number(120.0))
    );
    assert!(
        matches!(named(page, "generateStaticParams"), Value::Unknown(u) if u.reason == Reason::Function)
    );
    let metadata = named(page, "metadata");
    assert!(
        matches!(&metadata, Value::Object(o) if o.get("title") == Some(&Value::String("Fictional".into())))
    );
    assert_eq!(
        metadata.unknowns()[0].reason,
        Reason::Environment(Some("DESCRIPTION".into()))
    );
    let uses: Vec<_> = page.api_uses().collect();
    assert_eq!(uses.len(), 8, "{uses:#?}");
    let calls: Vec<_> = uses
        .iter()
        .filter(|u| u.kind == ImportedUseKind::Call)
        .map(|u| &PAGE[u.span.start()..u.span.end()])
        .collect();
    assert_eq!(
        calls,
        [
            "jar()",
            "(headers)()",
            "navigation.usePathname()",
            "navigation['useRouter']?.()",
            "navigation[key]()"
        ]
    );
    assert_eq!(uses[0].kind, ImportedUseKind::Reference);
    assert_eq!(uses[0].imported, ImportedName::Named("cookies".into()));
    assert_eq!(
        &PAGE[uses[0].source.start()..uses[0].source.end()],
        "'next/headers'"
    );
    assert_eq!(uses[3].members, [Some("usePathname".into())]);
    assert_eq!(uses[5].members, [None]);
    assert_eq!(
        uses.iter()
            .filter(|u| u.kind == ImportedUseKind::Render)
            .count(),
        2
    );
    assert_eq!(
        page.import_uses
            .iter()
            .filter(|u| u.specifier == "fictional-framework")
            .count(),
        1
    );
    assert!(
        !page
            .import_uses
            .iter()
            .any(|u| u.imported == ImportedName::Named("NextRequest".into()))
    );
}

#[test]
fn configuration_preserves_known_properties_unknown_causes_and_bundler_concerns() {
    let text = r#"import { base } from './config-base.mjs';
export default {
  ...process.env.CONFIG,
  ...base,
  pageExtensions: ['page.tsx', 'mdx'],
  images: { unoptimized: true, loader: process.env.LOADER },
  webpack(config) { throw new Error('must not run'); },
  turbopack: { resolveAlias: { fake: './real' } },
  experimental: { turbo: { rules: {} } },
};"#;
    let facts = analyze(
        "configuration",
        &[
            ("app/page.tsx", "export default () => null;"),
            (
                "config-base.mjs",
                "export const base = { output: 'export', trailingSlash: true };",
            ),
            ("next.config.mjs", text),
        ],
        Some(config_file("next.config.mjs")),
    );
    let config = &facts.configuration;
    assert!(
        matches!(config.property(&["output"]), ConfigProperty::Value(Value::String(v)) if v == "export")
    );
    assert_eq!(config.property(&["unknown"]), ConfigProperty::Unknown);
    assert_eq!(
        config.property(&["images", "missing"]),
        ConfigProperty::Absent
    );
    assert_eq!(
        config.property(&["images", "unoptimized"]),
        ConfigProperty::Value(&Value::Bool(true))
    );
    assert_eq!(
        config.page_extensions(),
        PageExtensions::Known(vec!["page.tsx".into(), "mdx".into()])
    );
    assert_eq!(
        config
            .bundler_settings()
            .iter()
            .map(|(kind, _)| *kind)
            .collect::<Vec<_>>(),
        [
            BundlerSetting::Webpack,
            BundlerSetting::Turbopack,
            BundlerSetting::LegacyTurbo
        ]
    );
    assert!(
        matches!(config.bundler_settings()[0].1, ConfigProperty::Value(Value::Unknown(u)) if u.reason == Reason::Function)
    );
    let value = config.value.as_ref().unwrap();
    assert!(
        value
            .unknowns()
            .iter()
            .any(|u| u.reason == Reason::Environment(Some("CONFIG".into())))
    );
    assert!(
        value
            .unknowns()
            .iter()
            .any(|u| u.reason == Reason::Environment(Some("LOADER".into())))
    );
    // Route discovery keeps its explicit caller options; configuration is evidence.
    assert!(
        facts
            .app
            .routes
            .files
            .iter()
            .any(|f| f.path == path("app/page.tsx"))
    );
}

#[test]
fn wrappers_and_function_configs_never_fall_back_to_defaults() {
    let cases = [
        (
            "import wrap from 'fictional-plugin'; export default wrap({ output: 'export' });",
            Reason::Call(Some("wrap".into())),
        ),
        (
            "export default () => ({ output: 'export' });",
            Reason::Function,
        ),
    ];
    for (i, (text, reason)) in cases.into_iter().enumerate() {
        let facts = analyze(
            &format!("wrapper-{i}"),
            &[
                ("app/page.tsx", "export default () => null;"),
                ("next.config.mjs", text),
            ],
            Some(config_file("next.config.mjs")),
        );
        let value = facts.configuration.value.as_ref().unwrap();
        assert!(matches!(value, Value::Unknown(u) if u.reason == reason));
        assert_eq!(
            facts.configuration.page_extensions(),
            PageExtensions::Unknown
        );
        assert_eq!(facts.configuration.bundler_settings().len(), 3);
        if i == 0 {
            let usage = &module(&facts, "next.config.mjs").import_uses[0];
            assert_eq!(usage.specifier, "fictional-plugin");
            assert_eq!(usage.imported, ImportedName::Default);
            assert_eq!(usage.kind, ImportedUseKind::Call);
            assert!(
                module(&facts, "next.config.mjs")
                    .api_uses()
                    .next()
                    .is_none()
            );
        }
    }
}

#[test]
fn missing_configuration_differs_from_loading_uncertainty_and_invalid_values() {
    let files = [
        ("app/page.tsx", "export default () => null;"),
        (
            "next.config.mts",
            "export default { pageExtensions: ['ts'] };",
        ),
    ];
    let absent = analyze("no-config", &files, None);
    assert_eq!(
        absent.configuration.page_extensions(),
        PageExtensions::Known(EXTENSIONS.iter().map(|s| (*s).into()).collect())
    );
    assert!(absent.configuration.bundler_settings().is_empty());
    for load in [ConfigLoad::DependsOnNode, ConfigLoad::Unsupported] {
        let config = ConfigFile {
            path: path("next.config.mts"),
            load,
        };
        let facts = analyze(&format!("load-{load:?}"), &files, Some(config));
        assert!(facts.configuration.value.is_none());
        assert_eq!(
            facts.configuration.page_extensions(),
            PageExtensions::Unknown
        );
    }
    for (i, value) in [
        "null",
        "[]",
        "42",
        "{ pageExtensions: [] }",
        "{ pageExtensions: ['../ts'] }",
        "{ pageExtensions: [3] }",
    ]
    .into_iter()
    .enumerate()
    {
        let text = format!("export default {value};");
        let facts = analyze(
            &format!("invalid-config-{i}"),
            &[
                ("app/page.tsx", "export default () => null;"),
                ("next.config.mjs", &text),
            ],
            Some(config_file("next.config.mjs")),
        );
        assert_eq!(
            facts.configuration.page_extensions(),
            PageExtensions::Invalid,
            "{value}"
        );
    }
}

#[test]
fn unknown_spread_order_is_visible_to_configuration_consumers() {
    let facts = analyze(
        "config-overwrite",
        &[
            ("app/page.tsx", "export default () => null;"),
            (
                "next.config.mjs",
                "export default { output: 'export', ...plugin, trailingSlash: false };",
            ),
        ],
        Some(config_file("next.config.mjs")),
    );
    assert!(matches!(
        facts.configuration.property(&["output"]),
        ConfigProperty::Value(Value::Unknown(_))
    ));
    assert_eq!(
        facts.configuration.property(&["trailingSlash"]),
        ConfigProperty::Value(&Value::Bool(false))
    );
    assert_eq!(
        facts.configuration.property(&["pageExtensions"]),
        ConfigProperty::Unknown
    );
}

#[test]
fn convention_handlers_generators_and_star_exports_are_explicit() {
    let facts = analyze(
        "handlers",
        &[
            (
                "app/page.tsx",
                "export * from '../shared'; export { metadata } from '../shared'; export type { runtime } from '../shared';",
            ),
            (
                "shared.ts",
                "export const metadata = { title: 'shared' }; export type runtime = string;",
            ),
            (
                "app/api/feed/route.ts",
                "export function GET() { throw new Error('never run'); } export const POST = unknown; export const revalidate = false;",
            ),
            (
                "app/sitemap.ts",
                "export default function sitemap() { throw 0; } export function generateSitemaps() { return []; }",
            ),
            ("lib/plain.ts", "export const dynamic = 'force-dynamic';"),
        ],
        None,
    );
    let page = module(&facts, "app/page.tsx");
    assert_eq!(page.star_exports.len(), 1);
    assert_eq!(page.exports.len(), 1);
    assert!(named(page, "metadata").is_known());
    let route = module(&facts, "app/api/feed/route.ts");
    assert_eq!(
        route
            .exports
            .iter()
            .filter(|e| e.kind == FacetKind::Handler)
            .count(),
        2
    );
    assert!(matches!(named(route, "GET"), Value::Unknown(u) if u.reason == Reason::Function));
    assert_eq!(
        module(&facts, "app/sitemap.ts")
            .exports
            .iter()
            .map(|e| e.kind)
            .collect::<Vec<_>>(),
        [FacetKind::MetadataRoute, FacetKind::Generator]
    );
    assert!(module(&facts, "lib/plain.ts").exports.is_empty());
}

#[test]
fn syntax_failures_withhold_facts_and_source_changes_stop_the_analysis() {
    let files = [
        ("app/page.tsx", "export const metadata = ;"),
        ("next.config.mjs", "export default {;"),
    ];
    let facts = analyze("bad-syntax", &files, Some(config_file("next.config.mjs")));
    assert!(facts.modules.is_empty());
    assert!(
        matches!(facts.configuration.value, Some(Value::Unknown(u)) if u.reason == Reason::Syntax)
    );
    assert!(matches!(
        facts.app.graph.node(&path("app/page.tsx")).unwrap().state,
        ModuleState::SyntaxError(_)
    ));
    let snapshot = open("changed", &[("app/page.tsx", "export default () => null;")]);
    let file = support::temporary().join("nextjs-facts/changed/app/page.tsx");
    fs::write(file, "changed after discovery").unwrap();
    assert!(
        analyze_app_facts(
            &snapshot,
            None,
            &path("app"),
            EXTENSIONS,
            None,
            FactLimits::default()
        )
        .is_err()
    );
}

#[test]
fn commonjs_and_ancestor_configuration_use_only_the_snapshot() {
    let files = [
        ("apps/site/app/page.tsx", "export default () => null;"),
        (
            "next.config.js",
            "module.exports = { output: 'export', webpack: undefined };",
        ),
        ("package.json", "{}"),
    ];
    let snapshot = open("ancestor-config", &files);
    let facts = analyze_app_facts(
        &snapshot,
        Some(&path("apps/site")),
        &path("apps/site/app"),
        EXTENSIONS,
        Some(&config_file("next.config.js")),
        FactLimits::default(),
    )
    .unwrap();
    assert_eq!(
        facts.configuration.property(&["output"]),
        ConfigProperty::Value(&Value::String("export".into()))
    );
    assert!(
        facts
            .modules
            .iter()
            .all(|m| m.path.as_str().starts_with("apps/site/"))
    );
}

#[test]
fn extraction_is_deterministic_and_cumulative_budgets_fail_explicitly() {
    let snapshot = open("determinism", &[("app/page.tsx", PAGE)]);
    let run = |limits| analyze_app_facts(&snapshot, None, &path("app"), EXTENSIONS, None, limits);
    assert_eq!(
        run(FactLimits::default()).unwrap(),
        run(FactLimits::default()).unwrap()
    );
    let mut limits = FactLimits::default();
    limits.evaluate.max_work = 0;
    let limited = run(limits).unwrap();
    assert!(
        module(&limited, "app/page.tsx")
            .exports
            .iter()
            .all(|e| matches!(&e.value, Value::Unknown(u) if matches!(u.reason, Reason::WorkLimit | Reason::Function)))
    );
    limits.graph.modules.max_work = 0;
    assert!(matches!(
        run(limits),
        Err(FactError::App(AppError::Modules(ModuleError::WorkLimit))
            | FactError::Evaluate(frameshift_engine::evaluate::EvaluateError::Modules(
                ModuleError::WorkLimit
            )))
    ));
}

fn golden() -> String {
    let facts = analyze(
        "golden",
        &[
            ("app/page.tsx", PAGE),
            (
                "next.config.mjs",
                "export default { ...process.env.CONFIG, output: 'export', webpack() {} };",
            ),
        ],
        Some(config_file("next.config.mjs")),
    );
    format!(
        "configuration {:#?}\nmodules {:#?}\n",
        facts.configuration, facts.modules
    )
}

#[test]
fn nextjs_fact_golden() {
    assert_eq!(golden(), include_str!("golden/nextjs-facts.txt"));
}

#[test]
#[ignore = "explicit golden update"]
fn update_nextjs_fact_golden() {
    fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/nextjs-facts.txt"),
        golden(),
    )
    .unwrap();
}

#[test]
fn retained_import_provenance_and_composed_visitors_remain_bounded() {
    let specifier = format!("next/{}", "long".repeat(256));
    let text = format!(
        "import value from '{specifier}'; {} export default () => null;",
        "value();".repeat(40)
    );
    let snapshot = open("retained-import-budget", &[("app/page.tsx", &text)]);
    let mut limits = FactLimits::default();
    limits.graph.modules.max_work = 20_000;
    assert!(matches!(
        analyze_app_facts(&snapshot, None, &path("app"), EXTENSIONS, None, limits),
        Err(FactError::App(AppError::Modules(ModuleError::WorkLimit)))
    ));
    let mut visited = Vec::new();
    frameshift_engine::nextjs::analyze_app_facts_with(
        &snapshot,
        None,
        &path("app"),
        EXTENSIONS,
        None,
        FactLimits::default(),
        &mut |path, _, _| {
            visited.push(path.clone());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(visited, [path("app/page.tsx")]);
    let failure = frameshift_engine::nextjs::analyze_app_facts_with(
        &snapshot,
        None,
        &path("app"),
        EXTENSIONS,
        None,
        FactLimits::default(),
        &mut |_, _, _| Err(ModuleError::EdgeLimit),
    );
    assert_eq!(
        failure,
        Err(FactError::App(AppError::Modules(ModuleError::EdgeLimit)))
    );
}

#[test]
fn generic_optional_escaped_and_namespace_jsx_uses_preserve_import_provenance() {
    let text = r#"import * as n from 'next/navigation';
import Widget from 'next/fictional-widget';
import { cookies as read } from 'next/headers';
export default function Page() {
    (read as (() => unknown))?.();
    n.use\u0052outer();
    return <n.Widget<string>><Widget<string> /></n.Widget>;
}"#;
    let facts = analyze("typed-apis", &[("app/page.tsx", text)], None);
    let uses: Vec<_> = module(&facts, "app/page.tsx").api_uses().collect();
    assert_eq!(uses.len(), 4, "{uses:#?}");
    assert_eq!(uses[0].kind, ImportedUseKind::Call);
    assert_eq!(uses[1].members, [Some("useRouter".into())]);
    assert_eq!(uses[2].members, [Some("Widget".into())]);
    assert_eq!(uses[2].kind, ImportedUseKind::Render);
    assert_eq!(uses[3].kind, ImportedUseKind::Render);
}
