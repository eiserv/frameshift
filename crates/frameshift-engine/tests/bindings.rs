//! Scope and binding contracts through the engine facade, on fictional code.

use frameshift_engine::bindings::{
    Bindings, DeclarationKind, ExportKind, ExportName, ImportedName, Meaning, ScopeKind, Target,
    analyze,
};
use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{JsDialect, ParseLimits, SourceGoal, SyntaxTree, parse_js_program};

fn parse(source: &SourceText, goal: SourceGoal, dialect: JsDialect) -> SyntaxTree<'_> {
    parse_js_program(source, goal, dialect, ParseLimits::default()).unwrap()
}

/// Byte offsets of every identifier-like occurrence of `name`.
fn occurrences(source: &str, name: &str) -> Vec<usize> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    source
        .match_indices(name)
        .filter(|(i, _)| {
            !source[..*i].chars().next_back().is_some_and(word)
                && !source[i + name.len()..].chars().next().is_some_and(word)
        })
        .map(|(i, _)| i)
        .collect()
}

/// Describes each reference as `name<occurrence>-><target>`, where the
/// target is the declaring occurrence, `global`, `dynamic`, or `implicit`.
fn describe(source: &str, tree: &SyntaxTree<'_>, b: &Bindings) -> Vec<String> {
    let mut out = Vec::new();
    for r in b.references() {
        let start = tree.node(r.node).unwrap().span().start();
        let all = occurrences(source, &r.name);
        let index = all.iter().position(|&o| o == start).unwrap();
        let target = match r.target {
            Target::Global => "global".to_owned(),
            Target::Dynamic => "dynamic".to_owned(),
            Target::Binding(id) => {
                let binding = b.binding(id);
                match binding.declarations[0].name {
                    None => "implicit".to_owned(),
                    Some(name) => {
                        let at = tree.node(name).unwrap().span().start();
                        let decl = occurrences(source, &binding.name)
                            .iter()
                            .position(|&o| o == at)
                            .map_or_else(|| format!("@{at}"), |i| i.to_string());
                        format!("{}{decl}", binding.name)
                    }
                }
            }
        };
        out.push((start, format!("{}{index}->{target}", r.name)));
    }
    out.sort();
    out.into_iter().map(|(_, s)| s).collect()
}

fn check(source: &str, goal: SourceGoal, dialect: JsDialect, expected: &[&str]) {
    let text = SourceText::new(source.to_owned(), TextLimits::default()).unwrap();
    let tree = parse(&text, goal, dialect);
    let b = analyze(&tree).unwrap();
    let actual = describe(source, &tree, &b);
    for e in expected {
        assert!(actual.iter().any(|a| a == e), "missing {e} in {actual:?}");
    }
}

fn module(source: &str, expected: &[&str]) {
    check(source, SourceGoal::Module, JsDialect::TypeScript, expected);
}

fn script(source: &str, expected: &[&str]) {
    check(source, SourceGoal::Script, JsDialect::JavaScript, expected);
}

#[test]
fn lexical_scopes_hoisting_and_shadowing() {
    module(
        "use(x); let x = 1; { let x = 2; use(x); } function f(x) { return x; } use(x);",
        &["x0->x1", "x3->x2", "x5->x4", "x6->x1", "use0->global"],
    );
    module(
        "f(); function f() { var v; { var v; } return v; }",
        &["f0->f1", "v2->v0"],
    );
    // Parameter expressions cannot see the body's declarations.
    module(
        "function g(a = w, b = a) { var w; var a; return w; } function h(x: T): T { type T = 1; }",
        &["w0->global", "a1->a0", "w2->w1", "T0->global", "T1->global"],
    );
    module(
        "for (let i = 0; i < 1; i++) { i; } i; for (const k of ks) k;",
        &[
            "i1->i0",
            "i2->i0",
            "i3->i0",
            "i4->global",
            "k1->k0",
            "ks0->global",
        ],
    );
    module(
        "try {} catch (e) { e; } e; switch (s) { case 1: let c = 1; default: c; }",
        &["e1->e0", "e2->global", "c1->c0"],
    );
    module(
        "const g = function h() { return h; }; h; class C { m() { return C; } } const D = class E { n() { return E; } }; E;",
        &["h1->h0", "h2->global", "C1->C0", "E1->E0", "E2->global"],
    );
}

