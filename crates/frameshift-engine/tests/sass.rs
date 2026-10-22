//! Sass syntax contracts and a parse-only external oracle (`D-079`).

#[path = "sass/condition.rs"]
mod condition_cases;
#[path = "sass/css_function.rs"]
mod css_function_cases;
#[path = "sass/extend.rs"]
mod extend_cases;
#[path = "sass/if_expression.rs"]
mod if_cases;
#[path = "sass/import.rs"]
mod import_cases;
#[path = "sass/url.rs"]
mod url_cases;

use std::collections::BTreeSet;

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    SassAtRule, SassBinaryOperator, SassDocument, SassErrorKind, SassLimits, SassNamespace,
    SassNodeId, SassNodeKind, SassSyntax, SassUncheckedKind, parse_sass,
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

const MODULE_CASES: &[(&str, bool)] = &[
    ("@use 'sass:color';", true),
    ("@use '../_palette.scss?theme#fragment';", true),
    ("@use 'https://example.invalid/_palette.scss';", true),
    ("@use 'theme%2Eextra.scss';", true),
    ("@use '%70alette';", true),
    ("@use '123' as palette;", true),
    ("@use '' as *;", true),
    ("@use '#{literal}' as *;", true),
    ("@forward '#{literal}';", true),
    ("@use 'a\\\nb';", true),
    ("@use 'a\\\r\nb';", false),
    ("@use '123';", false),
    ("@use '_';", false),
    ("@use '';", false),
    ("@use 'palette/';", false),
    ("@use 'palette/..';", false),
    ("@use 'a%2Fb';", false),
    ("@use '%ff';", false),
    ("@use '%ff' as x;", true),
    ("@use 'http://[::1]/palette';", true),
    ("@use 'http://[broken]/palette' as x;", false),
    ("@forward 'https://example.invalid:wrong/a';", false),
    ("@forward ':palette';", false),
    ("@forward 'data:wrong';", false),
    ("@forward 'data:,palette';", true),
    ("@forward 'data:;base64,YQ==';", true),
    ("@forward 'data:;base64,?';", false),
    ("@use 'a' AS p;", true),
    ("@use 'a' \\61s p;", true),
    ("@use 'a' as\\20 p;", false),
    ("@use 'a' as p WITH($tone: red);", true),
    ("@forward 'a' as theme_* SHOW $tone, paint;", true),
    (
        "@forward 'a' HIDE $tone, paint WITH($tone: red !default);",
        true,
    ),
    ("@forward 'a' show paint, paint;", true),
    ("@forward 'a' as *;", false),
    ("@forward 'a' as theme- *;", false),
    ("@forward 'a' show;", false),
    ("@forward 'a' show paint,;", false),
    ("@forward 'a' show paint hide other;", false),
    ("@forward 'a' with($x: 1) as p-*;", false),
    ("@use 'a' with();", false),
    ("@use 'a' with($x: 1 !default);", false),
    ("@forward 'a' with($x: 1 !DEFAULT);", false),
    ("@forward 'a' with($x: 1 !de\\66 ault,);", true),
    ("@use 'a' with($_tone: 1, $-tone: 2);", false),
    ("@use 'a' with($tone: 1, $t\\6f ne: 2);", false),
    ("@use 'a' with($_private: 1,);", true),
];

fn module_cases() -> Vec<(String, SassSyntax)> {
    MODULE_CASES
        .iter()
        .flat_map(|(text, _)| {
            [
                (text.to_string(), SassSyntax::Scss),
                (
                    format!("{}\n", text.strip_suffix(';').unwrap()),
                    SassSyntax::Indented,
                ),
            ]
        })
        .collect()
}

#[test]
fn module_urls_namespaces_and_clauses_follow_the_pinned_grammar() {
    for ((text, syntax), (_, accepted)) in module_cases()
        .iter()
        .zip(MODULE_CASES.iter().flat_map(|c| [c, c]))
    {
        let source = source(text);
        let result = parse_sass(&source, *syntax, SassLimits::default());
        assert_eq!(
            result.is_ok(),
            *accepted,
            "{syntax:?}: {text:?}: {result:?}"
        );
        if let Ok(doc) = result {
            check(&doc);
            assert!(doc.unchecked_syntax().next().is_none(), "{text}");
        }
    }
}

#[test]
fn module_clause_nodes_preserve_spelling_and_boundaries() {
    let text = "\u{feff}@use '123' as *;\r\n@forward 'palette' as theme_* hide $tone, paint with($tone: red !default);";
    let input = source(text);
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let clauses: Vec<_> = doc
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.kind,
                SassNodeKind::ModuleNamespace { .. }
                    | SassNodeKind::ForwardPrefix
                    | SassNodeKind::ForwardVisibility { .. }
            )
        })
        .map(|node| {
            (
                node.kind.clone(),
                input.slice(node.span).unwrap(),
                node.children.len(),
            )
        })
        .collect();
    assert_eq!(
        clauses,
        [
            (SassNodeKind::ModuleNamespace { global: true }, "as *", 0),
            (SassNodeKind::ForwardPrefix, "as theme_*", 1),
            (
                SassNodeKind::ForwardVisibility { hidden: true },
                "hide $tone, paint",
                2
            ),
        ]
    );
    let input = source("@use 'palette' as p; @forward 'palette' show $tone;");
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    assert!(
        doc.nodes()
            .iter()
            .any(|n| n.kind == SassNodeKind::ModuleNamespace { global: false })
    );
    assert!(
        doc.nodes()
            .iter()
            .any(|n| n.kind == SassNodeKind::ForwardVisibility { hidden: false })
    );
}

