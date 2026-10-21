//! Generic CSS parser behavior and resource boundaries (`D-078`).

use frameshift_engine::foundation::{SourceText, Span, TextLimits};
use frameshift_engine::syntax::{
    CssComponentId, CssDeclaration, CssDiagnosticKind, CssDocument, CssLimits, CssNodeKind,
    CssParserError, CssParserLimits, CssRule, CssTokenKind, css_token_value, parse_css,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn parse(text: &str) -> (SourceText, CssDocument) {
    let source = source(text);
    let doc = parse_css(&source, CssLimits::default(), CssParserLimits::default()).unwrap();
    assert_eq!(
        parse_css(&source, CssLimits::default(), CssParserLimits::default()).as_ref(),
        Ok(&doc)
    );
    for component in doc.components() {
        source.slice(component.span).unwrap();
        assert!(component.span.start() <= component.token.span.start());
        assert!(component.token.span.end() <= component.span.end());
        let mut end = component.token.span.end();
        for &id in &component.values {
            let child = doc.component(id).unwrap();
            assert!(child.span.start() >= end);
            assert!(child.span.end() <= component.span.end());
            end = child.span.end();
        }
        if let Some(close) = component.closing {
            assert!(close.span.start() >= end);
            assert_eq!(close.span.end(), component.span.end());
        }
    }
    for node in doc.nodes() {
        source.slice(node.span).unwrap();
        if let CssNodeKind::Rule(rule) = &node.kind {
            for &id in &rule.children {
                let child = doc.node(id).unwrap();
                assert!(child.span.start() >= node.span.start());
                assert!(child.span.end() <= node.span.end());
            }
        }
    }
    assert!(
        doc.diagnostics()
            .windows(2)
            .all(|pair| pair[0].offset <= pair[1].offset)
    );
    for error in doc.diagnostics() {
        source.validate_offset(error.offset).unwrap();
    }
    (source, doc)
}

fn rule(doc: &CssDocument, index: usize) -> &CssRule {
    let CssNodeKind::Rule(rule) = &doc.node(doc.rules()[index]).unwrap().kind else {
        panic!("not a rule")
    };
    rule
}

fn declaration<'a>(doc: &'a CssDocument, rule: &CssRule, index: usize) -> &'a CssDeclaration {
    let CssNodeKind::Declaration(decl) = &doc.node(rule.children[index]).unwrap().kind else {
        panic!("not a declaration")
    };
    decl
}

fn text(source: &SourceText, doc: &CssDocument, values: &[CssComponentId]) -> String {
    values
        .iter()
        .map(|&id| source.slice(doc.component(id).unwrap().span).unwrap())
        .collect()
}

#[test]
fn nested_rules_at_rules_and_ordered_declarations() {
    let (source, doc) = parse(
        "\u{feff}/*a*/<!-- @import 'theme.css'; .card { color: red; &:hover { color: blue } color: green; @media (width > 1px) { & .icon {opacity: .5} } } -->",
    );
    assert!(!doc.has_errors());
    assert!(doc.tokens().has_bom());
    assert_eq!(doc.tokens().comments().len(), 1);
    assert_eq!(doc.rules().len(), 2);
    assert_eq!(
        css_token_value(&source, &rule(&doc, 0).name.unwrap())
            .unwrap()
            .as_deref(),
        Some("import")
    );
    let outer = rule(&doc, 1);
    assert_eq!(outer.children.len(), 4);
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 0).value),
        "red"
    );
    assert!(matches!(
        doc.node(outer.children[1]).unwrap().kind,
        CssNodeKind::Rule(_)
    ));
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 2).value),
        "green"
    );
    assert_eq!(
        source
            .slice(doc.node(doc.rules()[0]).unwrap().span)
            .unwrap(),
        "@import 'theme.css';"
    );
}

#[test]
fn generic_disambiguation_and_custom_property_blocks() {
    let (source, doc) = parse(
        "x { a:hover { color:red } foo: {} !important; --theme: before { arbitrary: data; } after; --empty:; unknown:made-up; }",
    );
    assert!(!doc.has_errors());
    let outer = rule(&doc, 0);
    assert_eq!(outer.children.len(), 5);
    assert!(matches!(
        doc.node(outer.children[0]).unwrap().kind,
        CssNodeKind::Rule(_)
    ));
    let block = declaration(&doc, outer, 1);
    assert!(block.important);
    assert_eq!(text(&source, &doc, &block.value), "{}");
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 2).value),
        "before { arbitrary: data; } after"
    );
    assert!(declaration(&doc, outer, 3).value.is_empty());
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 4).value),
        "made-up"
    );
}

