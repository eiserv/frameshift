//! Compares reference resolution with the pinned TypeScript 6.0.3 checker
//! on the generated corpora and hand-written fictional edge cases.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use frameshift_corpus::{Corpus, CorpusFile, DEFAULT_SEED};
use frameshift_engine::bindings::{Bindings, DeclarationKind, Target, analyze};
use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    JsDialect, JsonDialect, JsonLimits, ParseLimits, SourceGoal, parse_js_program, parse_json,
};

// The reference resolves every identifier as its language service does:
// shorthand properties and export specifiers through their local targets.
const SCRIPT: &str = r#"
const ts = require(process.argv[1]);
if (ts.version !== '6.0.3') throw Error('expected TypeScript 6.0.3');
const files = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const options = { noLib: true, allowJs: true, jsx: ts.JsxEmit.Preserve, target: ts.ScriptTarget.ESNext,
 module: ts.ModuleKind.ESNext, noResolve: true, alwaysStrict: false, types: [], noEmit: true };
const out = [];
for (const [name, text] of files) {
 const host = ts.createCompilerHost(options);
 host.getSourceFile = (n, v) => n === name ? ts.createSourceFile(n, text, v, true) : undefined;
 host.fileExists = n => n === name; host.readFile = n => n === name ? text : undefined;
 const program = ts.createProgram([name], options, host);
 const sf = program.getSourceFile(name), checker = program.getTypeChecker();
 const rows = [];
 const start = d => (ts.getNameOfDeclaration(d) ?? d).getStart(sf);
 let crashed = false;
 // The reference's checker can exhaust its stack on generated recursive types.
 try { (function visit(n) {
  if (ts.isIdentifier(n)) {
   const p = n.parent;
   let symbol;
   if (ts.isShorthandPropertyAssignment(p) && p.name === n) symbol = checker.getShorthandAssignmentValueSymbol(p);
   else if (ts.isExportSpecifier(p) && !p.parent.parent.moduleSpecifier && (p.propertyName ?? p.name) === n)
    symbol = checker.getExportSpecifierLocalTargetSymbol(p);
   else symbol = checker.getSymbolAtLocation(n);
   const member = (ts.isPropertyAccessExpression(p) && p.name === n) || (ts.isQualifiedName(p) && p.right === n)
    || (ts.isImportSpecifier(p) && p.propertyName === n) || ts.isJsxAttribute(p)
    || (ts.isExportSpecifier(p) && (p.parent.parent.moduleSpecifier || (p.propertyName && p.name === n)))
    || (ts.getNameOfDeclaration(p) === n);
   // JavaScript expando assignments such as `Promise.items = 1` are the
   // reference's own inference, not ECMAScript declarations.
   const expando = d => ts.isPropertyAccessExpression(d) || ts.isElementAccessExpression(d)
    || ts.isBinaryExpression(d) || ts.isIdentifier(d) || ts.isCallExpression(d);
   // Export specifiers are aliases, never local declarations.
   const decls = symbol ? (symbol.declarations ?? []).filter(d => d.getSourceFile() === sf && !expando(d)
    && !ts.isExportSpecifier(d)).map(start) : null;
   rows.push([n.getStart(sf), member ? 1 : 0, decls, symbol && symbol.escapedName === 'arguments' ? 1 : 0]);
  }
  ts.forEachChild(n, visit);
 })(sf); } catch (e) { if (!(e instanceof RangeError)) throw e; crashed = true; }
 // Redeclaration errors leave a conflicting declaration outside the symbol,
 // which the comparison excuses only for the names they report.
 const redeclared = (sf.bindDiagnostics ?? []).filter(d => [2300, 2451, 2392, 2393, 2394, 2395, 2440, 2813, 2814]
  .includes(d.code)).map(d => d.start);
 out.push(crashed ? [[[-1, 0, null, 0]], []] : [rows, redeclared]);
}
process.stdout.write(JSON.stringify(out));
"#;