fn generated_module_cases() -> Vec<(String, SassSyntax)> {
    let mut urls = vec!["".to_owned()];
    for c in (0u8..=127).map(char::from).chain(['色', '😀', '\u{80}']) {
        if matches!(c, '\'' | '\\' | '\r' | '\n' | '\u{c}') {
            continue;
        }
        for url in [
            format!("{c}"),
            format!("a{c}b"),
            format!("{c}:palette"),
            format!("http://a{c}b/palette"),
            format!("http://a:{c}/palette"),
        ] {
            urls.push(url);
        }
    }
    for url in [
        "a/.",
        "a/..",
        "a/../palette",
        "a/%2e",
        "a/%2E%2e",
        "a/%2e%2e/palette",
        "%41",
        "%00",
        "%2e",
        "%5c31%20palette",
        "%F0%9F%98%80",
        "%ff",
        "%",
        "%1",
        "%zz",
        "http://[::]/palette",
        "http://[v1.a]/palette",
        "http://[::ffff:192.000.2.1]/palette",
        "http://[::1%25eth0]/palette",
        "http://x:0x10/palette",
        "http://x:-1/palette",
        "http://x:+1/palette",
        "http://x:999999999999999999999/palette",
        "data:text/plain,palette",
        "data:;base64,YQ",
        "data:;base64,YQ=",
        "data:;base64,YQ===",
        "data:;base64,Y%51==",
        "data:;x=y,palette",
        "DATA:,palette",
        "data:,a/b?x",
        "data:,a/b#x",
        "data:,a/b%23x",
        "data:,a/b%3fx",
        "data:,a\\\\31 ",
        "data:;base64,abc_abcd",
        "data:;base64,abc%2Fabcd",
        "data:;base64,abc-abcd",
    ] {
        urls.push(url.to_string());
    }
    for host in [
        "[",
        "]",
        "[::]x",
        "[::1]",
        "[::1]:",
        "[::1]:0x10",
        "[::1]:-0x10",
        "[::1]:+",
        "[::1]: 1",
        "[::1%]",
        "[::1%25]",
        "[::1%25eth%zz]",
        "[::1%25eth%41]",
        "[v1.a]",
        "[V1.a]",
        "[v.ab]",
        "[v1.]",
        "[v1.a%20]",
        "[v1.a色]",
        "[::ffff:192.0.2.1]",
        "[::ffff:192.00.2.1]",
        "[1:2:3:4:5:6:7:8]",
        "[1:2:3:4:5:6:7:8::]",
        "::1",
        "::1:",
        "user@[::1]",
        "user@@host",
        "user[bad]@host",
        "host:0Xff",
        "host:-0x1",
        "host:1e2",
        "host:1.0",
        "host:0b10",
        "host:99999999999999999999999999999999999999999",
    ] {
        urls.push(format!("http://{host}/palette"));
    }
    for c in [
        '\u{85}', '\u{a0}', '\u{feff}', '\u{200b}', '\u{2028}', '\u{202f}',
    ] {
        urls.push(format!("http://x:{c}1{c}/palette"));
    }
    for mime in ["", "text/plain", "text", "/", "a/b/c", "a /b", "色/字"] {
        for param in [
            "",
            ";charset=UTF-8",
            ";base64",
            ";BASE64",
            ";x=y;base64",
            ";base64;x=y",
            ";x",
            ";=",
            ";x=y=z",
        ] {
            for payload in [
                "", "a", "YQ", "YQ=", "YQ==", "YQ===", "YQ====", "YWJj", "YWJj=", "Y%51==", "%zz",
                "?", "#", "palette", "a/b", "é",
            ] {
                urls.push(format!("data:{mime}{param},{payload}"));
            }
        }
    }
    let mut cases = Vec::new();
    for url in urls {
        for (directive, clause) in [("use", ""), ("use", " as *"), ("forward", "")] {
            for (syntax, end) in [(SassSyntax::Scss, ";"), (SassSyntax::Indented, "\n")] {
                cases.push((format!("@{directive} '{url}'{clause}{end}"), syntax));
            }
        }
    }
    for directive in ["use", "forward"] {
        for name in [
            "p", "_p", "--", "-1", "\\31 p", "\\0", "\\d800", "\\110000", "a\\20 b", "色", "😀",
        ] {
            for clause in [
                format!("as {name}{}", if directive == "forward" { "*" } else { "" }),
                format!("with(${name}: 1)"),
                format!("show ${name}"),
            ] {
                for (syntax, end) in [(SassSyntax::Scss, ";"), (SassSyntax::Indented, "\n")] {
                    cases.push((format!("@{directive} 'palette' {clause}{end}"), syntax));
                }
            }
        }
        for gap in [" ", "\n", "\r\n", "/* x */", "// x\n"] {
            for clause in [
                "as p",
                "AS p",
                "\\61s p",
                "as\\20 p",
                "with($x: 1)",
                "WITH($x: 1)",
                "w\\69th($x: 1)",
                "as p-*",
                "SHOW $tone, paint",
                "h\\69 de $tone",
                "with($x: 1 !default)",
            ] {
                for (syntax, end) in [(SassSyntax::Scss, ";"), (SassSyntax::Indented, "\n")] {
                    // After an indented module's newline, the same text is a
                    // separate style rule, whose grammar is a different test.
                    let after_url = if syntax == SassSyntax::Indented {
                        " "
                    } else {
                        gap
                    };
                    cases.push((
                        format!("@{directive}{gap}'palette'{after_url}{clause}{end}"),
                        syntax,
                    ));
                }
            }
        }
    }
    cases
}

