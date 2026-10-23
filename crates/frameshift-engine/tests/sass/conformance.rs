//! Remaining Sass lexical and statement grammar regressions (`D-090`).

use super::*;

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = Vec::new();
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for value in [
            "var(--tone,)",
            "VAR(--tone, /* fallback */)",
            "v\\61 r(--tone,)",
            "var($tone: red,)",
            "var($tones...,)",
            "m.var(--tone,)",
            "v#{ar}(--tone,)",
            "/assets",
            "1 / / /assets",
            "(a, / 2)",
            "shade %",
            "f(shade %)",
        ] {
            cases.push((
                format!(
                    "$value: {value}{}",
                    if syntax == SassSyntax::Scss {
                        ";"
                    } else {
                        "\n"
                    }
                ),
                syntax,
            ));
        }
    }
    for text in [
        "@charset\n 'UTF-8';",
        ".panel { --#{tone}: 1 + 2; -#{name}-tone: blue; }",
        ".panel { --tokens: // (\n ); }",
        ".panel { tone // comment\n : blue; }",
        ".panel /*#{broken*/ { tone: blue; }",
    ] {
        cases.push((text.to_owned(), SassSyntax::Scss));
    }
    for text in [
        "@debug\n  shade\n",
        "@warn\n  shade\n",
        "@error\n  shade\n",
        "@if\n  true\n",
        "@while\n  false\n",
        "@for\n  $n from 1 through 2\n",
        "@each\n  $tone in red,\n  blue\n",
        "@mixin\n  panel\n",
        "@include\n  panel\n",
        "@function panel()\n  @return\n    shade\n",
        ".panel\n  tone: red, blue,\n",
    ] {
        cases.push((text.to_owned(), SassSyntax::Indented));
    }
    cases
}

#[test]
fn remaining_reference_accepted_forms_parse_in_both_syntaxes() {
    for (text, syntax) in cases() {
        accepts(&text, syntax);
    }
}

#[test]
fn var_empty_fallback_is_a_source_located_empty_string_argument() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for name in ["var", "VAR", "v\\61 r"] {
            let text = format!(
                "$色: {name}(--tone, /* fallback */){}",
                if syntax == SassSyntax::Scss {
                    ";"
                } else {
                    "\n"
                }
            );
            let src = source(&text);
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let args = doc
                .nodes()
                .iter()
                .find(|node| node.kind == SassNodeKind::Arguments)
                .unwrap();
            assert_eq!(args.children.len(), 2, "{text}");
            let arg = doc.node(args.children[1]).unwrap();
            assert_eq!(arg.kind, SassNodeKind::Argument);
            assert_eq!(arg.span.start(), text.find(')').unwrap());
            assert_eq!(arg.span.start(), arg.span.end());
            let value = doc.node(arg.children[0]).unwrap();
            assert_eq!(value.kind, SassNodeKind::String { quoted: false });
            assert_eq!(value.span, arg.span);
            assert!(value.children.is_empty());
        }
    }
}

