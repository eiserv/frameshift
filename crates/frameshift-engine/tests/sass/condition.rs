//! Media queries, supports conditions, and import modifiers (`D-083`).

use frameshift_engine::syntax::{
    SassAtRule, SassConditionOperator, SassLimits, SassMediaRelation, SassNodeKind as Kind,
    SassSyntax, SassUncheckedKind, parse_sass,
};

use super::{check, source};

pub(super) const MEDIA_QUERIES: &[&str] = &[
    "screen",
    "not screen",
    "only screen",
    "not only screen",
    "screen and (a)",
    "screen and not (a)",
    "not (a)",
    "(a) and (b)",
    "(a) or (b)",
    "(a) and (b) or (c)",
    "(a) and (b) and (c)",
    "(a)and (b)",
    "(a) and(b)",
    "screen and (a) and (b)",
    "only screen and (a) or (b)",
    "screen and (a) or (b)",
    "#{$q}",
    "screen and #{$q}",
    "(a) and #{$q}",
    "not #{$q}",
    "#{$t} and (a)",
    "scr#{e}en",
    "--x",
    "SCREEN AND (A)",
    "screen AnD (a)",
    "Not (a)",
    "screen, print",
    "screen,print",
    "screen ,",
    ",screen",
    "",
    "screen and",
    "screen (a)",
    "not",
    "only",
    "(a",
    "a)",
    "(a))",
    "((a))",
    "((a) and (b))",
    "((a) or (b) and (c))",
    "((a) and (b) and (c))",
    "(not (a))",
    "(not(a))",
    "(not)",
    "(not-a)",
    "(not a)",
    "(a)",
    "(min-width: 100px)",
    "(width >= 600px)",
    "(width>600px)",
    "(400px <= width <= 700px)",
    "(400px < width > 700px)",
    "(a < b <= c)",
    "(a <= b < c)",
    "(a > b >= c)",
    "(a < = b)",
    "(a <b)",
    "(a = b)",
    "(a == b)",
    "(a = b = c)",
    "(a: 1 < 2)",
    "(a < b : c)",
    "(a: )",
    "(:a)",
    "()",
    "( a : b )",
    "(a: b, c)",
    "(a b: c)",
    "(a: $b)",
    "(a: f(1))",
    "(a: \"b\")",
    "(a: 'b)c')",
    "(a: null)",
    "(a: (b))",
    "(a: (b: c))",
    "(a: [b])",
    "(a: 1px + 2px)",
    "(a: 16/9)",
    "(a: #fff)",
    "(a: true)",
    "(a: not b)",
    "(a: b !important)",
    "(-webkit-min-device-pixel-ratio: 2)",
    "(a: -1)",
    "(a: calc(1px + 2px))",
    "(a: url(x))",
    "(a: &)",
    "(a: b; c)",
    "(a: {b})",
    "(a /* c */ : b)",
    "(a: b) /* c */ and (c)",
    "(a: #{$b})",
    "(#{$a}: b)",
    "(a: \\28 )",
    "(1px < 2px)",
    "(a: b == c)",
    "(a: b) and (c: d), print and (e)",
    "not screen and (a)",
    "screen and not (a) and (b)",
    "only screen and",
    "(a) and not (b)",
    "(a) or not (b)",
    "screen and (a), #{$q}",
    "screen/**/and (a)",
    "screen and/**/(a)",
    "(a: b)(c: d)",
    "(a + b < c)",
    "(a: (1 < 2))",
    "(f(a < b) < c)",
    "(a: b == 1 < 2)",
    "(#{$a} < b)",
    "(a: u+0-7f)",
    "not#{$q}",
    "sc\\72 een",
    "\\6e ot (a)",
    "screen \\61 nd (a)",
];

