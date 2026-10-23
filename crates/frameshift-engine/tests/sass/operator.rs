//! Operator classification and exact operand spans, including escaped words.
use super::*;
use frameshift_engine::syntax::SassUnaryOperator;

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = Vec::new();
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let end = if syntax == SassSyntax::Scss {
            ";"
        } else {
            "\n"
        };
        for a in [
            "1",
            "-1",
            "+1",
            "a",
            "#{1}",
            "(a)",
            "f(a)",
            "$色",
            "var(--a,)",
            "/asset",
        ] {
            for b in [
                "2",
                "-2",
                "+2",
                "b",
                "#{2}",
                "(b)",
                "f(b)",
                "$類",
                "var(--b,)",
                "/next",
            ] {
                for operator in [
                    "+", "-", "*", "/", "%", "==", "!=", "and", "aNd", "a\\6e d", "\\61nd", "And",
                    "or", "oR", "o\\72", "Or",
                ] {
                    for separator in [" ", ""] {
                        cases.push((
                            format!("$色: {a}{separator}{operator}{separator}{b}{end}"),
                            syntax,
                        ));
                    }
                }
            }
            for operator in ["not", "nOt", "NOT", "n\\6f t", "\\6e ot", "no\\74", "n_ot"] {
                cases.push((format!("$色: {operator} {a}{end}"), syntax));
                cases.push((format!("$色: a, {operator} {a}{end}"), syntax));
            }
        }
        for value in [
            "a !i\\6d portant",
            "a !\\69mportant",
            "a ! default",
            "a ! important",
            "a !iMPORTANT",
        ] {
            cases.push((format!("$色: {value}{end}"), syntax));
        }
    }
    cases
}

#[test]
fn escaped_word_operators_have_reference_kinds_and_original_spans() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (value, kind) in [
            ("n\\6f t a", SassNodeKind::Unary(SassUnaryOperator::Not)),
            ("\\6e ot a", SassNodeKind::Unary(SassUnaryOperator::Not)),
            ("a aNd b", SassNodeKind::Binary(SassBinaryOperator::And)),
            ("a a\\6e d b", SassNodeKind::Binary(SassBinaryOperator::And)),
            ("a o\\72  b", SassNodeKind::Binary(SassBinaryOperator::Or)),
            ("-#{a}", SassNodeKind::String { quoted: false }),
            (
                "a !i\\6d portant",
                SassNodeKind::List {
                    separator: frameshift_engine::syntax::SassListSeparator::Space,
                    bracketed: false,
                },
            ),
        ] {
            let text = format!(
                "\u{feff}$色: {value}{}",
                if syntax == SassSyntax::Scss {
                    ";\r\n"
                } else {
                    "\r\n"
                }
            );
            let src = source(&text);
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let statement = doc.node(doc.statements()[0]).unwrap();
            let expression = doc.node(statement.children[1]).unwrap();
            assert_eq!(expression.kind, kind, "{value}");
            assert_eq!(src.slice(expression.span).unwrap(), value);
        }
        for (value, second) in [
            ("a, +1", SassNodeKind::Unary(SassUnaryOperator::Plus)),
            ("1-#{a}", SassNodeKind::String { quoted: false }),
        ] {
            let src = source(&format!(
                "$v: {value}{}",
                if syntax == SassSyntax::Scss {
                    ";"
                } else {
                    "\n"
                }
            ));
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let statement = doc.node(doc.statements()[0]).unwrap();
            let list = doc.node(statement.children[1]).unwrap();
            assert!(matches!(list.kind, SassNodeKind::List { .. }));
            assert_eq!(doc.node(list.children[1]).unwrap().kind, second);
        }
    }
}

