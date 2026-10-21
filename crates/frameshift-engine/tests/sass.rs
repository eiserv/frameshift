//! Sass syntax contracts and a parse-only external oracle (`D-079`).

use std::collections::BTreeSet;

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    SassAtRule, SassBinaryOperator, SassDocument, SassErrorKind, SassLimits, SassNodeId,
    SassNodeKind, SassSyntax, parse_sass,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn check(doc: &SassDocument<'_>) {
    let source = doc.source();
    let mut visited = BTreeSet::new();
    let mut queue: Vec<SassNodeId> = doc.statements().to_vec();
    for comment in doc.comments() {
        source.slice(comment.span).unwrap();
        queue.extend_from_slice(&comment.interpolations);
    }
    while let Some(id) = queue.pop() {
        let node = doc.node(id).unwrap();
        assert!(visited.insert(id), "duplicate node: {node:?}");
        source.slice(node.span).unwrap();
        let mut end = node.span.start();
        for &child in &node.children {
            let child = doc.node(child).unwrap();
            assert!(
                child.span.start() >= end && child.span.end() <= node.span.end(),
                "unordered child {child:?} of {node:?}"
            );
            end = child.span.end();
        }
        queue.extend_from_slice(&node.children);
    }
    assert_eq!(visited.len(), doc.nodes().len(), "unreachable nodes");
    assert!(
        doc.comments()
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
    );
    assert_eq!(
        parse_sass(source, doc.syntax(), SassLimits::default()).as_ref(),
        Ok(doc)
    );
}

fn accepts(text: &str, syntax: SassSyntax) {
    let source = source(text);
    let doc = parse_sass(&source, syntax, SassLimits::default())
        .unwrap_or_else(|error| panic!("{syntax:?}: {text:?}: {error}"));
    check(&doc);
}

const SCSS_CASES: &[&str] = &[
    "",
    "$size: 1px !default; .card { width: $size * 2; }",
    "$map: (a: 1, b: [2, 3]); $list: 1 2, 3 4;",
    "$x: (1,); $y: []; $z: ();",
    ".a { color: #abc; background: rgba(1, 2, 3, .5); }",
    ".a { color: true; content: null; value: not false and 1 < 2 or 3 == 4; }",
    ".a { font: { size: 12px; weight: bold; } &:hover { color: red; } }",
    "$key: x; .#{$key} { --#{$key}: #{$key}; content: \"a#{$key}b\"; }",
    "// silent #{$ignored}\n/* loud #{1 + 2} */ .a { color: red; /* inline */ }",
    "@mixin card($size: 2px, $rest...) { width: $size; @content($size); }",
    "@include card(3px, $kind: primary) using ($size) { .a { width: $size; } }",
    "@function twice($n) { @return $n * 2; } .a { width: twice(3px); }",
    "@if true { $x: 1; } @else if false { $x: 2; } @else { $x: 3; }",
    "@each $key, $value in (a: 1, b: 2) { .#{$key} { width: $value; } }",
    "@for $i from 1 through 3 { .n-#{$i} { order: $i; } }",
    "$n: 2; @while $n > 0 { $n: $n - 1; }",
    "@use 'palette' as p with ($tone: red); .a { color: p.$tone; }",
    "@forward 'palette' as theme-* show $tone, paint with ($tone: red !default);",
    "@import 'fictional'; @media screen and (width > 3px) { .a { display: grid; } }",
    ".a { @extend %base !optional; @at-root .b { color: red; } }",
    "@debug (a: 1); @warn 'check'; @error 'fictional';",
    ".a { width: calc(100% - #{$offset}); background: url(https://example.invalid/a.svg); }",
    ".a { content: \"a\\\r\nb\"; width: 1e-3rem; }",
    ".a { x: f($list..., $named...); }",
    ".a { x: f($a: 1, $b: 2); }",
    ".a { x: 1 -2; y: 1 - 2 * 3; }",
    "@unknown #{$name} { .a { prop: value; } }",
    "\u{feff}$色: 2px;\r\n.色 { width: $色; }",
    ".a { width: 2px!important; color: red !IMPORTANT; }",
    ".a { width: calc($width + 2px); filter: alpha(opacity=20); }",
    ".a { .#{$name}:hover { color: red; } #{$property}: 2px; }",
    "@mixin a($first\n : 1, $rest\n ...) {}",
    "$x: 1 + 2 3; $y: 1 2 + 3;",
    "$x: #123g; $y: 1$var; $z: 1#{a};",
    "$x: U+00A0-00FF; $y: u+4??; $z: \"\\110000\";",
    "@if true {} @elseif false {} @else {}",
    ".a { a:-webkit-any-link { color: red; } }",
];

