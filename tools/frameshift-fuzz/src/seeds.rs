//! Fictional seed inputs and mutation dictionaries of the fuzz targets
//! (`D-044`).
//!
//! The parser targets start from small files of the generated corpora
//! (`frameshift-corpus`) and from hand-written snippets of constructs that
//! the generators avoid: Annex B forms, hashbangs, private names, JSX
//! namespaces, and TypeScript declarations. The other targets start from
//! hand-written literals, references, and encoded edit cases. All content is
//! invented for Frameshift.

use frameshift_corpus::{generate_component_file, generate_file, generate_typescript_file};
use frameshift_engine::{JsDialect, SourceGoal};

use crate::{edits::Case, targets::Target};

/// Generated files per corpus family.
const GENERATED_PER_FAMILY: u64 = 8;

/// Target size of generated seed files; the generator exceeds it by at most
/// one top-level item.
const GENERATED_TARGET: usize = 256;

const PROGRAMS: [&str; 12] = [
    "#!/usr/bin/env node\n'use strict';\nvar total = 0;\nfor (var i = 0; i < 3; i++) { total += i; }\nmodule.exports = total;\n",
    "<!-- legacy\nwith (scope) { value = 017 + 08 + '\\101'; }\nlabel: function legacy() {}\nif (flag) function inner() {}\n--> trailing\n",
    "import base, * as all from './base.js' with { type: 'json' };\nexport { base as default, all };\nexport * as more from './more.js';\nawait Promise.resolve(import.meta.url);\n",
    "class Shape extends Base {\n  static #count = 0;\n  #id;\n  static { Shape.#count++; }\n  constructor() { super(); this.#id = Shape.#count; }\n  get id() { return this.#id; }\n  static has(value) { return #id in value; }\n  async *items() { yield* [1, 2]; }\n}\n",
    "async function load({ a, b: [c, ...d] = [], ...rest }, ...more) {\n  for await (const item of source) { await using handle = item; }\n  try { throw new Error(`fail ${a}`); } catch { return c ?? d?.[0]?.(rest); } finally { debugger; }\n}\n",
    "const view = <section className=\"card\" data-id={id} {...props}>\n  <h1>Title &amp; more &#x41;</h1>\n  {items.map((item) => <Item key={item.id} {...item} />)}\n  <></>\n  <svg:rect xlink:href=\"#a\" />\n  {/* note */}\n</section>;\n",
    "interface Box<T extends object = {}> { readonly value: T; get size(): number; [key: string]: unknown }\ntype Required2<T> = { -readonly [P in keyof T]-?: T[P] };\ntype Tail<T> = T extends [infer _, ...infer R] ? R : never;\nenum Color { Red = 1, Green = Red << 1 }\nnamespace Geometry.Inner { export const unit = 1; }\ndeclare module 'virtual' { export function make(): void; }\nabstract class Model<T> { constructor(private readonly value: T) {} abstract get size(): number; }\nconst checked = value satisfies Box<string> as unknown as number;\nconst bound = make<string>;\n",
    "const Generic = <T,>(value: T) => <span>{value as unknown as string}</span>;\nexport default function Card<P extends object>({ title }: { title: string } & P) {\n  return <Box<string> value={title!}>{title}</Box>;\n}\n",
    "export declare const version: string;\nexport interface Options { readonly name?: string }\ndeclare global { interface Window { app: unknown } }\nexport const fallback: Options;\n",
    "let a = /(?<year>\\d{4})-\\k<year>/dgu, b = `line\\u{1F600}${a}\\\nnext`, c = 0x1F_FF + 1e-3 + .5 + 10n;\nouter: for (;;) { break outer; }\nswitch (a) { case 1: default: }\n",
    "using res = getResource();\nlet x = a ? b : c, y = (d, e) => d ** e, z = !f, w = typeof g, v = void 0;\nx ||= y &&= z ??= w;\n",
    "\u{FEFF}// signature\n/* block */ var \u{E9} = '\\u00e9', \u{1D49C} = \"\\x41\";\n",
];

