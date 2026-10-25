//! Source-model assembly, accounting, and route dependency contracts.

use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{EntryKind, Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{FileKind, Resolution, UnresolvedReason};
use frameshift_engine::nextjs::Convention;
use frameshift_engine::project::{self, FileState, Project, ProjectError};
use std::fs;

mod support;

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn open(name: &str, files: &[(&str, &str)], limits: &Limits) -> Snapshot {
    let root = support::temporary().join("project-model").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    for (name, text) in files {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    snapshot(
        &root,
        limits.snapshot(),
        &mut FindingCollector::new(FindingLimits::default()),
    )
    .unwrap()
}

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];
const SOURCE: &[(&str, &str)] = &[
    (
        "app/layout.tsx",
        "import '../styles/root.css'; import '../lib/layout'; export default ({children}) => children;",
    ),
    (
        "app/page.tsx",
        "import '../lib/a'; import type { T } from '../lib/types'; import 'fictional-ui'; import 'node:fs'; import './absent'; export default () => null;",
    ),
    ("app/other/page.tsx", "export default () => null;"),
    (
        "app/other/loading.tsx",
        "import '../../lib/loading'; export default () => null;",
    ),
    ("app/favicon.ico", "fictional icon"),
    (
        "app/error.tsx",
        "import '../lib/error'; export default () => null;",
    ),
    (
        "lib/a.ts",
        "import './b'; import './data.json'; import '../public/picture.svg';",
    ),
    ("lib/b.ts", "import './a'; import './broken';"),
    ("lib/broken.ts", "export const = ;"),
    ("lib/layout.ts", "export const layout = 1;"),
    ("lib/loading.ts", "export const loading = 1;"),
    ("lib/error.ts", "export const error = 1;"),
    ("lib/types.ts", "export type T = number;"),
    ("lib/data.json", "{}"),
    ("public/picture.svg", "fictional image"),
    ("styles/root.css", "body {}"),
    ("notes.txt", "unclassified, never implicitly copied"),
    (".gitignore", "hidden/\n"),
    ("hidden/private.ts", "this is ignored"),
];

fn build(name: &str) -> (Snapshot, Project) {
    let limits = Limits::default();
    let snap = open(name, SOURCE, &limits);
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    (snap, model)
}

fn route(model: &Project, file: &str) -> usize {
    model
        .nextjs()
        .routes
        .routes
        .iter()
        .position(|r| r.file.as_str() == file)
        .unwrap()
}

#[test]
fn inventories_admitted_and_excluded_entries_without_reading_assets() {
    let (snap, model) = build("accounting");
    assert_eq!(
        model.files().len(),
        snap.inventory().files().len() + snap.inventory().entries().len()
    );
    assert!(model.files().windows(2).all(|w| w[0].path < w[1].path));
    for (id, path) in snap.inventory().files().iter() {
        let file = model.files().iter().find(|f| f.path == *path).unwrap();
        assert_eq!(file.state, FileState::Admitted);
        assert_eq!(file.kind, Some(FileKind::of(path)));
        if matches!(
            file.kind,
            Some(FileKind::Asset | FileKind::Stylesheet | FileKind::Json)
        ) {
            assert!(!snap.is_read(id), "{}", path.as_str());
        }
    }
    assert!(model.files().iter().any(|f| f.path == path("hidden")
        && matches!(f.state, FileState::Other(EntryKind::Ignored { .. }))));
    assert!(
        !model
            .files()
            .iter()
            .any(|f| f.path.as_str().contains("private"))
    );
    assert_eq!(
        model
            .files()
            .iter()
            .find(|f| f.path == path("app/layout.tsx"))
            .unwrap()
            .convention,
        Some(Convention::Layout)
    );
}

#[test]
fn route_closure_includes_chains_cycles_assets_and_explicit_uncertainty() {
    let (_, model) = build("closure");
    let result = model
        .route_dependencies(route(&model, "app/page.tsx"))
        .unwrap();
    let files: Vec<_> = result.files.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        files,
        [
            "app/error.tsx",
            "app/layout.tsx",
            "app/page.tsx",
            "lib/a.ts",
            "lib/b.ts",
            "lib/broken.ts",
            "lib/data.json",
            "lib/error.ts",
            "lib/layout.ts",
            "public/picture.svg",
            "styles/root.css"
        ]
    );
    assert_eq!(result.unparsed, [&path("lib/broken.ts")]);
    assert!(
        result
            .edges
            .iter()
            .any(|e| matches!(e.edge.resolution, Resolution::External { .. }))
    );
    assert!(
        result
            .edges
            .iter()
            .any(|e| matches!(e.edge.resolution, Resolution::NodeBuiltin))
    );
    assert!(result.edges.iter().any(|e| matches!(
        e.edge.resolution,
        Resolution::Unresolved(UnresolvedReason::NotFound)
    )));
    assert!(result.edges.iter().all(|e| !e.edge.type_only));
    assert!(
        result
            .edges
            .windows(2)
            .all(|w| w[0].importer <= w[1].importer)
    );
    let other = model
        .route_dependencies(route(&model, "app/other/page.tsx"))
        .unwrap();
    assert!(other.files.contains(&&path("lib/loading.ts")));
    assert!(!other.files.contains(&&path("lib/a.ts")));
    let icon = model
        .route_dependencies(route(&model, "app/favicon.ico"))
        .unwrap();
    assert!(icon.files.contains(&&path("app/favicon.ico")));
    assert_eq!(
        model.route_dependencies(usize::MAX),
        Err(ProjectError::InvalidRoute)
    );
}