const SASS_CASES: &[&str] = &[
    "$size: 2px\n.card\n  width: $size * 2\n  &:hover\n    color: red\n.next\n  color: blue\n",
    "@mixin card($n: 2)\n  width: $n\n  @content\n.a\n  @include card(3)\n",
    "@if true\n  $n: 1\n@else\n  $n: 2\n",
    "@function twice($n)\n  @return $n * 2\n.a\n  width: twice(2px)\n",
    ".a\n\tcolor: red\n\t&:hover\n\t\tcolor: blue\n",
    "$x: (a: 1,\n  b: 2)\n.a\n  width: map.get($x, a)\n",
    "// silent\n.a\n  // nested\n  color: red\n",
    "@mixin empty($x)\n.next\n  color: red;\n",
    "// comment\n  arbitrary words {\n.a\n  color: red\n",
    "/* comment #{1 + 2}\n  more comment\n.a\n  color: red\n",
];

const MIXIN_EXTENSION_CASES: &[(&str, SassSyntax)] = &[
    ("@mixin button { @extend %base; }", SassSyntax::Scss),
    (
        "@mixin button { @if true { @extend %base; } }",
        SassSyntax::Scss,
    ),
    ("@mixin button\n  @extend %base\n", SassSyntax::Indented),
    (
        "@mixin button\n  @if true\n    @extend %base\n",
        SassSyntax::Indented,
    ),
];

const INDENTED_SELECTOR_CASES: &[&str] = &[
    ".a\n  a:hover\n    color: red\n",
    ".a\n  a:hover\n    @media screen\n      color: red\n",
    ".a\n  a:#{hover}\n    color: red\n",
    ".a\n  a:-webkit-any-link\n    color: red\n",
    ".a\n  a:hover\n",
];

const CONTENT_CASES: &[(&str, SassSyntax, bool)] = &[
    (
        "@mixin host { .a { @content; } } @include host { color: red; @extend %base; }",
        SassSyntax::Scss,
        true,
    ),
    (
        "@include host using ($tone) { @if true { color: $tone; @extend %base; } }",
        SassSyntax::Scss,
        true,
    ),
    (
        "@include host { @include other { color: red; } } @mixin after {}",
        SassSyntax::Scss,
        true,
    ),
    (
        "@mixin host { @include other { @content; } }",
        SassSyntax::Scss,
        true,
    ),
    (
        "@include host { @mixin nested {} }",
        SassSyntax::Scss,
        false,
    ),
    (
        "@include host { @function nested() { @return 1; } }",
        SassSyntax::Scss,
        false,
    ),
    ("@include host { @content; }", SassSyntax::Scss, false),
    (
        "@include host { font: { @extend %base; } }",
        SassSyntax::Scss,
        false,
    ),
    (
        "@include host\n  color: red\n  @extend %base\n",
        SassSyntax::Indented,
        true,
    ),
    (
        "@include host using ($tone)\n  @if true\n    color: $tone\n    @extend %base\n",
        SassSyntax::Indented,
        true,
    ),
    (
        "@include host\n  @include other\n    color: red\n@mixin after\n",
        SassSyntax::Indented,
        true,
    ),
    (
        "@include host\n  @mixin nested\n",
        SassSyntax::Indented,
        false,
    ),
    (
        "@include host\n  @function nested()\n    @return 1\n",
        SassSyntax::Indented,
        false,
    ),
    ("@include host\n  @content\n", SassSyntax::Indented, false),
];

const INDENTED_COMMENT_CASES: &[(&str, bool)] = &[
    (".a\n  color: red /* unclosed", false),
    (".a\n  color: red /* comment\n.b\n  width: 1px\n", false),
    ("$x: (1 +\n  /* unclosed\n  2)\n", false),
    (".a\n  color: red /* comment\n*/\n  width: 1px\n", true),
    (".a\n  color: red /* comment */\n  width: 1px\n", true),
    ("/* comment", true),
    ("/* comment\n  more\n.a\n  color: red\n", true),
    (".a\n  /* comment\n    more\n  color: red\n", true),
    ("$x: (1 +\n  /* closed */\n  2)\n", true),
];

