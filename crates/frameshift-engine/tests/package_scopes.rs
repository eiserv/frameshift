//! Package scopes, `imports`, self-references, and local directory entry
//! points through the engine facade, with fictional manifests only.

use std::fs;
use std::path::PathBuf;

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, PathLimits, SourcePath, SourceText, TextLimits,
};
use frameshift_engine::input::snapshot;
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{
    EdgeKind, FileKind, GraphLimits, ModuleGraph, ModuleLimits, PackageManifest, Resolution,
    ResolutionConfig, ResolutionMode as M, Resolver, UnresolvedReason as U, build_module_graph,
};
use frameshift_engine::syntax::JsonLimits;

mod support;

fn path(s: &str) -> SourcePath {
    SourcePath::new(s, PathLimits::default()).unwrap()
}
fn text(s: &str) -> SourceText {
    SourceText::new(s.to_owned(), TextLimits::default()).unwrap()
}
fn manifest(s: &str) -> Option<PackageManifest> {
    PackageManifest::parse(&text(s), JsonLimits::default())
}
fn config(s: &str) -> ResolutionConfig {
    ResolutionConfig::parse(&text(s), &path("tsconfig.json"), JsonLimits::default())
}

/// A resolver over `files`, where entries with content are manifests.
fn resolver(files: &[(&str, Option<&str>)], config: ResolutionConfig) -> Resolver {
    let mut r = Resolver::new(
        files.iter().map(|(p, _)| path(p)).collect(),
        None,
        config,
        ModuleLimits::default(),
    )
    .unwrap();
    for (p, content) in files {
        if let Some(content) = content {
            r.add_manifest(&path(p), manifest(content)).unwrap();
        }
    }
    r
}

fn local(target: &str) -> Resolution {
    Resolution::Local {
        path: path(target),
        kind: FileKind::of(&path(target)),
    }
}

fn get(r: &mut Resolver, importer: &str, specifier: &str, mode: M) -> Resolution {
    r.resolve_in_mode(&path(importer), specifier, mode).unwrap()
}

const IMPORTS: &str = r##"{
  "name": "fictional-root",
  "imports": {
    "#x": "./src/x.js",
    "#n": "./src/x",
    "#p/*": "./src/*.js",
    "#d/": "./src/",
    "#c": { "import": "./src/x.js", "require": "./src/y.js" },
    "#nc": { "import": null, "default": "./src/x.js" },
    "#a": ["./src/missing.js", "./src/x.js"],
    "#null": null,
    "#bare": "fictional-dependency/sub",
    "#self": "#x",
    "#loop": "#loop",
    "#rel": "../outside.js",
    "#dots": "./src/../src/x.js",
    "#css": "./styles/s.css",
    "#asset": "./styles/t.css",
    "#versioned": { "types@>=5": "./src/y.d.ts", "default": "./src/x.js" }
  }
}"##;

fn imports_resolver() -> Resolver {
    resolver(
        &[
            ("package.json", Some(IMPORTS)),
            ("src/main.ts", None),
            ("src/x.ts", None),
            ("src/x.js", None),
            ("src/y.js", None),
            ("src/y.d.ts", None),
            ("src/a$b.ts", None),
            ("styles/s.css", None),
            ("styles/s.d.css.ts", None),
            ("styles/t.css", None),
        ],
        ResolutionConfig::default(),
    )
}