const EDGE_CASES: &[(&str, &str)] = &[
    (
        "edge1.ts",
        "export const X = 1; export type X = string; let v: X = X; namespace N { export const a = 1; } namespace N { a; } enum E { A = 1, B = A } N.a; E.B;",
    ),
    (
        "edge2.tsx",
        "import L, { a as b, type T } from './m'; import * as ns from './n'; export { b as c, T }; export const e = <L.Item><Local x={ns} /></L.Item>; function Local() { return null; } export default function F<P>(p: P): P { return p; }",
    ),
    (
        "edge3.ts",
        "function o(x: string): void; function o(x: any) { return x; } class K<T> implements I<T> { constructor(public p: T) { p; } m<U>(u: U): T { return this.p; } } interface I<T> { m<U>(u: U): T } type C<A> = A extends Array<infer E> ? E : A; type M<P> = { [K in keyof P]: P[K] };",
    ),
    (
        "edge4.js",
        "var o = {}; function f(a, b = a) { var c = arguments; { let a = 1; a; } return { a, b, c }; } const g = function h() { return h; }; class D { #x = 1; static { var s; } } try { f(); } catch (e) { e; } for (const k in o) k; label: for (;;) break label;",
    ),
    (
        "edge5.ts",
        "declare global { var gv: number } declare module 'm' { export const mv: number; } import Q = N2.Inner; namespace N2.Inner { export type T = 1; } let r: typeof gv; let t: Q.T; export = r;",
    ),
];

/// Generates programs that redeclare a four-name pool in nested scopes, so
/// that most references depend on shadowing, hoisting, and type meanings.
struct Shadowing {
    state: u64,
    typescript: bool,
}

impl Shadowing {
    fn next(&mut self, bound: u64) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) % bound
    }

    fn name(&mut self) -> &'static str {
        ["a", "b", "c", "d"][self.next(4) as usize]
    }

    fn expression(&mut self, depth: u32) -> String {
        match if depth == 0 { 0 } else { self.next(6) } {
            0 | 1 => self.name().to_owned(),
            2 => format!(
                "{} + {}",
                self.expression(depth - 1),
                self.expression(depth - 1)
            ),
            3 => format!("(({}) => {})", self.name(), self.expression(depth - 1)),
            4 => format!(
                "function ({}) {{ return {}; }}",
                self.name(),
                self.expression(depth - 1)
            ),
            _ => format!("{}({})", self.name(), self.expression(depth - 1)),
        }
    }

    fn annotation(&mut self) -> String {
        if self.typescript && self.next(2) == 0 {
            format!(": {}", self.name())
        } else {
            String::new()
        }
    }

    fn block(&mut self, depth: u32) -> String {
        let count = 1 + self.next(4);
        (0..count).map(|_| self.statement(depth)).collect()
    }

    fn statement(&mut self, depth: u32) -> String {
        let choice = if depth == 0 {
            self.next(3)
        } else {
            self.next(if self.typescript { 12 } else { 10 })
        };
        let inner = depth.saturating_sub(1);
        match choice {
            0 => format!(
                "{} {}{} = {};\n",
                ["let", "const", "var"][self.next(3) as usize],
                self.name(),
                self.annotation(),
                self.expression(2)
            ),
            1 | 2 => format!("{};\n", self.expression(2)),
            3 => format!(
                "function {}({}{}) {{\n{}}}\n",
                self.name(),
                self.name(),
                self.annotation(),
                self.block(inner)
            ),
            4 => format!("{{\n{}}}\n", self.block(inner)),
            5 => format!(
                "for (let {} of {}) {{\n{}}}\n",
                self.name(),
                self.name(),
                self.block(inner)
            ),
            6 => format!(
                "try {{\n{}}} catch ({}) {{\n{}}}\n",
                self.block(inner),
                self.name(),
                self.block(inner)
            ),
            7 => format!(
                "class {} {{ m({}) {{\n{}}} }}\n",
                self.name(),
                self.name(),
                self.block(inner)
            ),
            8 => format!(
                "switch ({}) {{ case 1: let {} = {}; {}; }}\n",
                self.name(),
                self.name(),
                self.name(),
                self.name()
            ),
            9 => format!(
                "const {} = function {}() {{\n{}}};\n",
                self.name(),
                self.name(),
                self.block(inner)
            ),
            10 => format!("type {} = {}[];\n", self.name(), self.name()),
            _ => format!(
                "interface {}<{}> {{ p: {}; q: {} }}\n",
                self.name(),
                self.name(),
                self.name(),
                self.name()
            ),
        }
    }
}