#[test]
fn priority_is_case_insensitive_and_decoded_but_not_nested() {
    let (source, doc) = parse(
        r"x { COLOR: red ! /*p*/ \69MPORTANT; --x: f(!important); a: !important; b: f(red!important) }",
    );
    assert!(!doc.has_errors());
    let outer = rule(&doc, 0);
    let first = declaration(&doc, outer, 0);
    assert!(first.important);
    assert_eq!(source.slice(first.name.span).unwrap(), "COLOR");
    assert_eq!(text(&source, &doc, &first.value), "red");
    assert!(!declaration(&doc, outer, 1).important);
    assert!(declaration(&doc, outer, 2).important);
    assert!(declaration(&doc, outer, 2).value.is_empty());
    assert!(!declaration(&doc, outer, 3).important);
    assert_eq!(doc.tokens().comments().len(), 1);
}

#[test]
fn functions_blocks_and_eof_closure_keep_original_spans() {
    let (source, doc) = parse("x { a: f([x], (y), {z}); b: f([q");
    assert!(!doc.has_errors());
    let outer = rule(&doc, 0);
    let first = declaration(&doc, outer, 0);
    let f = doc.component(first.value[0]).unwrap();
    assert_eq!(f.token.kind, CssTokenKind::Function);
    assert!(f.closing.is_some());
    assert_eq!(source.slice(f.span).unwrap(), "f([x], (y), {z})");
    let last = declaration(&doc, outer, 1);
    let f = doc.component(last.value[0]).unwrap();
    assert_eq!(source.slice(f.span).unwrap(), "f([q");
    assert!(f.closing.is_none());
    assert!(
        doc.component(outer.block.unwrap())
            .unwrap()
            .closing
            .is_none()
    );
}

#[test]
fn unicode_range_descriptor_uses_its_own_tokenization_context() {
    let (source, doc) =
        parse(r"@font-face { \75nicode-range: U+0025-00FF, U+4?? !important; other: U+4??; }");
    assert!(!doc.has_errors());
    let outer = rule(&doc, 0);
    let decl = declaration(&doc, outer, 0);
    assert!(decl.important);
    assert_eq!(text(&source, &doc, &decl.value), "U+0025-00FF, U+4??");
    assert_eq!(
        doc.component(decl.value[0]).unwrap().token.kind,
        CssTokenKind::UnicodeRange {
            start: 0x25,
            end: 0xff
        }
    );
    assert_eq!(
        css_token_value(&source, &doc.component(decl.value[0]).unwrap().token),
        Ok(None)
    );
    assert!(
        !declaration(&doc, outer, 1).value.iter().any(|&id| matches!(
            doc.component(id).unwrap().token.kind,
            CssTokenKind::UnicodeRange { .. }
        ))
    );
}

#[test]
fn recovery_keeps_later_siblings_and_blocks_whole_file_transformation() {
    let (source, doc) =
        parse("x { broken; color:red; also broken; @unknown something; a:blue; } tail");
    assert!(doc.has_errors());
    assert_eq!(doc.diagnostics().len(), 3);
    let outer = rule(&doc, 0);
    assert_eq!(outer.children.len(), 3);
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 0).value),
        "red"
    );
    assert_eq!(
        text(&source, &doc, &declaration(&doc, outer, 2).value),
        "blue"
    );
    assert_eq!(doc.diagnostics().last().unwrap().offset, source.len());
    // Invalid custom-property-looking top-level rules cannot create a rule.
    let (_, doc) = parse("--x:hover {a:b} y { c:d }");
    assert!(doc.has_errors());
    assert_eq!(doc.rules().len(), 1);
}

#[test]
fn lexical_errors_unmatched_closers_and_bad_priorities_are_visible() {
    let (_, doc) = parse("x { a: ]; b: red !no; c: 'bad\n; d: blue; }");
    assert!(doc.has_errors());
    assert!(
        doc.diagnostics()
            .iter()
            .any(|error| error.kind == CssDiagnosticKind::UnexpectedClosing)
    );
    assert!(
        doc.diagnostics()
            .iter()
            .any(|error| error.kind == CssDiagnosticKind::InvalidDeclarationValue)
    );
    assert!(
        doc.diagnostics()
            .iter()
            .any(|error| matches!(error.kind, CssDiagnosticKind::Tokenization(_)))
    );
    assert_eq!(rule(&doc, 0).children.len(), 4);
}