#[test]
fn package_imports_follow_keys_conditions_arrays_and_targets() {
    let mut r = imports_resolver();
    let mut at = |specifier, mode| get(&mut r, "src/main.ts", specifier, mode);
    assert_eq!(at("#x", M::Import), local("src/x.ts"));
    assert_eq!(at("#p/x", M::Import), local("src/x.ts"));
    assert_eq!(at("#p/y", M::Import), local("src/y.d.ts"));
    // `$` patterns of JavaScript string replacement apply to the capture.
    assert_eq!(at("#p/a$b", M::Import), local("src/a$b.ts"));
    assert_eq!(at("#p/a$$b", M::Import), local("src/a$b.ts"));
    assert_eq!(at("#d/x.js", M::Import), local("src/x.ts"));
    assert_eq!(at("#c", M::Import), local("src/x.ts"));
    assert_eq!(at("#c", M::Require), local("src/y.d.ts"));
    assert_eq!(
        at("#nc", M::Import),
        Resolution::Unresolved(U::MappedToNull)
    );
    assert_eq!(at("#nc", M::Require), local("src/x.ts"));
    assert_eq!(at("#a", M::Import), local("src/x.ts"));
    assert_eq!(
        at("#null", M::Import),
        Resolution::Unresolved(U::MappedToNull)
    );
    assert_eq!(
        at("#bare", M::Import),
        Resolution::External {
            package: "fictional-dependency".into()
        }
    );
    assert_eq!(at("#self", M::Import), local("src/x.ts"));
    assert_eq!(at("#css", M::Import), local("styles/s.d.css.ts"));
    // Assets are a fallback after arbitrary-extension declarations.
    assert_eq!(at("#asset", M::Import), local("styles/t.css"));
    for unresolved in ["#n", "#p/../x", "#rel", "#dots", "#unknown", "#"] {
        assert_eq!(
            at(unresolved, M::Import),
            Resolution::Unresolved(U::NotFound),
            "{unresolved}"
        );
    }
    // Unmodeled reference semantics never select a guessed target.
    assert_eq!(
        at("#loop", M::Import),
        Resolution::Unresolved(U::PackageImports)
    );
    assert_eq!(
        at("#versioned", M::Import),
        Resolution::Unresolved(U::PackageImports)
    );
}

#[test]
fn self_references_use_exports_and_respect_allow_js_passes() {
    let manifest = r#"{
      "name": "@fictional/self",
      "exports": {
        ".": "./src/x.js",
        "./p/*": "./src/*.js",
        "./j": { "import": "./src/only.js", "types": "./src/only-types.d.ts" },
        "./n": null
      }
    }"#;
    let files = [
        ("package.json", Some(manifest)),
        ("src/main.ts", None),
        ("src/x.ts", None),
        ("src/y.d.ts", None),
        ("src/only.js", None),
        ("src/only-types.d.ts", None),
    ];
    for (allow_js, j) in [(false, "src/only-types.d.ts"), (true, "src/only.js")] {
        let options = format!(r#"{{"compilerOptions":{{"allowJs":{allow_js}}}}}"#);
        let mut r = resolver(&files, config(&options));
        let mut at = |specifier| get(&mut r, "src/main.ts", specifier, M::Import);
        assert_eq!(at("@fictional/self"), local("src/x.ts"));
        assert_eq!(at("@fictional/self/p/y"), local("src/y.d.ts"));
        assert_eq!(at("@fictional/self/j"), local(j), "allowJs {allow_js}");
        assert_eq!(
            at("@fictional/self/n"),
            Resolution::Unresolved(U::MappedToNull)
        );
        // A subpath outside the map continues to bare classification.
        assert_eq!(
            at("@fictional/self/missing"),
            Resolution::External {
                package: "@fictional/self".into()
            }
        );
        assert_eq!(
            at("@fictional/other"),
            Resolution::External {
                package: "@fictional/other".into()
            }
        );
    }
}