pub(super) const SUPPORTS_CONDITIONS: &[&str] = &[
    "(a: b)",
    "not (a: b)",
    "not(a: b)",
    "(a: b) and (c: d)",
    "(a: b) or (c: d)",
    "(a: b) and (c: d) or (e: f)",
    "(a: b) and (c: d) and (e: f)",
    "((a: b))",
    "((a: b) and (c: d))",
    "(not (a: b))",
    "(not (a: b) )",
    "( (a: b) )",
    "(a:b)",
    "( a : b )",
    "(a: b c)",
    "(a: b, c)",
    "(--a: b)",
    "(--a: {b})",
    "(--a:)",
    "(--a: )",
    "(--a: b; c)",
    "(--a: (b)",
    "(--a: b)c)",
    "(--#{$x}: y)",
    "(#{$a}: b)",
    "(a: #{$b})",
    "#{$c}",
    "not #{$c}",
    "#{$c} and (a: b)",
    "(#{$c})",
    "(#{$c} and (a: b))",
    "(#{$c} or (a: b))",
    "(#{$c} foo)",
    "(#{$c} and)",
    "(#{$c} and (a: b) or (c: d))",
    "selector(a > b)",
    "selector( a )",
    "selector()",
    "selector(a;b)",
    "selector(a{b})",
    "selector(a: b)",
    "selector(a(b)",
    "font-tech(color-COLRv1)",
    "selector(a) and (b: c)",
    "(foo bar)",
    "(foo)",
    "(foo: )",
    "(foo bar: baz)",
    "(a: b !important)",
    "(a: $b)",
    "(a b)",
    "(a  (b))",
    "(foo(bar))",
    "(url(x))",
    "(a: url(x y))",
    "(a /* c */ : b)",
    "(a: b) /* x */ and (c: d)",
    "(a: b)and (c: d)",
    "(a: b) and(c: d)",
    "not",
    "",
    "(a: b) foo",
    "not not (a: b)",
    "(not)",
    "(not a)",
    "(and: b)",
    "(a: b) AND (c: d)",
    "(a: b) and (c: d) AND (e: f)",
    "#{$a}(b)",
    "-#{$a}",
    "--x(b)",
    "(a: b) or #{$c}",
    "(\\61: b)",
    "(a: 'b)c')",
    "(a: 1 < 2)",
    "(a: \"x\" + y)",
    "(foo: bar: baz)",
    "(foo bar baz)",
    "(foo [bar])",
    "(foo {bar})",
    "(foo; bar)",
    "(foo /* ) */ bar)",
    "(foo // x\n)",
    "(foo url(a b))",
    "(foo url(a)b)",
    "(foo \\29 )",
    "(foo 'a)')",
    "(foo #{$x}--1)",
    "(foo ])",
    "(a: b) and not (c: d)",
    "#{$c}#{$d}",
    "#{$c}x",
    "(#{$c}x)",
    "NOT (a: b)",
    "(a: b)or (c: d)",
];

pub(super) const IMPORT_MODIFIERS: &[&str] = &[
    "screen",
    "screen, print",
    "screen and (a)",
    "(a)",
    "(a: b)",
    "supports(a: b)",
    "supports((a: b))",
    "supports(not (a: b))",
    "supports(not(a: b))",
    "supports(selector(a))",
    "supports(selector(a) )",
    "supports( (a: b) )",
    "supports((a: b) and (c: d))",
    "supports(--x: y)",
    "supports(--x: y) screen",
    "supports(a: b) screen",
    "supports(a: b) (a)",
    "layer",
    "layer(base)",
    "layer()",
    "layer(a;b)",
    "layer(a) supports(b: c) screen and (d)",
    "#{$m}",
    "#{$m}(x)",
    "-#{$m}",
    "and (a)",
    "and(a)",
    "screen and",
    "screen,",
    "supports(a)",
    "supports()",
    "supports(a b)",
    "supports(#{$x})",
    "supports(#{$x}: y)",
    "SUPPORTS(a: b)",
    "Layer(x)",
    "foo(bar) baz(qux)",
    "url(x)",
    "supports(not #{$x})",
    "supports(a: b) layer",
    "supports(a: b),",
    "layer(x) , screen",
    "screen supports(a: b)",
    "screen print",
    "screen and (a) supports(b: c)",
];

