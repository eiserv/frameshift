//! Reference-dispatched raw CSS special functions.

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
            "type",
            "TYPE",
            "t\\79pe",
            "-vendor-type",
            "_type",
            "ns.type",
            "#{'type'}",
            "calc",
            "CALC",
            "-webkit-calc",
            "-moz-calc",
            "-fictional-calc",
            "-v-c\\61lc",
            "--calc",
            "_calc",
            "ns.element",
            "element",
            "ELEMENT",
            "el\\65ment",
            "-vendor-element",
            "expression",
            "-vendor-expression",
            "-v-expr\\65ssion",
            "progid",
            "progid:DXImageTransform.Microsoft.Alpha",
            "PROGID:Alpha",
            "-vendor-progid:Alpha",
            "pr\\6fgid:Alpha",
            "progid:",
            "progid:A1",
            "progid:A-B",
            "progid:A_B",
            "progid:雪",
            "progid:A B",
            "progid:A\\42",
        ] {
            for body in [
                "",
                " ",
                "1 + 2",
                "$x + 2",
                "a:b",
                "a=b",
                "a b",
                "a;b",
                "(a;b)",
                "[a;b]",
                "{a;b}",
                "[a)",
                "a]",
                "#{1}",
                "#{1 +}",
                "'a;b'",
                "'a#{1}'",
                "/* #{ */",
                "/* once */ #{1 /* inside */}",
                "a// silent\nb",
                "a\nb",
                "(a\nb)",
                "url([a)",
                "url(a;b)",
                "URL([a)",
                "\\110000",
                "\\)",
                "a\\\nb",
                "f({[a]})",
                "f({[a})",
                "a; const",
                "a\r\nb",
            ] {
                cases.push((declaration(&format!("{name}({body})"), syntax), syntax));
            }
        }
    }
    cases
}

#[test]
fn raw_special_functions_follow_decoded_unvendored_name_dispatch() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (expression, special) in [
            ("type(a:b)", true),
            ("TYPE(a:b)", true),
            ("t\\79pe(a:b)", true),
            ("-vendor-type(1)", false),
            ("_type(1)", false),
            ("ns.type(1)", false),
            ("#{'type'}(1)", false),
            ("calc(1 + 2)", false),
            ("--calc(1)", false),
            ("-fictional-calc(a:b)", true),
            ("-v-c\\61lc(a:b)", true),
            ("expression(a:b)", true),
            ("-vendor-expression(a:b)", true),
            ("element(a:b)", true),
            ("el\\65ment(a:b)", true),
            ("-vendor-element(a:b)", true),
            ("ns.element(1)", false),
            ("progid(1)", false),
            ("progid:DXImageTransform.Microsoft.Alpha(a=b)", true),
            ("-vendor-progid:Alpha(a=b)", true),
            ("progid:(a=b)", true),
        ] {
            let src = source(&declaration(expression, syntax));
            let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let statement = doc.node(doc.statements()[0]).unwrap();
            let value = doc.node(*statement.children.last().unwrap()).unwrap();
            assert_eq!(
                value.kind,
                if special {
                    Kind::SpecialFunction
                } else {
                    Kind::Call
                },
                "{expression}"
            );
            assert_eq!(src.slice(value.span).unwrap(), expression);
            assert_eq!(
                doc.unchecked_syntax()
                    .any(|e| e.kind == SassUncheckedKind::SpecialFunction),
                special
            );
            if special {
                assert_eq!(value.children.len(), 2);
                let name = doc.node(value.children[0]).unwrap();
                let contents = doc.node(value.children[1]).unwrap();
                assert_eq!(name.kind, Kind::Identifier);
                assert_eq!(contents.kind, Kind::InterpolatedText);
                assert_eq!(
                    src.slice(name.span).unwrap(),
                    expression.split_once('(').unwrap().0
                );
                assert!(src.slice(contents.span).unwrap().starts_with('('));
                assert!(src.slice(contents.span).unwrap().ends_with(')'));
            }
        }
    }
}

