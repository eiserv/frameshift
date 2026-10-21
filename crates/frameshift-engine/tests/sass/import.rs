//! Import items, classification, literal paths, and retained modifier evidence.

use super::{SassNodeKind as Kind, *};

pub(super) struct Case {
    pub text: String,
    pub syntax: SassSyntax,
    css: Option<Vec<bool>>,
}

pub(super) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let separator = if syntax == SassSyntax::Scss {
            ";"
        } else {
            "\n"
        };
        for (url, css, valid) in [
            ("'fictional'", false, true),
            ("''", false, true),
            ("'#{$literal}'", false, true),
            ("'file.css'", true, true),
            ("'file.CSS'", false, true),
            ("'.css'", false, true),
            ("'file.css?q'", false, true),
            ("'//a'", false, true),
            ("'//雪'", false, true),
            ("'//ab'", false, true),
            ("'//abc'", true, true),
            ("'http://[invalid]'", true, true),
            ("'https://[invalid]'", true, true),
            ("'HTTP://[invalid]'", false, false),
            ("'1:invalid'", false, false),
            ("'HTTP://host:bad/path'", false, false),
            ("'data:invalid'", false, false),
            ("'\\\\\\\\[invalid]\\\\share'", false, false),
            ("'C:/fictional'", false, true),
            ("'C:\\5c fictional'", false, true),
            ("'\\\\\\\\fictional\\\\share'", false, true),
            ("'file\\2e css'", true, true),
            ("url(https://example.invalid/a.css)", true, true),
            ("URL(  path  )", true, true),
            ("u\\72l(path)", true, true),
            ("url()", true, true),
            ("url('#{$interpolated}')", true, true),
            ("url(#{$path})", true, true),
            ("url($path)", true, true),
            ("url($path + '/file')", true, true),
            ("url(a b)", true, true),
            ("url(a(b))", true, true),
            ("url('unterminated)", true, false),
            ("url(path", true, false),
        ] {
            for modifier in ["", " screen", " layer(base)", " supports(display: grid)"] {
                let css = css || !modifier.is_empty();
                // CSS URL and modifier forms bypass dynamic URI validation.
                let valid = valid || !modifier.is_empty() && !url.starts_with("url(");
                for context in ["root", "style", "mixin", "control", "content", "function"] {
                    let item = format!("@import {url}{modifier}{separator}");
                    let text = match (syntax, context) {
                        (_, "root") => item,
                        (SassSyntax::Scss, "style") => format!(".a {{ {item} }}"),
                        (SassSyntax::Scss, "mixin") => format!("@mixin a {{ {item} }}"),
                        (SassSyntax::Scss, "control") => format!("@if true {{ {item} }}"),
                        (SassSyntax::Scss, "content") => format!("@include a {{ {item} }}"),
                        (SassSyntax::Scss, "function") => format!("@function a() {{ {item} }}"),
                        (SassSyntax::Indented, "style") => format!(".a\n  {item}"),
                        (SassSyntax::Indented, "mixin") => format!("@mixin a\n  {item}"),
                        (SassSyntax::Indented, "control") => format!("@if true\n  {item}"),
                        (SassSyntax::Indented, "content") => format!("@include a\n  {item}"),
                        (SassSyntax::Indented, "function") => format!("@function a()\n  {item}"),
                        _ => unreachable!(),
                    };
                    let allowed =
                        context != "function" && (css || !matches!(context, "mixin" | "control"));
                    cases.push(Case {
                        text,
                        syntax,
                        css: (valid && allowed).then(|| vec![css]),
                    });
                }
            }
        }
        for (header, css) in [
            ("'a', 'b.css', url(c)", Some(vec![false, true, true])),
            ("'a' layer(base), 'b'", Some(vec![true, false])),
            ("'a' screen, print", Some(vec![true])),
            ("'a' (width > 2px), print", Some(vec![true])),
            (
                "'a' supports(display: grid) screen, print",
                Some(vec![true]),
            ),
            ("'a' #{$media}", Some(vec![true])),
            ("'a' -#{$media}", Some(vec![true])),
            ("'a' an\\64 (width: 1px), print", Some(vec![true])),
            ("'a' /* #{1 + 2} */ layer( #{$layer} )", Some(vec![true])),
            (
                "'a',",
                (syntax == SassSyntax::Indented).then(|| vec![false, false]),
            ),
            ("", (syntax == SassSyntax::Indented).then(|| vec![false])),
            ("123", (syntax == SassSyntax::Indented).then(|| vec![false])),
            ("'a' {}", None),
        ] {
            cases.push(Case {
                text: format!("@import {header}{separator}"),
                syntax,
                css,
            });
        }
    }
    for (header, css) in [
        ("fictional", vec![false]),
        ("foo, bar.css", vec![false, true]),
        ("'foo', url(bar)", vec![false, true]),
        ("foo /* literal */", vec![false]),
        ("#{$literal}", vec![false]),
        ("file.css ", vec![false]),
        ("file.css", vec![true]),
        ("url-not-a-function", vec![false]),
        ("url (literal)", vec![false]),
    ] {
        cases.push(Case {
            text: format!("@import {header}\n"),
            syntax: SassSyntax::Indented,
            css: Some(css),
        });
    }
    cases.push(Case {
        text: "@import 'a',\r\n'b'\r\nscreen;".to_owned(),
        syntax: SassSyntax::Scss,
        css: Some(vec![false, true]),
    });
    cases
}

