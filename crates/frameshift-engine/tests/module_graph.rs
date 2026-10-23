//! Fictional resolver and dependency-graph contracts through the engine facade.

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, PathLimits, SourcePath, SourceText, TextLimits,
};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{
    ConfigProblem, EdgeKind, FileKind, GraphLimits, ModuleError, ModuleGraph, ModuleLimits,
    ModuleState, Resolution, ResolutionConfig, Resolver, UnresolvedReason as U, build_module_graph,
};
use frameshift_engine::syntax::JsonLimits;

mod support;

#[test]
#[ignore = "requires an external TypeScript 6.0.3 installation"]
fn external_typescript_bundler_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let module = std::env::var("FRAMESHIFT_TYPESCRIPT_MODULE")
        .expect("set FRAMESHIFT_TYPESCRIPT_MODULE to an external typescript package directory");
    let script = r#"
const ts = require(process.argv[1]);
if (ts.version !== '6.0.3') throw Error('expected TypeScript 6.0.3');
const [config, filenames, requests] = require('fs').readFileSync(0, 'utf8').split('\n\n');
const files = new Set(filenames.split('\n').map(p => '/project/' + p));
const host = { fileExists: p => files.has(p), readFile: () => undefined,
 directoryExists: p => [...files].some(f => f.startsWith(p.replace(/\/$/, '') + '/')),
 getCurrentDirectory: () => '/project', realpath: p => p };
const options = { moduleResolution: ts.ModuleResolutionKind.Bundler,
 module: ts.ModuleKind.ESNext, allowJs: true, resolveJsonModule: true,
 ...JSON.parse(config) };