#[test]
fn generated_module_rules_preserve_tree_and_error_contracts() {
    for (text, syntax) in generated_module_cases() {
        let input = source(&text);
        match parse_sass(&input, syntax, SassLimits::default()) {
            Ok(doc) => check(&doc),
            Err(error) => {
                input.validate_offset(error.offset).unwrap();
                assert_eq!(
                    parse_sass(&input, syntax, SassLimits::default()).unwrap_err(),
                    error
                );
            }
        }
    }
}

const MODULE_CONTEXT_CASES: &[(&str, SassSyntax, bool)] = &[
    ("@charsetfoo 'utf-8';", SassSyntax::Scss, false),
    ("@charset-foo 'utf-8';", SassSyntax::Scss, false),
    ("@charset色 'utf-8';", SassSyntax::Scss, false),
    ("@charset\\66 oo 'utf-8';", SassSyntax::Scss, false),
    (".a { @charsetfoo 'utf-8'; }", SassSyntax::Scss, true),
    ("@charsetfoo 'utf-8'\n", SassSyntax::Indented, false),
    ("@charset-foo 'utf-8'\n", SassSyntax::Indented, false),
    ("@charset色 'utf-8'\n", SassSyntax::Indented, false),
    ("@charset\\66 oo 'utf-8'\n", SassSyntax::Indented, false),
    (".a\n  @charsetfoo 'utf-8'\n", SassSyntax::Indented, true),
    ("@charset 'utf-8'; @use 'palette';", SassSyntax::Scss, true),
    (
        "$tone: red; @use 'palette' with($tone: $tone); @forward 'other';",
        SassSyntax::Scss,
        true,
    ),
    ("@use 'palette'; @use 'palette';", SassSyntax::Scss, true),
    (".a {} @use 'palette';", SassSyntax::Scss, false),
    (".a { @forward 'palette'; }", SassSyntax::Scss, false),
    ("@if true { @use 'palette'; }", SassSyntax::Scss, false),
    (
        "@charset 'utf-8'\n@use 'palette'\n",
        SassSyntax::Indented,
        true,
    ),
    (
        "@use 'palette' as p\n.a\n  color: p.$tone\n",
        SassSyntax::Indented,
        true,
    ),
    (".a\n  @use 'palette'\n", SassSyntax::Indented, false),
];

#[test]
fn module_rules_obey_root_ordering_and_resource_limits() {
    for &(text, syntax, accepted) in MODULE_CONTEXT_CASES {
        let input = source(text);
        let result = parse_sass(&input, syntax, SassLimits::default());
        assert_eq!(result.is_ok(), accepted, "{text:?}: {result:?}");
        if let Ok(doc) = result {
            check(&doc);
        }
    }
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let text = format!(
            "@forward 'https://example.invalid/palette' as p-* hide $tone with($tone: (a: (b: 1))){}",
            if syntax == SassSyntax::Scss {
                ";"
            } else {
                "\n"
            }
        );
        let input = source(&text);
        let doc = parse_sass(&input, syntax, SassLimits::default()).unwrap();
        let entries = doc.nodes().len() + doc.comments().len();
        for (limits, error) in [
            (
                SassLimits {
                    max_nodes: entries - 1,
                    ..SassLimits::default()
                },
                SassErrorKind::NodeLimit,
            ),
            (
                SassLimits {
                    max_depth: 2,
                    ..SassLimits::default()
                },
                SassErrorKind::DepthLimit,
            ),
            (
                SassLimits {
                    max_work: 100,
                    ..SassLimits::default()
                },
                SassErrorKind::WorkLimit,
            ),
        ] {
            assert_eq!(parse_sass(&input, syntax, limits).unwrap_err().kind, error);
        }
        assert!(
            parse_sass(
                &input,
                syntax,
                SassLimits {
                    max_nodes: entries,
                    ..SassLimits::default()
                }
            )
            .is_ok()
        );
    }
}