#[test]
fn directory_entry_points_prefer_types_then_main_then_index() {
    let files = [
        ("src/main.ts", None),
        ("src/x.ts", None),
        ("d1/package.json", Some(r#"{"main":"./lib/index.js"}"#)),
        ("d1/lib/index.js", None),
        ("d1/lib/index.d.ts", None),
        (
            "d2/package.json",
            Some(r#"{"types":"./t.d.ts","main":"./m.js"}"#),
        ),
        ("d2/t.d.ts", None),
        ("d2/m.js", None),
        // A missing `types` target never falls back to `main`.
        (
            "d3/package.json",
            Some(r#"{"types":"./missing.d.ts","main":"./m.js"}"#),
        ),
        ("d3/m.js", None),
        ("d3/index.js", None),
        ("d4/package.json", Some(r#"{"main":"lib"}"#)),
        ("d4/lib/index.tsx", None),
        ("d5/package.json", Some(r#"{"main":"","typings":7}"#)),
        ("d5/index.d.ts", None),
        ("d6/package.json", Some(r#"{"main":"../src/x.js"}"#)),
        (
            "d7/package.json",
            Some(r#"{"typesVersions":{"*":{"*":["t/*"]}}}"#),
        ),
        ("d7/index.ts", None),
        ("d8/package.json", Some("{ invalid")),
        ("d8/index.ts", None),
        ("d9/package.json", None),
        ("d9/index.ts", None),
    ];
    let mut r = resolver(&files, ResolutionConfig::default());
    let mut at = |specifier| get(&mut r, "src/main.ts", specifier, M::Import);
    assert_eq!(at("../d1"), local("d1/lib/index.d.ts"));
    assert_eq!(at("../d2"), local("d2/t.d.ts"));
    assert_eq!(at("../d3"), local("d3/index.js"));
    assert_eq!(at("../d4/"), local("d4/lib/index.tsx"));
    assert_eq!(at("../d5"), local("d5/index.d.ts"));
    assert_eq!(at("../d6"), local("src/x.ts"));
    assert_eq!(at("../d7"), Resolution::Unresolved(U::DirectoryPackage));
    assert_eq!(at("../d8"), Resolution::Unresolved(U::PackageManifest));
    // An admitted manifest that was not supplied is not guessed.
    assert_eq!(at("../d9"), Resolution::Unresolved(U::DirectoryPackage));
}

#[test]
fn modes_follow_extensions_edge_kinds_and_emitted_format() {
    let r = resolver(
        &[
            ("a.ts", None),
            ("a.cts", None),
            ("a.mts", None),
            ("a.d.cts", None),
        ],
        ResolutionConfig::default(),
    );
    assert_eq!(r.mode(&path("a.ts"), EdgeKind::Import), M::Import);
    assert_eq!(r.mode(&path("a.cts"), EdgeKind::Import), M::Require);
    assert_eq!(r.mode(&path("a.d.cts"), EdgeKind::ImportType), M::Require);
    assert_eq!(r.mode(&path("a.mts"), EdgeKind::ImportEquals), M::Require);
    for (options, ts, dynamic_cts) in [
        (
            r#"{"compilerOptions":{"module":"CommonJS"}}"#,
            M::Require,
            M::Require,
        ),
        (
            r#"{"compilerOptions":{"target":"es5"}}"#,
            M::Require,
            M::Require,
        ),
        (
            r#"{"compilerOptions":{"module":"preserve"}}"#,
            M::Import,
            M::Import,
        ),
        (
            r#"{"compilerOptions":{"target":"ES2017"}}"#,
            M::Import,
            M::Require,
        ),
    ] {
        let r = resolver(&[("a.ts", None), ("a.cts", None)], config(options));
        assert_eq!(r.mode(&path("a.ts"), EdgeKind::Import), ts, "{options}");
        assert_eq!(
            r.mode(&path("a.cts"), EdgeKind::DynamicImport),
            dynamic_cts,
            "{options}"
        );
    }
    for unsupported in [
        r#"{"compilerOptions":{"module":"nodenext"}}"#,
        r#"{"compilerOptions":{"module":"amd"}}"#,
    ] {
        assert!(!config(unsupported).issues().is_empty(), "{unsupported}");
    }
    assert!(
        !config(r#"{"compilerOptions":{"module":"unknown"}}"#)
            .issues()
            .is_empty()
    );
}

#[test]
fn output_directories_and_unknown_scopes_never_guess() {
    let files = [
        ("package.json", Some(r##"{"imports":{"#x":"./src/x.js"}}"##)),
        ("src/main.ts", None),
        ("src/x.ts", None),
        ("nested/package.json", Some("[]")),
        ("nested/main.ts", None),
    ];
    let mut r = resolver(&files, config(r#"{"compilerOptions":{"outDir":"dist"}}"#));
    assert_eq!(
        get(&mut r, "src/main.ts", "#x", M::Import),
        Resolution::Unresolved(U::Configuration)
    );
    let mut r = resolver(&files, ResolutionConfig::default());
    assert_eq!(
        get(&mut r, "src/main.ts", "#x", M::Import),
        local("src/x.ts")
    );
    // The nearest manifest is the scope, even when it is invalid.
    for specifier in ["#x", "fictional-dependency"] {
        assert_eq!(
            get(&mut r, "nested/main.ts", specifier, M::Import),
            Resolution::Unresolved(U::PackageManifest),
            "{specifier}"
        );
    }
}

fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = support::temporary().join("package-scopes").join(name);
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

fn graph(name: &str, files: &[(&str, &str)], package: Option<&str>) -> ModuleGraph {
    let root = scratch(name, files);
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(&root, Limits::default().snapshot(), &mut findings).unwrap();
    build_module_graph(
        &snapshot,
        package.map(path).as_ref(),
        GraphLimits::default(),
    )
    .unwrap()
}

#[test]
fn snapshot_graph_reads_manifests_and_assigns_edge_modes() {
    let g = graph(
        "edge-modes",
        &[
            ("package.json", r#"{"name":"fictional-workspace"}"#),
            (
                "app/package.json",
                r##"{"name":"fictional-app","imports":{"#c":{"require":"./r.ts","import":"./i.ts"}},"exports":{"./e":"./i.ts"}}"##,
            ),
            (
                "app/main.ts",
                "import a from '#c';\nimport b = require('#c');\nimport type {T} from '#c' with { \"resolution-mode\": \"require\" };\ntype U = import('#c', { with: { \"resolution-mode\": \"require\" } }).U;\nimport c from 'fictional-app/e';\nimport d from './lib';\n",
            ),
            ("app/main.cts", "import a from '#c';\n"),
            ("app/i.ts", ""),
            ("app/r.ts", ""),
            ("app/lib/package.json", r#"{"types":"./types.d.ts"}"#),
            ("app/lib/types.d.ts", ""),
        ],
        Some("app"),
    );
    let edges = |file: &str| -> Vec<(M, Resolution)> {
        g.node(&path(file))
            .unwrap()
            .edges
            .iter()
            .map(|e| (e.mode, e.resolution.clone()))
            .collect()
    };
    assert_eq!(
        edges("app/main.ts"),
        [
            (M::Import, local("app/i.ts")),
            (M::Require, local("app/r.ts")),
            (M::Require, local("app/r.ts")),
            (M::Require, local("app/r.ts")),
            (M::Import, local("app/i.ts")),
            (M::Import, local("app/lib/types.d.ts")),
        ]
    );
    assert_eq!(edges("app/main.cts"), [(M::Require, local("app/r.ts"))]);
}

#[test]
#[ignore = "requires an external TypeScript 6.0.3 installation"]
fn external_typescript_package_scope_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let module = std::env::var("FRAMESHIFT_TYPESCRIPT_MODULE")
        .expect("set FRAMESHIFT_TYPESCRIPT_MODULE to an external typescript package directory");
    // The reference computes each mode itself from a one-line source file,
    // so the comparison covers mode assignment as well as lookup.
    let script = r#"
const ts = require(process.argv[1]);
if (ts.version !== '6.0.3') throw Error('expected TypeScript 6.0.3');
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const files = new Map(Object.entries(input.files).map(([p, c]) => ['/project/' + p, c]));
const host = { fileExists: p => files.has(p), readFile: p => files.get(p) ?? undefined,
 directoryExists: p => [...files.keys()].some(f => f.startsWith(p.replace(/\/$/, '') + '/')),
 getCurrentDirectory: () => '/project', realpath: p => p };
const options = ts.convertCompilerOptionsFromJson(input.options, '/project').options;
if (input.options && Object.keys(input.options).length && !options) throw Error('options');
options.moduleResolution = ts.ModuleResolutionKind.Bundler;
options.resolveJsonModule = true;
for (const [importer, specifier, kind] of input.requests) {
 const literal = JSON.stringify(specifier);
 const code = kind === 'dynamic' ? `import(${literal});` : kind === 'equals'
  ? `import x = require(${literal});` : `import ${literal};`;
 const sf = ts.createSourceFile('/project/' + importer, code, ts.ScriptTarget.Latest, true);
 sf.impliedNodeFormat = ts.getImpliedNodeFormatForFile('/project/' + importer, undefined, host, options);
 let usage;
 (function visit(n) { if (ts.isStringLiteral(n)) usage = n; ts.forEachChild(n, visit); })(sf);
 const mode = ts.getModeForUsageLocation(sf, usage, options);
 const r = ts.resolveModuleName(specifier, '/project/' + importer, options, host, undefined, undefined, mode).resolvedModule;
 process.stdout.write((r ? r.resolvedFileName.slice('/project/'.length) : '-') + ' ' + (mode === 1 ? 'require' : 'import') + '\n');
}
"#;
    let (files, requests) = oracle_project();
    let profiles = [
        r#"{"allowJs":true,"module":"esnext"}"#,
        r#"{"module":"esnext"}"#,
        r#"{"allowJs":true,"module":"commonjs"}"#,
        r#"{"target":"es5"}"#,
        r#"{"allowJs":true,"module":"preserve"}"#,
        r#"{"checkJs":true,"target":"es2022"}"#,
    ];
    let mut total = 0;
    let (mut locals, mut required) = (0, 0);
    let mut differences = Vec::new();
    for profile in profiles {
        let input = format!(
            r#"{{"options":{profile},"files":{{{}}},"requests":[{}]}}"#,
            files
                .iter()
                .map(|(p, c)| format!(
                    "{}:{}",
                    json(p),
                    c.as_ref().map_or("null".to_owned(), |c| json(c))
                ))
                .collect::<Vec<_>>()
                .join(","),
            requests
                .iter()
                .map(|(i, s, k)| format!("[{},{},{}]", json(i), json(s), json(k)))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut child = Command::new("node")
            .args(["-e", script, &module])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let answers = String::from_utf8(output.stdout).unwrap();
        let answers: Vec<_> = answers.lines().collect();
        assert_eq!(answers.len(), requests.len());
        let mut r = resolver(
            &files
                .iter()
                .map(|(p, c)| (p.as_str(), c.as_deref()))
                .collect::<Vec<_>>(),
            config(&format!(r#"{{"compilerOptions":{profile}}}"#)),
        );
        for ((importer, specifier, kind), expected) in requests.iter().zip(answers) {
            let kind = match kind.as_str() {
                "dynamic" => EdgeKind::DynamicImport,
                "equals" => EdgeKind::ImportEquals,
                _ => EdgeKind::Import,
            };
            let mode = r.mode(&path(importer), kind);
            let resolved = r.resolve_in_mode(&path(importer), specifier, mode).unwrap();
            let target = match &resolved {
                Resolution::Local { path, .. } => path.as_str(),
                Resolution::External { .. }
                | Resolution::NodeBuiltin
                | Resolution::Unresolved(_) => "-",
            };
            let mode = if mode == M::Require {
                "require"
            } else {
                "import"
            };
            let actual = format!("{target} {mode}");
            locals += usize::from(target != "-");
            required += usize::from(mode == "require");
            // The asset fallback is a documented extension of the profile.
            let asset = matches!(
                &resolved,
                Resolution::Local {
                    kind: FileKind::Stylesheet | FileKind::Asset,
                    ..
                }
            ) && expected.starts_with("- ");
            if actual != expected && !asset {
                differences.push(format!(
                    "{profile} {importer} {specifier} {kind:?}: {actual:?} != {expected:?}"
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
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    // Guard against a vacuous comparison.
    assert!(
        locals > total / 4 && required > total / 8,
        "{locals} {required}"
    );
    eprintln!(
        "TypeScript 6.0.3 package scope oracle: {total} cases, {locals} local, {required} require mode"
    );
}

fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A fictional workspace: one root package whose import map combines every
/// key shape, packages with distinct export maps, and directory packages.
#[expect(clippy::type_complexity, reason = "test data")]
fn oracle_project() -> (Vec<(String, Option<String>)>, Vec<(String, String, String)>) {
    let sources = [
        "src/x.ts",
        "src/x.js",
        "src/y.js",
        "src/y.d.ts",
        "src/z.tsx",
        "src/w.mjs",
        "src/w.d.mts",
        "src/v.cjs",
        "src/v.cts",
        "src/u.json",
        "src/only.js",
        "src/only-types.d.ts",
        "src/a$b.ts",
        "src/sub/index.ts",
        "src/sub/k.d.ts",
        "styles/s.css",
        "styles/s.d.css.ts",
        "lib/index.js",
        "lib/index.d.ts",
    ];
    let importers = ["src/main.ts", "src/main.cts", "src/main.mts", "src/main.js"];
    let targets = [
        "./src/x.js",
        "./src/x.ts",
        "./src/x",
        "./src/y.js",
        "./src/y.d.ts",
        "./src/z.js",
        "./src/w.mjs",
        "./src/v.cjs",
        "./src/v.cts",
        "./src/u.json",
        "./src/only.js",
        "./styles/s.css",
        "./src/sub/k.js",
        "./src/../src/x.js",
        "./node_modules/x.js",
        "../outside.js",
        "/abs.js",
        "fictional-dependency",
        "#x",
        "./src\\x.js",
        "./src//x.js",
    ];
    let mut imports = Vec::new();
    let mut requests: Vec<(String, String)> = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        imports.push(format!("{}:{}", json(&format!("#t{i}")), json(target)));
        requests.push(("root".into(), format!("#t{i}")));
    }
    let patterns = [
        ("#x", "./src/x.js"),
        ("#p/*", "./src/*.js"),
        ("#p/s*", "./src/sub/*.js"),
        ("#q/*.js", "./src/*.js"),
        ("#q/*", "./lib/*"),
        ("#d/", "./src/"),
        ("#e/", "./src/x.js"),
        ("#pfx*", "./src/*.ts"),
        ("#/*", "./src/*.js"),
        ("#k/*", "./src/x.js"),
        ("#m/*", "./src/*/index.js"),
        ("#ab*b", "./src/*.js"),
    ];
    for (key, target) in patterns {
        imports.push(format!("{}:{}", json(key), json(target)));
    }
    for request in [
        "#p/x",
        "#p/y",
        "#p/z",
        "#p/sub/k",
        "#p/s",
        "#p/sx",
        "#p/../x",
        "#p/a$b",
        "#p/a$$b",
        "#p/a$&",
        "#p/missing",
        "#q/x.js",
        "#q/y.js",
        "#q/index.js",
        "#q/x",
        "#d/x.js",
        "#d/sub/k.js",
        "#d/x",
        "#e/",
        "#e/x",
        "#pfxx",
        "#pfx",
        "#/x",
        "#k/anything",
        "#m/sub",
        "#ab",
        "#abxb",
        "#",
        "#unknown",
    ] {
        requests.push(("root".into(), request.to_owned()));
    }
    let conditions = [
        r#"{"import":"./src/x.js","require":"./src/y.js"}"#,
        r#"{"types":"./src/y.d.ts","default":"./src/x.js"}"#,
        r#"{"node":"./src/y.js","default":"./src/x.js"}"#,
        r#"{"default":"./src/x.js","import":"./src/y.js"}"#,
        r#"{"import":{"types":"./src/y.d.ts","default":"./src/x.js"},"default":"./src/z.js"}"#,
        r#"{"import":null,"default":"./src/x.js"}"#,
        r#"{"browser":"./src/y.js"}"#,
        r#"["./src/missing.js","./src/x.js"]"#,
        r#"["./src/missing.js",null,"./src/x.js"]"#,
        r#"[]"#,
        r#"true"#,
        r#"0"#,
        r#"null"#,
        r#"{"1":"./src/y.js","import":"./src/x.js","0":"./src/z.js","default":"./src/v.cjs"}"#,
        r#"{"require":["./src/missing.js","./src/v.cjs"],"default":"./src/w.mjs"}"#,
    ];
    for (i, condition) in conditions.iter().enumerate() {
        imports.push(format!("{}:{condition}", json(&format!("#c{i}"))));
        requests.push(("root".into(), format!("#c{i}")));
    }
    let mut files: Vec<(String, Option<String>)> = Vec::new();
    files.push((
        "root/package.json".into(),
        Some(format!(
            "{{\"name\":\"@fictional/root\",\"imports\":{{{}}},\"exports\":{{\".\":\"./src/x.js\",\"./p/*\":\"./src/*.js\",\"./j\":{{\"import\":\"./src/only.js\",\"types\":\"./src/only-types.d.ts\"}},\"./n\":null}}}}",
            imports.join(",")
        )),
    ));
    for request in [
        "@fictional/root",
        "@fictional/root/p/y",
        "@fictional/root/p/z",
        "@fictional/root/j",
        "@fictional/root/n",
        "@fictional/root/missing",
        "@fictional/other",
    ] {
        requests.push(("root".into(), request.to_owned()));
    }
    let exports = [
        r#""./src/x.js""#,
        r#"["./src/missing.js","./src/x.js"]"#,
        r#"{"import":"./src/x.js","require":"./src/v.cjs"}"#,
        r#"{".":null,"./a":"./src/y.js"}"#,
        r#"{".":{"types":"./src/y.d.ts","default":"./src/only.js"},"./d/":"./src/","./q/*.js":"./src/*.js"}"#,
        r#"{"./only":"./src/only.js","./types":{"types":"./src/only-types.d.ts"}}"#,
        r#"{}"#,
        r#"""#,
    ];
    for (i, map) in exports.iter().enumerate() {
        let name = format!("fictional-e{i}");
        files.push((
            format!("e{i}/package.json"),
            Some(format!("{{\"name\":\"{name}\",\"exports\":{map}}}")),
        ));
        for suffix in ["", "/a", "/d/x.js", "/q/y.js", "/only", "/types", "/x"] {
            requests.push((format!("e{i}"), format!("{name}{suffix}")));
        }
    }
    let directories = [
        r#"{"main":"./lib/index.js"}"#,
        r#"{"types":"./src/y.d.ts","main":"./src/x.js"}"#,
        r#"{"types":"./missing.d.ts","main":"./src/x.js"}"#,
        r#"{"main":"lib"}"#,
        r#"{"main":"./lib/"}"#,
        r#"{"main":"","typings":7}"#,
        r#"{"main":"src/v.cjs"}"#,
        r#"{"main":"./src/only.js"}"#,
        r#"{"typings":"./src/sub/k.d.ts","types":"./src/y.d.ts"}"#,
        r#"{"main":"./src/u.json"}"#,
        r#"{"main":"./styles/s.css"}"#,
        r#"{"main":"./src\\x.js"}"#,
        r#"{"main":"../e0/src/x.js"}"#,
        r#"{"exports":"./src/z.js"}"#,
        r#"{"main":"./src/sub"}"#,
    ];
    for (i, manifest) in directories.iter().enumerate() {
        files.push((format!("d{i}/package.json"), Some((*manifest).to_owned())));
        requests.push(("root".into(), format!("../../d{i}")));
        requests.push(("root".into(), format!("../../d{i}/")));
    }
    let mut packages: Vec<String> = vec!["root".into()];
    packages.extend((0..exports.len()).map(|i| format!("e{i}")));
    packages.extend((0..directories.len()).map(|i| format!("d{i}")));
    for package in &packages {
        for source in sources.iter().chain(importers.iter()) {
            files.push((format!("{package}/{source}"), None));
        }
    }
    // Directory packages without index files exercise the main-only paths.
    files.retain(|(p, _)| !(p.starts_with("d3/") && p.ends_with("index.js")));
    files.sort();
    let mut expanded = Vec::new();
    for (package, request) in requests {
        for importer in importers {
            for kind in ["static", "dynamic", "equals"] {
                if kind == "equals" && importer.ends_with(".js") {
                    continue;
                }
                expanded.push((
                    format!("{package}/{importer}"),
                    request.clone(),
                    kind.to_owned(),
                ));
            }
        }
    }
    (files, expanded)
}
