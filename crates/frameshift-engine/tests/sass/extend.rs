//! `@extend` targets and `@at-root` queries (`D-084`).

use frameshift_engine::syntax::{
    SassAtRule, SassLimits, SassNodeKind as Kind, SassSyntax, SassUncheckedKind, parse_sass,
};

use super::{check, selector_text, source};

pub(super) const TARGETS: &[&str] = &[
    ".a",
    "%a",
    "a",
    "*",
    "*|a",
    "|a",
    "[a]",
    "[a=b]",
    "[a='b' i]",
    "::before",
    ":hover",
    ":not(.a)",
    ":not(.a .b)",
    ":not(&)",
    ":is(a, b)",
    ":nth-child(2n+1)",
    ":nth-child(2n+1 of .a)",
    "&",
    "&-a",
    ".a&",
    ".a.b",
    "a.b",
    ".a .b",
    "> .a",
    ".a >",
    ".a + .b",
    ".a, .b",
    ".a,",
    ".a, , .b",
    ",.a",
    ".a, .b .c",
    ".#{$x}",
    "#{$x}",
    ".a#{$x}",
    "url(a)",
    "url(a;b)",
    "u\\72l(a)",
    "(a)",
    "[a",
    "a)",
    "",
    "/* c */ .a",
    ".a /* c */",
    ".a/* c */.b",
    ".a /* #{ */",
    ".a\\ ",
    "\\.a",
    ".\\61",
    ".a\\31 ",
    "'a'",
    "\"a\"",
    ".a{",
    ":nth-child(2n+1",
    "#a",
    "#1",
    "-a",
    "--a",
];

const FLAGS: &[&str] = &[
    "",
    " !optional",
    "!optional",
    " !OPTIONAL",
    " !Optional",
    " ! optional",
    " !optionalx",
    " !default",
    " !optional x",
    " !opt\\69onal",
    " !optional !optional",
];

const QUERIES: &[&str] = &[
    "(with: media)",
    "(without: media)",
    "(with: rule)",
    "(without: all)",
    "(with: media supports)",
    "(WITH: media)",
    "(With : Media)",
    "(without:media)",
    "( without : media )",
    "(\\77ith: media)",
    "(w\\69thout: media)",
    "(with)",
    "(media)",
    "(with: )",
    "(: media)",
    "(with: media, supports)",
    "(with: media and supports)",
    "(with: not media)",
    "(with: \"media\")",
    "(\"with\": media)",
    "(with: $m)",
    "($w: media)",
    "(with: #{$m})",
    "(#{$w}: media)",
    "(with: m#{e}dia)",
    "(with: f(a))",
    "(with: 1)",
    "(with: true)",
    "(with: null)",
    "(with: red)",
    "(with: #foo)",
    "(with: %)",
    "(with: -moz-document)",
    "(with: --x)",
    "(with: a b c)",
    "(with: [a])",
    "(with: (a))",
    "(with: a: b)",
    "(with: a)b",
    "()",
    "(with: media",
    "(with: media))",
    "(with: media\\2c)",
    "(with: a/b)",
    "(with: a-b)",
    "(with: a,)",
    "(with: !important)",
    "(with: media /* c */)",
    "(/* c */ with: media)",
    "(with: media // c\n)",
];