pub(super) fn generated_cases() -> Vec<(String, SassSyntax)> {
    let atoms = [
        "a",
        "and",
        "or",
        "not",
        "true",
        "null",
        "1",
        "-1",
        "+1",
        ".5",
        "+.5",
        "1e+2",
        "1px",
        "1px-2px",
        "1--unit",
        "\"a\"",
        "'x'",
        "\"x#{1}\"",
        "\\61",
        "a\\0",
        "#{a}",
        "-#{a}",
        "foo#{a}",
        "--#{a}",
        "[a, b]",
        "(a: b)",
        "()",
        "&",
        "%",
        "/a",
        "!important",
        "#abc",
        "#12",
        "#abcdefg",
        "a.b()",
        "a.$b",
        "a.$-b",
        "url(a//b)",
        "var(--a,)",
        "if(true,a,b)",
        "if(else:a)",
        "calc(1 + 2)",
        "type(foo;bar)",
        "1e",
        "--",
        "\\",
        "\"\\0\"",
    ];
    let mut expressions: BTreeSet<String> = atoms.iter().map(|s| (*s).to_owned()).collect();
    for a in atoms {
        for b in atoms {
            for operator in [" ", ",", "+", " - ", "/", "%", "==", " and "] {
                expressions.insert(format!("{a}{operator}{b}"));
            }
        }
    }
    for name in [
        "f", "var", "VAR", "v\\61 r", "m.var", "v#{ar}", "url", "type", "-x-calc",
    ] {
        for a in atoms {
            for tail in ["", ",", ", a", ", $b: a", "...", "..., a", ", $b...", ",,"] {
                expressions.insert(format!("{name}({a}{tail})"));
            }
        }
    }
    let mut cases = Vec::new();
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let end = if syntax == SassSyntax::Scss {
            ";"
        } else {
            "\n"
        };
        for expression in &expressions {
            cases.push((format!("$value: {expression}{end}"), syntax));
        }
        for directive in [
            "@if",
            "@for",
            "$v:",
            "@each",
            "@mixin",
            "@include",
            "@debug",
            "@use",
            "@media",
            "@supports",
            "@unknown",
            "@-moz-document",
        ] {
            for ws in ["", " ", "\n", "/*x*/", "//x\n", "\r\n  "] {
                for body in [
                    "a",
                    "true",
                    "a()",
                    "1",
                    "(a: b)",
                    "\"x\"",
                    "$a in b",
                    "$a from 1 to 2",
                    "#{a}",
                ] {
                    cases.push((
                        format!(
                            "{directive}{ws}{body}{}",
                            if syntax == SassSyntax::Scss {
                                " {}"
                            } else {
                                "\n"
                            }
                        ),
                        syntax,
                    ));
                }
            }
        }
        for name in [
            "tone",
            "--tone",
            "--#{tone}",
            "#{tone}",
            "-#{tone}",
            "tone/*x*/",
            "*tone",
            ":tone",
            ".tone",
            "#tone",
        ] {
            for ws in ["", " ", "\n", "/*x*/", "//x\n"] {
                for value in [
                    "a",
                    "a()",
                    "%",
                    "/asset",
                    "type(!#{1})",
                    "{}",
                    "[",
                    "(a)",
                    "url(http://a)",
                    "a !important",
                    "",
                    "// ()",
                    "#{1}",
                ] {
                    for nested in [false, true] {
                        let declaration = format!("{name}{ws}:{ws}{value}");
                        let text = if syntax == SassSyntax::Scss {
                            format!(
                                ".panel {{ {declaration}{} }}",
                                if nested { " { next: a; }" } else { ";" }
                            )
                        } else {
                            format!(
                                ".panel\n  {declaration}{}\n",
                                if nested { "\n    next: a" } else { "" }
                            )
                        };
                        cases.push((text, syntax));
                    }
                }
            }
        }
        for name in [
            "and",
            "AND",
            "url",
            "URL",
            "_url",
            "expression",
            "Expression",
            "element",
            "-x-element",
            "_x_element",
            "type",
            "TYPE",
            "not",
            "--tone",
            "\\2d\\2d tone",
            "if",
            "If",
        ] {
            for directive in ["mixin", "function", "include"] {
                for args in [
                    "",
                    "()",
                    "($a)",
                    "($a: red)",
                    "($a..., $b)",
                    "($a..., $b...)",
                ] {
                    cases.push((
                        format!(
                            "@{directive} {name}{args}{}",
                            if syntax == SassSyntax::Scss {
                                " {}"
                            } else {
                                "\n"
                            }
                        ),
                        syntax,
                    ));
                }
            }
        }
        for header in [
            "@-moz-document url(a)",
            "@-moz-document URL(a)",
            "@-moz-document regexp(\"x\")",
            "@-moz-document domain( a )",
            "@-moz-document url(\"a\" )",
            "@-moz-document url(a),domain(b)",
            "@-moz-document #{a}",
            "@unknown foo(!)",
            "@unknown url(//x)",
            "@unknown \\75rl(//x)",
            "@unknown foo#{a}",
        ] {
            for tail in ["", ";", " {}", "\n .panel\n  tone: blue\n"] {
                cases.push((format!("{header}{tail}"), syntax));
            }
        }
    }
    cases
}

#[test]
fn generated_conformance_cases_preserve_arenas_errors_and_determinism() {
    for (text, syntax) in generated_cases() {
        let src = source(&text);
        let result = parse_sass(&src, syntax, SassLimits::default());
        match &result {
            Ok(doc) => check(doc),
            Err(error) => src.validate_offset(error.offset).unwrap(),
        }
        assert_eq!(parse_sass(&src, syntax, SassLimits::default()), result);
    }
}