#[test]
fn special_function_contents_use_raw_declaration_boundaries_and_comments() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for body in [
            "",
            "(a;b)",
            "url([a)",
            "a:b",
            "/* #{ */ #{1 /* inside */}",
            "'a#{1}'",
        ] {
            accepts(&declaration(&format!("type({body})"), syntax), syntax);
        }
        for expression in [
            "type(a;b)",
            "type([a))",
            "type(a])",
            "type(#{1 +})",
            "type(\\110000)",
            "progid:A1(a)",
            "progid:雪(a)",
        ] {
            let src = source(&declaration(expression, syntax));
            assert!(
                parse_sass(&src, syntax, SassLimits::default()).is_err(),
                "{expression}"
            );
        }
        let src = source(&declaration("type(/* #{ */ #{1 /* inside */})", syntax));
        let doc = parse_sass(&src, syntax, SassLimits::default()).unwrap();
        check(&doc);
        assert_eq!(
            doc.comments()
                .iter()
                .map(|c| src.slice(c.span).unwrap())
                .collect::<Vec<_>>(),
            ["/* #{ */", "/* inside */"]
        );
        assert_eq!(
            doc.nodes()
                .iter()
                .filter(|n| n.kind == Kind::Interpolation)
                .count(),
            1
        );
    }
    accepts("$x: type(a\nb);", SassSyntax::Scss);
    assert!(
        parse_sass(
            &source("$x: type(a\nb)\n"),
            SassSyntax::Indented,
            SassLimits::default()
        )
        .is_err()
    );
    accepts("$x: type((a\nb))\n", SassSyntax::Indented);
}

#[test]
fn generated_special_functions_preserve_tree_errors_and_limits() {
    for (text, syntax) in cases() {
        let src = source(&text);
        let result = parse_sass(&src, syntax, SassLimits::default());
        assert_eq!(parse_sass(&src, syntax, SassLimits::default()), result);
        match result {
            Ok(doc) => check(&doc),
            Err(error) => src.validate_offset(error.offset).unwrap(),
        }
    }
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let src = source(&declaration("type((#{1 /* once */}))", syntax));
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
                Err(e) => {
                    assert_eq!(e.kind, SassErrorKind::WorkLimit);
                    lower = mid + 1;
                }
            }
        }
        assert!(lower >= src.len());
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

#[test]
#[ignore = "requires external pinned sass-parser 0.4.58 and Sass 1.105.2"]
fn external_special_function_classification_oracle() {
    url_cases::check_expression_classification(cases(), "special-function");
}

#[test]
fn outer_special_function_parentheses_consume_and_release_depth() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for name in [
            "type",
            "expression",
            "element",
            "-vendor-calc",
            "-vendor-expression",
            "progid:Alpha",
        ] {
            for nested in 0..=4 {
                let expression = format!("{name}({}{})", "(".repeat(nested), ")".repeat(nested));
                let src = source(&declaration(&expression, syntax));
                // Root statements, expression, binary operand, outer function,
                // and then each additional raw group use the shared depth.
                let required = 4 + nested;
                let limits = SassLimits {
                    max_depth: required,
                    ..SassLimits::default()
                };
                assert!(
                    parse_sass(&src, syntax, limits).is_ok(),
                    "{syntax:?}: {expression}"
                );
                assert_eq!(
                    parse_sass(
                        &src,
                        syntax,
                        SassLimits {
                            max_depth: required - 1,
                            ..limits
                        }
                    )
                    .unwrap_err()
                    .kind,
                    SassErrorKind::DepthLimit,
                    "{syntax:?}: {expression}"
                );
            }
        }
        let text = [
            "type()",
            "expression()",
            "-vendor-calc()",
            "progid:Alpha()",
            "f()",
        ]
        .map(|e| declaration(e, syntax))
        .join("");
        let src = source(&text);
        let doc = parse_sass(
            &src,
            syntax,
            SassLimits {
                max_depth: 4,
                ..SassLimits::default()
            },
        )
        .unwrap();
        check(&doc);
        assert_eq!(doc.statements().len(), 5);
    }
}