/// Every generated and hand-written case, in both surface syntaxes where
/// the form allows it.
pub(super) fn cases() -> Vec<(String, SassSyntax)> {
    let mut cases = Vec::new();
    for target in TARGETS {
        for flag in FLAGS {
            cases.push((
                format!(".p {{ @extend {target}{flag}; }}"),
                SassSyntax::Scss,
            ));
            if !target.contains(['{', '\n']) {
                cases.push((
                    format!(".p\n  @extend {target}{flag}\n"),
                    SassSyntax::Indented,
                ));
            }
        }
        cases.push((
            format!("@mixin m {{ @extend {target}; }}"),
            SassSyntax::Scss,
        ));
        cases.push((
            format!("@include m {{ @extend {target} }}"),
            SassSyntax::Scss,
        ));
    }
    for query in QUERIES {
        cases.push((
            format!(".p {{ @at-root {query} {{ .c {{ x: y; }} }} }}"),
            SassSyntax::Scss,
        ));
        cases.push((
            format!("@keyframes k {{ @at-root {query} {{ .c {{ x: y; }} }} }}"),
            SassSyntax::Scss,
        ));
        if !query.contains('\n') {
            cases.push((
                format!(".p\n  @at-root {query}\n    .c\n      x: y\n"),
                SassSyntax::Indented,
            ));
        }
    }
    cases.extend(
        [
            "@extend .a;",
            ".p { @extend .a }",
            ".p { @extend .a; @extend .b !optional }",
            ".p { a: { @extend .a; } }",
            "@function f() { @extend .a; @return 1; }",
            ".p { @extend\n.a; }",
            ".p { @extend .a\n!optional; }",
            ".p { @extend .a !optional\n; }",
            ".p { @extend .a(\n) !optional; }",
            ".p { @extend .a {} }",
            ".p { @at-root (with: media) }",
            ".p { @at-root (with: media); }",
            ".p { @at-root (with: media) .c { x: y; } }",
            "@at-root (without: rule) { .c { x: y; } }",
            "@at-root(without: rule) { .c { x: y; } }",
            ".p { @at-root\n(with:\nmedia)\n{ .c { x: y; } } }",
        ]
        .iter()
        .map(|text| (text.to_string(), SassSyntax::Scss)),
    );
    cases.extend(
        [
            ".p\n  @extend\n    .a !optional\n",
            ".p\n  @extend .a,\n    .b !optional\n",
            ".p\n  @extend .a !optional\n    x: y\n",
            ".p\n  @extend .a\\31\n  x: y\n",
            ".p\n  @extend .a(\n  b) !optional\n",
            ".p\n  @extend .a[\n  b] !optional\n",
            ".p\n  @extend .a\n  @extend .b !optional\n",
            ".p\n  @extend .a;\n",
            ".p\n  @at-root (with:\n    media)\n    .c\n      x: y\n",
            ".p\n  @at-root (with\n    : media)\n    .c\n      x: y\n",
            ".p\n  @at-root (with: media)\n",
            ".p\n  @at-root (with: media) .c\n    x: y\n",
            "@keyframes k\n  @at-root (without: keyframes)\n    .c\n      x: y\n",
        ]
        .iter()
        .map(|text| (text.to_string(), SassSyntax::Indented)),
    );
    cases
}

/// Frameshift's verdict for the last `@extend` target: no document, or
/// whether the target carries selector evidence.
pub(super) fn extend_verdict(text: &str, syntax: SassSyntax) -> Option<bool> {
    let input = source(text);
    let doc = parse_sass(&input, syntax, SassLimits::default()).ok()?;
    let rule = doc
        .nodes()
        .iter()
        .rfind(|node| node.kind == Kind::AtRule(SassAtRule::Extend))?;
    let target = doc.node(rule.children[1]).unwrap();
    Some(
        doc.unchecked_syntax()
            .any(|entry| entry.kind == SassUncheckedKind::Selector && entry.span == target.span),
    )
}

/// Frameshift's verdict for a case: no document, or whether it carries
/// `@at-root` query evidence.
pub(super) fn query_verdict(text: &str, syntax: SassSyntax) -> Option<bool> {
    let input = source(text);
    let doc = parse_sass(&input, syntax, SassLimits::default()).ok()?;
    Some(
        doc.unchecked_syntax()
            .any(|entry| entry.kind == SassUncheckedKind::AtRootPrelude),
    )
}

#[test]
fn generated_rules_preserve_tree_and_repetition_contracts() {
    let mut accepted = 0;
    for (text, syntax) in cases() {
        let input = source(&text);
        if let Ok(doc) = parse_sass(&input, syntax, SassLimits::default()) {
            check(&doc);
            accepted += 1;
        }
    }
    assert!(accepted > 700, "{accepted}");
}

