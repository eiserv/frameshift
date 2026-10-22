//! Modern and legacy `if()` disambiguation and condition grammar.

use super::{check, source};
use frameshift_engine::syntax::{
    SassErrorKind, SassLimits, SassNodeKind as Kind, SassSyntax, parse_sass,
};

const GROUPS: &[&str] = &[
    "sass(true)",
    "sass($x > 1)",
    "sass((a: b))",
    "sass()",
    "SASS(a:b)",
    "s\\61ss(true)",
    "style(--a: b)",
    "media(width > 1px)",
    "supports(display: grid)",
    "foo()",
    "foo(a;b:c)",
    "foo([a])",
    "foo(#{1 + 2})",
    "foo(/* #{ */ a)",
    "var(--a)",
    "VAR(--a)",
    "attr(a)",
    "if(a:b)",
    "--a()",
    "--#{$x}()",
    "#{$x}",
    "#{true}",
    "#{$x}()",
    "f#{$x}(a)",
    "and(a)",
    "not(a)",
    "(sass(true))",
    "(not sass(false))",
    "not sass(false)",
    "NOT sass(false)",
    "not(sass(false))",
    "else",
    "ELSE",
    "else()",
    "sass(1 +)",
    "1",
    "",
    "(foo())",
];

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut values = vec![
        "if()".to_owned(),
        "IF()".to_owned(),
        "if(true, a, b)".to_owned(),
        "IF(true, a, b)".to_owned(),
        "i\\66(true, a, b)".to_owned(),
        "if($condition: true, $if-true: a, $if-false: b)".to_owned(),
        "math.if(true, a, b)".to_owned(),
        "if(else:a; else:b)".to_owned(),
        "if(else:if(sass(true):a;else:b))".to_owned(),
        "if(sass(true): a,b;else:c)".to_owned(),
    ];
    for group in GROUPS {
        for tail in [":a)", ":a;)", ":a;else:b)", ":)", ":a;;)", ":a", ",a,b)"] {
            values.push(format!("if({group}{tail}"));
        }
        for second in GROUPS {
            for operator in [" and ", " or ", " "] {
                values.push(format!("if({group}{operator}{second}:a)"));
            }
        }
    }
    for condition in [
        "foo() and bar() or baz()",
        "var(--a) foo() and bar() baz()",
        "foo() var(--a) and bar() baz()",
        "foo() #{$x} bar()",
        "foo() not bar()",
        "foo() and(foo())",
        "foo()and foo()",
        "not/*c*/(foo())",
        "sass(true)\nand\nfoo()",
    ] {
        values.push(format!("if({condition}: a;else:b)"));
    }
    values
        .into_iter()
        .flat_map(|value| {
            [
                (format!(".a {{ x: {value}; y: 2; }}"), SassSyntax::Scss),
                (format!(".a\n  x: {value}\n  y: 2\n"), SassSyntax::Indented),
            ]
        })
        .collect()
}

#[test]
fn modern_if_keeps_branch_and_condition_structure() {
    let text = ".a { x: if(sass($x > 1) and (not style(--a: b)): red; else: blue;); }";
    let source = source(text);
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let expression = doc
        .nodes()
        .iter()
        .find(|n| n.kind == Kind::IfExpression)
        .unwrap();
    assert_eq!(
        source.slice(expression.span).unwrap(),
        "if(sass($x > 1) and (not style(--a: b)): red; else: blue;)"
    );
    assert_eq!(expression.children.len(), 3);
    let branches: Vec<_> = expression.children[1..]
        .iter()
        .map(|&id| doc.node(id).unwrap())
        .collect();
    assert_eq!(branches[0].kind, Kind::IfBranch { otherwise: false });
    assert_eq!(branches[1].kind, Kind::IfBranch { otherwise: true });
    assert_eq!(
        doc.node(branches[0].children[0]).unwrap().kind,
        Kind::IfConditionOperation
    );
    assert!(doc.nodes().iter().any(|n| n.kind == Kind::IfConditionSass));
    assert!(
        doc.nodes()
            .iter()
            .any(|n| n.kind == Kind::IfConditionNegation)
    );
    assert!(
        doc.nodes()
            .iter()
            .any(|n| n.kind == Kind::IfConditionParenthesized)
    );
    assert_eq!(doc.unchecked_syntax().count(), 0);
}

#[test]
fn modern_if_generated_tree_and_error_contracts() {
    for (text, syntax) in cases() {
        let source = source(&text);
        let result = parse_sass(&source, syntax, SassLimits::default());
        if let Ok(doc) = &result {
            check(doc);
        } else {
            assert_eq!(
                result,
                parse_sass(&source, syntax, SassLimits::default()),
                "{text}"
            );
        }
    }
}

#[test]
fn modern_if_is_not_a_generic_call_and_legacy_if_still_is() {
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        for (value, kind) in [
            ("if(else: a)", Kind::IfExpression),
            ("if(sass(true) and foo(/* #{ */ a):a)", Kind::IfExpression),
            ("IF(else:a)", Kind::IfExpression),
            ("if(true, a, b)", Kind::Call),
            ("i\\66(true, a, b)", Kind::Call),
            ("if(var(--a) style(--b:c):a)", Kind::IfExpression),
        ] {
            let text = if syntax == SassSyntax::Scss {
                format!("$x: {value};")
            } else {
                format!("$x: {value}\n")
            };
            let source = source(&text);
            let doc = parse_sass(&source, syntax, SassLimits::default()).unwrap();
            assert!(doc.nodes().iter().any(|n| n.kind == kind), "{text}");
            check(&doc);
        }
    }
    for value in [
        "IF(true,a,b)",
        "if(not(foo()):a)",
        "if(foo() and bar() or baz():a)",
        "if(sass():a)",
    ] {
        assert!(
            parse_sass(
                &source(&format!("$x: {value};")),
                SassSyntax::Scss,
                SassLimits::default()
            )
            .is_err(),
            "{value}"
        );
    }
}

#[test]
fn modern_if_respects_shared_limits_and_media_evidence() {
    let source = source("@media (width: if(sass(true): 1px; else: 2px)) { .a {x:1} }");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert!(
        doc.unchecked_syntax()
            .any(|e| e.kind == frameshift_engine::syntax::SassUncheckedKind::MediaQuery)
    );
    let count = doc.nodes().len() + doc.comments().len();
    assert!(
        parse_sass(
            &source,
            SassSyntax::Scss,
            SassLimits {
                max_nodes: count,
                ..SassLimits::default()
            }
        )
        .is_ok()
    );
    assert_eq!(
        parse_sass(
            &source,
            SassSyntax::Scss,
            SassLimits {
                max_nodes: count - 1,
                ..SassLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::NodeLimit
    );
    assert_eq!(
        parse_sass(
            &source,
            SassSyntax::Scss,
            SassLimits {
                max_work: 1,
                ..SassLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::WorkLimit
    );
    let text = format!(
        "$x: IF({}sass(true){}: a);",
        "(".repeat(128),
        ")".repeat(128)
    );
    assert_eq!(
        parse_sass(
            &super::source(&text),
            SassSyntax::Scss,
            SassLimits::default()
        )
        .unwrap_err()
        .kind,
        SassErrorKind::DepthLimit
    );
}