const EXPRESSIONS: [&str; 16] = [
    "a ? b : c",
    "(a, b) => a + b * c ** d",
    "async (x) => { for await (const y of x); }",
    "function* g() { yield 1; }",
    "class A extends B { #x = 1; static { this.y = 2; } get z() { return this.#x; } }",
    "tag`a${b}c\\unicode`",
    "/a(?<n>b)\\k<n>/dgv.exec(text)",
    "({ a, b: [c, ...d], ...e } = { [key]: 1, method() {}, get value() { return 2; } })",
    "a?.b?.[c]?.(d) ?? (e || f)",
    "new Outer(new Inner)(x)",
    "<div className=\"a\">{b} &amp; text</div>",
    "<><A.B c={d} {...e} /></>",
    "<T,>(value: T) => value",
    "value as unknown as T satisfies object",
    "f<string>(x)! + <T>value",
    "(a: number, b?: string): void => {}",
];

const REGEXPS: [&str; 16] = [
    "/a|b/g",
    "/(?<year>\\d{4})-\\k<year>/u",
    "/[\\p{Script=Greek}--[\u{3B1}-\u{3B3}]]/v",
    "/(?i:ab)c/",
    "/\\cJ\\0\\x41\\u{1F600}/u",
    "/[^]/",
    "/a{2,3}?b+/y",
    "/(?=a)(?!b)(?<=c)(?<!d)./s",
    "/\\1(a)\\8/",
    "/[\\q{abc|d}]/v",
    "/]{}/",
    "/\\p{RGI_Emoji}/v",
    "/(?<a>x)|(?<a>y)/d",
    "/[\u{1F600}-\u{1F602}]/u",
    "/\\k<n>(?<n>.)/",
    "/(?-i:a)[\\d-z]/mi",
];

const LEXICAL: [&str; 10] = [
    "'a\\x41\\u{1F600}\\\n' \"\\0\\1\\8\"",
    "0x1F_FF 1e-3 07 08 .5 1n 0b1_0 0o7 089.5",
    "`a${b}c\\u{}` } tail` }mid${",
    "/* c */ // d\n<!-- e\n--> f",
    "a\\u0062c #priv \\u{1D49C}x \u{E9}\u{200D}",
    "data-x='1' \"y\" text{ <",
    "?. ?.5 >>>= ... => **= ??= >>",
    "\u{2028}\u{FEFF}\u{A0}x\u{3000}",
    "#!shebang\n#x",
    "if instanceof yield await \\u0069f",
];

const REFERENCES: [&str; 8] = [
    "&amp;&lt;&#65;&#x41;&#X41;&notit;&lang;&rang;",
    "&#128;&#0;&#xD800;&check;&amp",
    "a=&amp=&ampx &AElig &aelig; &#1114112; &#x10FFFF;",
    "&&&;;&#;&#x;&#9999999999;",
    "&nbsp&nbsp;&NotNestedGreaterGreater;&nbsp=",
    "<p title=\"&copy x\">&copy;2026 &mdash;</p>",
    "&#x1F600;&#128512;&#x80;&#159;",
    "&Agrave;&agrave&quot;&apos;&ap;",
];

const JAVASCRIPT_TOKENS: [&str; 96] = [
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "async",
    "await",
    "of",
    "get",
    "set",
    "as",
    "from",
    "meta",
    "target",
    "using",
    "accessor",
    "declare",
    "abstract",
    "readonly",
    "type",
    "interface",
    "namespace",
    "module",
    "keyof",
    "infer",
    "satisfies",
    "asserts",
    "is",
    "override",
    "unique",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "...",
    ";",
    ",",
    "<",
    ">",
    "=>",
    "?.",
    "??",
    "**",
    "#x",
    "@",
    "`",
    "${",
    "'",
    "\"",
    "/",
    "/*",
    "*/",
    "//",
    "<!--",
    "-->",
    "\n",
    "<div>",
    "</div>",
    "<>",
    "</>",
];

const REGEXP_TOKENS: [&str; 40] = [
    "/",
    "(",
    ")",
    "(?:",
    "(?=",
    "(?!",
    "(?<=",
    "(?<!",
    "(?<n>",
    "\\k<n>",
    "(?i:",
    "(?-m:",
    "[",
    "]",
    "[^",
    "\\p{",
    "\\P{",
    "}",
    "Script=",
    "gc=",
    "L",
    "Lu",
    "RGI_Emoji",
    "&&",
    "--",
    "\\q{",
    "|",
    "{2,3}",
    "*?",
    "+",
    "\\d",
    "\\u{",
    "\\c",
    "\\0",
    "\\1",
    "u",
    "v",
    "dgimsuvy",
    "\\",
    "\u{1F600}",
];

const LEXICAL_TOKENS: [&str; 30] = [
    "'", "\"", "`", "${", "}", "\\", "\\u", "\\u{", "\\x", "0x", "0b", "0o", "0", "1_0", "e+", "n",
    ".", "#", "/*", "*/", "//", "<!--", "-->", "\n", "\r\n", ">>>=", "?.", "...", "=>", "&",
];