#[test]
fn generated_operator_cases_keep_tree_error_and_work_contracts() {
    for (text, syntax) in cases() {
        let src = source(&text);
        let result = parse_sass(&src, syntax, SassLimits::default());
        match &result {
            Ok(doc) => check(doc),
            Err(error) => src.validate_offset(error.offset).unwrap(),
        }
        assert_eq!(parse_sass(&src, syntax, SassLimits::default()), result);
    }
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let text = format!(
            "$色: n\\6f t $a a\\6e d $b o\\72 $c{}",
            if syntax == SassSyntax::Scss {
                ";"
            } else {
                "\n"
            }
        );
        let src = source(&text);
        let expected = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        let (mut low, mut high) = (0, SassLimits::default().max_work);
        while low < high {
            let mid = low + (high - low) / 2;
            match parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_work: mid,
                    ..SassLimits::default()
                },
            ) {
                Ok(doc) => {
                    assert_eq!(doc, expected);
                    high = mid;
                }
                Err(error) => {
                    assert_eq!(error.kind, SassErrorKind::WorkLimit);
                    low = mid + 1;
                }
            }
        }
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_work: low,
                    ..SassLimits::default()
                }
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            parse_sass(
                &src,
                syntax,
                SassLimits {
                    max_work: low - 1,
                    ..SassLimits::default()
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
fn external_operator_span_oracle() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let module = std::env::var("FRAMESHIFT_SASS_PARSER_MODULE").expect("external sass-parser path");
    let script = r#"
const fs = require('node:fs'), path = require('node:path'), p = require(process.argv[1]);
if (require(process.argv[1] + '/package.json').version !== '0.4.58') throw Error('unpinned parser');
if (require(path.join(path.dirname(require.resolve('sass', {paths: [process.argv[1]]})), 'package.json')).version !== '1.105.2') throw Error('unpinned Sass');
const data = fs.readFileSync(0); let at = 0;
while (at < data.length) {
 let end = data.indexOf(10, at); const syntax = data.subarray(at,end).toString(); at=end+1;
 end=data.indexOf(10,at); const length=Number(data.subarray(at,end).toString()); at=end+1;
 const text=data.subarray(at,at+length).toString(); at+=length;
 let root; try {root=p[syntax].parse(text);} catch {process.stdout.write('!\n');continue;}
 const operators=[];
 function visit(node) {
  if (node.sassType==='unary-operation'||node.sassType==='binary-operation') {
   const start=Buffer.byteLength(text.slice(0,node.source.start.offset)), end=Buffer.byteLength(text.slice(0,node.source.end.offset));
   operators.push(node.sassType+':'+node.operator+':'+start+':'+end);
  }
  for(const child of node.nonStatementChildren??[])visit(child);
 }
 visit(root.nodes[0].expression);process.stdout.write(operators.sort().join(',')+'\n');
}
"#;
    let mut cases = cases();
    cases.extend(
        super::conformance_cases::generated_cases()
            .into_iter()
            .filter(|(text, _)| text.starts_with("$value: ")),
    );
    let mut child = Command::new("node")
        .args(["-e", script, &module])
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
    let verdicts: Vec<_> = output.lines().collect();
    assert_eq!(verdicts.len(), cases.len());
    let mut differences = Vec::new();
    for ((text, syntax), expected) in cases.iter().zip(verdicts) {
        let src = source(text);
        let actual = match parse_sass(&src, *syntax, SassLimits::default()) {
            Err(_) => "!".to_owned(),
            Ok(doc) => {
                let mut operators = Vec::new();
                for node in doc.nodes() {
                    let operator = if let SassNodeKind::Unary(operator) = node.kind {
                        Some((
                            "unary-operation",
                            match operator {
                                SassUnaryOperator::Not => "not",
                                SassUnaryOperator::Plus => "+",
                                SassUnaryOperator::Minus => "-",
                                SassUnaryOperator::Divide => "/",
                            },
                        ))
                    } else if let SassNodeKind::Binary(operator) = node.kind {
                        Some((
                            "binary-operation",
                            match operator {
                                SassBinaryOperator::Or => "or",
                                SassBinaryOperator::And => "and",
                                SassBinaryOperator::Equal => "==",
                                SassBinaryOperator::NotEqual => "!=",
                                SassBinaryOperator::Less => "<",
                                SassBinaryOperator::LessEqual => "<=",
                                SassBinaryOperator::Greater => ">",
                                SassBinaryOperator::GreaterEqual => ">=",
                                SassBinaryOperator::Add => "+",
                                SassBinaryOperator::Subtract => "-",
                                SassBinaryOperator::Multiply => "*",
                                SassBinaryOperator::Divide => "/",
                                SassBinaryOperator::Modulo => "%",
                                SassBinaryOperator::SingleEqual => "=",
                            },
                        ))
                    } else {
                        None
                    };
                    if let Some((kind, operator)) = operator {
                        operators.push(format!(
                            "{kind}:{operator}:{}:{}",
                            node.span.start(),
                            node.span.end()
                        ));
                    }
                }
                operators.sort();
                operators.join(",")
            }
        };
        if actual != expected {
            differences.push(format!(
                "{syntax:?} {text:?}: ours={actual}, reference={expected}"
            ));
        }
    }
    eprintln!("Sass operator-span oracle: {} fictional cases", cases.len());
    assert!(
        differences.is_empty(),
        "{} disagreements:\n{}",
        differences.len(),
        differences
            .into_iter()
            .take(30)
            .collect::<Vec<_>>()
            .join("\n")
    );
}