const MEDIA_FEATURES: &[&str] = &[
    "(a)",
    "(min-width: 1px)",
    "(width >= 1px)",
    "(1px < width < 2px)",
    "(a: b c)",
    "(a: 16/9)",
    "(a: $b)",
    "(a: \"b\")",
    "(a: 1px + 1px)",
    "((a) and (b))",
    "(not (a))",
    "#{$q}",
    "(a: null)",
    "(-x-a: 2)",
    "(a = 1px)",
    "(a: b, c)",
];

const MEDIA_HEADS: &[&str] = &[
    "",
    "screen and ",
    "not screen and ",
    "only screen and ",
    "screen and not ",
    "not ",
];

const SUPPORTS_PIECES: &[&str] = &[
    "(a: b)",
    "(--a: b)",
    "not (a: b)",
    "selector(a)",
    "(foo bar)",
    "#{$c}",
    "((a: b))",
    "(not (a: b))",
    "(a: $b)",
    "(#{$c})",
];

const MODIFIER_PIECES: &[&str] = &[
    "supports(a: b)",
    "supports((a: b) or (c: d))",
    "layer",
    "layer(x)",
    "screen",
    "(a: b)",
    "screen and (a)",
    "#{$m}",
];

/// Combinations of the pieces above, in style rules or after a CSS import.
fn generated() -> Vec<String> {
    let mut queries = Vec::new();
    for head in MEDIA_HEADS {
        for feature in MEDIA_FEATURES {
            queries.push(format!("{head}{feature}"));
        }
    }
    for left in MEDIA_FEATURES {
        for right in MEDIA_FEATURES {
            for form in ["{} and {}", "{} or {}", "{}, {}", "screen and {} and {}"] {
                queries.push(form.replacen("{}", left, 1).replacen("{}", right, 1));
            }
        }
    }
    let mut conditions: Vec<String> = SUPPORTS_PIECES.iter().map(|c| c.to_string()).collect();
    for left in SUPPORTS_PIECES {
        for right in SUPPORTS_PIECES {
            for form in ["{} and {}", "{} or {}", "({} and {})"] {
                conditions.push(form.replacen("{}", left, 1).replacen("{}", right, 1));
            }
        }
    }
    let mut texts: Vec<_> = queries
        .iter()
        .map(|query| format!("@media {query}"))
        .chain(conditions.iter().map(|c| format!("@supports {c}")))
        .collect();
    for left in MODIFIER_PIECES {
        for right in MODIFIER_PIECES {
            for separator in [" ", ", "] {
                texts.push(format!("@import 'a.css' {left}{separator}{right}"));
            }
        }
    }
    texts
}

pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = Vec::new();
    for text in generated() {
        if text.starts_with("@import") {
            cases.push((format!("{text};"), SassSyntax::Scss));
            cases.push((format!("{text}\n"), SassSyntax::Indented));
        } else {
            cases.push((format!(".p {{ {text} {{ x: y; }} }}"), SassSyntax::Scss));
            cases.push((format!(".p\n  {text}\n    x: y\n"), SassSyntax::Indented));
        }
    }
    for query in MEDIA_QUERIES {
        cases.push((
            format!(".p {{ @media {query} {{ x: y; }} }}"),
            SassSyntax::Scss,
        ));
        if !query.contains('\n') {
            cases.push((
                format!(".p\n  @media {query}\n    x: y\n"),
                SassSyntax::Indented,
            ));
        }
    }
    for condition in SUPPORTS_CONDITIONS {
        cases.push((
            format!(".p {{ @supports {condition} {{ x: y; }} }}"),
            SassSyntax::Scss,
        ));
        if !condition.contains('\n') {
            cases.push((
                format!(".p\n  @supports {condition}\n    x: y\n"),
                SassSyntax::Indented,
            ));
        }
    }
    for modifiers in IMPORT_MODIFIERS {
        for tail in ["", ", 'b.css'"] {
            cases.push((
                format!("@import 'a.css' {modifiers}{tail};"),
                SassSyntax::Scss,
            ));
            cases.push((
                format!("@import 'a.css' {modifiers}{tail}\n"),
                SassSyntax::Indented,
            ));
        }
    }
    cases.extend(
        [
            "@media screen { a: b }",
            "@media screen { .a { b: c } }",
            "@supports (a: b) { .a { b: c } }",
            "@supports (a: b) { a: b }",
            ".p { @media screen; }",
            ".p { @supports (a: b); }",
            "@mixin m { @media screen { a: b } }",
            "@include m { @supports (a: b) { a: b } }",
            "@function f() { @media screen {} @return 1; }",
            "@function f() { @supports (a: b) {} @return 1; }",
            ".p { a: { @media screen { b: c } } }",
            ".p { a: { @supports (a: b) { b: c } } }",
            "@media screen { @media print { .a { b: c } } }",
            "@media screen { @use 'a'; }",
            "@media screen {} @use 'a';",
            "@keyframes k { @media screen { from { a: b } } }",
            "@if true { @media screen { .a { b: c } } }",
            ".p { @media #{$q} and (a: $b) { x: y } }",
            "@MEDIA screen { .a { b: c } }",
            "@m\\65 dia screen { .a { b: c } }",
            "@SUPPORTS (a: b) { .a { b: c } }",
            "@media",
            "@supports",
            "@media screen",
            ".p { @media screen {",
        ]
        .iter()
        .map(|text| (text.to_string(), SassSyntax::Scss)),
    );
    cases.extend(
        [
            "@media screen\n  .a\n    b: c\n",
            // Root `a: b` would be a selector that only evaluation rejects.
            "@media screen\n  a:b\n",
            "@foo screen\n  a: b\n",
            ".p\n  @media screen\n",
            ".p\n  @media (a:\n    b)\n    x: y\n",
            ".p\n  @media screen and\n    (a)\n    x: y\n",
            ".p\n  @media ((a)\n    and (b))\n    x: y\n",
            ".p\n  @media ((a) and\n    (b))\n    x: y\n",
            ".p\n  @media screen,\n    print\n    x: y\n",
            ".p\n  @supports (a:\n    b)\n    x: y\n",
            ".p\n  @supports (a: b) and\n    (c: d)\n    x: y\n",
            ".p\n  @supports ((a: b) and\n    (c: d))\n    x: y\n",
            ".p\n  @supports (--a: b\n    c)\n    x: y\n",
            ".p\n  @supports selector(a\n    b)\n    x: y\n",
            ".p\n  @supports (foo\n    bar)\n    x: y\n",
            "@import 'a.css' supports(\n  a: b)\n",
            "@import 'a.css' layer(\n  a)\n",
            "@import 'a.css' screen and\n  (a)\n",
            "@import 'a.css' (a:\n  b)\n",
            "@import 'a.css' supports(--x: a\n  b)\n",
        ]
        .iter()
        .map(|text| (text.to_string(), SassSyntax::Indented)),
    );
    cases
}

/// Whether Frameshift accepts a case and whether it carries media query
/// value-dependence evidence.
pub(super) fn media_verdict(text: &str, syntax: SassSyntax) -> Option<bool> {
    let input = source(text);
    let doc = parse_sass(&input, syntax, SassLimits::default()).ok()?;
    Some(
        doc.unchecked_syntax()
            .any(|entry| entry.kind == SassUncheckedKind::MediaQuery),
    )
}