#[test]
fn established_extend_targets_are_selector_lists() {
    let text = ".p { @extend %a, .b, :not(.c .d), [e='f' i] !optional; @extend g; }";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert_eq!(doc.unchecked_syntax().count(), 0);
    let rules: Vec<_> = doc
        .nodes()
        .iter()
        .filter(|node| node.kind == Kind::AtRule(SassAtRule::Extend))
        .collect();
    let target = doc.node(rules[0].children[1]).unwrap();
    assert_eq!(target.kind, Kind::SelectorList);
    assert_eq!(
        input.slice(target.span).unwrap(),
        "%a, .b, :not(.c .d), [e='f' i]"
    );
    assert_eq!(
        selector_text(&doc, rules[0].children[1]),
        "[placeholder(a), class(b), :(not [class(c) class(d)]), attribute(e Equal 'f' i)]"
    );
    let flag = doc.node(rules[0].children[2]).unwrap();
    assert_eq!(flag.kind, Kind::Flag);
    assert_eq!(input.slice(flag.span).unwrap(), "!optional");
    assert_eq!(rules[1].children.len(), 2);
    assert_eq!(selector_text(&doc, rules[1].children[1]), "[type(g)]");
}

#[test]
fn other_extend_targets_stay_raw_with_evidence() {
    for (target, raw) in [
        (".a", false),
        ("%a", false),
        (":not(.a .b)", false),
        ("\\.a", false),
        (".a\\31 ", false),
        (".a /* c */", false),
        (".a .b", true),
        ("> .a", true),
        (".a.b", true),
        ("&", true),
        ("&-a", true),
        (":not(&)", true),
        (".a, .b .c", true),
        (".#{$x}", true),
        ("url(a)", true),
        ("'a'", true),
        ("", true),
    ] {
        for (text, syntax) in [
            (
                format!(".p {{ @extend {target} !optional; }}"),
                SassSyntax::Scss,
            ),
            (
                format!(".p\n  @extend {target} !optional\n"),
                SassSyntax::Indented,
            ),
        ] {
            assert_eq!(extend_verdict(&text, syntax), Some(raw), "{text:?}");
            let input = source(&text);
            let doc = parse_sass(&input, syntax, SassLimits::default()).unwrap();
            check(&doc);
            let rule = doc
                .nodes()
                .iter()
                .find(|node| node.kind == Kind::AtRule(SassAtRule::Extend))
                .unwrap();
            let node = doc.node(rule.children[1]).unwrap();
            assert_eq!(
                node.kind,
                if raw {
                    Kind::InterpolatedText
                } else {
                    Kind::SelectorList
                },
                "{text:?}"
            );
            // Evaluation trims the target text; a selector list also ends
            // before trailing comments.
            let expected = if raw {
                target.trim_end_matches(' ')
            } else {
                target.trim_end_matches(" /* c */").trim_end_matches(' ')
            };
            assert_eq!(input.slice(node.span).unwrap(), expected);
        }
    }
}

#[test]
fn extend_flags_and_boundaries_follow_the_reference() {
    for (text, accepted) in [
        (".p { @extend .a !optional; }", true),
        (".p { @extend .a!OPTIONAL; }", true),
        (".p { @extend .a !optional }", true),
        (".p { @extend ; }", true),
        (".p { @extend !optional; }", true),
        (".p { @extend url(a;b); }", true),
        (".p { @extend .a ! optional; }", false),
        (".p { @extend .a !optionalx; }", false),
        (".p { @extend .a !opt\\69onal; }", true),
        (".p { @extend .a !opt\\69 onal; }", true),
        (".p { @extend .a !optional\\31; }", false),
        (".p { @extend .a !default; }", false),
        (".p { @extend .a !optional x; }", false),
        (".p { @extend .a); }", false),
        (".p { @extend .a {} }", false),
        ("@extend .a;", false),
    ] {
        let input = source(text);
        assert_eq!(
            parse_sass(&input, SassSyntax::Scss, SassLimits::default()).is_ok(),
            accepted,
            "{text:?}"
        );
    }
    for (text, accepted) in [
        (".p\n  @extend\n    .a !optional\n", true),
        (".p\n  @extend .a(\n  b) !optional\n", true),
        (".p\n  @extend .a\\31\n  x: y\n", true),
        (".p\n  @extend .a,\n    .b\n", false),
        (".p\n  @extend .a\n    x: y\n", false),
    ] {
        let input = source(text);
        assert_eq!(
            parse_sass(&input, SassSyntax::Indented, SassLimits::default()).is_ok(),
            accepted,
            "{text:?}"
        );
    }
}