for (const specifier of requests.split('\n')) {
 const r = ts.resolveModuleName(specifier, '/project/src/main.ts', options, host).resolvedModule;
 process.stdout.write((r ? r.resolvedFileName.slice('/project/'.length) : '-') + '\n');
}
"#;
    let mut files = vec!["src/main.ts".to_owned()];
    let mut requests = Vec::new();
    for mask in 1..64 {
        for (bit, ext) in ["ts", "tsx", "d.ts", "js", "jsx", "json"]
            .iter()
            .enumerate()
        {
            if mask & (1 << bit) != 0 {
                files.push(format!("src/c{mask}/value.{ext}"));
                files.push(format!("src/c{mask}/index.{ext}"));
            }
        }
        for suffix in ["", ".js", ".jsx", ".ts", ".tsx", ".json"] {
            requests.push(format!("./c{mask}/value{suffix}"));
        }
        requests.push(format!("./c{mask}"));
        requests.push(format!("./c{mask}/"));
        requests.push(format!("alias/c{mask}/value"));
        requests.push(format!("c{mask}/value"));
    }
    for (ext, substitutions) in [
        ("mjs", ["mts", "d.mts", "mjs"]),
        ("cjs", ["cts", "d.cts", "cjs"]),
    ] {
        for mask in 1..8 {
            for (bit, ext) in substitutions.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    files.push(format!("src/{mask}-{ext}.{ext}"));
                }
            }
            // Distinct stems exercise every priority combination.
            for (bit, substitution) in substitutions.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    files.push(format!("src/k{mask}-{ext}.{substitution}"));
                }
            }
            requests.push(format!("./k{mask}-{ext}.{ext}"));
        }
    }
    for (i, extension) in ["css", "scss", "svg", "png", "custom", "module.css"]
        .iter()
        .enumerate()
    {
        for include_asset in [false, true] {
            let name = format!("resource-{i}-{include_asset}.{extension}");
            let (stem, extension) = name.rsplit_once('.').unwrap();
            files.push(format!("src/{stem}.d.{extension}.ts"));
            if include_asset {
                files.push(format!("src/{name}"));
            }
            requests.push(format!("./{name}"));
        }
    }
    files.extend([
        "src/directory.ts".to_owned(),
        "src/directory/index.ts".to_owned(),
    ]);
    requests.extend(
        [
            "./directory",
            "./directory/",
            "alias/directory/",
            "directory/",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    files.extend(
        ["src.ts", "src/index.ts", "index.ts", ".ts"]
            .iter()
            .map(|s| (*s).to_owned()),
    );
    requests.extend(
        [
            ".",
            "..",
            "./directory/..",
            "./directory/.",
            "./directory/../",
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    let profiles = [
        (
            r#"{"baseUrl":"/project"}"#,
            r#"{"compilerOptions":{"baseUrl":"./"}}"#,
        ),
        (
            r#"{"baseUrl":"/project/src"}"#,
            r#"{"compilerOptions":{"baseUrl":"src/"}}"#,
        ),
        (
            r#"{"baseUrl":"/project"}"#,
            r#"{"compilerOptions":{"baseUrl":""}}"#,
        ),
        ("{}", "{}"),
        (
            r#"{"baseUrl":"/project/src"}"#,
            r#"{"compilerOptions":{"baseUrl":"src"}}"#,
        ),
        (
            r#"{"paths":{"alias/*":["./absent/*","./src/*"]},"pathsBasePath":"/project"}"#,
            r#"{"compilerOptions":{"paths":{"alias/*":["./absent/*","./src/*"]}}}"#,
        ),
        (
            r#"{"baseUrl":"/project/src","paths":{"alias/*":["absent/*","*"]}}"#,
            r#"{"compilerOptions":{"baseUrl":"src","paths":{"alias/*":["absent/*","*"]}}}"#,
        ),
    ];
    let mut total = 0;
    let mut differences = Vec::new();
    for (reference_config, own_config) in profiles {
        let mut child = Command::new("node")
            .args(["-e", script, &module])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        write!(
            child.stdin.take().unwrap(),
            "{reference_config}\n\n{}\n\n{}",
            files.join("\n"),
            requests.join("\n")
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = String::from_utf8(output.stdout).unwrap();
        let answers: Vec<_> = result.lines().collect();
        assert_eq!(answers.len(), requests.len());
        let mut resolver = Resolver::new(
            files.iter().map(|p| path(p)).collect(),
            None,
            config(own_config, "tsconfig.json"),
            ModuleLimits::default(),
        )
        .unwrap();
        for (specifier, expected) in requests.iter().zip(answers) {
            let resolved = resolver.resolve(&path("src/main.ts"), specifier).unwrap();
            let actual = if let Resolution::Local { path, .. } = &resolved {
                path.as_str()
            } else {
                "-"
            };
            if actual != expected {
                differences.push(format!(
                    "{reference_config}, {specifier}: {actual:?} != {expected:?}"
                ));
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
    eprintln!("TypeScript 6.0.3 bundler oracle: {total} cases");
}

fn path(s: &str) -> SourcePath {
    SourcePath::new(s, PathLimits::default()).unwrap()
}
fn text(s: &str) -> SourceText {
    SourceText::new(s.to_owned(), TextLimits::default()).unwrap()
}
fn config(s: &str, file: &str) -> ResolutionConfig {
    ResolutionConfig::parse(&text(s), &path(file), JsonLimits::default())
}
fn resolver(files: &[&str], config: ResolutionConfig) -> Resolver {
    Resolver::new(
        files.iter().map(|s| path(s)).collect(),
        None,
        config,
        ModuleLimits::default(),
    )
    .unwrap()
}
fn local(resolver: &mut Resolver, specifier: &str, expected: &str) {
    assert_eq!(
        resolver.resolve(&path("src/main.ts"), specifier).unwrap(),
        Resolution::Local {
            path: path(expected),
            kind: FileKind::of(&path(expected))
        },
        "{specifier}"
    );
}
fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = support::temporary().join("module-graph").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    root
}
fn open(root: &Path) -> Snapshot {
    let mut findings = FindingCollector::new(FindingLimits::default());
    snapshot(root, Limits::default().snapshot(), &mut findings).unwrap()
}
fn graph(name: &str, files: &[(&str, &str)]) -> ModuleGraph {
    let root = scratch(name, files);
    build_module_graph(&open(&root), None, GraphLimits::default()).unwrap()
}

#[test]
fn extension_substitution_and_index_precedence_are_explicit() {
    let mut r = resolver(
        &[
            "src/main.ts",
            "src/a.ts",
            "src/a.js",
            "src/a/index.ts",
            "src/b.tsx",
            "src/b.jsx",
            "src/c.d.ts",
            "src/c.js",
            "src/d.mts",
            "src/d.mjs",
            "src/e.cts",
            "src/e.cjs",
            "src/dir/index.ts",
            "src/data.json",
            "src/style.scss",
            "src/logo.svg",
        ],
        ResolutionConfig::default(),
    );
    for (s, p) in [
        ("./a", "src/a.ts"),
        ("./a.js", "src/a.ts"),
        ("./b.jsx", "src/b.tsx"),
        ("./c", "src/c.d.ts"),
        ("./d.mjs", "src/d.mts"),
        ("./e.cjs", "src/e.cts"),
        ("./dir/", "src/dir/index.ts"),
        ("./data.json", "src/data.json"),
        ("./style.scss", "src/style.scss"),
        ("./logo.svg", "src/logo.svg"),
    ] {
        local(&mut r, s, p);
    }
    assert_eq!(
        r.resolve(&path("src/main.ts"), "./missing").unwrap(),
        Resolution::Unresolved(U::NotFound)
    );
}

#[test]
fn aliases_use_exact_then_longest_prefix_and_ordered_target_fallback() {
    let c = config(
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["absent/*","src/*"],"@/special/*":["special/*"],"@/exact":["exact.ts"]}}}"#,
        "tsconfig.json",
    );
    let mut r = resolver(
        &[
            "src/main.ts",
            "src/other.ts",
            "special/one.ts",
            "exact.ts",
            "bare.ts",
        ],
        c,
    );
    for (s, p) in [
        ("@/other", "src/other.ts"),
        ("@/special/one", "special/one.ts"),
        ("@/exact", "exact.ts"),
        ("bare", "bare.ts"),
    ] {
        local(&mut r, s, p);
    }
    assert_eq!(
        r.resolve(&path("src/main.ts"), "@scope/pkg/sub/path")
            .unwrap(),
        Resolution::External {
            package: "@scope/pkg".to_owned()
        }
    );
    assert_eq!(
        r.resolve(&path("src/main.ts"), "pkg/sub").unwrap(),
        Resolution::External {
            package: "pkg".to_owned()
        }
    );
    assert_eq!(
        r.resolve(&path("src/main.ts"), "node:fs/promises").unwrap(),
        Resolution::NodeBuiltin
    );
}

#[test]
fn typed_extension_families_preserve_the_reference_priority() {
    let mut r = resolver(
        &[
            "src/main.ts",
            "src/a.ts",
            "src/a.tsx",
            "src/b.js",
            "src/c.d.ts",
            "src/c.js",
            "src/data.json",
            "src/data.d.json.ts",
            "src/implicit.js.ts",
        ],
        ResolutionConfig::default(),
    );
    local(&mut r, "./a.jsx", "src/a.tsx");
    local(&mut r, "./a.tsx", "src/a.tsx");
    local(&mut r, "./a.ts", "src/a.ts");
    local(&mut r, "./b.jsx", "src/b.js");
    local(&mut r, "./c.d.ts", "src/c.d.ts");
    local(&mut r, "./data.json", "src/data.d.json.ts");
    local(&mut r, "./implicit.js", "src/implicit.js.ts");
    assert_eq!(
        r.resolve(&path("src/main.ts"), "./data").unwrap(),
        Resolution::Unresolved(U::NotFound)
    );
}

#[test]
fn arbitrary_extension_declarations_use_ambient_parsing_and_type_only_edges() {
    for file in [
        "a.d.json.ts",
        "a.d.css.ts",
        "a.d.foo.bar.ts",
        "a.d..ts",
        ".d.foo.ts",
        "a.d.mts",
        "a.d.cts",
    ] {
        assert_eq!(FileKind::of(&path(file)), FileKind::Declaration, "{file}");
    }
    for file in [
        "a.d.foo.mts",
        "a.d.foo.cts",
        "a.d.tsx",
        "a.D.foo.ts",
        "dir.d.foo/value.ts",
    ] {
        assert_eq!(FileKind::of(&path(file)), FileKind::Module, "{file}");
    }
    let g = graph(
        "arbitrary-declarations",
        &[
            (
                "types.d.json.ts",
                "import {T} from './a'; export const value: T;",
            ),
            ("a.ts", "export type T=string;"),
        ],
    );
    let node = g.node(&path("types.d.json.ts")).unwrap();
    assert_eq!(node.state, ModuleState::Parsed);
    assert!(node.edges.iter().all(|edge| edge.type_only));
}

#[test]
fn review_arbitrary_declarations_precede_assets_and_work_without_assets() {
    for files in [
        vec!["src/main.ts", "src/style.d.css.ts"],
        vec![
            "src/main.ts",
            "src/style.css",
            "src/style.css.ts",
            "src/style.d.css.ts",
        ],
    ] {
        let mut r = resolver(&files, ResolutionConfig::default());
        local(&mut r, "./style.css", "src/style.d.css.ts");
    }
    let mut r = resolver(
        &["src/main.ts", "src/theme.module.d.scss.ts"],
        ResolutionConfig::default(),
    );
    local(&mut r, "./theme.module.scss", "src/theme.module.d.scss.ts");
}

#[test]
fn review_excluded_exact_inheritance_never_selects_a_json_fallback() {
    let g = graph(
        "excluded-extends",
        &[
            (".gitignore", "base\n"),
            ("tsconfig.json", r#"{"extends":"./base"}"#),
            ("base", "{}"),
            ("base.json", "{}"),
            ("src/main.ts", "import './dep';"),
            ("src/dep.ts", ""),
        ],
    );
    assert!(
        g.configuration_issues()
            .iter()
            .any(|issue| issue.path == path("base") && issue.problem == ConfigProblem::Unreadable)
    );
    assert_eq!(
        g.node(&path("src/main.ts")).unwrap().edges[0].resolution,
        Resolution::Unresolved(U::Configuration)
    );
}

#[test]
fn review_empty_base_url_denotes_the_originating_config_directory() {
    for (file, target) in [
        ("tsconfig.json", "local.ts"),
        ("config/tsconfig.json", "config/local.ts"),
    ] {
        let c = config(r#"{"compilerOptions":{"baseUrl":""}}"#, file);
        assert!(c.issues().is_empty());
        let mut r = resolver(&["src/main.ts", target], c);
        local(&mut r, "local", target);
    }
    let g = graph(
        "inherited-empty-base",
        &[
            ("tsconfig.json", r#"{"extends":"./config/base"}"#),
            ("config/base.json", r#"{"compilerOptions":{"baseUrl":""}}"#),
            ("config/local.ts", ""),
            ("src/main.ts", "import 'local';"),
        ],
    );
    assert!(g.configuration_issues().is_empty());
    assert_eq!(
        g.node(&path("src/main.ts")).unwrap().edges[0].resolution,
        Resolution::Local {
            path: path("config/local.ts"),
            kind: FileKind::Module
        }
    );
}

#[test]
fn review_trailing_base_url_separator_preserves_the_config_origin() {
    for (base, file, target) in [
        ("./", "tsconfig.json", "local.ts"),
        ("src/", "tsconfig.json", "src/local.ts"),
        ("./src/", "config/tsconfig.json", "config/src/local.ts"),
    ] {
        let c = config(
            &format!(r#"{{"compilerOptions":{{"baseUrl":"{base}"}}}}"#),
            file,
        );
        assert!(c.issues().is_empty(), "{base}");
        let mut r = resolver(&["src/main.ts", target], c);
        local(&mut r, "local", target);
    }
    for base in ["/", "//", "src//", "src//nested"] {
        assert!(
            !config(
                &format!(r#"{{"compilerOptions":{{"baseUrl":"{base}"}}}}"#),
                "tsconfig.json"
            )
            .issues()
            .is_empty()
        );
    }
}

#[test]
fn review_trailing_import_slash_selects_only_directory_candidates() {
    let files = [
        "src/main.ts",
        "src/dir.ts",
        "src/dir/index.ts",
        "src.ts",
        "src/index.ts",
        "index.ts",
        ".ts",
    ];
    let mut r = resolver(&files, ResolutionConfig::default());
    local(&mut r, "./dir", "src/dir.ts");
    local(&mut r, "./dir/", "src/dir/index.ts");
    local(&mut r, ".", "src/index.ts");
    local(&mut r, "..", "index.ts");
    local(&mut r, "./dir/..", "src/index.ts");
    local(&mut r, "./dir/.", "src/dir/index.ts");
    let c = config(
        r#"{"compilerOptions":{"baseUrl":"src","paths":{"alias/*":["*"],"directory":["dir/"]}}}"#,
        "tsconfig.json",
    );
    let mut r = resolver(&files, c);
    local(&mut r, "alias/dir/", "src/dir/index.ts");
    local(&mut r, "directory", "src/dir/index.ts");
    local(&mut r, "dir/", "src/dir/index.ts");
}

#[test]
fn review_cts_and_declaration_cts_keep_module_dependencies() {
    let g = graph(
        "cts-modules",
        &[
            (
                "entry.cts",
                "import {value} from './dep'; export {value}; export * from './dep';",
            ),
            (
                "ambient.d.cts",
                "import type {T} from './dep'; export const current: T;",
            ),
            ("dep.ts", "export const value=1; export type T=string;"),
            ("sloppy.cjs", "with (scope) { require('./dep'); }"),
        ],
    );
    for (file, count) in [("entry.cts", 2), ("ambient.d.cts", 1), ("sloppy.cjs", 1)] {
        let node = g.node(&path(file)).unwrap();
        assert_eq!(node.state, ModuleState::Parsed, "{file}");
        assert_eq!(node.edges.len(), count, "{file}");
    }
    assert!(
        g.node(&path("ambient.d.cts"))
            .unwrap()
            .edges
            .iter()
            .all(|e| e.type_only)
    );
}

#[test]
fn invalid_configuration_and_unmodeled_options_never_guess() {
    for source in [
        r#"{"compilerOptions":{"paths":{"x**":["x"]}}}"#,
        r#"{"compilerOptions":{"paths":{"x":[]}}}"#,
        r#"{"compilerOptions":{"baseUrl":1}}"#,
        r#"{"compilerOptions":{"baseUrl":".","baseUrl":"src"}}"#,
        r#"{"compilerOptions":{"rootDirs":["a","b"]}}"#,
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
        r#"{"extends":"some-config"}"#,
        "{",
    ] {
        let c = config(source, "tsconfig.json");
        assert!(!c.issues().is_empty(), "{source}");
        let mut r = resolver(&["src/main.ts", "src/a.ts"], c);
        assert_eq!(
            r.resolve(&path("src/main.ts"), "./a").unwrap(),
            Resolution::Unresolved(U::Configuration)
        );
    }
}

#[test]
fn references_are_canonicalized_only_inside_the_package_boundary() {
    let mut r = resolver(
        &["src/main.ts", "shared.ts", "src/real.ts"],
        ResolutionConfig::default(),
    );
    local(&mut r, "./sub/../real", "src/real.ts");
    local(&mut r, "../shared", "shared.ts");
    for s in ["../../outside", "../../../outside"] {
        assert_eq!(
            r.resolve(&path("src/main.ts"), s).unwrap(),
            Resolution::Unresolved(U::OutsideRoot)
        );
    }
    for s in [
        "/etc/passwd",
        "C:/secret",
        "https://host/file",
        "./file?raw",
        "./file#fragment",
        "./bad\\path",
        "@scope",
        "pkg//sub",
        "pkg/../sub",
    ] {
        assert!(
            matches!(
                r.resolve(&path("src/main.ts"), s).unwrap(),
                Resolution::Unresolved(_)
            ),
            "{s}"
        );
    }
    assert_eq!(
        r.resolve(&path("src/main.ts"), "#internal").unwrap(),
        Resolution::Unresolved(U::PackageImports)
    );
    assert_eq!(
        r.resolve(&path("missing.ts"), "pkg"),
        Err(ModuleError::InvalidRoot)
    );
}

#[test]
fn graph_extracts_static_dynamic_type_and_commonjs_evidence_with_original_spans() {
    let input = "\u{feff}import type {T} from './types';\r\nimport {type U} from './types';\r\nexport {x} from './dep'; export * from './star';\r\nconst x = import('./\\u0064ep'); const y = import(value);\r\ntype A = import('./types').T; import B = require('./dep'); require('./dep');";
    let g = graph(
        "syntax",
        &[
            ("entry.ts", input),
            (
                "types.ts",
                "export type T = string; export type U = number;",
            ),
            ("dep.ts", "export const x=1;"),
            ("star.ts", "export const a=1;"),
        ],
    );
    let n = g.node(&path("entry.ts")).unwrap();
    assert_eq!(n.state, ModuleState::Parsed);
    assert_eq!(n.edges.len(), 9);
    assert_eq!(
        n.edges.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [
            EdgeKind::Import,
            EdgeKind::Import,
            EdgeKind::ReExport,
            EdgeKind::ReExport,
            EdgeKind::DynamicImport,
            EdgeKind::DynamicImport,
            EdgeKind::ImportType,
            EdgeKind::ImportEquals,
            EdgeKind::RequireCandidate
        ]
    );
    assert_eq!(
        n.edges.iter().map(|e| e.type_only).collect::<Vec<_>>(),
        [true, false, false, false, false, false, true, false, false]
    );
    assert_eq!(
        &input[n.edges[4].span.start()..n.edges[4].span.end()],
        "'./\\u0064ep'"
    );
    assert_eq!(n.edges[4].specifier.as_deref(), Some("./dep"));
    assert_eq!(n.edges[5].resolution, Resolution::Unresolved(U::Computed));
    assert_eq!(
        n.edges[8].resolution,
        Resolution::Unresolved(U::BindingAnalysis)
    );
    assert!(
        n.edges
            .windows(2)
            .all(|e| e[0].span.start() < e[1].span.start())
    );
}

#[test]
fn erroneous_and_non_utf8_sources_have_no_partial_dependencies() {
    let root = scratch(
        "invalid",
        &[
            ("bad.ts", "import './a'; const = ;"),
            ("a.ts", "export const a=1;"),
            ("bad-encoding.js", ""),
        ],
    );
    fs::write(root.join("bad-encoding.js"), [0xff]).unwrap();
    let g = build_module_graph(&open(&root), None, GraphLimits::default()).unwrap();
    let bad = g.node(&path("bad.ts")).unwrap();
    assert!(matches!(bad.state, ModuleState::SyntaxError(_)));
    assert!(bad.edges.is_empty());
    assert_eq!(
        g.node(&path("bad-encoding.js")).unwrap().state,
        ModuleState::InvalidText
    );
}

#[test]
fn cyclic_closure_is_iterative_sorted_and_distinguishes_explicit_type_edges() {
    let g = graph(
        "cycles",
        &[
            (
                "a.ts",
                "import './b'; import type {T} from './types'; import './style.css'; import 'external';",
            ),
            ("b.ts", "export * from './a';"),
            ("types.ts", "export type T=string;"),
            ("style.css", "body {}"),
        ],
    );
    assert_eq!(
        g.closure(&path("a.ts"), false)
            .unwrap()
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["a.ts", "b.ts"]
    );
    assert_eq!(
        g.closure(&path("a.ts"), true)
            .unwrap()
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["a.ts", "b.ts", "types.ts"]
    );
    assert_eq!(
        g.closure(&path("absent.ts"), false),
        Err(ModuleError::InvalidRoot)
    );
    assert_eq!(g.nodes().len(), 3);
}

#[test]
fn config_inheritance_preserves_origins_and_supports_multiple_bases() {
    let g = graph(
        "inheritance",
        &[
            (
                "tsconfig.json",
                r#"{"extends":["./config/base","./config/override"],"compilerOptions":{"strict":true}}"#,
            ),
            (
                "config/base.json",
                r#"{"compilerOptions":{"paths":{"lib/*":["../src/*"]}}}"#,
            ),
            (
                "config/override.json",
                r#"{"compilerOptions":{"strict":false}}"#,
            ),
            ("src/main.ts", "import 'lib/dep';"),
            ("src/dep.ts", "export const x=1;"),
        ],
    );
    assert!(g.configuration_issues().is_empty());
    assert_eq!(
        g.node(&path("src/main.ts")).unwrap().edges[0].resolution,
        Resolution::Local {
            path: path("src/dep.ts"),
            kind: FileKind::Module
        }
    );
    for (name, base, problem) in [
        ("cycle", "./tsconfig", ConfigProblem::Cycle),
        ("missing", "./missing", ConfigProblem::Missing),
        ("package-base", "package-config", ConfigProblem::Unsupported),
    ] {
        let source = format!(r#"{{"extends":"{base}"}}"#);
        let g = graph(
            name,
            &[
                ("tsconfig.json", &source),
                ("entry.ts", "import './dep';"),
                ("dep.ts", ""),
            ],
        );
        assert!(
            g.configuration_issues()
                .iter()
                .any(|i| i.problem == problem)
        );
        assert_eq!(
            g.nodes()[1].edges[0].resolution,
            Resolution::Unresolved(U::Configuration)
        );
    }
}

#[test]
fn excluded_higher_priority_candidates_and_configurations_block_fallback() {
    let root = scratch(
        "excluded",
        &[
            (".gitignore", "dep.ts\ntsconfig.json\n"),
            ("entry.ts", "import './dep';"),
            ("dep.ts", ""),
            ("dep.js", ""),
            ("tsconfig.json", "{}"),
            ("jsconfig.json", "{}"),
        ],
    );
    let g = build_module_graph(&open(&root), None, GraphLimits::default()).unwrap();
    assert_eq!(
        g.configuration_issues()[0].problem,
        ConfigProblem::Unreadable
    );
    fs::remove_file(root.join("tsconfig.json")).unwrap();
    let g = build_module_graph(&open(&root), None, GraphLimits::default()).unwrap();
    assert!(g.configuration_issues().is_empty());
    assert_eq!(
        g.node(&path("entry.ts")).unwrap().edges[0].resolution,
        Resolution::Unresolved(U::Excluded)
    );
}

#[test]
fn selected_workspace_package_keeps_sibling_code_outside_its_graph() {
    let root = scratch(
        "scope",
        &[
            ("apps/web/entry.ts", "import '../../sibling/value';"),
            ("sibling/value.ts", "export const x=1;"),
        ],
    );
    let g = build_module_graph(
        &open(&root),
        Some(&path("apps/web")),
        GraphLimits::default(),
    )
    .unwrap();
    assert_eq!(g.nodes().len(), 1);
    assert_eq!(
        g.nodes()[0].edges[0].resolution,
        Resolution::Unresolved(U::OutsideRoot)
    );
    assert_eq!(
        build_module_graph(&open(&root), Some(&path("missing")), GraphLimits::default()),
        Err(ModuleError::InvalidRoot)
    );
}

#[test]
fn file_edge_work_and_inheritance_limits_are_deterministic() {
    let root = scratch(
        "limits",
        &[
            ("entry.ts", "import './a'; import './b';"),
            ("a.ts", ""),
            ("b.ts", ""),
        ],
    );
    let snapshot = open(&root);
    let mut limits = GraphLimits::default();
    limits.modules.max_files = 2;
    assert_eq!(
        build_module_graph(&snapshot, None, limits),
        Err(ModuleError::FileLimit)
    );
    limits.modules.max_files = 3;
    limits.modules.max_edges = 1;
    assert_eq!(
        build_module_graph(&snapshot, None, limits),
        Err(ModuleError::EdgeLimit)
    );
    limits.modules.max_edges = 2;
    assert!(build_module_graph(&snapshot, None, limits).is_ok());
    limits.modules.max_work = 0;
    assert_eq!(
        build_module_graph(&snapshot, None, limits),
        Err(ModuleError::WorkLimit)
    );
    let root = scratch(
        "depth-limit",
        &[("tsconfig.json", "{}"), ("entry.ts", "import 'external';")],
    );
    let mut limits = GraphLimits::default();
    limits.modules.max_config_depth = 0;
    let g = build_module_graph(&open(&root), None, limits).unwrap();
    assert_eq!(
        g.configuration_issues()[0].problem,
        ConfigProblem::DepthLimit
    );
}

#[test]
fn incomplete_inventory_and_changed_snapshot_never_return_a_graph() {
    let root = scratch(
        "snapshot-safety",
        &[("entry.ts", "import './a';"), ("a.ts", "")],
    );
    let s = open(&root);
    fs::write(root.join("entry.ts"), "different length").unwrap();
    assert!(matches!(
        build_module_graph(&s, None, GraphLimits::default()),
        Err(ModuleError::Snapshot(_))
    ));
    let mut limits = Limits::default().snapshot();
    limits.discovery.run.max_files = 1;
    let mut findings = FindingCollector::new(FindingLimits::default());
    let s = snapshot(&root, limits, &mut findings).unwrap();
    assert_eq!(
        build_module_graph(&s, None, GraphLimits::default()),
        Err(ModuleError::IncompleteInventory)
    );
}

#[test]
fn graph_is_repeatable_and_source_is_unchanged() {
    let root = scratch(
        "repeatability",
        &[
            (
                "entry.tsx",
                "import './view'; export default () => <div />;",
            ),
            ("view.js", "export const view = <p>Hello</p>;"),
            ("ambient.d.ts", "import type {T} from './types';"),
            ("types.ts", "export type T=string;"),
        ],
    );
    let s = open(&root);
    let before = s.identity().unwrap();
    let a = build_module_graph(&s, None, GraphLimits::default()).unwrap();
    let b = build_module_graph(&open(&root), None, GraphLimits::default()).unwrap();
    assert_eq!(a, b);
    assert_eq!(open(&root).identity().unwrap(), before);
    assert!(a.nodes().iter().all(|n| n.state == ModuleState::Parsed));
    assert!(
        a.node(&path("ambient.d.ts"))
            .unwrap()
            .edges
            .iter()
            .all(|e| e.type_only)
    );
}

#[test]
fn existing_strict_trees_supply_the_same_edges_without_reparsing() {
    use frameshift_engine::modules::collect_dependencies;
    use frameshift_engine::syntax::{JsDialect, ParseLimits, SourceGoal, parse_js_program};
    let source = text("import './a'; export * from './a'; const p=import('./a');");
    let tree = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .unwrap();
    let files = ["src/main.ts", "src/a.ts"];
    let mut r = resolver(&files, ResolutionConfig::default());
    let edges = collect_dependencies(&tree, &path("src/main.ts"), &mut r, 3).unwrap();
    assert_eq!(edges.len(), 3);
    let mut r = resolver(&files, ResolutionConfig::default());
    assert_eq!(
        collect_dependencies(&tree, &path("src/main.ts"), &mut r, 2),
        Err(ModuleError::EdgeLimit)
    );
    assert_eq!(
        collect_dependencies(&tree, &path("absent.ts"), &mut r, 3),
        Err(ModuleError::InvalidRoot)
    );
    let g = graph(
        "shared-syntax",
        &[("src/main.ts", source.as_str()), ("src/a.ts", "")],
    );
    assert_eq!(g.node(&path("src/main.ts")).unwrap().edges, edges);
}

#[test]
fn long_module_chains_and_cycles_do_not_use_recursive_graph_traversal() {
    let count = 1024;
    let files: Vec<_> = (0..count)
        .map(|i| {
            (
                format!("m{i:04}.ts"),
                format!("export * from './m{:04}';", (i + 1) % count),
            )
        })
        .collect();
    let borrowed: Vec<_> = files
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let g = graph("long-chain", &borrowed);
    let closure = g.closure(&path("m0000.ts"), false).unwrap();
    assert_eq!(closure.len(), count);
    assert!(
        closure
            .windows(2)
            .all(|nodes| nodes[0].path < nodes[1].path)
    );
}

#[test]
fn graph_budgets_visit_earlier_source_before_later_large_expressions() {
    use frameshift_engine::modules::collect_dependencies;
    use frameshift_engine::syntax::{JsDialect, ParseLimits, SourceGoal, parse_js_program};
    let source = text(&format!(
        "import './a'; const later=[{}];",
        vec!["0"; 100].join(",")
    ));
    let tree = parse_js_program(
        &source,
        SourceGoal::Module,
        JsDialect::TypeScript,
        ParseLimits::default(),
    )
    .unwrap();
    let mut r = Resolver::new(
        vec![path("src/main.ts"), path("src/a.ts")],
        None,
        ResolutionConfig::default(),
        ModuleLimits {
            max_work: 64,
            ..ModuleLimits::default()
        },
    )
    .unwrap();
    // The first import reaches the edge bound before the later array can
    // consume the work budget. This makes source-order charging observable.
    assert_eq!(
        collect_dependencies(&tree, &path("src/main.ts"), &mut r, 0),
        Err(ModuleError::EdgeLimit)
    );
}

#[test]
fn inherited_configuration_candidate_precedence_matches_typescript() {
    let cases = [
        ("base", false, false, None),
        ("base", true, false, Some("first")),
        ("base", false, true, Some("second")),
        ("base", true, true, Some("first")),
        ("base.json", false, false, None),
        ("base.json", true, false, Some("first")),
        ("base.json", false, true, None),
        ("base.json", true, true, Some("first")),
        (".json", false, true, None),
        ("base.json.json", false, true, None),
        ("nested/base", false, true, Some("second")),
        ("nested/base.json", false, true, None),
        ("base.js", false, true, Some("second")),
        ("BASE.JSON", false, true, Some("second")),
    ];
    for (index, (name, exact, suffixed, selected)) in cases.into_iter().enumerate() {
        let configuration = format!(r#"{{"extends":"./{name}"}}"#);
        let root = scratch(
            &format!("config-precedence-{index}"),
            &[
                ("tsconfig.json", &configuration),
                ("entry.ts", "import 'dep';"),
            ],
        );
        for (present, filename, target) in [
            (exact, name.to_owned(), "first"),
            (suffixed, format!("{name}.json"), "second"),
        ] {
            if !present {
                continue;
            }
            let file = root.join(filename);
            let directory = file.parent().unwrap();
            fs::create_dir_all(directory).unwrap();
            let settings = format!(r#"{{"compilerOptions":{{"baseUrl":"{target}"}}}}"#);
            fs::write(&file, settings).unwrap();
            let dependency = directory.join(target).join("dep.ts");
            fs::create_dir_all(dependency.parent().unwrap()).unwrap();
            fs::write(dependency, "").unwrap();
        }
        let g = build_module_graph(&open(&root), None, GraphLimits::default()).unwrap();
        let edge = &g.node(&path("entry.ts")).unwrap().edges[0];
        if let Some(target) = selected {
            assert!(g.configuration_issues().is_empty(), "{name}");
            let target = if let Some((directory, _)) = name.rsplit_once('/') {
                format!("{directory}/{target}/dep.ts")
            } else {
                format!("{target}/dep.ts")
            };
            assert_eq!(
                edge.resolution,
                Resolution::Local {
                    path: path(&target),
                    kind: FileKind::Module,
                },
                "{name}"
            );
        } else {
            assert!(!g.configuration_issues().is_empty(), "{name}");
            assert_eq!(edge.resolution, Resolution::Unresolved(U::Configuration));
        }
    }
}