#[test]
fn preserves_ambiguous_routes_and_context_issues() {
    let limits = Limits::default();
    let snap = open(
        "ambiguity",
        &[
            (
                "app/(a)/page.tsx",
                "'use client'; import 'server-only'; export default () => null;",
            ),
            ("app/(b)/page.tsx", "export default () => null;"),
        ],
        &limits,
    );
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    assert_eq!(model.nextjs().routes.routes.len(), 2);
    assert!(!model.nextjs().routes.issues.is_empty());
    assert!(!model.nextjs().contexts.issues.is_empty());
    for i in 0..2 {
        assert!(model.route_dependencies(i).is_ok());
    }
}

#[test]
fn package_boundaries_preserve_root_coordinates() {
    let limits = Limits::default();
    let snap = open(
        "package",
        &[
            ("packages/web/app/page.tsx", "export default () => null;"),
            ("packages/web/data.txt", "data"),
            (
                "packages/web-other/app/page.tsx",
                "export default () => null;",
            ),
        ],
        &limits,
    );
    let package = path("packages/web");
    let model = project::build(
        &snap,
        Some(&package),
        &path("packages/web/app"),
        EXTENSIONS,
        &limits,
    )
    .unwrap();
    assert_eq!(model.package(), Some(&package));
    assert!(
        model
            .files()
            .iter()
            .all(|f| f.path.as_str().starts_with("packages/web/"))
    );
    assert_eq!(model.modules().nodes().len(), 1);
    assert_eq!(
        project::build(
            &snap,
            Some(&package),
            &path("packages/web-other/app"),
            EXTENSIONS,
            &limits
        ),
        Err(ProjectError::OutsidePackage)
    );
}

#[test]
fn rejects_incomplete_discovery_and_invalid_limits() {
    let mut limits = Limits::default();
    limits.run.max_files = 1;
    let snap = open("incomplete", SOURCE, &limits);
    assert!(!snap.inventory().is_complete());
    assert!(project::build(&snap, None, &path("app"), EXTENSIONS, &limits).is_err());
    limits.run.max_file_bytes = limits.run.max_total_bytes + 1;
    assert!(matches!(
        project::build(&snap, None, &path("app"), EXTENSIONS, &limits),
        Err(ProjectError::Limits(_))
    ));
}

#[test]
fn repeatability_across_discovery_order() {
    let (snap, first) = build("repeat-first");
    let mut reversed = SOURCE.to_vec();
    reversed.reverse();
    let limits = Limits::default();
    let second_snap = open("repeat-second", &reversed, &limits);
    let second = project::build(&second_snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first,
        project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap()
    );
    for i in 0..first.nextjs().routes.routes.len() {
        assert_eq!(first.route_dependencies(i), second.route_dependencies(i));
    }
}

