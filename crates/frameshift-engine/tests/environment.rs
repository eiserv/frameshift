//! Environment classification contracts (`D-096`): where modules are
//! evaluated and which environment variables and runtime APIs they use.

use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::nextjs::{
    ApiKind, BROWSER_GLOBALS, ModuleEnvironment, NODE_BUILTINS, NODE_GLOBALS, Runtime,
    VariableAccess,
};
use frameshift_engine::project::{self, Project};
use std::fs;

mod support;

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn open(name: &str, files: &[(&str, &str)], limits: &Limits) -> Snapshot {
    let root = support::temporary().join("environment").join(name);
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
        "import fs from 'node:fs';\nimport path from 'path';\n\
         const root = process.env.ROOT_DIR;\n\
         export default function RootLayout({ children }) {\n\
         fs.readFileSync(path.join(__dirname, 'data'));\n\
         return <html><body>{children}</body></html>;\n}\n",
    ),
    (
        "app/page.tsx",
        "import Widget from '../components/widget';\n\
         import Panel from '../components/panel';\n\
         import { load } from '../lib/server';\n\
         import { B } from '../lib/alias';\n\
         import '../lib/shadow';\nimport '../lib/types';\n\
         const env = process.env;\n\
         const { NEXT_PUBLIC_SITE, SECRET: secret = 'x', ...rest } = process.env;\n\
         const key = 'DYNAMIC';\n\
         console.log(process.env);\n\
         process.cwd();\n\
         export default function Page() {\n\
         return <><Widget /><Panel />{env.API_URL}{process.env[key]}{(process.env as any).CAST}{process['env']['QUOTED']}</>;\n}\n",
    ),
    (
        "components/widget.tsx",
        "'use client';\n\
         import dynamic from 'next/dynamic';\n\
         import { helper } from '../lib/shared';\n\
         const Chart = dynamic(() => import('./chart'), { ssr: false });\n\
         const width = window.innerWidth;\n\
         export default function Widget() {\n\
         if (typeof window !== 'undefined') { window.scrollTo(0, 0); }\n\
         return <Chart />;\n}\n",
    ),
    (
        "components/chart.tsx",
        "export default function Chart() { localStorage.getItem('key'); return null; }\n",
    ),
    (
        "components/panel.tsx",
        "'use client';\nimport { helper } from '../lib/shared';\nexport default () => null;\n",
    ),
    (
        "lib/shared.ts",
        "export const helper = () => process.env.NEXT_PUBLIC_FLAG;\n",
    ),
    (
        "lib/server.ts",
        "import { readFile } from 'fs/promises';\n\
         import type { Stats } from 'node:fs';\n\
         const os = require('os');\n\
         export async function load() { const crypto = await import('node:crypto'); return Buffer.from(''); }\n",
    ),
    (
        "lib/alias.ts",
        "const e = process.env;\nconst e2 = e;\nconst { A } = e2;\nexport const B = e2.B;\n\
         let w = process.env;\nw = {};\n",
    ),
    (
        "lib/shadow.ts",
        "const window = {};\nwindow.x = 1;\nfunction f(process) { return process.env.X; }\n\
         const require = (s) => s;\nrequire('fs');\n",
    ),
    (
        "lib/types.ts",
        "let w: typeof window;\nlet d: Document;\nlet q: typeof window.location;\nexport {};\n",
    ),
    ("lib/unused.ts", "document.title = 'x';\n"),
    ("lib/broken.ts", "export const = ;\n"),
    ("lib/types.d.ts", "declare const window: any;\n"),
];

fn build(name: &str, files: &[(&str, &str)]) -> Project {
    let limits = Limits::default();
    let snap = open(name, files, &limits);
    project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap()
}

fn module<'a>(model: &'a Project, file: &str) -> &'a ModuleEnvironment {
    model.environment().module(&path(file)).unwrap()
}

fn text<'a>(files: &[(&str, &'a str)], file: &str) -> &'a str {
    files.iter().find(|(name, _)| *name == file).unwrap().1
}