const REFERENCE_TOKENS: [&str; 24] = [
    "&", "#", "x", "X", ";", "=", "amp", "lt", "not", "notin", "lang", "rang", "check", "AElig",
    "nbsp", "&#", "&#x", "0", "9", "F", "128", "55296", "1114112", "\u{E9}",
];

const ASTRO_TOKENS: [&str; 40] = [
    "<",
    ">",
    "</",
    "/>",
    "<>",
    "</>",
    "{",
    "}",
    "{...",
    "=",
    "\"",
    "'",
    "&amp;",
    "&copy",
    "&#32;",
    "&nbsp;",
    "\n",
    "\t",
    "className",
    "htmlFor",
    "key",
    "ref",
    "style",
    "dangerouslySetInnerHTML",
    "__html",
    "children",
    "onClick",
    "data-x",
    "aria-label",
    "hidden",
    "title",
    "pre",
    "svg",
    "Card",
    "Menu.Item",
    "&&",
    "?",
    ":",
    ".map((x) => ",
    "/* c */",
];

const ASTRO: [&str; 8] = [
    "export default function Page({ children }) {\n  return (\n    <main className=\"page\">\n      <h1>Title &amp; more</h1>\n      {children}\n    </main>\n  );\n}\n",
    "const list = <ul>{items.map((item) => <li key={item.id} className=\"row\">{item.name}</li>)}</ul>;\n",
    "const card = <Card title=\"A\" featured {...rest}><Link href=\"/\">Home</Link></Card>;\n",
    "const head = <><title>{title}</title><meta name=\"description\" content={text} /></>;\n",
    "const raw = <div dangerouslySetInnerHTML={{ __html: html }} style={{ color: 'red' }} />;\n",
    "const text = <p>\n  Clicked {count} times\n  {/* note */}\n  {ok ? <b>yes</b> : 'no'}\n</p>;\n",
    "const form = <label htmlFor=\"name\" tabIndex={0}><input id=\"name\" disabled readOnly /></label>;\n",
    "const pre = <pre>\n  kept\n</pre>;\nconst svg = <svg viewBox=\"0 0 1 1\" />;\n",
];

const EDIT_TOKENS: [&str; 6] = ["\u{E9}", "\u{1F600}", "\0", "\u{7F}", "\u{FF}", "abc"];

/// Returns the tokens that the mutator inserts for `target`.
pub fn dictionary(target: Target) -> &'static [&'static str] {
    match target {
        Target::Program | Target::Expression => &JAVASCRIPT_TOKENS,
        Target::RegExp => &REGEXP_TOKENS,
        Target::Lexical => &LEXICAL_TOKENS,
        Target::References => &REFERENCE_TOKENS,
        Target::Edits => &EDIT_TOKENS,
        Target::Astro => &ASTRO_TOKENS,
        Target::Collections => &["/", "//", "\0", "\u{7f}", "\u{80}", "a/b/a", "é"],
        Target::Hashes => &[
            "sha256:",
            "SHA256:",
            "sha512:",
            "0123456789abcdef",
            "ABCDEF",
            "\0",
            "é",
            "\n",
        ],
        Target::Identifiers => &[
            "FS",
            "0001",
            "-to-",
            "/",
            ".",
            "-",
            "#",
            "%",
            "%23",
            "\0",
            "é",
            "\u{1b}",
            "route",
            "component",
            "nextjs",
            "astro-preact",
            "api",
            "link",
            "[...slug]",
        ],
        Target::Paths => &[
            "/",
            "\\",
            ".",
            "..",
            "C:",
            "\0",
            "\u{1b}",
            "é",
            "e\u{301}",
            "🦀",
            "CON",
            "AUX .txt",
            "COM¹",
            "lpt9.ts",
            "CONIN$",
            "CONOUT$",
            ":",
            "*",
            "?",
            " ",
            "a",
            "[...slug]",
        ],
    }
}