fn shadowing_programs(count: usize) -> Vec<(String, CorpusFile)> {
    let mut files = Vec::new();
    let mut generator = Shadowing {
        state: DEFAULT_SEED,
        typescript: false,
    };
    let mut attempts = 0;
    while files.len() < count && attempts < count * 20 {
        attempts += 1;
        generator.typescript = files.len() % 2 == 1;
        let text = format!("export {{}};\n{}", generator.block(4));
        let dialect = if generator.typescript {
            JsDialect::TypeScript
        } else {
            JsDialect::JavaScript
        };
        // Generated redeclarations are early errors; keep valid programs only.
        let source = SourceText::new(text.clone(), TextLimits::default()).unwrap();
        if parse_js_program(&source, SourceGoal::Module, dialect, ParseLimits::default()).is_err() {
            continue;
        }
        let extension = if generator.typescript { "ts" } else { "mjs" };
        files.push((
            format!("shadow{}.{extension}", files.len()),
            CorpusFile {
                goal: SourceGoal::Module,
                dialect,
                text,
            },
        ));
    }
    files
}

fn extension(file: &CorpusFile) -> &'static str {
    match file.dialect {
        JsDialect::JavaScript if file.goal == SourceGoal::Script => "cjs",
        JsDialect::JavaScript => "mjs",
        JsDialect::Jsx => "jsx",
        JsDialect::TypeScript => "ts",
        JsDialect::Tsx => "tsx",
        JsDialect::TypeScriptDeclaration => "d.ts",
    }
}

fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One row of the reference output: start, member flag, declaration
/// starts (absent without a symbol), and whether it is `arguments`.
type Row = (usize, bool, Option<Vec<usize>>, bool);

/// One file of the reference output: its rows, and the declaration-name
/// starts that its binder reports as redeclared.
struct Reference {
    rows: Vec<Row>,
    redeclared: BTreeSet<usize>,
}

fn reference(value: &str) -> Vec<Reference> {
    let limits = TextLimits {
        max_source_bytes: 1 << 30,
    };
    let text = SourceText::new(value.to_owned(), limits).unwrap();
    let doc = parse_json(
        &text,
        JsonDialect::Json,
        JsonLimits {
            max_nodes: 1 << 26,
            ..JsonLimits::default()
        },
    )
    .unwrap();
    let number = |id| -> i64 {
        doc.value(id)
            .and_then(|v| v.as_number())
            .and_then(|n| n.as_i64())
            .unwrap()
    };
    let array = |id| doc.value(id).and_then(|v| v.as_array()).unwrap();
    let mut files = Vec::new();
    for file in array(doc.root()).iter() {
        let parts = array(file);
        let mut rows = Vec::new();
        for row in array(parts.get(0).unwrap()).iter() {
            let row = array(row);
            let decls = row.get(2).unwrap();
            let decls = doc
                .value(decls)
                .unwrap()
                .as_array()
                .map(|a| a.iter().map(|d| number(d) as usize).collect());
            rows.push((
                number(row.get(0).unwrap()) as usize,
                number(row.get(1).unwrap()) == 1,
                decls,
                number(row.get(3).unwrap()) == 1,
            ));
        }
        let redeclared = array(parts.get(1).unwrap())
            .iter()
            .map(|d| number(d) as usize)
            .collect();
        files.push(Reference { rows, redeclared });
    }
    files
}