#[test]
fn patterns_defaults_writes_and_arguments() {
    let source = "let { a, b: [c = a, ...d] = [] } = o; [a, c] = [c, a]; a += 1; c++; function f(p = q, q) { return arguments; } const g = () => arguments;";
    module(
        source,
        &[
            "a1->a0",
            "a2->a0",
            "c1->c0",
            "a4->a0",
            "o0->global",
            "q0->q1",
            "arguments0->implicit",
            "arguments1->global",
        ],
    );
    let text = SourceText::new(source.to_owned(), TextLimits::default()).unwrap();
    let tree = parse(&text, SourceGoal::Module, JsDialect::TypeScript);
    let b = analyze(&tree).unwrap();
    let writes: Vec<_> = b
        .references()
        .iter()
        .filter(|r| r.write)
        .map(|r| (r.name.as_str(), r.read))
        .collect();
    assert_eq!(
        writes,
        [("a", false), ("c", false), ("a", true), ("c", true)]
    );
}

#[test]
fn annex_b_block_functions_and_dynamic_scopes() {
    script(
        "{ function f() {} } f; if (x) function g() {} g;",
        &["f1->f0", "g1->g0"],
    );
    // A lexical declaration blocks the Annex B variable.
    script("let f; { function f() {} f; } f;", &["f2->f1", "f3->f0"]);
    // Strict code keeps block functions block-scoped.
    module("{ function f() {} } f;", &["f1->global"]);
    script(
        "var o; with (o) { p; var q; } function s() { eval('x'); return y; } y;",
        &["o1->o0", "p0->dynamic", "y0->dynamic", "y1->global"],
    );
    // A local `eval` binding is not a direct eval.
    script(
        "function t(eval) { eval('x'); return z; }",
        &["z0->global", "eval1->eval0"],
    );
}

#[test]
fn imports_exports_and_jsx_references() {
    let source = "import L, { a as b, type T, default as D } from './m';\nimport * as ns from \"./n\";\nexport { b as c, T };\nexport const e = <L.Item><Local /><div />{ns}</L.Item>;\nexport default function F() {}\nexport * from './o';\nexport { x as y } from './p';\nfunction Local() {}\n";
    let text = SourceText::new(source.to_owned(), TextLimits::default()).unwrap();
    let tree = parse(&text, SourceGoal::Module, JsDialect::Tsx);
    let b = analyze(&tree).unwrap();
    let actual = describe(source, &tree, &b);
    for e in [
        "L1->L0",
        "L2->L0",
        "Local0->Local1",
        "ns1->ns0",
        "b1->b0",
        "T1->T0",
    ] {
        assert!(actual.iter().any(|a| a == e), "missing {e} in {actual:?}");
    }
    assert!(!actual.iter().any(|a| a.starts_with("div")));
    let imports: Vec<_> = b
        .bindings()
        .iter()
        .filter_map(|binding| {
            binding.import.as_ref().map(|i| {
                (
                    binding.name.as_str(),
                    i.imported.clone(),
                    i.specifier.as_deref(),
                    i.type_only,
                )
            })
        })
        .collect();
    assert_eq!(
        imports,
        [
            ("L", ImportedName::Default, Some("./m"), false),
            ("b", ImportedName::Named("a".into()), Some("./m"), false),
            ("T", ImportedName::Named("T".into()), Some("./m"), true),
            ("D", ImportedName::Default, Some("./m"), false),
            ("ns", ImportedName::Namespace, Some("./n"), false),
        ]
    );
    let names: Vec<_> = b.exports().iter().map(|e| e.name.clone()).collect();
    assert_eq!(
        names,
        [
            ExportName::Named("c".into()),
            ExportName::Named("T".into()),
            ExportName::Named("e".into()),
            ExportName::Default,
            ExportName::Star,
            ExportName::Named("y".into()),
        ]
    );
    let ExportKind::Local(Target::Binding(c)) = b.exports()[0].kind else {
        panic!("local export");
    };
    assert_eq!(b.binding(c).name, "b");
    assert!(matches!(
        &b.exports()[5].kind,
        ExportKind::ReExport {
            imported: ImportedName::Named(x),
            ..
        } if x == "x"
    ));
}