fn imports(doc: &SassDocument<'_>) -> Vec<bool> {
    doc.nodes()
        .iter()
        .filter_map(|node| {
            if let Kind::Import { css } = node.kind {
                Some(css)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn import_items_classify_without_loading_and_enforce_context() {
    for case in cases() {
        let source = source(&case.text);
        let result = parse_sass(&source, case.syntax, SassLimits::default());
        if let Some(expected) = case.css {
            let doc = result.unwrap_or_else(|e| panic!("{:?}: {:?}: {e}", case.syntax, case.text));
            check(&doc);
            assert_eq!(
                imports(&doc),
                expected,
                "{:?}: {:?}",
                case.syntax,
                case.text
            );
        } else {
            assert!(result.is_err(), "{:?}: {:?}", case.syntax, case.text);
        }
    }
}

#[test]
fn import_spans_comments_literal_paths_and_modifier_evidence() {
    let text = "\u{feff}@import '#{$literal}', /* #{1 + 2} */ url(#{$path}) layer(base);\r\n$x: 1;";
    let src = source(text);
    let doc = parse_sass(&src, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let rule = doc.node(doc.statements()[0]).unwrap();
    assert_eq!(rule.kind, Kind::AtRule(SassAtRule::Import));
    assert_eq!(rule.children.len(), 3);
    let first = doc.node(rule.children[1]).unwrap();
    let second = doc.node(rule.children[2]).unwrap();
    assert_eq!(src.slice(first.span).unwrap(), "'#{$literal}'");
    assert_eq!(src.slice(second.span).unwrap(), "url(#{$path}) layer(base)");
    let literal = doc.node(first.children[0]).unwrap();
    assert!(literal.children.is_empty());
    let unchecked: Vec<_> = doc.unchecked_syntax().collect();
    assert_eq!(unchecked.len(), 1);
    assert_eq!(unchecked[0].kind, SassUncheckedKind::ImportPrelude);
    assert_eq!(src.slice(unchecked[0].span).unwrap(), "layer(base)");
    assert_eq!(doc.comments().len(), 1);
    assert_eq!(doc.comments()[0].interpolations.len(), 1);
    assert_eq!(doc.statements().len(), 2);
}

#[test]
fn import_url_backtracking_and_limits_preserve_bounded_arenas() {
    for (text, syntax) in [
        (
            "@import url(#{$a} '$b' + $c), 'HTTP://host/path';",
            SassSyntax::Scss,
        ),
        (
            "@import url(#{$a} '$b' + $c), 'HTTP://host/path'\n",
            SassSyntax::Indented,
        ),
    ] {
        let src = source(text);
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        for limits in [
            SassLimits {
                max_nodes: doc.nodes().len() - 1,
                ..SassLimits::default()
            },
            SassLimits {
                max_depth: 2,
                ..SassLimits::default()
            },
            SassLimits {
                max_work: text.len(),
                ..SassLimits::default()
            },
        ] {
            let error = parse_sass(&src, syntax, limits).unwrap_err();
            assert!(matches!(
                error.kind,
                SassErrorKind::NodeLimit | SassErrorKind::DepthLimit | SassErrorKind::WorkLimit
            ));
        }
    }
}

#[test]
#[ignore = "requires external pinned sass-parser 0.4.58 and Sass 1.105.2"]
fn external_import_classification_oracle() {
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
  try {
    const root = p[syntax].parse(source); let bits = '';
    root.walkAtRules(rule => {
      if (rule.imports) for (const item of rule.imports.nodes) {
        if (!['static-import', 'dynamic-import'].includes(item.sassType)) throw Error('unknown import kind');
        bits += item.sassType === 'static-import' ? 'c' : 's';
      }
    });
    process.stdout.write(bits + '\n');
  } catch (e) { process.stdout.write('!\n'); }
}
"#;
    let mut cases = cases();
    // Dynamic paths share URI syntax with modules but accept Windows absolute
    // paths first. Plain CSS paths deliberately bypass the URI parser.
    for scheme in ["", "HTTP:", "1:", "data:", "C:", "z:"] {
        for path in [
            "//[invalid]",
            "//host:bad/path",
            "//host:/path",
            "//host:0x10/path",
            "//[::1]/path",
            "/file",
            "\\file",
            "\\\\[invalid]\\share",
            "\\\\host\\share",
            "//雪",
            "//😀",
            "//éx",
            "//ab",
            "//abc",
            "file.css",
            "file.css?query",
            "#{$literal}",
        ] {
            let url = format!("{scheme}{path}").replace('\\', "\\\\");
            for modifier in ["", " screen"] {
                cases.push(Case {
                    text: format!("@import '{url}'{modifier};"),
                    syntax: SassSyntax::Scss,
                    css: None,
                });
            }
        }
    }
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .arg(module)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for case in &cases {
        writeln!(
            input,
            "{}\n{}",
            if case.syntax == SassSyntax::Scss {
                "scss"
            } else {
                "sass"
            },
            case.text.len()
        )
        .unwrap();
        input.write_all(case.text.as_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    let verdicts: Vec<_> = output.lines().collect();
    assert_eq!(verdicts.len(), cases.len());
    let mut differences = Vec::new();
    for (case, reference) in cases.iter().zip(verdicts) {
        let src = source(&case.text);
        let actual = parse_sass(&src, case.syntax, SassLimits::default())
            .map(|doc| {
                check(&doc);
                imports(&doc)
                    .into_iter()
                    .map(|css| if css { 'c' } else { 's' })
                    .collect::<String>()
            })
            .unwrap_or_else(|_| "!".to_owned());
        if actual != reference {
            differences.push(format!(
                "{:?} {:?}: ours={actual}, reference={reference}",
                case.syntax, case.text
            ));
        }
    }
    eprintln!(
        "Import classification oracle: {} fictional cases",
        cases.len()
    );
    assert!(
        differences.is_empty(),
        "{} mismatches:\n{}",
        differences.len(),
        differences
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