#[test]
#[ignore = "requires external pinned sass-spec and Sass 1.105.2"]
fn external_sass_conformance() {
    use std::{
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
    };
    let root = PathBuf::from(
        std::env::var_os("FRAMESHIFT_SASS_SPEC").expect("external sass-spec checkout"),
    );
    let revision = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(revision.status.success());
    assert_eq!(
        String::from_utf8(revision.stdout).unwrap().trim(),
        "58295762b6e8d9d2c22e6137bf9f0a70bb718c49",
        "unpinned sass-spec"
    );
    let mut paths = vec![root.join("spec")];
    let mut files = Vec::new();
    while let Some(path) = paths.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                paths.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    let mut cases = Vec::new();
    let mut non_utf8 = 0;
    for file in files {
        let extension = file.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(extension, "hrx" | "scss" | "sass") {
            continue;
        }
        let bytes = std::fs::read(&file).unwrap();
        let sections = if extension == "hrx" {
            hrx_sources(&bytes)
        } else {
            vec![(file.to_str().unwrap().to_owned(), bytes.as_slice())]
        };
        for (name, bytes) in sections {
            if !name.ends_with(".scss") && !name.ends_with(".sass") {
                continue;
            }
            let Ok(text) = std::str::from_utf8(bytes) else {
                non_utf8 += 1;
                continue;
            };
            let syntax = if name.ends_with(".sass") {
                SassSyntax::Indented
            } else {
                SassSyntax::Scss
            };
            cases.push((
                format!("{}:{name}", file.strip_prefix(&root).unwrap().display()),
                text.to_owned(),
                syntax,
            ));
        }
    }
    assert_eq!((cases.len(), non_utf8), (15_740, 10));
    let external_count = cases.len();
    cases.extend(
        generated_cases()
            .into_iter()
            .chain(self::cases())
            .chain(super::operator_cases::cases())
            .enumerate()
            .map(|(i, (text, syntax))| (format!("fictional-{i}"), text, syntax)),
    );
    let module =
        std::env::var("FRAMESHIFT_SASS_PARSER_MODULE").expect("external sass-parser package path");
    // The upstream JS API forces parseSelectors=true. Change only that wrapper
    // option in memory to exercise Sass's normal parse-time grammar, not evaluation.
    let script = r#"
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module');
if (require(process.argv[1] + '/package.json').version !== '0.4.58') throw Error('unpinned parser');
const sassFile = require.resolve('sass', {paths: [process.argv[1]]});
const directory = path.dirname(sassFile);
if (require(path.join(directory, 'package.json')).version !== '1.105.2') throw Error('unpinned Sass');
const filename = path.join(directory, 'sass.dart.js');
const original = fs.readFileSync(filename, 'utf8');
const call = 'A.Stylesheet_Stylesheet$parse0(css, t1, true, A.NullableExtension_andThen0(path, A.path__toUri$closure()))';
if (original.split(call).length !== 2) throw Error('reference wrapper changed');
const loaded = new Module(filename, module);
loaded.filename = filename; loaded.paths = Module._nodeModulePaths(directory);
loaded._compile(original.replace(call, call.replace(', true,', ', false,')), filename);
const sass = globalThis._cliPkgExports.pop();
if (globalThis._cliPkgExports.length === 0) delete globalThis._cliPkgExports;
sass.load({util: require('node:util'), stream: require('node:stream'), nodeModule: Module, fs, immutable: require(require.resolve('immutable', {paths: [directory]}))});
const parser = sass.loadParserExports_();
const data = fs.readFileSync(0); let at = 0; const verdicts = [];
while (at < data.length) {
  let end = data.indexOf(10, at); const syntax = data.subarray(at, end).toString(); at = end + 1;
  end = data.indexOf(10, at); const length = Number(data.subarray(at, end).toString()); at = end + 1;
  const text = data.subarray(at, at + length).toString(); at += length;
  try { parser.parse(text, syntax, null); verdicts.push('1'); }
  catch { verdicts.push('0'); }
}
process.stdout.write(verdicts.join('\n') + '\n');
"#;
    let mut child = Command::new("node")
        .args(["-e", script, &module])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for (_, text, syntax) in &cases {
        writeln!(
            input,
            "{}\n{}",
            if *syntax == SassSyntax::Scss {
                "scss"
            } else {
                "sass"
            },
            text.len()
        )
        .unwrap();
        input.write_all(text.as_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    let verdicts: Vec<_> = output.lines().collect();
    assert_eq!(verdicts.len(), cases.len());
    let mut differences = Vec::new();
    for ((id, text, syntax), verdict) in cases.iter().zip(verdicts) {
        let src = source(text);
        let result = parse_sass(&src, *syntax, SassLimits::default());
        if result.is_ok() != (verdict == "1") {
            differences.push(format!(
                "{id}: ours={:?}, reference={verdict}",
                result.as_ref().err()
            ));
        }
        if let Ok(doc) = result {
            check(&doc);
        }
    }
    eprintln!(
        "Sass normal-parser conformance: {external_count} external UTF-8 cases, {non_utf8} non-UTF-8 input excluded, {} fictional cases",
        cases.len() - external_count
    );
    assert!(
        differences.is_empty(),
        "{} disagreements:\n{}",
        differences.len(),
        differences
            .into_iter()
            .take(40)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn hrx_sources(bytes: &[u8]) -> Vec<(String, &[u8])> {
    let Some(marker_end) = bytes.iter().position(|&b| b == b'>') else {
        return Vec::new();
    };
    let marker = &bytes[..=marker_end];
    if marker.first() != Some(&b'<') || !marker[1..marker_end].iter().all(|&b| b == b'=') {
        return Vec::new();
    }
    let mut sections = Vec::new();
    let mut previous: Option<(String, usize)> = None;
    let mut offset = 0;
    for line in bytes.split_inclusive(|&b| b == b'\n') {
        if let Some(tail) = line.strip_prefix(marker) {
            if let Some((name, start)) = previous.take() {
                sections.push((
                    name,
                    &bytes[start..if offset > start && bytes[offset - 1] == b'\n' {
                        offset - 1
                    } else {
                        offset
                    }],
                ));
            }
            let name = std::str::from_utf8(tail).unwrap().trim();
            previous = Some((name.to_owned(), offset + line.len()));
        }
        offset += line.len();
    }
    if let Some((name, start)) = previous {
        sections.push((
            name,
            &bytes[start..bytes.len() - usize::from(bytes.len() > start && bytes.ends_with(b"\n"))],
        ));
    }
    sections
}

#[test]
fn ordinary_trailing_commas_do_not_create_var_fallbacks() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (call, count) in [
            ("f(--tone,)", 1),
            ("m.var(--tone,)", 1),
            ("-vendor-var(--tone,)", 1),
            ("v#{ar}(--tone,)", 1),
            ("var($tone: red,)", 1),
            ("var($tones...,)", 1),
            ("var(--tone, blue,)", 2),
            ("var()", 0),
            ("var(--tone)", 1),
        ] {
            let src = source(&format!(
                "$v: {call}{}",
                if syntax == SassSyntax::Scss {
                    ";"
                } else {
                    "\n"
                }
            ));
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let args = doc
                .nodes()
                .iter()
                .find(|n| n.kind == SassNodeKind::Arguments)
                .unwrap();
            assert_eq!(args.children.len(), count, "{call}");
        }
    }
}

#[test]
fn signed_numbers_slash_and_trailing_percent_keep_distinct_structure() {
    use frameshift_engine::syntax::{SassListSeparator, SassUnaryOperator};
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (value, expected) in [
            ("-1px", SassNodeKind::Number),
            ("+.5rem", SassNodeKind::Number),
            ("/asset", SassNodeKind::Unary(SassUnaryOperator::Divide)),
            ("1 +2", SassNodeKind::Binary(SassBinaryOperator::Add)),
            (
                "shade, /*tail*/",
                SassNodeKind::List {
                    separator: SassListSeparator::Comma,
                    bracketed: false,
                },
            ),
            (
                "shade %",
                SassNodeKind::List {
                    separator: SassListSeparator::Space,
                    bracketed: false,
                },
            ),
        ] {
            let src = source(&format!(
                "$色: {value}{}",
                if syntax == SassSyntax::Scss {
                    ";"
                } else {
                    "\n"
                }
            ));
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let statement = doc.node(doc.statements()[0]).unwrap();
            let expression = doc.node(statement.children[1]).unwrap();
            assert_eq!(expression.kind, expected, "{value}");
            assert_eq!(src.slice(expression.span).unwrap(), value);
        }
        for value in [
            "a,and",
            "a,or",
            "a,%",
            "f(-1...)",
            "f(+1...)",
            "var(--tone,,)",
        ] {
            assert!(
                parse_sass(
                    &source(&format!(
                        "$v: {value}{}",
                        if syntax == SassSyntax::Scss {
                            ";"
                        } else {
                            "\n"
                        }
                    )),
                    syntax,
                    SassLimits::default()
                )
                .is_err(),
                "{value}"
            );
        }
    }
}

#[test]
fn raw_declarations_and_headers_follow_the_normal_parser_grammar() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let text = if syntax == SassSyntax::Scss {
            ".panel { --#{tone}/*name*/: //literal {#{1}}; value: type(!{[]}); }"
        } else {
            ".panel\n  --#{tone}/*name*/: //literal {#{1}}\n  value: type(!{[]})\n"
        };
        let src = source(text);
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        let declaration = doc
            .nodes()
            .iter()
            .find(|node| node.kind == SassNodeKind::Declaration)
            .unwrap();
        let raw = doc.node(declaration.children[1]).unwrap();
        assert_eq!(raw.kind, SassNodeKind::InterpolatedText);
        assert_eq!(src.slice(raw.span).unwrap(), " //literal {#{1}}");
        assert_eq!(doc.comments().len(), 1);
        let unknown = if syntax == SassSyntax::Scss {
            "@unmodeled foo(!);"
        } else {
            "@unmodeled foo(!)\n"
        };
        accepts(unknown, syntax);
        let invalid = if syntax == SassSyntax::Scss {
            "@unmodeled !important;"
        } else {
            "@unmodeled !important\n"
        };
        assert!(parse_sass(&source(invalid), syntax, SassLimits::default()).is_err());
    }
    for text in [
        ".panel { :foo(!) { value: blue; } }",
        ".panel { :foo({}) { value: blue; } }",
    ] {
        assert!(parse_sass(&source(text), SassSyntax::Scss, SassLimits::default()).is_err());
    }
    for text in [
        ".panel\n  @extend .other\n  !optional\n",
        ".panel\n  value: a,\n    blue\n",
        "@-moz-document\n  url-prefix(a)\n",
    ] {
        assert!(parse_sass(&source(text), SassSyntax::Indented, SassLimits::default()).is_err());
    }
    accepts("[\\a", SassSyntax::Indented);
}