#[test]
fn classifies_build_browser_and_shared_modules() {
    let model = build("classification", SOURCE);
    let runtime = |file: &str| module(&model, file).runtime();
    assert_eq!(runtime("app/layout.tsx"), Some(Runtime::Build));
    assert_eq!(runtime("app/page.tsx"), Some(Runtime::Build));
    assert_eq!(runtime("lib/server.ts"), Some(Runtime::Build));
    // A client boundary is prerendered on the server and runs in the browser.
    assert_eq!(runtime("components/widget.tsx"), Some(Runtime::Shared));
    assert_eq!(runtime("components/panel.tsx"), Some(Runtime::Shared));
    assert_eq!(runtime("lib/shared.ts"), Some(Runtime::Shared));
    // `ssr: false` keeps the loaded module out of prerendering.
    assert_eq!(runtime("components/chart.tsx"), Some(Runtime::Browser));
    assert_eq!(runtime("lib/unused.ts"), None);
    let broken = module(&model, "lib/broken.ts");
    assert!(!broken.parsed && broken.variables.is_empty() && broken.apis.is_empty());
    // Declaration files are not evaluated and have no entry.
    assert!(
        model
            .environment()
            .module(&path("lib/types.d.ts"))
            .is_none()
    );
    assert!(
        model
            .environment()
            .modules
            .windows(2)
            .all(|w| w[0].path < w[1].path)
    );

    // The same module imported normally as well is prerendered.
    let mut files = SOURCE.to_vec();
    files.push((
        "components/panel.tsx",
        "'use client';\nimport Chart from './chart';\nexport default () => null;\n",
    ));
    let files: Vec<_> = files
        .iter()
        .enumerate()
        .filter(|(i, (name, _))| *name != "components/panel.tsx" || *i == files.len() - 1)
        .map(|(_, f)| *f)
        .collect();
    let model = build("prerendered-chart", &files);
    assert_eq!(
        module(&model, "components/chart.tsx").runtime(),
        Some(Runtime::Shared)
    );
}

#[test]
fn records_direct_aliased_and_destructured_variables() {
    let model = build("variables", SOURCE);
    let page = module(&model, "app/page.tsx");
    let source = text(SOURCE, "app/page.tsx");
    let uses: Vec<_> = page
        .variables
        .iter()
        .map(|v| {
            (
                &source[v.span.start()..v.span.end()],
                v.name.as_deref(),
                v.access,
                v.deferred,
            )
        })
        .collect();
    assert_eq!(
        uses,
        [
            (
                "NEXT_PUBLIC_SITE",
                Some("NEXT_PUBLIC_SITE"),
                VariableAccess::Destructured,
                false
            ),
            (
                "SECRET: secret = 'x'",
                Some("SECRET"),
                VariableAccess::Destructured,
                false
            ),
            ("...rest", None, VariableAccess::Whole, false),
            ("process.env", None, VariableAccess::Whole, false),
            ("env.API_URL", Some("API_URL"), VariableAccess::Alias, true),
            ("process.env[key]", None, VariableAccess::Direct, true),
            (
                "(process.env as any).CAST",
                Some("CAST"),
                VariableAccess::Direct,
                true
            ),
            (
                "process['env']['QUOTED']",
                Some("QUOTED"),
                VariableAccess::Direct,
                true
            ),
        ]
    );
    assert_eq!(page.variables[0].is_public(), Some(true));
    assert_eq!(page.variables[1].is_public(), Some(false));
    assert_eq!(page.variables[2].is_public(), None);
    // `process.cwd()` is a Node.js global use; `process.env` reads are not.
    let globals: Vec<_> = page
        .apis
        .iter()
        .map(|a| (a.kind, a.name.as_str()))
        .collect();
    assert_eq!(globals, [(ApiKind::NodeGlobal, "process")]);

    let alias = module(&model, "lib/alias.ts");
    let source = text(SOURCE, "lib/alias.ts");
    let uses: Vec<_> = alias
        .variables
        .iter()
        .map(|v| {
            (
                &source[v.span.start()..v.span.end()],
                v.name.as_deref(),
                v.access,
            )
        })
        .collect();
    assert_eq!(
        uses,
        [
            ("A", Some("A"), VariableAccess::Destructured),
            ("e2.B", Some("B"), VariableAccess::Alias),
            ("w = process.env", None, VariableAccess::Whole),
        ]
    );
    let shared = module(&model, "lib/shared.ts");
    assert_eq!(shared.variables.len(), 1);
    assert_eq!(
        shared.variables[0].name.as_deref(),
        Some("NEXT_PUBLIC_FLAG")
    );
    assert!(shared.variables[0].deferred);
}

