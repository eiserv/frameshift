//! Numeric unit boundaries follow the pinned Sass identifier(unit: true) reader.

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
        for number in [
            "0", "1", ".5", "2.75", "1e2", "3E-2", "-1", "+1", "-.5", "+.5",
        ] {
            for unit in [
                "px", "rem", "-unit", "--unit", "px-name", "px_1", "色", "p\\78",
            ] {
                for tail in [
                    "", "-2px", "-.5rem", "--2px", "-name", "--name", "-\\32 px", "-.", "-.name",
                ] {
                    cases.push((
                        declaration(&format!("{number}{unit}{tail}"), syntax),
                        syntax,
                    ));
                }
            }
        }
    }
    cases
}

#[test]
fn generated_unit_boundaries_keep_tree_error_and_resource_contracts() {
    for (text, syntax) in cases() {
        let src = source(&text);
        let parsed = parse_sass(&src, syntax, SassLimits::default());
        assert_eq!(parse_sass(&src, syntax, SassLimits::default()), parsed);
        match parsed {
            Ok(doc) => check(&doc),
            Err(error) => src.validate_offset(error.offset).unwrap(),
        }
    }
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let src = source(&declaration("1色-2px + 3--unit", syntax));
        let defaults = SassLimits::default();
        let doc = parse_sass(&src, syntax, defaults).unwrap();
        let nodes = doc.nodes().len() + doc.comments().len();
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_nodes: nodes,
                    ..defaults
                }
            )
            .unwrap(),
            doc
        );
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_nodes: nodes - 1,
                    ..defaults
                }
            )
            .unwrap_err()
            .kind,
            SassErrorKind::NodeLimit
        );
        let depth = (1..=defaults.max_depth)
            .find(|&depth| {
                parse_sass(
                    &src,
                    syntax,
                    SassLimits {
                        max_depth: depth,
                        ..defaults
                    },
                )
                .is_ok()
            })
            .unwrap();
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_depth: depth - 1,
                    ..defaults
                }
            )
            .unwrap_err()
            .kind,
            SassErrorKind::DepthLimit
        );
        let (mut lower, mut upper) = (0, defaults.max_work);
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
        assert!(lower >= src.len());
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_work: lower,
                    ..defaults
                }
            )
            .unwrap(),
            doc
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

#[test]
#[ignore = "requires external pinned sass-parser 0.4.58 and Sass 1.105.2"]
fn external_number_span_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let module = std::env::var("FRAMESHIFT_SASS_PARSER_MODULE")
        .expect("path to external sass-parser package");
    let script = r#"