#[test]
fn binding_provenance_for_render_sites_and_environment_names() {
    use frameshift_engine::bindings::Target;
    let limits = Limits::default();
    let text = "import Widget from 'fictional-ui';\nconst Local = () => <Widget />;\nexport default function Page(Widget) { return <><Widget/><Local/><div/><UI.Item/></>; }\nconst a = process.env.NEXT_PUBLIC_LABEL;\nconst b = process['env']['SECRET'];\nconst c = process.env[key];\nconst d = process.env;\nfunction shadow(process) { return process.env.IGNORE; }\nconst e = pr\\u006fcess.e\\u006ev['NEXT_PUBLIC_ESCAPED'];\n";
    let snap = open("semantics", &[("app/page.tsx", text)], &limits);
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    let facts = &model.semantics()[0];
    assert_eq!(facts.renders.len(), 5);
    let imported = facts.renders[0].reference.as_ref().unwrap();
    let shadowed = facts.renders[1].reference.as_ref().unwrap();
    assert_ne!(imported.target, shadowed.target);
    let Target::Binding(id) = imported.target else {
        panic!("import binding")
    };
    assert_eq!(
        facts
            .bindings
            .binding(id)
            .import
            .as_ref()
            .unwrap()
            .specifier
            .as_deref(),
        Some("fictional-ui")
    );
    assert!(facts.renders[3].reference.is_none());
    assert_eq!(
        facts.renders[4].reference.as_ref().unwrap().target,
        Target::Global
    );
    assert!(facts.renders.iter().all(|r| r.owner.is_some()));
    assert_eq!(
        facts
            .environment
            .iter()
            .map(|e| (e.name.as_deref(), e.is_public()))
            .collect::<Vec<_>>(),
        [
            (Some("NEXT_PUBLIC_LABEL"), Some(true)),
            (Some("SECRET"), Some(false)),
            (None, None),
            (None, None),
            (Some("NEXT_PUBLIC_ESCAPED"), Some(true)),
        ]
    );
    assert!(facts.spans.iter().all(|s| s.end() <= text.len()));
    for reference in facts.bindings.references() {
        assert!(
            !&text[facts.spans[reference.node.index()].start()
                ..facts.spans[reference.node.index()].end()]
                .is_empty()
        );
    }
}

fn model_snapshot() -> String {
    use std::fmt::Write;
    let (_, model) = build("golden");
    let mut out = String::new();
    for file in model.files() {
        writeln!(
            out,
            "file {:?} {:?} {:?} {:?}",
            file.path.as_str(),
            file.state,
            file.kind,
            file.convention
        )
        .unwrap();
    }
    for (index, route) in model.nextjs().routes.routes.iter().enumerate() {
        writeln!(
            out,
            "route {:?} {:?} {:?}",
            route.pattern,
            route.kind,
            route.file.as_str()
        )
        .unwrap();
        for file in &route.chain {
            writeln!(
                out,
                "  chain {:?} {:?}",
                file.convention,
                file.path.as_str()
            )
            .unwrap();
        }
        let closure = model.route_dependencies(index).unwrap();
        for file in closure.files {
            writeln!(out, "  dependency {:?}", file.as_str()).unwrap();
        }
        for file in closure.unparsed {
            writeln!(out, "  unparsed {:?}", file.as_str()).unwrap();
        }
        for edge in closure.edges {
            writeln!(
                out,
                "  edge {:?} {:?} {:?}",
                edge.importer.as_str(),
                edge.edge.kind,
                edge.edge.resolution
            )
            .unwrap();
        }
    }
    for module in model.semantics() {
        writeln!(
            out,
            "semantics {:?}: {} bindings, {} render sites, {} environment uses",
            module.path.as_str(),
            module.bindings.bindings().len(),
            module.renders.len(),
            module.environment.len()
        )
        .unwrap();
    }
    for context in &model.nextjs().contexts.modules {
        writeln!(
            out,
            "context {:?}: server={} client={}",
            context.path.as_str(),
            context.server,
            context.client
        )
        .unwrap();
    }
    out
}

#[test]
fn project_model_golden() {
    assert_eq!(model_snapshot(), include_str!("golden/project.txt"));
}