#[test]
#[ignore = "requires an external TypeScript 6.0.3 installation"]
fn external_typescript_binding_oracle() {
    let module = std::env::var("FRAMESHIFT_TYPESCRIPT_MODULE")
        .expect("set FRAMESHIFT_TYPESCRIPT_MODULE to an external typescript package directory");
    let count = std::env::var("FRAMESHIFT_BINDING_ORACLE_FILES")
        .map_or(200, |v| v.parse().expect("file count"));
    let mut files: Vec<(String, CorpusFile)> = Vec::new();
    for (name, text) in EDGE_CASES {
        let dialect = match name.rsplit_once('.').unwrap().1 {
            "tsx" => JsDialect::Tsx,
            "js" => JsDialect::JavaScript,
            _ => JsDialect::TypeScript,
        };
        files.push((
            (*name).to_owned(),
            CorpusFile {
                goal: SourceGoal::Module,
                dialect,
                text: (*text).to_owned(),
            },
        ));
    }
    files.extend(shadowing_programs(count * 2));
    for (corpus, label) in [
        (Corpus::mixed(DEFAULT_SEED, count), "mixed"),
        (Corpus::components(DEFAULT_SEED, count), "component"),
        (Corpus::typescript(DEFAULT_SEED, count), "typescript"),
    ] {
        for (i, file) in corpus.files.into_iter().enumerate() {
            let name = format!("{label}{i}.{}", extension(&file));
            files.push((name, file));
        }
    }
    let input = format!(
        "[{}]",
        files
            .iter()
            .map(|(name, file)| format!("[{},{}]", json(name), json(&file.text)))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut child = Command::new("node")
        .args(["-e", SCRIPT, &module])
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
    let reference = reference(&String::from_utf8(output.stdout).unwrap());
    assert_eq!(reference.len(), files.len());
    let (mut compared, mut resolved, mut dynamic) = (0, 0, 0);
    let mut differences = Vec::new();
    let (mut crashed, mut excused) = (0, 0);
    for ((name, file), Reference { rows, redeclared }) in files.iter().zip(reference) {
        if rows.first().is_some_and(|r| r.0 == usize::MAX) {
            crashed += 1;
            continue;
        }
        let text = SourceText::new(file.text.clone(), TextLimits::default()).unwrap();
        let tree = parse_js_program(&text, file.goal, file.dialect, ParseLimits::default())
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let b = analyze(&tree).unwrap();
        // The reference reports UTF-16 offsets.
        let mut utf16 = vec![0; file.text.len() + 1];
        let mut units = 0;
        for (i, c) in file.text.char_indices() {
            utf16[i] = units;
            units += c.len_utf16();
        }
        utf16[file.text.len()] = units;
        let position = |n| utf16[tree.node(n).unwrap().span().start()];
        let by_start: BTreeMap<usize, &Row> = rows.iter().map(|r| (r.0, r)).collect();
        let starts = |b: &Bindings, id| -> BTreeSet<usize> {
            b.binding(id)
                .declarations
                .iter()
                .filter_map(|d| d.name)
                .map(position)
                .collect()
        };
        let mut referenced = BTreeSet::new();
        for r in b.references() {
            let start = position(r.node);
            referenced.insert(start);
            let Some(row) = by_start.get(&start) else {
                differences.push(format!(
                    "{name}@{start} {}: no reference identifier",
                    r.name
                ));
                continue;
            };
            compared += 1;
            let reference: Option<BTreeSet<usize>> =
                row.2.as_ref().map(|d| d.iter().copied().collect());
            let conflict = |set: &BTreeSet<usize>| set.iter().any(|d| redeclared.contains(d));
            let agrees = match r.target {
                Target::Dynamic => {
                    dynamic += 1;
                    true
                }
                Target::Global => reference.as_ref().is_none_or(BTreeSet::is_empty),
                Target::Binding(id) => {
                    resolved += 1;
                    let implicit = b.binding(id).declarations[0].kind == DeclarationKind::Arguments;
                    if implicit {
                        row.3 && reference.as_ref().is_none_or(BTreeSet::is_empty)
                    } else {
                        reference.as_ref() == Some(&starts(&b, id))
                    }
                }
            };
            let ours = match r.target {
                Target::Binding(id) => starts(&b, id),
                Target::Global | Target::Dynamic => BTreeSet::new(),
            };
            if !agrees && (conflict(&ours) || reference.as_ref().is_some_and(conflict)) {
                excused += 1;
            } else if !agrees {
                differences.push(format!(
                    "{name}@{start} {}: {:?} != {:?}",
                    r.name, r.target, row.2
                ));
            }
        }
        // Identifiers that the reference binds to a local declaration of this
        // file, outside declaration and member names, must be references.
        let declarations: BTreeSet<usize> = (0..b.bindings().len())
            .flat_map(|i| {
                b.bindings()[i]
                    .declarations
                    .iter()
                    .filter_map(|d| d.name)
                    .map(position)
                    .collect::<Vec<_>>()
            })
            .collect();
        for row in &rows {
            if let (false, Some(decls)) = (row.1, &row.2)
                && !decls.is_empty()
                && decls.iter().all(|d| declarations.contains(d))
                && !declarations.contains(&row.0)
                && !referenced.contains(&row.0)
                && !decls.iter().any(|d| redeclared.contains(d))
            {
                differences.push(format!("{name}@{}: missed reference to {decls:?}", row.0));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "{} differences in {compared} references:\n{}",
        differences.len(),
        differences
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(resolved > compared / 2, "{resolved} of {compared}");
    eprintln!(
        "TypeScript 6.0.3 binding oracle: {} files ({crashed} reference crashes skipped), {compared} references, {resolved} bound, {dynamic} dynamic, {excused} at reported redeclarations",
        files.len()
    );
}