#[test]
fn typescript_meanings_merging_and_type_scopes() {
    module(
        "const X = 1; type X = string; let v: X = X; interface I<T> extends J<T> { m<U>(u: U): T } function g<T>(t: T): T { return t; } type M<P> = { [K in keyof P]: P[K] }; type C<A> = A extends Array<infer E> ? E : A;",
        &[
            "X2->X0",
            "X3->X0",
            "T1->T0",
            "U1->U0",
            "T2->T0",
            "T4->T3",
            "T5->T3",
            "t1->t0",
            "P1->P0",
            "K1->K0",
            "A1->A0",
            "E1->E0",
            "A2->A0",
            "J0->global",
        ],
    );
    module(
        "namespace N { export const a = 1; const b = a; } namespace N { a; b; } N.a; enum E { A = 1, B = A } E.B; let q: N.T; let r: typeof N.a;",
        &[
            "a1->a0",
            "a2->a0",
            "b1->global",
            "N2->N0",
            "A1->A0",
            "E1->E0",
            "N3->N0",
            "N4->N0",
        ],
    );
    module(
        "function o(x: string): void; function o(x: any) { return x; } o(1); class K implements L { constructor(public p: number) { p; } } declare module 'm' { const inner: number; } inner;",
        &["o2->o0", "x2->x1", "p1->p0", "L0->global", "inner1->global"],
    );
}

#[test]
fn bindings_record_kinds_scopes_and_declarations() {
    let source = "import d from 'x'; var v; let l; const c = 1; function f(p) {} class K {} type T = 1; interface I {} enum E {} namespace N { export const n = 1; } namespace Types { export type U = 1; }";
    let text = SourceText::new(source.to_owned(), TextLimits::default()).unwrap();
    let tree = parse(&text, SourceGoal::Module, JsDialect::TypeScript);
    let b = analyze(&tree).unwrap();
    let kind = |name: &str| {
        let binding = b.bindings().iter().find(|x| x.name == name).unwrap();
        (binding.declarations[0].kind, binding.meaning)
    };
    assert_eq!(kind("d"), (DeclarationKind::Import, Meaning::ALL));
    assert_eq!(kind("v"), (DeclarationKind::Var, Meaning::VALUE));
    assert_eq!(kind("p"), (DeclarationKind::Parameter, Meaning::VALUE));
    assert_eq!(
        kind("K"),
        (DeclarationKind::Class, Meaning::VALUE.union(Meaning::TYPE))
    );
    assert_eq!(kind("T"), (DeclarationKind::TypeAlias, Meaning::TYPE));
    assert_eq!(kind("E"), (DeclarationKind::Enum, Meaning::ALL));
    assert_eq!(
        kind("N"),
        (
            DeclarationKind::Namespace,
            Meaning::NAMESPACE.union(Meaning::VALUE)
        )
    );
    assert_eq!(
        kind("Types"),
        (DeclarationKind::Namespace, Meaning::NAMESPACE)
    );
    let n = b.bindings().iter().find(|x| x.name == "n").unwrap();
    assert_eq!(n.container.map(|c| b.binding(c).name.as_str()), Some("N"));
    assert_eq!(b.scope(b.bindings()[0].scope).kind, ScopeKind::Module);
    assert!(b.scopes().iter().any(|s| s.kind == ScopeKind::Namespace));
    // Every declaring name maps back to its binding.
    for (index, binding) in b.bindings().iter().enumerate() {
        for d in &binding.declarations {
            if let Some(name) = d.name {
                assert_eq!(b.declared(name).map(|id| id.index()), Some(index));
            }
        }
    }
    assert_eq!(b, analyze(&tree).unwrap());
}

#[test]
fn deep_trees_do_not_recurse() {
    let depth = 50_000;
    let source = format!("let a; {}a{};", "(".repeat(depth), ")".repeat(depth));
    let text = SourceText::new(source, TextLimits::default()).unwrap();
    let limits = ParseLimits {
        max_nodes: 1_000_000,
        max_tokens: 1_000_000,
        max_nesting_depth: 100_000,
        ..ParseLimits::default()
    };
    let parsed = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            let tree =
                parse_js_program(&text, SourceGoal::Module, JsDialect::JavaScript, limits).unwrap();
            // Analysis itself runs on this thread's ordinary stack frames.
            analyze(&tree).unwrap().references().len()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(parsed, 1);
}