const fs = require('node:fs');
const path = require('node:path');
const p = require(process.argv[1]);
if (require(process.argv[1] + '/package.json').version !== '0.4.58') throw Error('unpinned parser');
const sass = require.resolve('sass', {paths: [process.argv[1]]});
if (require(path.join(path.dirname(sass), 'package.json')).version !== '1.105.2') throw Error('unpinned Sass');
const data = fs.readFileSync(0); let at = 0;
while (at < data.length) {
  let end = data.indexOf(10, at); const syntax = data.subarray(at, end).toString(); at = end + 1;
  end = data.indexOf(10, at); const size = Number(data.subarray(at, end).toString()); at = end + 1;
  const text = data.subarray(at, at + size).toString(); at += size;
  let root;
  try { root = p[syntax].parse(text); }
  catch { process.stdout.write('!\n'); continue; }
  const numbers = [];
  function visit(node) {
    if (node.sassType === 'number') {
      const start = node.source.start.offset, end = node.source.end.offset;
      numbers.push(Buffer.from(text.slice(start, end)).toString('hex') + ':' + Buffer.byteLength(text.slice(0, start)) + ':' + Buffer.byteLength(text.slice(0, end)));
    }
    for (const child of node.nonStatementChildren ?? []) visit(child);
  }
  visit(root.nodes[0].expression);
  process.stdout.write(numbers.join(',') + '\n');
}
"#;
    let cases = cases();
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
    let verdicts = String::from_utf8(output.stdout).unwrap();
    let verdicts: Vec<_> = verdicts.lines().collect();
    assert_eq!(verdicts.len(), cases.len());
    let mut differences = Vec::new();
    for ((text, syntax), verdict) in cases.iter().zip(verdicts) {
        let src = source(text);
        let actual = match parse_sass(&src, *syntax, SassLimits::default()) {
            Ok(doc) => numbers(&doc)
                .into_iter()
                .map(|(text, start, end)| {
                    let hex: String = text.bytes().map(|byte| format!("{byte:02x}")).collect();
                    format!("{hex}:{start}:{end}")
                })
                .collect::<Vec<_>>()
                .join(","),
            Err(_) => "!".to_owned(),
        };
        if actual != verdict {
            differences.push(format!(
                "{syntax:?} {text:?}: ours={actual}, oracle={verdict}"
            ));
        }
    }
    eprintln!("Sass number-span oracle: {} fictional cases", cases.len());
    assert!(
        differences.is_empty(),
        "{}",
        differences
            .into_iter()
            .take(30)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn numbers(doc: &SassDocument<'_>) -> Vec<(String, usize, usize)> {
    doc.nodes()
        .iter()
        .filter(|node| node.kind == Kind::Number)
        .map(|node| {
            (
                doc.source().slice(node.span).unwrap().to_owned(),
                node.span.start(),
                node.span.end(),
            )
        })
        .collect()
}

#[test]
fn units_end_before_literal_subtraction_but_preserve_identifier_hyphens() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (expression, expected) in [
            ("1px-2px", vec!["1px", "2px"]),
            ("1px-.5rem", vec!["1px", ".5rem"]),
            ("1px--2px", vec!["1px-", "2px"]),
            ("1px-name", vec!["1px-name"]),
            ("1px--name", vec!["1px--name"]),
            ("1-foo-2px", vec!["1-foo", "2px"]),
            ("1p\\78-2px", vec!["1p\\78", "2px"]),
            ("1px-\\32 px", vec!["1px-\\32 px"]),
            ("1色-.5色", vec!["1色", ".5色"]),
        ] {
            let src = source(&declaration(expression, syntax));
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let actual: Vec<_> = numbers(&doc).into_iter().map(|n| n.0).collect();
            assert_eq!(actual, expected, "{syntax:?}: {expression}");
            let subtracts = doc
                .nodes()
                .iter()
                .filter(|node| node.kind == Kind::Binary(SassBinaryOperator::Subtract))
                .count();
            assert_eq!(subtracts, expected.len() - 1, "{expression}");
        }
    }
}

#[test]
fn double_hyphen_prefix_is_a_separate_expression_and_spans_keep_original_coordinates() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let text = declaration("1--unit", syntax);
        let src = source(&text);
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        assert_eq!(numbers(&doc)[0].0, "1");
        let value = doc
            .node(
                *doc.node(doc.statements()[0])
                    .unwrap()
                    .children
                    .last()
                    .unwrap(),
            )
            .unwrap();
        assert!(matches!(
            value.kind,
            Kind::List {
                separator: frameshift_engine::syntax::SassListSeparator::Space,
                ..
            }
        ));
        assert_eq!(value.children.len(), 2);
        assert_eq!(
            src.slice(doc.node(value.children[1]).unwrap().span)
                .unwrap(),
            "--unit"
        );

        let src = source(&format!(
            "\u{feff}{}\r\n{}",
            declaration("1色-.5色", syntax).trim_end(),
            declaration("2rem-1px", syntax)
        ));
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        let expected: Vec<_> = ["1色", ".5色", "2rem", "1px"]
            .into_iter()
            .map(|number| {
                let start = src.as_str().find(number).unwrap();
                (number.to_owned(), start, start + number.len())
            })
            .collect();
        assert_eq!(numbers(&doc), expected);
    }
}
