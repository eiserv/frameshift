//! Raw URL syntax versus SassScript calls, checked against pinned Dart Sass.

use super::{SassNodeKind as Kind, *};

fn declaration(value: &str, syntax: SassSyntax) -> String {
    format!(
        "$value: {value}{}",
        if syntax == SassSyntax::Scss {
            ";"
        } else {
            "\n"
        }
    )
}

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = Vec::new();
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for name in [
            "url",
            "URL",
            "uRl",
            "u\\72l",
            "\\75rl",
            "_url",
            "-url",
            "--url",
            "-vendor-url",
            "-VENDOR-URL",
            "-v-u\\72l",
            "-v_url",
            "ns.url",
            "#{'url'}",
        ] {
            for body in [
                "",
                " ",
                "path",
                "雪.svg",
                "https://example.invalid/a.svg?q=1#part",
                "//example.invalid/a",
                "/*literal*/a",
                "a/*literal*/",
                "[a",
                "a]",
                "a{b",
                "a;b",
                "a:b",
                "$path",
                "a b",
                "a(b)",
                "'a b'",
                "a 'b'",
                "a\"b",
                "#{1}",
                "a#{1}b",
                "#{1} b",
                "#{1}/* #{ */ b",
                "#{1 +}",
                "#{url([a)}",
                "a\\ b",
                "\\)",
                "\\0",
                "\\110000",
                "a\nb",
                "a\n",
                "\npath\n",
                "a\r\nb",
                "a\u{c}b",
                "a b; c",
                "a,b",
                "a,b,",
                "(a: b)",
                "[a]",
                "a}b",
                "a\\\nb",
                "/* #{ */",
                "a\u{7f}b",
                "a\0b",
            ] {
                cases.push((declaration(&format!("{name}({body})"), syntax), syntax));
            }
        }
    }
    cases
}

fn value<'a>(doc: &'a SassDocument<'_>) -> &'a frameshift_engine::syntax::SassNode {
    let declaration = doc.node(doc.statements()[0]).unwrap();
    doc.node(*declaration.children.last().unwrap()).unwrap()
}

#[test]
fn raw_urls_and_sassscript_calls_have_distinct_structure() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (expression, raw) in [
            ("url()", true),
            ("url([a)", true),
            ("url(a])", true),
            ("url(a;b)", true),
            ("url(/*literal*/)", true),
            ("url(/* #{ */)", false),
            ("url(//example.invalid/a)", true),
            ("url(a#{1}b)", true),
            ("URL([a)", true),
            ("u\\72l([a)", true),
            ("-vendor-url([a)", true),
            ("url($path)", false),
            ("url('a b')", false),
            ("url(a b)", false),
            ("url(a(b))", false),
            ("url(#{1} b)", false),
            ("ns.url(a b)", false),
            ("_url(a b)", false),
            ("#{'url'}(a b)", false),
        ] {
            let text = declaration(expression, syntax);
            let src = source(&text);
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let node = value(&doc);
            assert_eq!(
                node.kind,
                if raw {
                    Kind::String { quoted: false }
                } else {
                    Kind::Call
                },
                "{text:?}"
            );
            assert_eq!(src.slice(node.span).unwrap(), expression);
            assert!(
                !doc.unchecked_syntax()
                    .any(|entry| entry.kind == SassUncheckedKind::SpecialFunction)
            );
            if raw {
                assert!(
                    node.children
                        .iter()
                        .all(|&id| doc.node(id).unwrap().kind == Kind::Interpolation)
                );
                assert!(
                    doc.comments().is_empty(),
                    "URL comment markers are text: {text:?}"
                );
            } else {
                assert_eq!(doc.node(node.children[1]).unwrap().kind, Kind::Arguments);
            }
        }
        for expression in [
            "url(a b; c)",
            "url(a\"b)",
            "url(\\110000)",
            "url(#{1 +})",
            "_url([a)",
            "ns.url([a)",
            "url(a\\\nb)",
        ] {
            let src = source(&declaration(expression, syntax));
            assert!(
                parse_sass(&src, syntax, SassLimits::default()).is_err(),
                "{expression:?}"
            );
        }
    }
}

#[test]
fn url_fallback_restores_interpolation_comments_and_spans() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let text = format!(
            "\u{feff}{}",
            declaration("url(#{1 /* once */} b) /* after */", syntax)
        );
        let src = source(&text);
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        assert_eq!(value(&doc).kind, Kind::Call);
        assert_eq!(
            doc.nodes()
                .iter()
                .filter(|node| node.kind == Kind::Interpolation)
                .count(),
            1
        );
        assert_eq!(
            doc.comments()
                .iter()
                .map(|c| src.slice(c.span).unwrap())
                .collect::<Vec<_>>(),
            ["/* once */", "/* after */"]
        );
        let interpolation = doc
            .nodes()
            .iter()
            .find(|node| node.kind == Kind::Interpolation)
            .unwrap();
        assert_eq!(src.slice(interpolation.span).unwrap(), "#{1 /* once */}");
    }
    let src = source("$x: url(\r\n  [a#{1}\r\n); $next: 2;");
    let doc = parse_sass(&src, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert_eq!(
        src.slice(value(&doc).span).unwrap(),
        "url(\r\n  [a#{1}\r\n)"
    );
    assert_eq!(doc.statements().len(), 2);
}