const SHORTHAND_CASES: &[(&str, bool)] = &[
    ("+ card\n", true),
    ("+#{card}\n", true),
    ("\\+card\n  color: red\n", true),
    (".a\n  \\:hover\n    color: red\n", true),
    (
        "=card($size: 1px, $rest...)\n  width: $size\n  @content($size)\n.a\n  +card(2px, $tone: red)\n",
        true,
    ),
    (
        "=card\n  @if true\n    @extend %base\n+card\n  color: red\n",
        true,
    ),
    (
        "+palette.card($args..., $named...) using ($tone)\n  color: $tone\n",
        true,
    ),
    ("+host\n  +nested\n    color: red\n=after\n", true),
    ("= card($n: 1)\n  width: $n\n", true),
    ("=\n  card\n    width: 1px\n", true),
    ("=/* name */card\n  width: 1px\n", true),
    ("=色($幅: 1px)\n\twidth: $幅\n.a\n\t+色(2px)\n", true),
    ("=\\63 ard($n: 1)\n  width: $n\n+\\63 ard(2)\n", true),
    (
        "+host(\n  1,\n  $tone: red\n) using ($value)\n  color: $value\n",
        true,
    ),
    ("=outer\n  =inner\n", false),
    ("+host\n  =inner\n", false),
    ("@if true\n  =inner\n", false),
    ("@function f()\n  +card\n", false),
    ("@function f()\n  =card\n", false),
    (".a\n  font:\n    +card\n", false),
    (".a\n  font:\n    =card\n", false),
    ("+palette._private\n", false),
    ("=card($font_size, $font-size)\n", false),
    ("+card($font_size: 1, $font-size: 2)\n", false),
    ("+card(opacity=20)\n", false),
    ("=\n", false),
    ("=card(\n", false),
    ("+card using ($x, $x)\n", false),
    ("=card\n@use 'fictional'\n", false),
    ("+card\n@use 'fictional'\n", false),
    ("+card trailing\n", false),
];

#[test]
fn legacy_mixins_share_directive_grammar_and_context_checks() {
    for &(text, valid) in SHORTHAND_CASES {
        if valid {
            accepts(text, SassSyntax::Indented);
        } else {
            assert!(
                parse_sass(&source(text), SassSyntax::Indented, SassLimits::default()).is_err(),
                "{text}"
            );
        }
    }
}

#[test]
fn legacy_mixin_nodes_preserve_markers_names_and_body_boundaries() {
    let input = source(
        "\u{feff}=card($n: 1)\r\n  width: $n\r\n+palette.card(2) using ($x)\r\n  color: $x\r\n+empty\r\n.next\r\n  height: 3px\r\n",
    );
    let doc = parse_sass(&input, SassSyntax::Indented, SassLimits::default()).unwrap();
    check(&doc);
    let kinds: Vec<_> = doc
        .statements()
        .iter()
        .map(|&id| doc.node(id).unwrap().kind.clone())
        .collect();
    assert_eq!(
        kinds,
        [
            SassNodeKind::AtRule(SassAtRule::Mixin),
            SassNodeKind::AtRule(SassAtRule::Include),
            SassNodeKind::AtRule(SassAtRule::Include),
            SassNodeKind::StyleRule,
        ]
    );
    let mixin = doc.node(doc.statements()[0]).unwrap();
    assert_eq!(
        input.slice(mixin.span).unwrap(),
        "=card($n: 1)\r\n  width: $n\r\n"
    );
    assert_eq!(
        input
            .slice(doc.node(mixin.children[0]).unwrap().span)
            .unwrap(),
        "card"
    );
    assert_eq!(
        doc.node(mixin.children[1]).unwrap().kind,
        SassNodeKind::Parameters
    );
    assert_eq!(
        doc.node(mixin.children[2]).unwrap().kind,
        SassNodeKind::Declaration
    );
    let include = doc.node(doc.statements()[1]).unwrap();
    let spellings: Vec<_> = include
        .children
        .iter()
        .map(|&id| input.slice(doc.node(id).unwrap().span).unwrap())
        .collect();
    assert_eq!(
        spellings,
        ["palette", "card", "(2)", "($x)", "color: $x\r\n"]
    );
    assert_eq!(
        input
            .slice(doc.node(doc.statements()[2]).unwrap().span)
            .unwrap(),
        "+empty"
    );
}