#[test]
fn records_node_modules_and_runtime_globals_by_provenance() {
    let model = build("apis", SOURCE);
    let summary = |file: &str| -> Vec<(String, ApiKind, String, bool, bool)> {
        let source = text(SOURCE, file);
        module(&model, file)
            .apis
            .iter()
            .map(|a| {
                (
                    source[a.span.start()..a.span.end()].to_owned(),
                    a.kind,
                    a.name.clone(),
                    a.deferred,
                    a.typeof_guard,
                )
            })
            .collect()
    };
    let api = |spelling: &str, kind, name: &str, deferred, guard| {
        (spelling.to_owned(), kind, name.to_owned(), deferred, guard)
    };
    assert_eq!(
        summary("app/layout.tsx"),
        [
            api("'node:fs'", ApiKind::NodeModule, "node:fs", false, false),
            api("'path'", ApiKind::NodeModule, "path", false, false),
            api("__dirname", ApiKind::NodeGlobal, "__dirname", true, false),
        ]
    );
    // Type-only imports are erased; `require` is the global, with its module.
    assert_eq!(
        summary("lib/server.ts"),
        [
            api(
                "'fs/promises'",
                ApiKind::NodeModule,
                "fs/promises",
                false,
                false
            ),
            api("require", ApiKind::NodeGlobal, "require", false, false),
            api("'os'", ApiKind::NodeModule, "os", false, false),
            api(
                "'node:crypto'",
                ApiKind::NodeModule,
                "node:crypto",
                true,
                false
            ),
            api("Buffer", ApiKind::NodeGlobal, "Buffer", true, false),
        ]
    );
    assert_eq!(
        summary("components/widget.tsx"),
        [
            api("window", ApiKind::BrowserGlobal, "window", false, false),
            api("window", ApiKind::BrowserGlobal, "window", true, true),
            api("window", ApiKind::BrowserGlobal, "window", true, false),
        ]
    );
    assert_eq!(
        summary("components/chart.tsx"),
        [api(
            "localStorage",
            ApiKind::BrowserGlobal,
            "localStorage",
            true,
            false
        )]
    );
    assert_eq!(
        summary("lib/unused.ts"),
        [api(
            "document",
            ApiKind::BrowserGlobal,
            "document",
            false,
            false
        )]
    );
    // Local bindings shadow the globals, and type queries are erased.
    let shadow = module(&model, "lib/shadow.ts");
    assert!(shadow.apis.is_empty() && shadow.variables.is_empty());
    let types = module(&model, "lib/types.ts");
    assert!(types.apis.is_empty());
}

#[test]
fn pinned_lists_are_sorted_and_disjoint() {
    for list in [NODE_BUILTINS, NODE_GLOBALS, BROWSER_GLOBALS] {
        assert!(list.windows(2).all(|w| w[0] < w[1]));
    }
    assert!(NODE_GLOBALS.iter().all(|n| !BROWSER_GLOBALS.contains(n)));
    // Names that the pinned Node.js also defines are not browser-only.
    for shared in ["navigator", "fetch", "URL", "crypto", "performance"] {
        assert!(!BROWSER_GLOBALS.contains(&shared));
    }
}

#[test]
fn analysis_is_repeatable() {
    let first = build("repeat-a", SOURCE);
    let second = build("repeat-b", SOURCE);
    assert_eq!(first.environment(), second.environment());
}

fn environment_snapshot() -> String {
    use std::fmt::Write;
    let model = build("golden", SOURCE);
    let mut out = String::new();
    for module in &model.environment().modules {
        writeln!(
            out,
            "module {:?} parsed={} build={} browser={} {:?}",
            module.path.as_str(),
            module.parsed,
            module.build,
            module.browser,
            module.runtime()
        )
        .unwrap();
        let source = text(SOURCE, module.path.as_str());
        for v in &module.variables {
            writeln!(
                out,
                "  variable {:?} {:?} {:?} deferred={} public={:?}",
                &source[v.span.start()..v.span.end()],
                v.name,
                v.access,
                v.deferred,
                v.is_public()
            )
            .unwrap();
        }
        for a in &module.apis {
            writeln!(
                out,
                "  api {:?} {:?} {:?} deferred={} typeof={}",
                &source[a.span.start()..a.span.end()],
                a.kind,
                a.name,
                a.deferred,
                a.typeof_guard
            )
            .unwrap();
        }
    }
    out
}

#[test]
fn environment_golden() {
    assert_eq!(
        environment_snapshot(),
        include_str!("golden/environment.txt")
    );
}

#[test]
#[ignore = "explicit golden update"]
fn update_environment_golden() {
    fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/environment.txt"),
        environment_snapshot(),
    )
    .unwrap();
}