#[test]
fn generated_url_cases_preserve_tree_and_error_contracts() {
    for (text, syntax) in cases() {
        let src = source(&text);
        let result = parse_sass(&src, syntax, SassLimits::default());
        assert_eq!(parse_sass(&src, syntax, SassLimits::default()), result);
        match result {
            Ok(doc) => check(&doc),
            Err(error) => src.validate_offset(error.offset).unwrap(),
        }
    }
}

#[test]
fn url_node_depth_and_speculative_work_bounds_are_enforced() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for expression in [
            "url([a#{1})",
            "url(#{1 /* discarded */} b)",
            "-vendor-url($path)",
        ] {
            let src = source(&declaration(expression, syntax));
            let defaults = SassLimits::default();
            let doc = parse_sass(&src, syntax, defaults).unwrap();
            let required = doc.nodes().len() + doc.comments().len();
            assert!(
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_nodes: required,
                        ..defaults
                    }
                )
                .is_ok()
            );
            assert_eq!(
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_nodes: required - 1,
                        ..defaults
                    }
                )
                .unwrap_err()
                .kind,
                SassErrorKind::NodeLimit
            );
            assert_eq!(
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_depth: 1,
                        ..defaults
                    }
                )
                .unwrap_err()
                .kind,
                SassErrorKind::DepthLimit
            );
            let mut lower = 0;
            let mut upper = defaults.max_work;
            while lower < upper {
                let mid = lower + (upper - lower) / 2;
                match parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_work: mid,
                        ..defaults
                    },
                ) {
                    Ok(_) => upper = mid,
                    Err(error) => {
                        assert_eq!(error.kind, SassErrorKind::WorkLimit);
                        lower = mid + 1;
                    }
                }
            }
            assert!(lower >= src.as_str().len());
            assert!(
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_work: lower,
                        ..defaults
                    }
                )
                .is_ok()
            );
            assert_eq!(
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_work: lower - 1,
                        ..defaults
                    }
                )
                .unwrap_err()
                .kind,
                SassErrorKind::WorkLimit
            );
        }
    }
}

#[test]
#[ignore = "requires external pinned sass-parser 0.4.58 and Sass 1.105.2"]
fn external_url_classification_oracle() {
    check_expression_classification(cases(), "URL");
}

pub(super) fn check_expression_classification(cases: Vec<(String, SassSyntax)>, label: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let module = std::env::var("FRAMESHIFT_SASS_PARSER_MODULE")
        .expect("path to external sass-parser package");
    let script = r#"
const fs = require('node:fs');
const p = require(process.argv[1]);
if (require(process.argv[1] + '/package.json').version !== '0.4.58') throw Error('unpinned parser');
const path = require('node:path');
const sass = require.resolve('sass', {paths:[process.argv[1]]});
if (require(path.join(path.dirname(sass), 'package.json')).version !== '1.105.2') throw Error('unpinned Sass');
const data = fs.readFileSync(0); let at = 0;
while (at < data.length) {
  let end = data.indexOf(10, at); const syntax = data.subarray(at, end).toString(); at = end + 1;
  end = data.indexOf(10, at); const size = Number(data.subarray(at, end).toString()); at = end + 1;
  const source = data.subarray(at, at + size).toString(); at += size;
  let root;
  try { root = p[syntax].parse(source); }
  catch { process.stdout.write('!\n'); continue; }
  const kind = root.nodes[0].expression.sassType;
  if (!['string', 'function-call', 'interpolated-function-call'].includes(kind)) throw Error('unknown expression: ' + kind);
  process.stdout.write(kind === 'string' ? 's\n' : 'c\n');
}
"#;
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(module)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for (text, syntax) in &cases {
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
    let answers: Vec<_> = output.lines().collect();
    assert_eq!(answers.len(), cases.len());
    for ((text, syntax), answer) in cases.iter().zip(answers) {
        let src = source(text);
        let actual = match parse_sass(&src, *syntax, SassLimits::default()) {
            Ok(doc) => {
                check(&doc);
                let kind = &value(&doc).kind;
                if matches!(
                    *kind,
                    Kind::String { quoted: false } | Kind::SpecialFunction
                ) {
                    "s"
                } else {
                    assert_eq!(*kind, Kind::Call, "{text:?}");
                    "c"
                }
            }
            Err(_) => "!",
        };
        assert_eq!(actual, answer, "{syntax:?}: {text:?}");
    }
    eprintln!(
        "{} {label} acceptance and classification cases agree",
        cases.len()
    );
}