#[test]
fn shorthand_is_indented_only_and_selector_prefixes_remain_distinct() {
    for (text, selector) in [
        ("+ card\n", "+ card"),
        ("+#{card}\n", "+#{card}"),
        ("\\+card\n", "+card"),
        ("\\:hover\n", ":hover"),
    ] {
        let input = source(text);
        let doc = parse_sass(&input, SassSyntax::Indented, SassLimits::default()).unwrap();
        check(&doc);
        let rule = doc.node(doc.statements()[0]).unwrap();
        assert_eq!(rule.kind, SassNodeKind::StyleRule, "{text}");
        assert_eq!(
            input
                .slice(doc.node(rule.children[0]).unwrap().span)
                .unwrap()
                .trim_end(),
            selector
        );
        assert_eq!(rule.span.start(), 0);
    }
    for text in ["+card {}", "=card {}"] {
        let input = source(text);
        let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
        check(&doc);
        assert_eq!(
            doc.node(doc.statements()[0]).unwrap().kind,
            SassNodeKind::StyleRule
        );
    }
}

#[test]
fn shorthand_charges_the_shared_parser_limits() {
    let input = source("=card($x: 1)\n+card(2)\n");
    let defaults = SassLimits::default();
    let doc = parse_sass(&input, SassSyntax::Indented, defaults).unwrap();
    let count = doc.nodes().len();
    assert_eq!(
        parse_sass(
            &input,
            SassSyntax::Indented,
            SassLimits {
                max_nodes: count,
                ..defaults
            }
        )
        .unwrap(),
        doc
    );
    for (limits, error) in [
        (
            SassLimits {
                max_nodes: count - 1,
                ..defaults
            },
            SassErrorKind::NodeLimit,
        ),
        (
            SassLimits {
                max_depth: 1,
                ..defaults
            },
            SassErrorKind::DepthLimit,
        ),
        (
            SassLimits {
                max_work: 1,
                ..defaults
            },
            SassErrorKind::WorkLimit,
        ),
    ] {
        assert_eq!(
            parse_sass(&input, SassSyntax::Indented, limits)
                .unwrap_err()
                .kind,
            error
        );
    }
}

#[test]
fn inline_indented_comments_require_a_closer() {
    for &(text, valid) in INDENTED_COMMENT_CASES {
        if valid {
            accepts(text, SassSyntax::Indented);
        } else {
            assert_eq!(
                parse_sass(&source(text), SassSyntax::Indented, SassLimits::default())
                    .unwrap_err()
                    .kind,
                SassErrorKind::Unterminated,
                "{text}"
            );
        }
    }
    let input = source(INDENTED_COMMENT_CASES[3].0);
    let doc = parse_sass(&input, SassSyntax::Indented, SassLimits::default()).unwrap();
    assert_eq!(
        input.slice(doc.comments()[0].span).unwrap(),
        "/* comment\n*/"
    );
    assert_eq!(
        doc.nodes()
            .iter()
            .filter(|n| n.kind == SassNodeKind::Declaration)
            .count(),
        2
    );
}

#[test]
fn include_content_has_its_own_lexical_context() {
    for &(text, syntax, valid) in CONTENT_CASES {
        if valid {
            accepts(text, syntax);
        } else {
            assert_eq!(
                parse_sass(&source(text), syntax, SassLimits::default())
                    .unwrap_err()
                    .kind,
                SassErrorKind::InvalidContext,
                "{text}"
            );
        }
    }
    let text = "@include host { color: red; --tone: blue; }";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    assert_eq!(
        doc.nodes()
            .iter()
            .filter(|n| n.kind == SassNodeKind::Declaration)
            .count(),
        2
    );
}

#[test]
fn pseudo_classes_remain_style_rules_in_both_surface_syntaxes() {
    for (text, syntax) in INDENTED_SELECTOR_CASES
        .iter()
        .map(|text| (*text, SassSyntax::Indented))
        .chain(std::iter::once((
            ".a { a:-webkit-any-link { color: red; } }",
            SassSyntax::Scss,
        )))
    {
        let source = source(text);
        let doc = parse_sass(&source, syntax, SassLimits::default()).unwrap();
        check(&doc);
        assert_eq!(
            doc.nodes()
                .iter()
                .filter(|node| node.kind == SassNodeKind::StyleRule)
                .count(),
            2,
            "{text}"
        );
    }
    accepts(
        ".a\n  --tone:red\n  color: red\n  font:\n    size: 2px\n",
        SassSyntax::Indented,
    );
}

#[test]
fn extensions_are_allowed_in_mixins_and_their_control_blocks() {
    for &(text, syntax) in MIXIN_EXTENSION_CASES {
        accepts(text, syntax);
    }
    for text in [
        "@extend %base;",
        "@function button() { @extend %base; }",
        ".a { font: { @extend %base; } }",
    ] {
        let source = source(text);
        assert_eq!(
            parse_sass(&source, SassSyntax::Scss, SassLimits::default())
                .unwrap_err()
                .kind,
            SassErrorKind::InvalidContext,
            "{text}"
        );
    }
}

