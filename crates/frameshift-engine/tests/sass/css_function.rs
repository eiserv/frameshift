//! Plain CSS function dispatch and raw result declarations (`D-079`).

use super::*;

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = vec![
        (
            "@function\n  theme\n  ($x)\n  @return $x\n".to_owned(),
            SassSyntax::Indented,
        ),
        (
            "@function\n  --theme()\n  result: 1 +\n".to_owned(),
            SassSyntax::Indented,
        ),
    ];
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for directive in ["function", "FUNCTION", "f\\75 nction", "mixin"] {
            for name in [
                "--theme()",
                "--theme",
                "--",
                "__theme()",
                "\\2d\\2d theme()",
                "theme()",
                "--#{a}()",
            ] {
                for body in [
                    "result: 1 +",
                    "result: {}",
                    "result: //literal",
                    "result:",
                    "RESULT: var(--x)",
                    "r\\65 sult: #{1 + 2}",
                    "result: (a;b)",
                    "result: [a)",
                    "result: !important",
                    "color: red",
                    "@return 1",
                    "$x: 1",
                ] {
                    let rule = match syntax {
                        SassSyntax::Scss => format!("@{directive} {name} {{ {body}; }}"),
                        SassSyntax::Indented => format!("@{directive} {name}\n  {body}\n"),
                    };
                    cases.push((rule.clone(), syntax));
                    cases.push((wrap(&rule, syntax, "@mixin host"), syntax));
                    cases.push((wrap(&rule, syntax, "@if true"), syntax));
                    cases.push((wrap(&rule, syntax, "@function host()"), syntax));
                }
            }
        }
        for rule in [
            "@function --x",
            "@function --x()",
            "@function !bad",
            "@FUNCTION",
            "@mixin --x",
            "@function __x()",
            "@function \\2d\\2d x()",
        ] {
            let rule = if syntax == SassSyntax::Scss {
                format!("{rule};")
            } else {
                format!("{rule}\n")
            };
            cases.push((rule, syntax));
        }
        for property in ["result", "RESULT", "r\\65 sult", "result /**/"] {
            for value in [
                "",
                " ",
                "{}",
                "a:b",
                "1 +",
                "a//b",
                "/* #{ } */",
                "#{1 + 2}",
                "(a; b)",
                "[a)",
                "url(https://example.invalid/x)",
            ] {
                let body = format!("{property}:{value}");
                let rule = match syntax {
                    SassSyntax::Scss => format!("@function --x() {{ {body}; }}"),
                    SassSyntax::Indented => format!("@function --x()\n  {body}\n"),
                };
                cases.push((rule, syntax));
            }
        }
    }
    cases
}

fn wrap(rule: &str, syntax: SassSyntax, header: &str) -> String {
    match syntax {
        SassSyntax::Scss => format!("{header} {{ {rule} }}"),
        SassSyntax::Indented => format!(
            "{header}\n{}",
            rule.lines()
                .map(|line| format!("  {line}\n"))
                .collect::<String>()
        ),
    }
}

#[test]
fn css_functions_keep_css_evidence_and_raw_result_values() {
    let text = "@function --theme($x: <color>) { RESULT: //literal {a;b} #{1 + 2}; }";
    let source = source(text);
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let rule = doc.node(doc.statements()[0]).unwrap();
    assert_eq!(rule.kind, SassNodeKind::AtRule(SassAtRule::Css));
    let result = doc.node(rule.children[2]).unwrap();
    assert_eq!(result.kind, SassNodeKind::Declaration);
    let value = doc.node(result.children[1]).unwrap();
    assert_eq!(value.kind, SassNodeKind::InterpolatedText);
    assert_eq!(
        source.slice(value.span).unwrap(),
        " //literal {a;b} #{1 + 2}"
    );
    assert_eq!(value.children.len(), 1);
    assert!(doc.comments().is_empty());
    assert!(
        doc.unchecked_syntax()
            .any(|entry| entry.kind == SassUncheckedKind::CssDirective && entry.span == rule.span)
    );
}

