//! Static evaluation contracts through the engine facade, on fictional
//! configuration files created at test time.

use std::fs;
use std::path::PathBuf;

use frameshift_engine::bindings::ExportName;
use frameshift_engine::evaluate::{EvaluateLimits, Evaluator, Number, Object, Reason, Value};
use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;

mod support;

fn path(s: &str) -> SourcePath {
    SourcePath::new(s, PathLimits::default()).unwrap()
}

fn open(name: &str, files: &[(&str, &str)]) -> Snapshot {
    let root: PathBuf = support::temporary().join("evaluate").join(name);
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

fn config(name: &str, files: &[(&str, &str)], module: &str) -> Value {
    config_with(name, files, module, EvaluateLimits::default())
}

fn config_with(name: &str, files: &[(&str, &str)], module: &str, limits: EvaluateLimits) -> Value {
    let snapshot = open(name, files);
    let mut evaluator =
        Evaluator::new(&snapshot, None, Limits::default().modules(), limits).unwrap();
    evaluator.config(&path(module)).unwrap()
}

fn string(s: &str) -> Value {
    Value::String(s.to_owned())
}

fn number(n: f64) -> Value {
    Value::Number(Number(n))
}

fn object(value: &Value) -> &Object {
    let Value::Object(object) = value else {
        panic!("not an object: {value:?}");
    };
    object
}

fn reason(value: &Value) -> &Reason {
    let Value::Unknown(unknown) = value else {
        panic!("not unknown: {value:?}");
    };
    &unknown.reason
}

#[test]
fn module_configs_evaluate_literals_templates_and_environment() {
    let value = config(
        "esm",
        &[(
            "next.config.mjs",
            "const base = '/docs';\nconst nextConfig = {\n  reactStrictMode: true,\n  output: 'export',\n  images: { unoptimized: !false, sizes: [16, 32, ...[64]] },\n  basePath: process.env.BASE_PATH,\n  env: { label: `v${1 + 1}-${base}`, sum: `${0.1 + 0.2}`, big: 1e21 + '' },\n  trailingSlash: base.length > 3 ? 'yes' : 'no',\n  [`computed${1}`]: null ?? 'fallback',\n  0x10: typeof base,\n};\nexport default nextConfig;\n",
        )],
        "next.config.mjs",
    );
    let o = object(&value);
    assert!(o.complete);
    assert_eq!(o.get("reactStrictMode"), Some(&Value::Bool(true)));
    assert_eq!(o.get("output"), Some(&string("export")));
    let images = object(o.get("images").unwrap());
    assert_eq!(
        images.get("sizes"),
        Some(&Value::Array(vec![
            number(16.0),
            number(32.0),
            number(64.0)
        ]))
    );
    assert_eq!(
        reason(o.get("basePath").unwrap()),
        &Reason::Environment(Some("BASE_PATH".into()))
    );
    let env = object(o.get("env").unwrap());
    assert_eq!(env.get("label"), Some(&string("v2-/docs")));
    assert_eq!(env.get("sum"), Some(&string("0.30000000000000004")));
    assert_eq!(env.get("big"), Some(&string("1e+21")));
    assert_eq!(o.get("trailingSlash"), Some(&string("yes")));
    assert_eq!(o.get("computed1"), Some(&string("fallback")));
    // Array-index keys come first, as JavaScript orders own keys.
    assert_eq!(o.entries[0], ("16".to_owned(), string("string")));
    assert!(!value.is_known());
    assert_eq!(value.unknowns().len(), 1);
}

#[test]
fn common_js_typescript_and_imports_resolve_through_local_modules() {
    let files = [
        ("package.json", r#"{"name":"fictional-site"}"#),
        (
            "next.config.js",
            "const shared = require('./shared');\nconst c = { trailingSlash: true, ...shared };\nmodule.exports = c;\n",
        ),
        (
            "next.config.ts",
            "import type { Config } from './types';\nimport { base } from './base.mjs';\nimport data from './data.json';\nimport * as all from './base.mjs';\nconst config: Config = { ...base, name: data.name, version: data.versions[1], deep: base.nested.flag } satisfies Config;\nexport default config;\n",
        ),
        (
            "types.ts",
            "export type Config = Record<string, unknown>;\n",
        ),
        ("base.mjs", "export { base } from './inner.mjs';\n"),
        (
            "inner.mjs",
            "export const base = { images: { unoptimized: true }, nested: { flag: 'on' } };\n",
        ),
        ("data.json", r#"{"name":"fictional","versions":[1,2]}"#),
        ("shared.js", "module.exports = { x: 1 };\n"),
    ];
    let value = config("cjs-ts", &files, "next.config.ts");
    let o = object(&value);
    assert!(o.complete && value.is_known(), "{value:?}");
    assert_eq!(o.get("name"), Some(&string("fictional")));
    assert_eq!(o.get("version"), Some(&number(2.0)));
    assert_eq!(o.get("deep"), Some(&string("on")));
    assert!(o.get("images").is_some());
    let value = config("cjs-ts-2", &files, "next.config.js");
    let o = object(&value);
    // `require` is a call, so the spread leaves the object incomplete.
    assert!(!o.complete);
    assert_eq!(o.get("trailingSlash"), Some(&Value::Bool(true)));
}

#[test]
fn calls_functions_packages_and_mutations_stay_unknown() {
    let cases = [
        (
            "const withX = require('x');\nmodule.exports = withX({ a: 1 });\n",
            Reason::Call(Some("withX".into())),
        ),
        (
            "module.exports = (phase) => ({ a: phase });\n",
            Reason::Function,
        ),
        (
            "const c = { a: 1 };\nc.b = 2;\nmodule.exports = c;\n",
            Reason::Mutable,
        ),
        (
            "const c = { list: [] };\nc.list.push(3);\nmodule.exports = c;\n",
            Reason::Mutable,
        ),
        (
            "const c = { a: 1 };\nconst d = { c };\nd.c.x = 1;\nmodule.exports = c;\n",
            Reason::Mutable,
        ),
        (
            "const c = { a: 1 };\nmutate(c);\nmodule.exports = c;\n",
            Reason::Mutable,
        ),
        ("let a = 1;\na = 2;\nmodule.exports = a;\n", Reason::Mutable),
        (
            "module.exports = { a: 1 };\nmodule.exports.b = 2;\n",
            Reason::Mutable,
        ),
        ("module.exports = /x/;\n", Reason::RegExp),
        ("module.exports = new Map();\n", Reason::Unsupported),
        (
            "module.exports = window.config;\n",
            Reason::Global("window".into()),
        ),
    ];
    for (i, (source, expected)) in cases.iter().enumerate() {
        let value = config(
            &format!("unknown-{i}"),
            &[("next.config.js", source)],
            "next.config.js",
        );
        assert_eq!(reason(&value), expected, "{source}");
    }
    let value = config(
        "package",
        &[(
            "next.config.mjs",
            "import withMDX from '@fictional/mdx';\nexport default withMDX;\n",
        )],
        "next.config.mjs",
    );
    assert_eq!(
        reason(&value),
        &Reason::Package {
            package: "@fictional/mdx".into(),
            name: "default".into()
        }
    );
    let value = config(
        "let",
        &[("next.config.js", "let a = { x: 1 };\nmodule.exports = a;\n")],
        "next.config.js",
    );
    assert!(value.is_known());
}

#[test]
fn cycles_limits_and_missing_exports_are_explicit() {
    let value = config(
        "cycle",
        &[
            (
                "a.mjs",
                "export { x } from './b.mjs';\nexport const y = 1;\n",
            ),
            (
                "b.mjs",
                "import { x as z } from './a.mjs';\nexport const x = z;\n",
            ),
            (
                "next.config.mjs",
                "import { x } from './a.mjs';\nexport default { x };\n",
            ),
        ],
        "next.config.mjs",
    );
    assert_eq!(reason(object(&value).get("x").unwrap()), &Reason::Cycle);
    let mut chain = String::from("const a0 = 'xxxxxxxx';\n");
    for i in 1..=24 {
        chain.push_str(&format!("const a{i} = a{} + a{};\n", i - 1, i - 1));
    }
    chain.push_str("export default a24;\n");
    let value = config("size", &[("next.config.mjs", &chain)], "next.config.mjs");
    assert_eq!(reason(&value), &Reason::ValueLimit);
    let value = config_with(
        "work",
        &[(
            "next.config.mjs",
            "export default [1, 2, 3, 4, 5, 6, 7, 8];\n",
        )],
        "next.config.mjs",
        EvaluateLimits {
            max_work: 4,
            ..EvaluateLimits::default()
        },
    );
    assert_eq!(reason(&value), &Reason::WorkLimit);
    let snapshot = open("missing", &[("a.mjs", "export const a = 1;\n")]);
    let mut evaluator = Evaluator::new(
        &snapshot,
        None,
        Limits::default().modules(),
        EvaluateLimits::default(),
    )
    .unwrap();
    assert_eq!(
        evaluator
            .export(&path("a.mjs"), &ExportName::Named("a".into()))
            .unwrap(),
        number(1.0)
    );
    assert_eq!(
        reason(
            &evaluator
                .export(&path("a.mjs"), &ExportName::Named("b".into()))
                .unwrap()
        ),
        &Reason::NotExported
    );
    assert_eq!(
        reason(&evaluator.config(&path("missing.mjs")).unwrap()),
        &Reason::Unresolved
    );
}