#[test]
fn scss_statements_and_sassscript() {
    for text in SCSS_CASES {
        accepts(text, SassSyntax::Scss);
    }
}

#[test]
fn indented_sass_preserves_original_spans() {
    for text in SASS_CASES {
        accepts(text, SassSyntax::Indented);
    }
}

#[test]
fn precedence_and_map_structure_are_explicit() {
    let source = source("$x: (a: 1 + 2 * 3);");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let add = doc
        .nodes()
        .iter()
        .find(|n| n.kind == SassNodeKind::Binary(SassBinaryOperator::Add))
        .unwrap();
    assert_eq!(
        doc.node(add.children[1]).unwrap().kind,
        SassNodeKind::Binary(SassBinaryOperator::Multiply)
    );
    assert!(doc.nodes().iter().any(|n| n.kind == SassNodeKind::MapEntry));
    assert!(doc.nodes().iter().any(|n| n.kind == SassNodeKind::Map));
}

const INVALID_SCSS: &[&str] = &[
    "$x: ;",
    "$: 1;",
    "$x: 1 !wrong;",
    "$x: (a:);",
    "$x: [1, 2;",
    "$x: \"unterminated",
    "/* unterminated",
    ".a { x: #{1 +}; }",
    ".a {",
    "}",
    ".a { x: $; }",
    "@return 1;",
    "@content;",
    "@else { $x: 1; }",
    "@mixin a($x, $x) {}",
    "@mixin a($a_b, $a-b) {}",
    ".a { x: f($a: 1, $a: 2); }",
    ".a { x: f($a: 1, 2); }",
    "@use 'a' with (x: 1);",
    ".a { @use 'a'; }",
    ".a {} @use 'a';",
    "@function a($x) { .a {} }",
    "@for $i from 1 { .a {} }",
    "@each $x (a, b) {}",
    ".a { value: ; }",
    ".a { value: calc(1px *); }",
    ".a { value: module.$_private; }",
    ".a { value: module._private(); }",
    "@use 'fictional' with ();",
    "@function url() { @return 1; }",
    ".a { font: { --x: red; } }",
    ".a { font: { @unknown; } }",
    "@if true {} @else {} @else {}",
    "$x: U+1234567;",
    "$x: #12zz;",
    "$x: 1e-foo;",
    "$x: f($a: 1...);",
    "$x: f($a..., $b..., 1);",
    "$x: f((a=b));",
    "$x: U+123-ABC-DEF;",
];

#[test]
fn syntax_errors_return_no_document() {
    for text in INVALID_SCSS {
        let source = source(text);
        let result = parse_sass(&source, SassSyntax::Scss, SassLimits::default());
        let error = result.expect_err(text);
        source.validate_offset(error.offset).unwrap();
    }
}

#[test]
fn indentation_is_checked_instead_of_normalized() {
    for text in [
        ".a\n \tcolor: red",
        ".a\n  .b\n    color: red\n   width: 2px",
        ".a\n\tcolor: red\n  width: 1px",
    ] {
        let source = source(text);
        let error = parse_sass(&source, SassSyntax::Indented, SassLimits::default()).unwrap_err();
        assert_eq!(error.kind, SassErrorKind::InvalidIndentation, "{text}");
    }
}