/// Returns the built-in seed inputs of `target`, none longer than its
/// default maximum length.
pub fn inputs(target: Target) -> Vec<Vec<u8>> {
    let texts: Vec<String> = match target {
        Target::Program => generated()
            .into_iter()
            .chain(PROGRAMS.iter().map(|text| (*text).to_owned()))
            .collect(),
        Target::Expression => EXPRESSIONS.iter().map(|text| (*text).to_owned()).collect(),
        Target::RegExp => REGEXPS.iter().map(|text| (*text).to_owned()).collect(),
        Target::Lexical => LEXICAL.iter().map(|text| (*text).to_owned()).collect(),
        Target::References => REFERENCES.iter().map(|text| (*text).to_owned()).collect(),
        Target::Edits => return edit_cases(),
        Target::Collections => [
            "",
            "\0",
            "\u{3}cba",
            "\u{5}abca",
            " src/a.ts/src/A.ts/é.ts/e\u{301}.ts",
            " a/b/a",
            "\u{1f}\0\u{1}\u{2}\0\u{1f}",
            "\u{7f}\u{7f}\u{7f}\u{1}\u{2}",
            "é\0\0\u{1}",
        ]
        .iter()
        .map(|text| (*text).to_owned())
        .collect(),
        Target::Hashes => [
            "",
            "abc",
            "abc\0",
            "\u{feff}abc\r\n",
            "sha256:é",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "SHA256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ]
        .iter()
        .map(|text| (*text).to_owned())
        .collect(),
        Target::Identifiers => [
            "FS0000",
            "FS9001",
            "nextjs.api.link",
            "react.hook.use-state",
            "nextjs-to-astro-preact/route.static-page",
            "a-to-b/x.y",
            "/",
            "/blog/[slug]",
            "/docs/[[...slug]]/",
            "nextjs.images",
            "src/a#b.tsx\0a#b%",
            "é/e\u{301}/🦀.tsx",
            "src/\u{1b}[31m",
            "/a//b",
            "a%23b",
            "a\0b\0c",
        ]
        .iter()
        .map(|text| (*text).to_owned())
        .collect(),
        Target::Paths => [
            "",
            "src/pages/[...slug].astro",
            "CON.ts",
            "src/COM².ts",
            "dir/ads:stream",
            "trailing.",
            "dir/../file",
            "C:file",
            "\\\\?\\C:\\file",
            "é/e\u{301}/🦀.tsx",
            "src/\u{1b}[31m",
            "a//b",
            "a\\b",
            "a\0b",
        ]
        .iter()
        .map(|text| (*text).to_owned())
        .chain([
            "a".repeat(255),
            "a".repeat(256),
            std::iter::repeat_n("a", 257).collect::<Vec<_>>().join("/"),
        ])
        .collect(),
        Target::Astro => (0..GENERATED_PER_FAMILY)
            .map(|seed| generate_component_file(seed, GENERATED_TARGET).text)
            .chain(ASTRO.iter().map(|text| (*text).to_owned()))
            .collect(),
    };
    texts
        .into_iter()
        .map(String::into_bytes)
        .filter(|input| input.len() <= target.max_len())
        .collect()
}

fn generated() -> Vec<String> {
    let mut texts = Vec::new();
    for seed in 0..GENERATED_PER_FAMILY {
        texts.push(generate_file(seed, SourceGoal::Script, GENERATED_TARGET).text);
        texts.push(generate_file(seed, SourceGoal::Module, GENERATED_TARGET).text);
        texts.push(generate_component_file(seed, GENERATED_TARGET).text);
        for dialect in [
            JsDialect::TypeScript,
            JsDialect::Tsx,
            JsDialect::TypeScriptDeclaration,
        ] {
            texts.push(generate_typescript_file(seed, dialect, GENERATED_TARGET).text);
        }
    }
    texts
}

fn edit_cases() -> Vec<Vec<u8>> {
    let source = "const label = `caf\u{E9}`; // note\n";
    [
        (&[(6, 5, 0, 0, "title")][..], None),
        (&[(0, 0, 0, 0, "/* a */ "), (33, 0, 1, 0, "\n")], None),
        (
            &[(6, 5, 0, 0, ""), (6, 0, 0, 1, "x"), (11, 0, 0, 2, "y")],
            None,
        ),
        (&[(6, 5, 0, 0, "a"), (8, 1, 0, 1, "b")], None),
        (&[(1, 0, 2, 2, "a"), (3, 0, 2, 2, "b")], None),
        (&[(20, 1, 0, 0, "")], None),
        (&[(35, 0, 0, 0, "x")], None),
        (&[(0, 5, 0, 0, "let"), (24, 7, 1, 3, "")], Some((1, 40))),
        (&[(0, 1, 0, 0, "longer")], Some((4, 2))),
    ]
    .into_iter()
    .map(|(edits, limits)| Case::encode(source, edits, limits))
    .collect()
}