#[test]
fn generated_conditions_preserve_tree_and_repetition_contracts() {
    let mut accepted = 0;
    for (text, syntax) in cases() {
        let input = source(&text);
        if let Ok(doc) = parse_sass(&input, syntax, SassLimits::default()) {
            check(&doc);
            accepted += 1;
        }
    }
    assert!(accepted > 3000, "{accepted}");
}

#[test]
fn media_queries_have_parse_time_structure() {
    let text = ".p { @media not screen and (1px < width <= $w), (a: b) or #{$q} { x: y; } }";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let rule = doc
        .nodes()
        .iter()
        .find(|node| node.kind == Kind::AtRule(SassAtRule::Media))
        .unwrap();
    let list = doc.node(rule.children[1]).unwrap();
    assert_eq!(list.kind, Kind::MediaQueryList);
    assert_eq!(
        input.slice(list.span).unwrap(),
        "not screen and (1px < width <= $w), (a: b) or #{$q}"
    );
    let kinds = |id| {
        doc.node(id)
            .unwrap()
            .children
            .iter()
            .map(|&child| doc.node(child).unwrap().kind.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(list.children[0]),
        [
            Kind::Identifier,
            Kind::Identifier,
            Kind::ConditionOperator(SassConditionOperator::And),
            Kind::MediaInParens,
        ]
    );
    let range = doc.node(list.children[0]).unwrap().children[3];
    assert_eq!(
        kinds(range),
        [
            Kind::Number,
            Kind::MediaRelation(SassMediaRelation::Less),
            Kind::String { quoted: false },
            Kind::MediaRelation(SassMediaRelation::LessEqual),
            Kind::Variable,
        ]
    );
    assert_eq!(
        kinds(list.children[1]),
        [
            Kind::MediaInParens,
            Kind::ConditionOperator(SassConditionOperator::Or),
            Kind::Interpolation,
        ]
    );
    let feature = doc.node(list.children[1]).unwrap().children[0];
    assert_eq!(
        kinds(feature),
        [
            Kind::String { quoted: false },
            Kind::MediaRelation(SassMediaRelation::Colon),
            Kind::String { quoted: false },
        ]
    );
    let unchecked: Vec<_> = doc.unchecked_syntax().collect();
    assert_eq!(unchecked.len(), 1);
    assert_eq!(unchecked[0].kind, SassUncheckedKind::MediaQuery);
    assert_eq!(unchecked[0].span, list.span);
}

#[test]
fn literal_media_queries_have_no_evidence_and_dependent_ones_do() {
    for (query, dependent) in [
        ("screen and (min-width: 100px)", false),
        ("(a: 1px + 2px) and (b: 16/9)", false),
        ("not (a: #fff)", false),
        ("(a: #fff) or (b: true)", false),
        ("(a: $b)", true),
        ("(a: f(1))", true),
        ("(a: \"b\")", true),
        ("(a: null)", true),
        ("#{$q}", true),
        ("scr#{e}en", true),
        ("(a: &)", true),
        ("(a: (b: c))", true),
    ] {
        for (text, syntax) in [
            (
                format!(".p {{ @media {query} {{ x: y; }} }}"),
                SassSyntax::Scss,
            ),
            (
                format!(".p\n  @media {query}\n    x: y\n"),
                SassSyntax::Indented,
            ),
        ] {
            assert_eq!(media_verdict(&text, syntax), Some(dependent), "{text:?}");
        }
    }
}

#[test]
fn supports_conditions_and_import_modifiers_have_no_evidence() {
    let text = ".p { @supports (not ((a: b) and (--c: {d}))) or selector(e > f) { x: y; } }\n\
                @import 'theme.css' supports((a: b) or #{$c}) layer(base) screen, (d: $e);";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert_eq!(doc.unchecked_syntax().count(), 0);
    let rule = doc
        .nodes()
        .iter()
        .find(|node| node.kind == Kind::AtRule(SassAtRule::Supports))
        .unwrap();
    let condition = doc.node(rule.children[1]).unwrap();
    assert_eq!(condition.kind, Kind::SupportsOperation);
    let negation = doc.node(condition.children[0]).unwrap();
    assert_eq!(negation.kind, Kind::SupportsNegation);
    let inner = doc.node(negation.children[1]).unwrap();
    assert_eq!(inner.kind, Kind::SupportsOperation);
    assert_eq!(input.slice(inner.span).unwrap(), "((a: b) and (--c: {d}))");
    let custom = doc.node(inner.children[2]).unwrap();
    assert_eq!(custom.kind, Kind::SupportsDeclaration);
    assert_eq!(
        doc.node(custom.children[1]).unwrap().kind,
        Kind::InterpolatedText
    );
    let function = doc.node(condition.children[2]).unwrap();
    assert_eq!(function.kind, Kind::SupportsFunction);
    assert_eq!(input.slice(function.span).unwrap(), "selector(e > f)");
    let modifiers = doc
        .nodes()
        .iter()
        .find(|node| node.kind == Kind::ImportModifiers)
        .unwrap();
    let kinds: Vec<_> = modifiers
        .children
        .iter()
        .map(|&id| doc.node(id).unwrap().kind.clone())
        .collect();
    assert_eq!(
        kinds,
        [
            Kind::ImportFunction,
            Kind::ImportFunction,
            Kind::Identifier,
            Kind::MediaQueryList,
        ]
    );
    let supports = doc.node(modifiers.children[0]).unwrap();
    let operation = doc.node(supports.children[1]).unwrap();
    assert_eq!(operation.kind, Kind::SupportsOperation);
    assert_eq!(
        doc.node(operation.children[0]).unwrap().kind,
        Kind::SupportsDeclaration
    );
    assert_eq!(
        doc.node(operation.children[2]).unwrap().kind,
        Kind::Interpolation
    );
}

#[test]
fn condition_rules_obey_contexts() {
    for (text, accepted) in [
        ("@media screen { .a { b: c } }", true),
        ("@media screen { a: b }", false),
        (".p { @media screen { a: b } }", true),
        ("@mixin m { @media screen { a: b } }", true),
        ("@include m { @supports (a: b) { a: b } }", true),
        ("@function f() { @media screen {} @return 1; }", false),
        ("@function f() { @supports (a: b) {} @return 1; }", false),
        (".p { a: { @media screen { b: c } } }", false),
        (".p { @media screen; }", false),
        ("@media screen {} @use 'a';", false),
    ] {
        let input = source(text);
        assert_eq!(
            parse_sass(&input, SassSyntax::Scss, SassLimits::default()).is_ok(),
            accepted,
            "{text:?}"
        );
    }
}

#[test]
fn condition_parsing_charges_the_shared_limits() {
    let nested = format!(
        ".p {{ @media {}a{} {{ x: y; }} }}",
        "(".repeat(200),
        ")".repeat(200)
    );
    let supports = format!(
        ".p {{ @supports {}a: b{} {{ x: y; }} }}",
        "(".repeat(200),
        ")".repeat(200)
    );
    let raw = format!(
        ".p {{ @supports selector({}{}) {{ x: y; }} }}",
        "[".repeat(200),
        "]".repeat(200)
    );
    for text in [nested, supports, raw] {
        let input = source(&text);
        let error = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap_err();
        assert_eq!(
            error.kind,
            frameshift_engine::syntax::SassErrorKind::DepthLimit
        );
    }
    let text = ".p { @media screen and (a: b), (c) or (d) { x: y; } }";
    let input = source(text);
    let full = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    for limits in [
        SassLimits {
            max_nodes: full.nodes().len() - 1,
            ..SassLimits::default()
        },
        SassLimits {
            max_work: text.len(),
            ..SassLimits::default()
        },
    ] {
        assert!(parse_sass(&input, SassSyntax::Scss, limits).is_err());
    }
}