#[test]
fn unchecked_grammars_are_explicit_and_do_not_validate_the_file() {
    let source = source(
        "@import 'fictional' screen; @media #{$q} { .a { color: red; } } @page x { .#{$b} { x: y; } }",
    );
    let doc = parse_sass(&source, SassSyntax::Scss, SassLimits::default()).unwrap();
    let unchecked: Vec<_> = doc.unchecked_syntax().collect();
    let selectors: Vec<_> = unchecked
        .iter()
        .filter(|entry| entry.kind == SassUncheckedKind::Selector)
        .map(|entry| source.slice(entry.span).unwrap())
        .collect();
    assert_eq!(selectors, [".#{$b}"]);
    assert!(
        unchecked
            .iter()
            .any(|entry| entry.kind == SassUncheckedKind::MediaQuery)
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

/// Renders selector structure compactly; other nodes render as spelled.
fn selector_text(doc: &SassDocument<'_>, id: SassNodeId) -> String {
    let node = doc.node(id).unwrap();
    let parts: Vec<_> = node
        .children
        .iter()
        .map(|&child| selector_text(doc, child))
        .collect();
    let qualified = |namespace: SassNamespace, parts: &[String], name: &str| {
        let (prefix, rest) = match namespace {
            SassNamespace::Unspecified => (String::new(), parts),
            SassNamespace::Empty => ("|".to_owned(), parts),
            SassNamespace::Any => ("*|".to_owned(), parts),
            SassNamespace::Named => (format!("{}|", parts[0]), &parts[1..]),
        };
        let mut rest = rest.to_vec();
        if !name.is_empty() {
            rest.insert(0, name.to_owned());
        }
        format!("{prefix}{}", rest.join(" "))
    };
    let kind = &node.kind;
    if *kind == SassNodeKind::SelectorList {
        format!("[{}]", parts.join(", "))
    } else if *kind == SassNodeKind::KeyframeSelectorList {
        format!("keyframes[{}]", parts.join(", "))
    } else if *kind == SassNodeKind::ComplexSelector {
        parts.join(" ")
    } else if *kind == SassNodeKind::CompoundSelector {
        parts.concat()
    } else if *kind == SassNodeKind::ClassSelector {
        format!("class({})", parts[0])
    } else if *kind == SassNodeKind::IdSelector {
        format!("id({})", parts[0])
    } else if *kind == SassNodeKind::PlaceholderSelector {
        format!("placeholder({})", parts[0])
    } else if *kind == SassNodeKind::NestingSelector {
        format!("&({})", parts.concat())
    } else if let SassNodeKind::TypeSelector { namespace } = kind {
        format!("type({})", qualified(*namespace, &parts, ""))
    } else if let SassNodeKind::UniversalSelector { namespace } = kind {
        format!("universal({})", qualified(*namespace, &parts, "*"))
    } else if let SassNodeKind::AttributeSelector {
        namespace,
        operator,
    } = kind
    {
        let mut parts = parts.clone();
        let name = usize::from(*namespace == SassNamespace::Named);
        if let Some(operator) = operator {
            parts.insert(name + 1, format!("{operator:?}"));
        }
        format!("attribute({})", qualified(*namespace, &parts, ""))
    } else if let SassNodeKind::PseudoSelector { element } = kind {
        let colons = if *element { "::" } else { ":" };
        format!("{colons}({})", parts.join(" "))
    } else {
        doc.source().slice(node.span).unwrap().to_owned()
    }
}

/// Each selector of a style rule, rendered with whether it is unchecked.
fn rule_selectors(text: &str, syntax: SassSyntax) -> Vec<(String, bool)> {
    let input = source(text);
    let doc = parse_sass(&input, syntax, SassLimits::default())
        .unwrap_or_else(|error| panic!("{syntax:?}: {text:?}: {error}"));
    check(&doc);
    let unchecked: Vec<_> = doc
        .unchecked_syntax()
        .filter(|entry| entry.kind == SassUncheckedKind::Selector)
        .map(|entry| entry.span)
        .collect();
    let mut rules: Vec<_> = doc
        .nodes()
        .iter()
        .filter(|node| node.kind == SassNodeKind::StyleRule)
        .collect();
    rules.sort_by_key(|rule| rule.span.start());
    rules
        .into_iter()
        .map(|rule| {
            let selector = doc.node(rule.children[0]).unwrap();
            (
                selector_text(&doc, rule.children[0]),
                unchecked.contains(&selector.span),
            )
        })
        .collect()
}

const SELECTOR_STRUCTURES: &[(&str, &str)] = &[
    ("a", "[type(a)]"),
    ("a.b#c%d", "[type(a)class(b)id(c)placeholder(d)]"),
    (
        "a > b + c ~ d e",
        "[type(a) > type(b) + type(c) ~ type(d) type(e)]",
    ),
    ("> a", "[> type(a)]"),
    ("a >", "[type(a) >]"),
    ("a > + b", "[type(a) > + type(b)]"),
    ("a,b , c,,d,", "[type(a), type(b), type(c), type(d)]"),
    ("&", "[&()]"),
    ("&-suffix.b", "[&(-suffix)class(b)]"),
    ("&__1:hover", "[&(__1):(hover)]"),
    (
        "*, *|*, |*, ns|*",
        "[universal(*), universal(*|*), universal(|*), universal(ns|*)]",
    ),
    ("ns|a, *|a, |a", "[type(ns|a), type(*|a), type(|a)]"),
    ("[f]", "[attribute(f)]"),
    ("[ f = g ]", "[attribute(f Equal g)]"),
    ("[f~=\"g\"]", "[attribute(f Include \"g\")]"),
    ("[ns|f|=g i]", "[attribute(ns|f Dash g i)]"),
    (
        "[*|f^=g][|f$=g][f*='g' S]",
        "[attribute(*|f Prefix g)attribute(|f Suffix g)attribute(f Substring 'g' S)]",
    ),
    ("a:hover::before", "[type(a):(hover)::(before)]"),
    (":not(.a, b > c)", "[:(not [class(a), type(b) > type(c)])]"),
    (":-webkit-any(a)", "[:(-webkit-any [type(a)])]"),
    ("::slotted(a)", "[::(slotted [type(a)])]"),
    (
        ":is(:where(:has(> a)))",
        "[:(is [:(where [:(has [> type(a)])])])]",
    ),
    (":nth-child(2n + 1)", "[:(nth-child 2n + 1)]"),
    (
        ":nth-child( -n+3 of .a , b )",
        "[:(nth-child -n+3 of [class(a), type(b)])]",
    ),
    (":nth-last-child(odd)", "[:(nth-last-child odd)]"),
    (":NOT(a)", "[:(NOT a)]"),
    (
        ":lang(en, \"de\") ::part(x y)",
        "[:(lang en, \"de\") ::(part x y)]",
    ),
    (":foo(url(//x) [c])", "[:(foo url(//x) [c])]"),
    (".\\31 x.a\\:b", "[class(\\31 x)class(a\\:b)]"),
    (".--, .-a, ._", "[class(--), class(-a), class(_)]"),
    (".a/* comment */.b", "[class(a) class(b)]"),
    ("a // comment\n, b", "[type(a), type(b)]"),
    ("色.類", "[type(色)class(類)]"),
];

#[test]
fn selectors_have_the_evaluation_grammar_structure() {
    for &(selector, expected) in SELECTOR_STRUCTURES {
        let text = format!(".p {{ {selector} {{ x: y; }} }}");
        let selectors = rule_selectors(&text, SassSyntax::Scss);
        assert_eq!(selectors[1], (expected.to_owned(), false), "{selector}");
    }
    let selectors = rule_selectors(
        ".p\n  a,\n  b > c\n    x: y\n  d, // comment\n    e\n    x: y\n",
        SassSyntax::Indented,
    );
    assert_eq!(
        selectors,
        [
            ("[class(p)]".to_owned(), false),
            ("[type(a), type(b) > type(c)]".to_owned(), false),
            ("[type(d), type(e)]".to_owned(), false),
        ]
    );
}

#[test]
fn keyframe_selectors_use_their_own_grammar() {
    let selectors = rule_selectors(
        "@keyframes k { from, TO { x: y; } 50%, +.5e1% { x: y; } fr\\6fm { x: y; } } \
         @-webkit-keyframes k { 0% { x: y; } } @media x { from { x: y; } }",
        SassSyntax::Scss,
    );
    assert_eq!(
        selectors,
        [
            ("keyframes[from, TO]".to_owned(), false),
            ("keyframes[50%, +.5e1%]".to_owned(), false),
            ("keyframes[fr\\6fm]".to_owned(), false),
            ("keyframes[0%]".to_owned(), false),
            ("[type(from)]".to_owned(), false),
        ]
    );
    let selectors = rule_selectors("@keyframes k\n  50%\n    x: y\n", SassSyntax::Indented);
    assert_eq!(selectors, [("keyframes[50%]".to_owned(), false)]);
}

#[test]
fn evaluation_dependent_selectors_keep_unchecked_evidence() {
    let cases: &[(&str, &[(&str, bool)])] = &[
        (
            "@mixin m { from { x: y; } } @include m { a { x: y; } }",
            &[("[type(from)]", true), ("[type(a)]", true)],
        ),
        (
            "@#{$k} k { 50% { x: y; } } @keyframes k { #{$p}% { x: y; } }",
            &[("50% ", true), ("#{$p}% ", true)],
        ),
        (
            "@keyframes k { from { a { x: y; } } }",
            &[("keyframes[from]", false), ("[type(a)]", true)],
        ),
        (
            "@keyframes k { @at-root (without: all) { b { x: y; } } }",
            &[("[type(b)]", true)],
        ),
        // Evaluation rejects these, but the compiler's parse step keeps them.
        (
            ".p { a:nth-child(foo) { x: y; } b[c=d foo] { x: y; } e[f=g i ] { x: y; } }",
            &[
                ("[class(p)]", false),
                ("a:nth-child(foo) ", true),
                ("b[c=d foo] ", true),
                ("e[f=g i ] ", true),
            ],
        ),
        (
            "@keyframes k { a { x: y; } } .p { :foo(!) { x: y; } }",
            &[("a ", true), ("[class(p)]", false), (":foo(!) ", true)],
        ),
    ];
    for &(text, expected) in cases {
        let actual = rule_selectors(text, SassSyntax::Scss);
        let expected: Vec<_> = expected
            .iter()
            .map(|&(selector, unchecked)| (selector.to_owned(), unchecked))
            .collect();
        assert_eq!(actual, expected, "{text}");
    }
    let interpolated = rule_selectors(".a#{$b} > c { x: y; }", SassSyntax::Scss);
    assert_eq!(
        interpolated,
        [("[class(a#{$b}) > type(c)]".to_owned(), true)]
    );
    // Indented Sass removes comments first, which would join these names.
    assert_eq!(
        rule_selectors(".p\n  a/**/b\n    x: y\n", SassSyntax::Indented)[1],
        ("a/**/b".to_owned(), true)
    );
}

#[test]
fn at_root_selectors_are_style_rules() {
    let input =
        source(".p { @at-root .a > b { x: y; } @at-root (without: media) { c { x: y; } } }");
    let doc = parse_sass(&input, SassSyntax::Scss, SassLimits::default()).unwrap();
    check(&doc);
    let at_root: Vec<_> = doc
        .nodes()
        .iter()
        .filter(|node| node.kind == SassNodeKind::AtRule(SassAtRule::AtRoot))
        .collect();
    let inline = doc.node(at_root[0].children[1]).unwrap();
    assert_eq!(inline.kind, SassNodeKind::StyleRule);
    assert_eq!(
        selector_text(&doc, inline.children[0]),
        "[class(a) > type(b)]"
    );
    // A literal query is established and carries no evidence (`D-084`).
    assert_eq!(doc.unchecked_syntax().count(), 0);
    let query = doc.node(at_root[1].children[1]).unwrap();
    assert_eq!(query.kind, SassNodeKind::AtRootQuery);
    assert_eq!(input.slice(query.span).unwrap(), "(without: media)");
    accepts("@at-root a\n  x: y\n", SassSyntax::Indented);
    assert!(
        parse_sass(
            &source("@at-root ;"),
            SassSyntax::Scss,
            SassLimits::default()
        )
        .is_err()
    );
}

#[test]
fn selector_parsing_shares_the_parser_limits() {
    let deep = format!(
        ".p {{ {}a{} {{ x: y; }} }}",
        ":not(".repeat(200),
        ")".repeat(200)
    );
    assert_eq!(
        parse_sass(&source(&deep), SassSyntax::Scss, SassLimits::default())
            .unwrap_err()
            .kind,
        SassErrorKind::DepthLimit
    );
    let input = source(".p { a > b:not(.c) { x: y; } }");
    let defaults = SassLimits::default();
    let doc = parse_sass(&input, SassSyntax::Scss, defaults).unwrap();
    let count = doc.nodes().len() + doc.comments().len();
    assert_eq!(
        parse_sass(
            &input,
            SassSyntax::Scss,
            SassLimits {
                max_nodes: count,
                ..defaults
            }
        )
        .unwrap(),
        doc
    );
    assert_eq!(
        parse_sass(
            &input,
            SassSyntax::Scss,
            SassLimits {
                max_nodes: count - 1,
                ..defaults
            }
        )
        .unwrap_err()
        .kind,
        SassErrorKind::NodeLimit
    );
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
    cases.extend(
        MODULE_CONTEXT_CASES
            .iter()
            .map(|(text, syntax, _)| (text.to_string(), *syntax)),
    );
    cases.extend(module_cases());
    cases.extend(generated_module_cases());
    cases.extend(
        import_cases::cases()
            .into_iter()
            .map(|case| (case.text, case.syntax)),
    );
    cases.extend(condition_cases::cases());
    cases.extend(extend_cases::cases());
    cases.extend(if_cases::cases());
    cases.extend(css_function_cases::cases());
    cases.extend(url_cases::cases());
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
    let mut differences = Vec::new();
    for ((text, syntax), oracle) in cases.iter().zip(verdicts) {
        let source = source(text);
        let accepted = parse_sass(&source, *syntax, SassLimits::default()).is_ok();
        if accepted != (oracle == "1") {
            differences.push(format!(
                "{syntax:?}: {text:?}: ours={accepted}, oracle={oracle}"
            ));
        }
    }
    assert!(
        differences.is_empty(),
        "{} disagreements:\n{}",
        differences.len(),
        differences
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

const SELECTOR_PIECES: &[&str] = &[
    "a",
    "*",
    ".b",
    "#c",
    "%d",
    "&",
    "&-e",
    "&1",
    "[f]",
    "[f=g]",
    "[f~=\"h\"]",
    "[ns|f|=g i]",
    "[*|f]",
    "[|f^='x' s]",
    "[f=g foo]",
    "[f=g i ]",
    "[f=]",
    "ns|a",
    "*|*",
    "|a",
    ":hover",
    "::before",
    ":not(.b, c)",
    ":not()",
    ":is(a > b)",
    ":nth-child(2n + 1)",
    ":nth-child(2n+1 of .x)",
    ":nth-last-child( odd )",
    ":nth-child(foo)",
    ":nth-child(-n- 2)",
    ":lang(en)",
    "::slotted(a)",
    ":-webkit-any(a)",
    ":foo(!)",
    ":foo(a;b)",
    ":foo({a})",
    ":foo(url(//x{y}))",
    ".\\31 x",
    ".-",
    ".--",
    "a&",
    ".a/**/.b",
    "::part(x)",
    "&__x:hover",
    ".色",
];

const KEYFRAME_PIECES: &[&str] = &[
    "from",
    "to",
    "FROM",
    "To",
    "50%",
    "+5.5e2%",
    ".5%",
    "5.%",
    "1e3%",
    "1E-3%",
    "5e%",
    "-1%",
    "50",
    "from, 50%",
    "from,",
    "fr\\6fm",
    "t\\6f",
    "a",
    "50% ,to",
    "0%,100%",
    "1e+2%",
    "5 %",
    "from to",
    "%",
];

/// Frameshift's verdict: no document, an established selector grammar, or
/// a parsed document whose innermost style rule selector stays unchecked.
fn selector_verdict(text: &str, syntax: SassSyntax) -> Option<bool> {
    let input = source(text);
    let doc = parse_sass(&input, syntax, SassLimits::default()).ok()?;
    let rule = doc
        .nodes()
        .iter()
        .filter(|node| node.kind == SassNodeKind::StyleRule)
        .max_by_key(|rule| rule.span.start())?;
    let selector = doc.node(rule.children[0]).unwrap();
    let unchecked = doc
        .unchecked_syntax()
        .any(|entry| entry.kind == SassUncheckedKind::Selector && entry.span == selector.span);
    Some(
        !unchecked
            && matches!(
                selector.kind,
                SassNodeKind::SelectorList | SassNodeKind::KeyframeSelectorList
            ),
    )
}

/// Compiles generated fictional cases with pinned Sass 1.105.2 and returns
/// whether each compiles. The cases must contain no module or import rule,
/// so compiling them cannot read files.
fn compile_with_oracle(cases: &[(String, SassSyntax)]) -> Vec<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    for (text, _) in cases {
        assert!(
            !["@use", "@forward", "@import", "load-css"]
                .iter()
                .any(|rule| text.contains(rule))
        );
    }
    let module = std::env::var("FRAMESHIFT_SASS_PARSER_MODULE")
        .expect("path to external sass-parser package");
    let script = r#"
const fs = require('node:fs');
const path = require('node:path');
const main = require.resolve('sass', {paths: [process.argv[1]]});
const s = JSON.parse(fs.readFileSync(path.join(path.dirname(main), 'package.json')));
if (s.version !== '1.105.2') throw Error('unpinned Sass');
const sass = require(main);
const data = fs.readFileSync(0); let at = 0;
while (at < data.length) {
  let end = data.indexOf(10, at); const syntax = data.subarray(at, end).toString(); at = end + 1;
  end = data.indexOf(10, at); const size = Number(data.subarray(at, end).toString()); at = end + 1;
  const source = data.subarray(at, at + size).toString(); at += size;
  try { sass.compileString(source, {syntax, logger: sass.Logger.silent}); process.stdout.write('1\n'); }
  catch (e) { process.stdout.write('0\n'); }
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
    for (text, syntax) in cases {
        let syntax = if *syntax == SassSyntax::Scss {
            "scss"
        } else {
            "indented"
        };
        writeln!(input, "{syntax}\n{}", text.len()).unwrap();
        input.write_all(text.as_bytes()).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let verdicts = String::from_utf8(output.stdout).unwrap();
    let verdicts: Vec<_> = verdicts.lines().map(|line| line == "1").collect();
    assert_eq!(verdicts.len(), cases.len());
    verdicts
}

#[test]
#[ignore = "requires external pinned Sass 1.105.2"]
fn external_sass_selector_oracle() {
    let mut cases = Vec::new();
    for left in SELECTOR_PIECES {
        cases.push((format!(".p {{ {left} {{ x: y; }} }}"), SassSyntax::Scss));
        if !left.contains("/*") {
            cases.push((format!(".p\n  {left}\n    x: y\n"), SassSyntax::Indented));
        }
        for right in SELECTOR_PIECES {
            for combinator in [" ", " > ", "+", " ~ ", ", ", " > + "] {
                let selector = format!("{left}{combinator}{right}");
                cases.push((format!(".p {{ {selector} {{ x: y; }} }}"), SassSyntax::Scss));
            }
            if !left.contains("/*") && !right.contains("/*") {
                cases.push((
                    format!(".p\n  {left},\n  {right}\n    x: y\n"),
                    SassSyntax::Indented,
                ));
            }
        }
    }
    // Without a parent rule, evaluation rejects parent selector suffixes.
    for piece in KEYFRAME_PIECES
        .iter()
        .chain(SELECTOR_PIECES.iter().filter(|piece| !piece.contains('&')))
    {
        for rule in ["keyframes", "-webkit-keyframes", "KEYFRAMES"] {
            cases.push((
                format!("@{rule} k {{ {piece} {{ x: y; }} }}"),
                SassSyntax::Scss,
            ));
        }
        cases.push((
            format!("@keyframes k\n  {piece}\n    x: y\n"),
            SassSyntax::Indented,
        ));
    }
    let verdicts = compile_with_oracle(&cases);
    eprintln!(
        "Sass selector compile oracle: {} fictional cases",
        cases.len()
    );
    let mut mismatches = Vec::new();
    for ((text, syntax), oracle) in cases.iter().zip(verdicts) {
        let verdict = selector_verdict(text, *syntax);
        // A compiled selector must be established; one that fails to
        // compile must not be.
        if oracle != (verdict == Some(true)) {
            mismatches.push(format!("{syntax:?} {oracle} {verdict:?}: {text:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
#[ignore = "requires external pinned Sass 1.105.2"]
fn external_sass_condition_oracle() {
    let cases: Vec<_> = condition_cases::cases()
        .into_iter()
        .filter(|(text, _)| {
            text.starts_with(".p") && (text.contains("@media") || text.contains("@supports"))
        })
        .collect();
    // Defined variables keep evaluation errors out of the comparison; the
    // definitions are not part of the parsed cases.
    let compiled: Vec<_> = cases
        .iter()
        .map(|(text, syntax)| {
            let separator = if *syntax == SassSyntax::Scss {
                ";"
            } else {
                "\n"
            };
            let mut prefix = String::new();
            for (name, value) in [
                ("a", "a"),
                ("b", "b"),
                ("c", "\"(c: d)\""),
                ("e", "e"),
                ("q", "screen"),
                ("t", "print"),
                ("w", "1px"),
                ("x", "x"),
            ] {
                prefix.push_str(&format!("${name}: {value}{separator}"));
            }
            if *syntax == SassSyntax::Scss {
                prefix.push(' ');
            }
            (format!("{prefix}{text}"), *syntax)
        })
        .collect();
    let verdicts = compile_with_oracle(&compiled);
    let mut mismatches = Vec::new();
    let mut established = 0;
    for ((text, syntax), compiled) in cases.iter().zip(verdicts) {
        // A case without evidence must compile, and every compiled case
        // must be accepted.
        let verdict = condition_cases::media_verdict(text, *syntax);
        established += usize::from(verdict == Some(false));
        if (verdict == Some(false) && !compiled) || (compiled && verdict.is_none()) {
            mismatches.push(format!("{syntax:?} {compiled} {verdict:?}: {text:?}"));
        }
    }
    eprintln!(
        "Sass condition compile oracle: {} fictional cases, {established} without evidence",
        cases.len()
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
#[ignore = "requires external pinned Sass 1.105.2"]
fn external_sass_extend_oracle() {
    // Optional extensions compile without a matching target; defined
    // variables keep evaluation errors out of the comparison.
    let mut cases = Vec::new();
    for target in extend_cases::TARGETS {
        if !target.contains('{') {
            cases.push((
                format!("$x: x; .p {{ @extend {target} !optional; }}"),
                SassSyntax::Scss,
            ));
            cases.push((
                format!("$x: x\n.p\n  @extend {target} !optional\n"),
                SassSyntax::Indented,
            ));
        }
    }
    let queries: Vec<_> = extend_cases::cases()
        .into_iter()
        // Style rules inside keyframes fail evaluation whatever the query.
        .filter(|(text, _)| {
            text.contains("@at-root") && !text.contains("@extend") && !text.contains("@keyframes")
        })
        .map(|(text, syntax)| {
            let prefix = if syntax == SassSyntax::Scss {
                "$m: media; $w: with; "
            } else {
                "$m: media\n$w: with\n"
            };
            (format!("{prefix}{text}"), syntax)
        })
        .collect();
    let extend_count = cases.len();
    cases.extend(queries);
    let verdicts = compile_with_oracle(&cases);
    let mut mismatches = Vec::new();
    let mut established = 0;
    for (index, ((text, syntax), compiled)) in cases.iter().zip(verdicts).enumerate() {
        let verdict = if index < extend_count {
            extend_cases::extend_verdict(text, *syntax)
        } else {
            extend_cases::query_verdict(text, *syntax)
        };
        // A case without evidence must compile, and every compiled case
        // must be accepted.
        established += usize::from(verdict == Some(false));
        if (verdict == Some(false) && !compiled) || (compiled && verdict.is_none()) {
            mismatches.push(format!("{syntax:?} {compiled} {verdict:?}: {text:?}"));
        }
    }
    eprintln!(
        "Sass extend and at-root compile oracle: {} fictional cases, {established} without evidence",
        cases.len()
    );
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
