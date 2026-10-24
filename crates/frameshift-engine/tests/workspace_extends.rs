//! Package-based configuration inheritance through linked workspace members
//! (`D-098`) through the engine facade, with fictional workspaces only.

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{
    ConfigProblem, FileKind, ModuleGraph, Resolution, ResolutionProfile, UnresolvedReason as U,
    build_module_graph,
};

mod support;

const ROOT: &str = r#"{"name":"fixture-root","private":true,"packageManager":"pnpm@10.0.0"}"#;
const PNPM: &str = "packages:\n  - \"apps/*\"\n  - \"packages/*\"\n";

fn path(s: &str) -> SourcePath {
    SourcePath::new(s, PathLimits::default()).unwrap()
}

/// Writes a pnpm workspace with the given files below a fresh directory.
fn scratch(name: &str, files: &[(String, String)]) -> PathBuf {
    let root = support::temporary().join("workspace-extends").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    let defaults = [("package.json", ROOT), ("pnpm-workspace.yaml", PNPM)];
    for (name, text) in defaults
        .iter()
        .filter(|(name, _)| !files.iter().any(|(file, _)| file == name))
        .map(|(name, text)| (*name, *text))
        .chain(
            files
                .iter()
                .map(|(name, text)| (name.as_str(), text.as_str())),
        )
    {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    root
}

fn owned(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect()
}

fn open(root: &Path) -> Snapshot {
    let mut findings = FindingCollector::new(FindingLimits::default());
    snapshot(root, Limits::default().snapshot(), &mut findings).unwrap()
}

fn graph(snapshot: &Snapshot, package: &str) -> ModuleGraph {
    build_module_graph(snapshot, Some(&path(package)), Limits::default().modules()).unwrap()
}

/// The inherited configuration files, or the problems with their paths.
fn outcome(graph: &ModuleGraph) -> Result<Vec<&str>, Vec<(&str, ConfigProblem)>> {
    let config = graph.configuration();
    if config.issues().is_empty() {
        Ok(config.files().iter().map(SourcePath::as_str).collect())
    } else {
        Err(config
            .issues()
            .iter()
            .map(|issue| (issue.path.as_str(), issue.problem))
            .collect())
    }
}

fn app(name: &str, manifest: &str, tsconfig: &str) -> [(String, String); 2] {
    [
        (format!("apps/{name}/package.json"), manifest.to_owned()),
        (format!("apps/{name}/tsconfig.json"), tsconfig.to_owned()),
    ]
}

fn extends(reference: &str) -> String {
    format!(r#"{{"extends":{reference:?}}}"#)
}

#[test]
fn linked_member_supplies_inherited_settings_with_their_origins() {
    let root = scratch(
        "linked",
        &owned(&[
            (
                "apps/docs/package.json",
                r#"{"name":"docs","devDependencies":{"@fixture/standard":"workspace:*"}}"#,
            ),
            (
                "apps/docs/tsconfig.json",
                r#"{"extends":"@fixture/standard/tsconfig/next.json","compilerOptions":{"paths":{"@/*":["./src/*"]}}}"#,
            ),
            (
                "apps/docs/src/page.tsx",
                "import a from '@/lib/a';\nimport b from 'helper';\n",
            ),
            ("apps/docs/src/lib/a.ts", "export default 1;\n"),
            (
                "apps/api/package.json",
                r#"{"name":"api","dependencies":{"@fixture/standard":"workspace:^1.0.0"}}"#,
            ),
            (
                "apps/api/tsconfig.json",
                r#"{"extends":"@fixture/standard/tsconfig/base-url"}"#,
            ),
            ("apps/api/main.ts", "import b from 'helper';\n"),
            (
                "packages/standard/package.json",
                r#"{"name":"@fixture/standard","version":"1.0.0"}"#,
            ),
            (
                "packages/standard/tsconfig/next.json",
                r#"{"extends":"./base.json","compilerOptions":{"moduleResolution":"Bundler","module":"ESNext"}}"#,
            ),
            (
                "packages/standard/tsconfig/base.json",
                r#"{"compilerOptions":{"moduleResolution":"Node","allowJs":true}}"#,
            ),
            (
                "packages/standard/tsconfig/base-url.json",
                r#"{"compilerOptions":{"baseUrl":"."}}"#,
            ),
            (
                "packages/standard/tsconfig/helper.ts",
                "export default 2;\n",
            ),
        ]),
    );
    let snapshot = open(&root);
    let g = graph(&snapshot, "apps/docs");
    assert_eq!(
        outcome(&g),
        Ok(vec![
            "packages/standard/tsconfig/base.json",
            "packages/standard/tsconfig/next.json",
            "apps/docs/tsconfig.json",
        ])
    );
    // The later Bundler setting overrides the inherited Node setting.
    assert_eq!(g.configuration().profile(), ResolutionProfile::Bundler);
    let edges = &g.node(&path("apps/docs/src/page.tsx")).unwrap().edges;
    assert_eq!(
        edges[0].resolution,
        Resolution::Local {
            path: path("apps/docs/src/lib/a.ts"),
            kind: FileKind::Module,
        }
    );
    assert!(matches!(edges[1].resolution, Resolution::External { .. }));
    // An inherited base URL keeps its origin in the member, outside the
    // selected package.
    let g = graph(&snapshot, "apps/api");
    assert_eq!(
        outcome(&g),
        Ok(vec![
            "packages/standard/tsconfig/base-url.json",
            "apps/api/tsconfig.json",
        ])
    );
    assert_eq!(
        g.node(&path("apps/api/main.ts")).unwrap().edges[0].resolution,
        Resolution::Unresolved(U::OutsideRoot)
    );
}

#[test]
fn workspace_declarations_are_the_only_link_evidence() {
    let target = extends("@fixture/base/tsconfig.json");
    let mut files = owned(&[
        (
            "packages/base/package.json",
            r#"{"name":"@fixture/base","version":"1.2.0"}"#,
        ),
        ("packages/base/tsconfig.json", "{}"),
        (
            "packages/twin-a/package.json",
            r#"{"name":"@fixture/twin"}"#,
        ),
        ("packages/twin-a/tsconfig.json", "{}"),
        (
            "packages/twin-b/package.json",
            r#"{"name":"@fixture/twin"}"#,
        ),
        ("packages/twin-b/tsconfig.json", "{}"),
        ("tools/outside/tsconfig.json", &target),
        (
            "tools/outside/package.json",
            r#"{"devDependencies":{"@fixture/base":"workspace:*"}}"#,
        ),
        (
            "apps/chain/package.json",
            r#"{"devDependencies":{"@fixture/base":"workspace:*"}}"#,
        ),
        (
            "apps/chain/tsconfig.json",
            r#"{"extends":"./config/app.json"}"#,
        ),
        ("apps/chain/config/app.json", &target),
    ]);
    let declared =
        |field: &str, spec: &str| format!(r#"{{"{field}":{{"@fixture/base":"{spec}"}}}}"#);
    let cases: Vec<(&str, String, String, bool)> = vec![
        ("star", declared("devDependencies", "workspace:*"), target.clone(), true),
        ("caret", declared("dependencies", "workspace:^"), target.clone(), true),
        ("tilde", declared("dependencies", "workspace:~"), target.clone(), true),
        ("range", declared("dependencies", "workspace:^1.0.0"), target.clone(), true),
        ("optional", declared("optionalDependencies", "workspace:*"), target.clone(), true),
        ("unsatisfied", declared("dependencies", "workspace:^2.0.0"), target.clone(), false),
        ("plain", declared("dependencies", "^1.0.0"), target.clone(), false),
        ("alias", declared("dependencies", "workspace:@fixture/other@*"), target.clone(), false),
        ("path", declared("dependencies", "workspace:../../packages/base"), target.clone(), false),
        ("empty", declared("dependencies", "workspace:"), target.clone(), false),
        ("peer", declared("peerDependencies", "workspace:*"), target.clone(), false),
        (
            "mixed",
            r#"{"dependencies":{"@fixture/base":"workspace:*"},"devDependencies":{"@fixture/base":"^1.0.0"}}"#.to_owned(),
            target.clone(),
            false,
        ),
        ("none", "{}".to_owned(), target.clone(), false),
        (
            "installed",
            r#"{"devDependencies":{"@fixture/installed":"workspace:*"}}"#.to_owned(),
            extends("@fixture/installed/tsconfig.json"),
            false,
        ),
        (
            "twin",
            r#"{"devDependencies":{"@fixture/twin":"workspace:*"}}"#.to_owned(),
            extends("@fixture/twin/tsconfig.json"),
            false,
        ),
        (
            "root-name",
            r#"{"devDependencies":{"fixture-root":"workspace:*"}}"#.to_owned(),
            extends("fixture-root/tsconfig.json"),
            false,
        ),
        (
            "self",
            r#"{"name":"@fixture/self","devDependencies":{"@fixture/self":"workspace:*"}}"#.to_owned(),
            extends("@fixture/self/tsconfig.json"),
            false,
        ),
        (
            "imports",
            r##"{"imports":{"#config":"./local.json"}}"##.to_owned(),
            extends("#config"),
            false,
        ),
        ("dots", declared("dependencies", "workspace:*"), extends("@fixture/base/../base/tsconfig.json"), false),
        ("rooted", declared("dependencies", "workspace:*"), extends("/tsconfig.json"), false),
    ];
    for (name, manifest, tsconfig, _) in &cases {
        files.extend(app(name, manifest, tsconfig));
    }
    let snapshot = open(&scratch("declarations", &files));
    for (name, _, _, linked) in &cases {
        let package = format!("apps/{name}");
        let tsconfig = format!("{package}/tsconfig.json");
        let expected = if *linked {
            Ok(vec!["packages/base/tsconfig.json", tsconfig.as_str()])
        } else {
            Err(vec![(tsconfig.as_str(), ConfigProblem::Unsupported)])
        };
        assert_eq!(outcome(&graph(&snapshot, &package)), expected, "{name}");
    }
    // The nearest manifest of an inherited relative file declares the link.
    assert_eq!(
        outcome(&graph(&snapshot, "apps/chain")),
        Ok(vec![
            "packages/base/tsconfig.json",
            "apps/chain/config/app.json",
            "apps/chain/tsconfig.json",
        ])
    );
    // A package outside the workspace patterns is not installed as a member.
    assert_eq!(
        outcome(&graph(&snapshot, "tools/outside")),
        Err(vec![(
            "tools/outside/tsconfig.json",
            ConfigProblem::Unsupported
        )])
    );
}

#[test]
fn links_require_a_complete_workspace() {
    let member = [
        (
            "apps/web/package.json",
            r#"{"devDependencies":{"@fixture/base":"workspace:*"}}"#,
        ),
        (
            "apps/web/tsconfig.json",
            r#"{"extends":"@fixture/base/tsconfig.json"}"#,
        ),
        ("packages/base/package.json", r#"{"name":"@fixture/base"}"#),
        ("packages/base/tsconfig.json", "{}"),
    ];
    let mut plain = owned(&member);
    plain.push((
        "pnpm-workspace.yaml".to_owned(),
        "packages: []\n".to_owned(),
    ));
    let mut hidden = owned(&member);
    hidden.push((".gitignore".to_owned(), "packages/hidden/\n".to_owned()));
    hidden.push((
        "packages/hidden/package.json".to_owned(),
        r#"{"name":"@fixture/base"}"#.to_owned(),
    ));
    for (name, files) in [("no-members", plain), ("incomplete", hidden)] {
        let snapshot = open(&scratch(name, &files));
        assert_eq!(
            outcome(&graph(&snapshot, "apps/web")),
            Err(vec![("apps/web/tsconfig.json", ConfigProblem::Unsupported)]),
            "{name}"
        );
    }
}

/// The files of one generated member, with `NAME` in its manifest.
type Member = &'static [(&'static str, &'static str)];

/// One member per case, each with an app that declares and extends it.
fn member_cases(name: &str, cases: &[(Member, &str)]) -> Snapshot {
    let mut files = Vec::new();
    for (index, (member, request)) in cases.iter().enumerate() {
        let package = format!("@fixture/m{index}");
        let manifest = member
            .iter()
            .find(|(file, _)| *file == "package.json")
            .map_or_else(
                || format!(r#"{{"name":"{package}"}}"#),
                |(_, text)| text.replace("NAME", &package),
            );
        files.push((format!("packages/m{index}/package.json"), manifest));
        for (file, text) in member.iter().filter(|(file, _)| *file != "package.json") {
            files.push((format!("packages/m{index}/{file}"), (*text).to_owned()));
        }
        files.extend(app(
            &format!("c{index}"),
            &format!(r#"{{"devDependencies":{{"{package}":"workspace:*"}}}}"#),
            &extends(&request.replace("PKG", &package)),
        ));
    }
    open(&scratch(name, &files))
}

#[test]
fn member_lookup_follows_the_reference_configuration_lookup() {
    use ConfigProblem as P;
    let cases: &[(Member, &str, Result<&str, P>)] = &[
        (&[("base.json", "{}")], "PKG/base.json", Ok("base.json")),
        (&[("base.json", "{}")], "PKG/base", Ok("base.json")),
        // Code extensions are replaced by `.json` in a configuration lookup.
        (&[("base.json", "{}")], "PKG/base.ts", Ok("base.json")),
        // A missing explicit `.json` file still tries an appended `.json`.
        (
            &[("base.json.json", "{}")],
            "PKG/base.json",
            Ok("base.json.json"),
        ),
        (
            &[("dir/tsconfig.json", "{}")],
            "PKG/dir",
            Ok("dir/tsconfig.json"),
        ),
        (&[("tsconfig.json", "{}")], "PKG", Ok("tsconfig.json")),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","tsconfig":"configs/app.json"}"#,
                ),
                ("configs/app.json", "{}"),
                ("tsconfig.json", "{}"),
            ],
            "PKG",
            Ok("configs/app.json"),
        ),
        (
            &[
                ("sub/package.json", r#"{"tsconfig":"inner.json"}"#),
                ("sub/inner.json", "{}"),
                ("sub/tsconfig.json", "{}"),
            ],
            "PKG/sub",
            Ok("sub/inner.json"),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","exports":{".":{"import":"./esm.json","node":"./node.json"}}}"#,
                ),
                ("esm.json", "{}"),
                ("node.json", "{}"),
            ],
            "PKG",
            Ok("node.json"),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","exports":{"./presets/*":"./presets/*.json"}}"#,
                ),
                ("presets/strict.json", "{}"),
            ],
            "PKG/presets/strict",
            Ok("presets/strict.json"),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","exports":{"./hidden":null,"./*":"./*.json"}}"#,
                ),
                ("hidden.json", "{}"),
            ],
            "PKG/hidden",
            Err(P::Missing),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","exports":{".":"./tsconfig.json"}}"#,
                ),
                ("tsconfig.json", "{}"),
                ("base.json", "{}"),
            ],
            "PKG/base.json",
            Err(P::Missing),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","typesVersions":{"*":{"*":["x/*"]}}}"#,
                ),
                ("base.json", "{}"),
            ],
            "PKG/base",
            Err(P::Unsupported),
        ),
        (
            &[
                (
                    "package.json",
                    r#"{"name":"NAME","exports":{"types@>=5":"./a.json","default":"./b.json"}}"#,
                ),
                ("a.json", "{}"),
                ("b.json", "{}"),
            ],
            "PKG",
            Err(P::Unsupported),
        ),
        (
            &[(
                "package.json",
                r#"{"name":"NAME","tsconfig":"../m0/base.json"}"#,
            )],
            "PKG",
            Err(P::Unsupported),
        ),
        (&[("base.json", "{}")], "PKG/absent", Err(P::Missing)),
    ];
    let snapshot = member_cases(
        "lookup",
        &cases
            .iter()
            .map(|(member, request, _)| (*member, *request))
            .collect::<Vec<_>>(),
    );
    for (index, (_, request, expected)) in cases.iter().enumerate() {
        let g = graph(&snapshot, &format!("apps/c{index}"));
        let tsconfig = format!("apps/c{index}/tsconfig.json");
        let expected = match expected {
            Ok(file) => Ok(vec![format!("packages/m{index}/{file}"), tsconfig.clone()]),
            Err(problem) => Err(vec![(tsconfig.clone(), *problem)]),
        };
        let actual = outcome(&g)
            .map(|files| files.into_iter().map(str::to_owned).collect::<Vec<_>>())
            .map_err(|issues| {
                issues
                    .into_iter()
                    .map(|(path, problem)| (path.to_owned(), problem))
                    .collect::<Vec<_>>()
            });
        assert_eq!(actual, expected, "{index} {request}");
    }
}

#[test]
fn member_files_inherit_relatively_and_cycles_stop() {
    let snapshot = open(&scratch(
        "chains",
        &owned(&[
            (
                "apps/web/package.json",
                r#"{"devDependencies":{"@fixture/a":"workspace:*"}}"#,
            ),
            ("apps/web/tsconfig.json", r#"{"extends":"@fixture/a/next"}"#),
            (
                "apps/loop/package.json",
                r#"{"devDependencies":{"@fixture/a":"workspace:*"}}"#,
            ),
            (
                "apps/loop/tsconfig.json",
                r#"{"extends":"@fixture/a/loop.json"}"#,
            ),
            (
                "apps/hidden/package.json",
                r#"{"devDependencies":{"@fixture/a":"workspace:*"}}"#,
            ),
            (
                "apps/hidden/tsconfig.json",
                r#"{"extends":"@fixture/a/secret.json"}"#,
            ),
            ("apps/local/package.json", "{}"),
            ("apps/local/tsconfig.json", r#"{"extends":".\\local.json"}"#),
            ("apps/local/local.json", "{}"),
            (
                "packages/a/package.json",
                r#"{"name":"@fixture/a","dependencies":{"@fixture/b":"workspace:^"}}"#,
            ),
            ("packages/a/.gitignore", "secret.json\n"),
            ("packages/a/secret.json", "{}"),
            ("packages/a/next.json", r#"{"extends":"./base.json"}"#),
            ("packages/a/base.json", "{}"),
            (
                "packages/a/loop.json",
                r#"{"extends":"@fixture/b/loop.json"}"#,
            ),
            (
                "packages/b/package.json",
                r#"{"name":"@fixture/b","dependencies":{"@fixture/a":"workspace:*"}}"#,
            ),
            (
                "packages/b/loop.json",
                r#"{"extends":"@fixture/a/loop.json"}"#,
            ),
        ]),
    ));
    assert_eq!(
        outcome(&graph(&snapshot, "apps/web")),
        Ok(vec![
            "packages/a/base.json",
            "packages/a/next.json",
            "apps/web/tsconfig.json",
        ])
    );
    assert_eq!(
        outcome(&graph(&snapshot, "apps/loop")),
        Err(vec![("packages/a/loop.json", ConfigProblem::Cycle)])
    );
    assert_eq!(
        outcome(&graph(&snapshot, "apps/hidden")),
        Err(vec![("packages/a/secret.json", ConfigProblem::Unreadable)])
    );
    // Backslashes are separators before the string is classified.
    assert_eq!(
        outcome(&graph(&snapshot, "apps/local")),
        Ok(vec!["apps/local/local.json", "apps/local/tsconfig.json"])
    );
}

/// Generated member layouts: manifest shapes, file subsets, and requests.
/// Every app declares the member with `workspace:*`, so the reference's
/// `node_modules` link of the app points at the member.
const SHAPES: &[&str] = &[
    r#"{"name":"@fx/p"}"#,
    r#"{"name":"@fx/p","tsconfig":"custom.json"}"#,
    r#"{"name":"@fx/p","tsconfig":"custom"}"#,
    r#"{"name":"@fx/p","tsconfig":"dir"}"#,
    r#"{"name":"@fx/p","tsconfig":"./dir/"}"#,
    r#"{"name":"@fx/p","tsconfig":"custom.ts"}"#,
    r#"{"name":"@fx/p","exports":{".":"./main.json","./base":"./base.json","./*.json":"./cfg/*.json","./*":"./cfg/*"}}"#,
    r#"{"name":"@fx/p","exports":{".":{"import":"./import.json","require":"./require.json"},"./x":{"node":"./node.json","default":"./default.json"},"./t":{"types":"./types.json","require":"./require.json"}}}"#,
    r#"{"name":"@fx/p","exports":"./main.json"}"#,
    r#"{"name":"@fx/p","exports":{"./a":null,"./b":["./missing.json","./base.json"],"./c":"./base.ts","./d":{"import":"./main.json"},"./dir/":"./dir/"}}"#,
    r#"{"name":"@fx/p","exports":null}"#,
    r#"{"name":"@fx/p","exports":{"import":"./import.json","default":"./default.json"}}"#,
];

const FILES: &[(&str, &str)] = &[
    ("tsconfig.json", "{}"),
    ("base.json", "{}"),
    ("base", "{}"),
    ("base.json.json", "{}"),
    ("base.yaml.json", "{}"),
    ("base.tsx.json", "{}"),
    ("custom.json", "{}"),
    ("custom", "{}"),
    ("custom.ts.json", "{}"),
    ("dir/tsconfig.json", "{}"),
    ("dir.json", "{}"),
    ("dir/package.json", r#"{"tsconfig":"../base.json"}"#),
    ("sub/package.json", r#"{"tsconfig":"inner.json"}"#),
    ("sub/inner.json", "{}"),
    ("sub/tsconfig.json", "{}"),
    ("sub.json", "{}"),
    ("cfg/base.json", "{}"),
    ("cfg/base", "{}"),
    ("main.json", "{}"),
    ("require.json", "{}"),
    ("node.json", "{}"),
    ("default.json", "{}"),
    ("types.json", "{}"),
    ("import.json", "{}"),
];

const REQUESTS: &[&str] = &[
    "",
    "/base",
    "/base.json",
    "/base.ts",
    "/base.js",
    "/base.d.ts",
    "/base.tsx",
    "/base.yaml",
    "/custom",
    "/dir",
    "/dir/tsconfig",
    "/dir/tsconfig.json",
    "/sub",
    "/sub/inner",
    "/cfg/base",
    "/x",
    "/t",
    "/a",
    "/b",
    "/c",
    "/d",
    "/main",
    "/main.json",
    "/tsconfig",
];

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Compares the inherited file of every generated request with the pinned
/// TypeScript 6.0.3 `getExtendsConfigPath`, run through
/// `getParsedCommandLineOfConfigFile` over an in-memory host whose
/// `node_modules` links resolve to the member, as a package manager installs
/// a `workspace:` dependency. Only the external reference package runs.
#[test]
#[ignore = "requires an external TypeScript 6.0.3 installation"]
fn external_typescript_workspace_extends_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let module = std::env::var("FRAMESHIFT_TYPESCRIPT_MODULE")
        .expect("set FRAMESHIFT_TYPESCRIPT_MODULE to an external typescript package directory");
    let script = r#"
const ts = require(process.argv[1]);
if (ts.version !== '6.0.3') throw Error('expected TypeScript 6.0.3');
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const files = new Map(), links = new Map(), dirs = new Set();
for (const [w, project] of input.entries()) {
 for (const [p, c] of Object.entries(project.files)) files.set(`/w${w}/${p}`, c);
 for (const [l, t] of project.links) links.set(`/w${w}/${l}`, `/w${w}/${t}`);
}
const resolve = p => { const i = p.lastIndexOf('/node_modules/');
 if (i < 0) return p;
 for (const n of [2, 3]) { const l = p.split('/').slice(0, p.slice(0, i).split('/').length + n).join('/');
  if (links.has(l)) return links.get(l) + p.slice(l.length); }
 return p; };
for (const f of [...files.keys(), ...links.keys()]) { let d = f; while ((d = d.slice(0, d.lastIndexOf('/')))) dirs.add(d); }
for (const l of links.keys()) dirs.add(l);
const host = { useCaseSensitiveFileNames: true, readDirectory: () => [], getCurrentDirectory: () => '/',
 fileExists: p => files.has(resolve(p)), readFile: p => files.get(resolve(p)),
 directoryExists: p => dirs.has(resolve(p.replace(/\/$/, ''))), realpath: p => resolve(p),
 onUnRecoverableConfigFileDiagnostic: d => { throw Error(String(d.messageText)); } };
for (const [w, project] of input.entries()) for (const app of project.apps) {
 const parsed = ts.getParsedCommandLineOfConfigFile(`/w${w}/${app}/tsconfig.json`, undefined, host);
 const found = parsed.options.configFile.extendedSourceFiles;
 const errors = parsed.errors.filter(e => e.code !== 18003).map(e => e.code);
 process.stdout.write((found && found.length ? found[0].slice(`/w${w}/`.length) : '-') + ' ' + errors.join(',') + '\n');
}
"#;
    // A fixed linear congruential sequence selects file subsets.
    let mut seed: u64 = 0x5eed;
    let mut masks = vec![0u64, (1 << FILES.len()) - 1];
    for _ in 0..14 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        masks.push((seed >> 20) & ((1 << FILES.len()) - 1));
    }
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for (shape_index, shape) in SHAPES.iter().enumerate() {
        for (mask_index, mask) in masks.iter().enumerate() {
            let name = format!("oracle-{shape_index}-{mask_index}");
            let mut files: Vec<(String, String)> = vec![
                ("package.json".to_owned(), ROOT.to_owned()),
                ("pnpm-workspace.yaml".to_owned(), PNPM.to_owned()),
                ("packages/p/package.json".to_owned(), (*shape).to_owned()),
            ];
            for (bit, (file, text)) in FILES.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    files.push((format!("packages/p/{file}"), (*text).to_owned()));
                }
            }
            let mut apps = Vec::new();
            for (index, request) in REQUESTS.iter().enumerate() {
                let app_name = format!("r{index}");
                files.extend(app(
                    &app_name,
                    &format!(
                        r#"{{"name":"{app_name}","devDependencies":{{"@fx/p":"workspace:*"}}}}"#
                    ),
                    &extends(&format!("@fx/p{request}")),
                ));
                apps.push(format!("apps/{app_name}"));
            }
            let snapshot = open(&scratch(&name, &files));
            for app in &apps {
                let g = graph(&snapshot, app);
                let tsconfig = format!("{app}/tsconfig.json");
                expected.push(match outcome(&g) {
                    Ok(files) if files.len() == 2 && files[1] == tsconfig => files[0].to_owned(),
                    Err(issues) if issues == [(tsconfig.as_str(), ConfigProblem::Missing)] => {
                        "-".to_owned()
                    }
                    other => format!("{other:?}"),
                });
            }
            let files_json: Vec<String> = files
                .iter()
                .map(|(file, text)| format!("{}:{}", json_string(file), json_string(text)))
                .collect();
            let links: Vec<String> = apps
                .iter()
                .map(|app| format!(r#"["{app}/node_modules/@fx/p","packages/p"]"#))
                .collect();
            let apps: Vec<String> = apps.iter().map(|app| json_string(app)).collect();
            input.push(format!(
                r#"{{"files":{{{}}},"links":[{}],"apps":[{}]}}"#,
                files_json.join(","),
                links.join(","),
                apps.join(",")
            ));
        }
    }
    let mut child = Command::new("node")
        .args(["-e", script, &module])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node runs the external reference");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", input.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "the reference script failed");
    let actual: Vec<String> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(actual.len(), expected.len());
    let mut differences = 0;
    let mut found = 0;
    for (index, (reference, ours)) in actual.iter().zip(&expected).enumerate() {
        let (target, errors) = reference.split_once(' ').unwrap();
        let reference = if target == "-" {
            assert_eq!(errors, "6053", "case {index}");
            "-"
        } else {
            assert_eq!(errors, "", "case {index}");
            found += 1;
            target
        };
        if reference != ours {
            differences += 1;
            if differences <= 20 {
                let shape = index / (masks.len() * REQUESTS.len());
                let request = REQUESTS[index % REQUESTS.len()];
                eprintln!(
                    "shape {shape} request {request:?}: reference {reference}, frameshift {ours}"
                );
            }
        }
    }
    eprintln!(
        "{} cases, {found} resolved by the reference, {differences} differences",
        expected.len()
    );
    assert_eq!(differences, 0);
    // Rule out a vacuous comparison.
    assert!(found * 4 > expected.len());
}