#[test]
fn resource_limits_fail_without_partial_trees() {
    let source = source("$x: 1;");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
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
                max_depth: 0,
                ..SassLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::DepthLimit
    );
    assert_eq!(
        parse_sass(
            &source,
            SassSyntax::Scss,
            SassLimits {
                max_work: 0,
                ..SassLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::WorkLimit
    );
    let nested = source_text_nested(200);
    assert_eq!(
        parse_sass(
            &nested,
            SassSyntax::Scss,
            SassLimits {
                max_depth: usize::MAX,
                ..SassLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::DepthLimit
    );
}

fn source_text_nested(depth: usize) -> SourceText {
    source(&format!("$x: {}1{};", "(".repeat(depth), ")".repeat(depth)))
}

#[test]
fn modules_are_syntax_only_and_never_loaded() {
    let source =
        source("@use '/path/that/does/not/exist'; @forward 'https://example.invalid/module';");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert_eq!(
        doc.node(doc.statements()[0]).unwrap().kind,
        SassNodeKind::AtRule(SassAtRule::Use)
    );
}

#[test]
fn unchecked_grammars_are_explicit_and_do_not_validate_the_file() {
    use frameshift_engine::syntax::SassUncheckedKind;
    let source = source("@import 'fictional'; @media x { .a { color: red; } }");
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    let unchecked: Vec<_> = doc.unchecked_syntax().collect();
    assert!(
        unchecked
            .iter()
            .any(|entry| entry.kind == SassUncheckedKind::ImportPrelude)
    );
    assert!(
        unchecked
            .iter()
            .any(|entry| entry.kind == SassUncheckedKind::CssDirective)
    );
    assert!(
        unchecked
            .iter()
            .any(|entry| entry.kind == SassUncheckedKind::Selector)
    );
    for entry in unchecked {
        source.slice(entry.span).unwrap();
    }
}

fn generated_cases() -> Vec<String> {
    let primaries = [
        "1", "2px", ".25", "$value", "true", "red", "'text'", "(a: 1)", "[1, 2]", "#abc", "f(1)",
        "#{1}",
    ];
    let mut cases = Vec::new();
    for left in primaries {
        for right in primaries {
            for operator in ["+", "-", "*", "/", "==", "and", "or"] {
                cases.push(format!("$result: {left} {operator} {right};"));
            }
            cases.push(format!(
                "$result: ({left}: {right}, key: [{left}, {right}]);"
            ));
            cases.push(format!("$result: f({left}, $keyword: {right});"));
        }
    }
    cases
}

#[test]
fn generated_sassscript_keeps_tree_and_repetition_contracts() {
    for text in generated_cases() {
        accepts(&text, SassSyntax::Scss);
    }
}

#[test]
#[ignore = "requires external pinned sass-parser 0.4.58 and Sass 1.105.2"]
fn external_sass_oracle() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let module = std::env::var("FRAMESHIFT_SASS_PARSER_MODULE")
        .expect("path to external sass-parser package");
    let script = r#"
const fs = require('node:fs');
const p = require(process.argv[1]);
if (require(process.argv[1] + '/package.json').version !== '0.4.58') throw Error('unpinned parser');
const path = require('node:path');
const s = JSON.parse(fs.readFileSync(path.join(path.dirname(require.resolve('sass', {paths:[process.argv[1]]})), 'package.json')));
if (s.version !== '1.105.2') throw Error('unpinned Sass');
const data = fs.readFileSync(0); let at = 0;
while (at < data.length) {
  let end = data.indexOf(10, at); const syntax = data.subarray(at, end).toString(); at = end + 1;
  end = data.indexOf(10, at); const size = Number(data.subarray(at, end).toString()); at = end + 1;
  const source = data.subarray(at, at + size).toString(); at += size;
  try { p[syntax].parse(source); process.stdout.write('1\n'); }
  catch (e) { process.stdout.write('0\n'); }
}
"#;
    let mut cases: Vec<(String, SassSyntax)> = SCSS_CASES
        .iter()
        .chain(INVALID_SCSS)
        .map(|s| (s.to_string(), SassSyntax::Scss))
        .chain(
            SASS_CASES
                .iter()
                .map(|s| (s.to_string(), SassSyntax::Indented)),
        )
        .collect();
    cases.extend(
        generated_cases()
            .into_iter()
            .map(|text| (text, SassSyntax::Scss)),
    );
    cases.extend(
        MIXIN_EXTENSION_CASES
            .iter()
            .map(|(text, syntax)| (text.to_string(), *syntax)),
    );
    cases.extend(
        INDENTED_SELECTOR_CASES
            .iter()
            .map(|text| (text.to_string(), SassSyntax::Indented)),
    );
    cases.extend(
        CONTENT_CASES
            .iter()
            .map(|(text, syntax, _)| (text.to_string(), *syntax)),
    );
    cases.extend(
        INDENTED_COMMENT_CASES
            .iter()
            .map(|(text, _)| (text.to_string(), SassSyntax::Indented)),
    );
    cases.extend(
        SHORTHAND_CASES
            .iter()
            .map(|(text, _)| (text.to_string(), SassSyntax::Indented)),
    );
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
    eprintln!("Sass parse-only oracle: {} fictional cases", cases.len());
    for ((text, syntax), oracle) in cases.iter().zip(verdicts) {
        let source = source(text);
        assert_eq!(
            parse_sass(&source, *syntax, SassLimits::default()).is_ok(),
            oracle == "1",
            "{syntax:?}: {text}"
        );
    }
}