#[test]
fn css_function_dispatch_uses_the_literal_prefix_and_preserves_sass_names() {
    accepts(
        "@function\n  theme\n  ($x)\n  @return $x\n",
        SassSyntax::Indented,
    );
    accepts(
        "@function\n  --theme()\n  result: 1 +\n",
        SassSyntax::Indented,
    );
    for text in [
        "@function __theme() { @return 1; }",
        "@function \\2d\\2d theme() { @return 1; }",
        "@mixin __theme {}",
        "@mixin \\2d\\2d theme {}",
    ] {
        let source = source(text);
        let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
        check(&doc);
        assert!(matches!(
            doc.node(doc.statements()[0]).unwrap().kind,
            SassNodeKind::AtRule(SassAtRule::Function | SassAtRule::Mixin)
        ));
    }
    for text in [
        "@function --theme;",
        "@function --theme() {}",
        "@FUNCTION --theme() { result: 1 +; }",
        "@mixin host { @function --theme() { result: 1; } }",
    ] {
        accepts(text, SassSyntax::Scss);
    }
    for text in [
        "@mixin --theme {}",
        "@function host() { @function --theme() {} }",
        "@function --theme() { @return 1; }",
    ] {
        assert!(
            parse_sass(&source(text), SassSyntax::Scss, SassLimits::default()).is_err(),
            "{text}"
        );
    }
}

#[test]
fn css_result_context_is_inherited_and_ends_at_the_function_boundary() {
    accepts(
        "@function --x() { @if true { result: 1 +; } @unknown { result: {}; } }",
        SassSyntax::Scss,
    );
    accepts(
        "@function --x()\n  @if true\n    result: 1 +\n  result: {}\n",
        SassSyntax::Indented,
    );
    for text in [
        "@function --x() {} .a { result: 1 +; }",
        ".a { result: 1 +; }",
    ] {
        assert!(parse_sass(&source(text), SassSyntax::Scss, SassLimits::default()).is_err());
    }
    let source = source("@function --x() { result: /* #{ } */; }");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert_eq!(doc.comments().len(), 1);
    assert!(doc.comments()[0].interpolations.is_empty());
}

#[test]
fn css_result_delimiters_and_non_result_properties_remain_distinct() {
    for text in [
        "@function --x() { result:; result: ; result::x; }",
        "@function --x() { res_ult: 1 + 2; #{result}: 1 + 2; *result: 1 + 2; }",
    ] {
        accepts(text, SassSyntax::Scss);
    }
    let text = source("@function --x() { result/**/: 1 + 2; }");
    let doc = parse_sass(&text, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let rule = doc.node(doc.statements()[0]).unwrap();
    let declaration = doc.node(rule.children[2]).unwrap();
    assert!(matches!(
        doc.node(declaration.children[1]).unwrap().kind,
        SassNodeKind::Binary(_)
    ));
    for text in [
        "@function --x() { result:{}; }",
        "@function --x() { *result:1 +; }",
        "@function --x() { #{result}: 1 +; }",
    ] {
        assert!(parse_sass(&source(text), SassSyntax::Scss, SassLimits::default()).is_err());
    }
    assert!(
        parse_sass(
            &source("@function --x()\n  result: \n"),
            SassSyntax::Indented,
            SassLimits::default()
        )
        .is_err()
    );
}

#[test]
fn generated_css_functions_keep_tree_error_and_resource_contracts() {
    for (text, syntax) in cases() {
        let source = source(&text);
        let parsed = parse_sass(&source, syntax, SassLimits::default());
        assert_eq!(parsed, parse_sass(&source, syntax, SassLimits::default()));
        match parsed {
            Ok(doc) => check(&doc),
            Err(error) => {
                source.validate_offset(error.offset).unwrap();
            }
        }
    }
    let source = source("@function --x() { result: (((#{1 + 2}))); }");
    for limits in [
        SassLimits {
            max_nodes: 3,
            ..SassLimits::default()
        },
        SassLimits {
            max_depth: 2,
            ..SassLimits::default()
        },
        SassLimits {
            max_work: 5,
            ..SassLimits::default()
        },
    ] {
        assert!(matches!(
            parse_sass(&source, SassSyntax::Scss, limits)
                .unwrap_err()
                .kind,
            SassErrorKind::NodeLimit | SassErrorKind::DepthLimit | SassErrorKind::WorkLimit
        ));
    }
}