#[test]
fn at_root_queries_have_expression_children() {
    let text = ".p { @at-root (without: media supports) { .c { x: y; } } \
                @at-root (with: $m) { .c { x: y; } } }";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let queries: Vec<_> = doc
        .nodes()
        .iter()
        .filter(|node| node.kind == Kind::AtRootQuery)
        .collect();
    assert_eq!(queries.len(), 2);
    assert_eq!(
        input.slice(queries[0].span).unwrap(),
        "(without: media supports)"
    );
    let kinds: Vec<_> = queries[0]
        .children
        .iter()
        .map(|&id| doc.node(id).unwrap().kind.clone())
        .collect();
    assert_eq!(
        kinds,
        [
            Kind::String { quoted: false },
            Kind::List {
                separator: frameshift_engine::syntax::SassListSeparator::Space,
                bracketed: false,
            },
        ]
    );
    let unchecked: Vec<_> = doc.unchecked_syntax().collect();
    assert_eq!(unchecked.len(), 1);
    assert_eq!(unchecked[0].kind, SassUncheckedKind::AtRootPrelude);
    assert_eq!(unchecked[0].span, queries[1].span);
}

#[test]
fn literal_queries_have_no_evidence_and_others_do() {
    for (query, dependent) in [
        ("(with: media)", false),
        ("(WITHOUT : rule supports)", false),
        ("(\\77ith: media)", false),
        ("(with: -moz-document)", false),
        ("(with: --x)", false),
        ("(with: media // c\n)", false),
        ("(with)", true),
        ("(media)", true),
        ("(with: $m)", true),
        ("(with: #{$m})", true),
        ("(with: \"media\")", true),
        ("(with: media, supports)", true),
        ("(with: media and supports)", true),
        ("(with: null)", true),
        ("(with: true)", true),
        ("(with: #foo)", true),
        ("(with: %)", true),
        ("(with: (a))", true),
        ("(with: f(a))", true),
        ("(with: media\\2c)", false),
    ] {
        for (text, syntax) in [
            (
                format!(".p {{ @at-root {query} {{ .c {{ x: y; }} }} }}"),
                SassSyntax::Scss,
            ),
            (
                format!(".p\n  @at-root {query}\n    .c\n      x: y\n"),
                SassSyntax::Indented,
            ),
        ] {
            if syntax == SassSyntax::Indented && query.contains('\n') {
                continue;
            }
            assert_eq!(query_verdict(&text, syntax), Some(dependent), "{text:?}");
        }
    }
}

#[test]
fn extend_and_at_root_charge_the_shared_limits() {
    let deep_target = format!(
        ".p {{ @extend {}.a{} !optional; }}",
        "(".repeat(200),
        ")".repeat(200)
    );
    let deep_query = format!(
        ".p {{ @at-root (with: {}a{}) {{ .c {{ x: y; }} }} }}",
        "(".repeat(200),
        ")".repeat(200)
    );
    let deep_pseudo = format!(
        ".p {{ @extend {}.a{} !optional; }}",
        ":not(".repeat(200),
        ")".repeat(200)
    );
    for text in [deep_query, deep_pseudo] {
        let input = source(&text);
        let error = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap_err();
        assert_eq!(
            error.kind,
            frameshift_engine::syntax::SassErrorKind::DepthLimit,
            "{text:?}"
        );
    }
    // Raw brackets are counted, not nested grammar calls.
    let input = source(&deep_target);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    for text in [
        ".p { @extend .a, .b, .c, .d !optional; }",
        ".p { @at-root (without: a b c d e) { .c { x: y; } } }",
    ] {
        let input = source(text);
        let nodes = parse_sass(&input, SassSyntax::Scss, SassLimits::default())
            .unwrap()
            .nodes()
            .len();
        for max_nodes in 1..nodes {
            let limits = SassLimits {
                max_nodes,
                ..SassLimits::default()
            };
            assert!(parse_sass(&input, SassSyntax::Scss, limits).is_err());
        }
        let limits = SassLimits {
            max_work: text.len() / 2,
            ..SassLimits::default()
        };
        assert_eq!(
            parse_sass(&input, SassSyntax::Scss, limits)
                .unwrap_err()
                .kind,
            frameshift_engine::syntax::SassErrorKind::WorkLimit
        );
    }
}