#[test]
#[ignore = "explicit golden update"]
fn update_project_golden() {
    fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/project.txt"),
        model_snapshot(),
    )
    .unwrap();
}

#[test]
fn strict_tree_visitor_runs_once_and_propagates_failure() {
    use frameshift_engine::modules::ModuleError;
    use frameshift_engine::nextjs::{AppError, analyze_app_with};
    let limits = Limits::default();
    let snap = open(
        "visitor",
        &[
            (
                "app/page.tsx",
                "import Widget from 'fictional-ui'; export default () => <Widget<number>/>;",
            ),
            ("lib/broken.ts", "export const = ;"),
            ("lib/helper.ts", "export const value = 1;"),
        ],
        &limits,
    );
    let graph = limits.modules();
    let mut visited = Vec::new();
    analyze_app_with(
        &snap,
        None,
        &path("app"),
        EXTENSIONS,
        graph,
        &mut |path, tree, bindings| {
            visited.push(path.clone());
            assert!(!tree.nodes().is_empty());
            assert!(!bindings.exports().is_empty());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(visited, [path("app/page.tsx"), path("lib/helper.ts")]);
    let result = analyze_app_with(
        &snap,
        None,
        &path("app"),
        EXTENSIONS,
        graph,
        &mut |_, _, _| Err(ModuleError::WorkLimit),
    );
    assert_eq!(result, Err(AppError::Modules(ModuleError::WorkLimit)));
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    let site = &model.semantics()[0].renders[0];
    assert_eq!(site.spelling, "Widget<number>");
    assert!(matches!(
        site.reference.as_ref().unwrap().target,
        frameshift_engine::bindings::Target::Binding(_)
    ));
}

#[test]
fn method_render_owners_and_computed_keys_keep_lexical_context() {
    let limits = Limits::default();
    let text = "class Card { render() { return <CardBody/>; } }\nfunction Outer() { class Inner { show() { return <InnerBody/>; } } const obj = { draw() { return <ObjectBody/>; }, get child() { return <GetterBody/>; }, [<Key/>]() { return <ComputedBody/>; } }; return obj; }\nconst obj = { [<TopKey/>]() { return <TopBody/>; } };";
    let snap = open("methods", &[("app/page.tsx", text)], &limits);
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    let sites = &model.semantics()[0].renders;
    assert_eq!(sites.len(), 8);
    for (name, prefix) in [
        ("CardBody", "render()"),
        ("InnerBody", "show()"),
        ("ObjectBody", "draw()"),
        ("GetterBody", "get child()"),
        ("Key", "function Outer()"),
        ("ComputedBody", "[<Key/>]()"),
        ("TopBody", "[<TopKey/>]()"),
    ] {
        let owner = sites
            .iter()
            .find(|site| site.spelling == name)
            .unwrap()
            .owner
            .unwrap();
        assert!(
            text[owner.start()..owner.end()].starts_with(prefix),
            "{name}: {:?}",
            &text[owner.start()..owner.end()]
        );
    }
    assert!(
        sites
            .iter()
            .find(|site| site.spelling == "TopKey")
            .unwrap()
            .owner
            .is_none()
    );
}

#[test]
fn runtime_declaration_targets_keep_parse_failures_without_traversal() {
    let limits = Limits::default();
    let snap = open(
        "declaration-target",
        &[
            (
                "app/page.tsx",
                "import '../lib/broken'; import '../lib/types'; export default () => null;",
            ),
            ("lib/broken.d.ts", "export declare const = ;"),
            ("lib/types.d.ts", "export { T } from './unreached';"),
            ("lib/unreached.ts", "export const T = 1;"),
        ],
        &limits,
    );
    let model = project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap();
    let closure = model.route_dependencies(0).unwrap();
    assert_eq!(closure.unparsed, [&path("lib/broken.d.ts")]);
    assert!(closure.files.contains(&&path("lib/types.d.ts")));
    assert!(!closure.files.contains(&&path("lib/unreached.ts")));
    assert!(
        closure
            .edges
            .iter()
            .all(|edge| edge.importer == &path("app/page.tsx"))
    );
}