#[test]
fn conformance_paths_keep_inclusive_shared_resource_bounds() {
    fn boundary(
        src: &SourceText,
        syntax: SassSyntax,
        upper: usize,
        kind: SassErrorKind,
        set: fn(&mut SassLimits, usize),
    ) {
        let expected = parse_sass(src, syntax, SassLimits::default()).unwrap();
        let (mut low, mut high) = (0, upper);
        while low < high {
            let mid = low + (high - low) / 2;
            let mut limits = SassLimits::default();
            set(&mut limits, mid);
            match parse_sass(src, syntax, limits) {
                Ok(doc) => {
                    assert_eq!(doc, expected);
                    high = mid;
                }
                Err(error) => {
                    assert_eq!(error.kind, kind);
                    low = mid + 1;
                }
            }
        }
        assert!(low > 0);
        let mut limits = SassLimits::default();
        set(&mut limits, low);
        assert_eq!(parse_sass(src, syntax, limits).unwrap(), expected);
        set(&mut limits, low - 1);
        assert_eq!(parse_sass(src, syntax, limits).unwrap_err().kind, kind);
    }
    for (text, syntax) in [
        (
            ".panel { --#{tone}: // {#{1}}; value: var(--tone, /*empty*/); next: /asset; }",
            SassSyntax::Scss,
        ),
        (
            "@mixin\n  panel\n  value: var(--tone, /*empty*/)\n  --tone: //literal\n",
            SassSyntax::Indented,
        ),
        (
            "@-moz-document domain(\"example.invalid\"), regexp(\"x#{1}\") { .panel { value: -1px; } }",
            SassSyntax::Scss,
        ),
    ] {
        let src = source(text);
        let defaults = SassLimits::default();
        boundary(
            &src,
            syntax,
            defaults.max_nodes,
            SassErrorKind::NodeLimit,
            |l, v| l.max_nodes = v,
        );
        boundary(
            &src,
            syntax,
            defaults.max_depth,
            SassErrorKind::DepthLimit,
            |l, v| l.max_depth = v,
        );
        boundary(
            &src,
            syntax,
            defaults.max_work,
            SassErrorKind::WorkLimit,
            |l, v| l.max_work = v,
        );
    }
}

#[test]
fn nested_ambiguous_selectors_bound_lookahead_before_reading_children() {
    for depth in [8, 16, 32, 48] {
        let text = format!(
            ".panel {{{} tone: blue; {} }}",
            "a:b {".repeat(depth),
            "}".repeat(depth)
        );
        let src = source(&text);
        let limits = SassLimits {
            max_work: text.len() * 32,
            ..SassLimits::default()
        };
        let doc = parse_sass(&src, SassSyntax::Scss, limits).unwrap();
        check(&doc);
        assert_eq!(
            doc.nodes()
                .iter()
                .filter(|n| n.kind == SassNodeKind::StyleRule)
                .count(),
            depth + 1
        );
    }
}