#[test]
fn node_diagnostic_token_and_work_limits_are_inclusive() {
    let (source, doc) = parse("x{a:b}");
    let count = doc.nodes().len() + doc.components().len();
    let limits = CssParserLimits {
        max_nodes: count,
        ..CssParserLimits::default()
    };
    assert_eq!(
        parse_css(&source, CssLimits::default(), limits).unwrap(),
        doc
    );
    assert_eq!(
        parse_css(
            &source,
            CssLimits::default(),
            CssParserLimits {
                max_nodes: count - 1,
                ..limits
            }
        ),
        Err(CssParserError::NodeLimit)
    );
    assert!(matches!(
        parse_css(&source, CssLimits { max_tokens: 0 }, limits),
        Err(CssParserError::Tokenization(_))
    ));
    let source = super_source("x {bad; bad;}");
    let limits = CssParserLimits {
        max_diagnostics: 2,
        ..CssParserLimits::default()
    };
    assert_eq!(
        parse_css(&source, CssLimits::default(), limits)
            .unwrap()
            .diagnostics()
            .len(),
        2
    );
    assert_eq!(
        parse_css(
            &source,
            CssLimits::default(),
            CssParserLimits {
                max_diagnostics: 1,
                ..limits
            }
        ),
        Err(CssParserError::DiagnosticLimit)
    );
    let source = super_source(" ");
    assert!(
        parse_css(
            &source,
            CssLimits::default(),
            CssParserLimits {
                max_work: 2,
                ..limits
            }
        )
        .is_ok()
    );
    assert_eq!(
        parse_css(
            &source,
            CssLimits::default(),
            CssParserLimits {
                max_work: 1,
                ..limits
            }
        ),
        Err(CssParserError::WorkLimit)
    );
    assert!(
        parse_css(
            &super_source(""),
            CssLimits { max_tokens: 0 },
            CssParserLimits {
                max_work: 0,
                max_nodes: 0,
                max_depth: 0,
                max_diagnostics: 0
            }
        )
        .is_ok()
    );
}

fn super_source(text: &str) -> SourceText {
    source(text)
}

#[test]
fn depth_limit_and_implementation_ceiling_prevent_stack_exhaustion() {
    for count in [1, 16, 128] {
        let text = format!("{}x{}", "(".repeat(count), ")".repeat(count));
        let source = source(&text);
        let limits = CssParserLimits {
            max_depth: count,
            ..CssParserLimits::default()
        };
        assert!(parse_css(&source, CssLimits::default(), limits).is_ok());
        assert_eq!(
            parse_css(
                &source,
                CssLimits::default(),
                CssParserLimits {
                    max_depth: count - 1,
                    ..limits
                }
            ),
            Err(CssParserError::DepthLimit)
        );
    }
    let source = source(&format!("{}x", "(".repeat(129)));
    assert_eq!(
        parse_css(
            &source,
            CssLimits::default(),
            CssParserLimits {
                max_depth: usize::MAX,
                ..CssParserLimits::default()
            }
        ),
        Err(CssParserError::DepthLimit)
    );
    // Rule nesting uses the same bound as components; destruction stays flat.
    let (_, doc) = parse(&format!("{}a:b{}", "x{".repeat(128), "}".repeat(128)));
    assert!(!doc.has_errors());
}

#[test]
fn repeated_declaration_ambiguity_has_linear_structural_work() {
    let text = format!("x {{{}}}", "a:hover {} ".repeat(5000));
    let source = source(&text);
    let tokens = frameshift_engine::syntax::tokenize_css(&source, CssLimits::default()).unwrap();
    let doc = parse_css(
        &source,
        CssLimits::default(),
        CssParserLimits {
            max_work: tokens.tokens().len() * 8,
            ..CssParserLimits::default()
        },
    )
    .unwrap();
    assert!(!doc.has_errors());
    assert_eq!(rule(&doc, 0).children.len(), 5000);
}

#[test]
fn comments_unicode_and_crlf_preserve_byte_coordinates() {
    let (source, doc) = parse("/*💡*/.雪\r\n{ /*note*/ --🌲: [é]; }");
    assert!(!doc.has_errors());
    assert_eq!(doc.tokens().comments().len(), 2);
    let decl = declaration(&doc, rule(&doc, 0), 0);
    assert_eq!(source.slice(decl.name.span).unwrap(), "--🌲");
    assert_eq!(text(&source, &doc, &decl.value), "[é]");
    assert_eq!(
        doc.node(doc.rules()[0]).unwrap().span,
        Span::new(8, source.len()).unwrap()
    );
}

#[test]
fn unicode_range_rescanning_stops_at_the_original_value_boundary() {
    let (source, doc) = parse("{unicode-range:U+4URL(>')[');");
    assert!(doc.has_errors());
    let decl = declaration(&doc, rule(&doc, 0), 0);
    assert_eq!(text(&source, &doc, &decl.value), "U+4URL(>')[')");
    // Descriptor rescanning can change token boundaries, but cannot read
    // the semicolon or any later declaration into the descriptor value.
    let end = source.len() - 1;
    for &id in &decl.value {
        assert!(doc.component(id).unwrap().span.end() <= end);
    }
}
