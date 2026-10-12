//! Expression parsing contracts through the public facade, using fictional text.

use frameshift_engine::{
    BinaryOperator, LexicalErrorKind, LexicalLimits, NodeId, ParseError, ParseErrorKind,
    ParseLimits, RegExpFlag, SourceText, SyntaxNodeKind, TemplateKind, TemplatePart, TextLimits,
    parse_js_expression,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn failure(source: &SourceText, offset: usize, limits: ParseLimits) -> ParseError {
    match parse_js_expression(source, offset, limits) {
        Ok(_) => panic!(
            "expected expression parsing to fail for {:?} at {offset}",
            source.as_str()
        ),
        Err(error) => error,
    }
}

fn text<'a>(
    source: &'a SourceText,
    tree: &frameshift_engine::SyntaxTree<'_>,
    id: NodeId,
) -> &'a str {
    source.slice(tree.node(id).unwrap().span()).unwrap()
}

#[test]
fn arena_nodes_preserve_precedence_spans_and_comments() {
    let source_text = "/* lead */ answer + 2 * (scale - 1) // tail";
    let input = source(source_text);
    let tree = parse_js_expression(&input, 0, ParseLimits::default()).unwrap();
    assert_eq!(tree.nodes().len(), 8);
    assert_eq!(tree.comments().len(), 2);
    assert_eq!(input.slice(tree.consumed_span()).unwrap(), source_text);
    assert_eq!(text(&input, &tree, tree.root()), "answer + 2 * (scale - 1)");

    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = tree.node(tree.root()).unwrap().kind()
    else {
        panic!("expected addition at the root");
    };
    assert_eq!(text(&input, &tree, left), "answer");
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Multiply,
        right: grouped,
        ..
    } = tree.node(right).unwrap().kind()
    else {
        panic!("expected multiplication on the right");
    };
    let SyntaxNodeKind::Parenthesized { expression } = tree.node(grouped).unwrap().kind() else {
        panic!("expected a retained parenthesized node");
    };
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Subtract,
        ..
    } = tree.node(expression).unwrap().kind()
    else {
        panic!("expected subtraction inside the parentheses");
    };
}

#[test]
fn exponentiation_is_right_associative_and_division_is_context_selected() {
    let input = source("a + b * c ** d ** e");
    let tree = parse_js_expression(&input, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Add,
        right,
        ..
    } = tree.node(tree.root()).unwrap().kind()
    else {
        panic!("expected addition");
    };
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Multiply,
        right,
        ..
    } = tree.node(right).unwrap().kind()
    else {
        panic!("expected multiplication");
    };
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Exponent,
        right,
        ..
    } = tree.node(right).unwrap().kind()
    else {
        panic!("expected exponentiation");
    };
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Exponent,
        ..
    } = tree.node(right).unwrap().kind()
    else {
        panic!("expected a right-associated exponent");
    };
    assert_eq!(text(&input, &tree, tree.root()), "a + b * c ** d ** e");

    let unary_right = source("2 ** -2");
    assert!(parse_js_expression(&unary_right, 0, ParseLimits::default()).is_ok());

    let relational = source("a == b in c");
    let relational_tree = parse_js_expression(&relational, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Equal,
        right,
        ..
    } = relational_tree.node(relational_tree.root()).unwrap().kind()
    else {
        panic!("expected equality at the root");
    };
    assert!(matches!(
        relational_tree.node(right).unwrap().kind(),
        SyntaxNodeKind::Binary {
            operator: BinaryOperator::In,
            ..
        }
    ));

    let division_input = source("42 / 6 / 2");
    let division = parse_js_expression(&division_input, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Divide,
        left,
        ..
    } = division.node(division.root()).unwrap().kind()
    else {
        panic!("expected division at the root");
    };
    assert!(matches!(
        division.node(left).unwrap().kind(),
        SyntaxNodeKind::Binary {
            operator: BinaryOperator::Divide,
            ..
        }
    ));

    let regexp_input = source("/a+/gu + 1");
    let regexp = parse_js_expression(&regexp_input, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::Binary { left, .. } = regexp.node(regexp.root()).unwrap().kind() else {
        panic!("expected a binary expression");
    };
    let SyntaxNodeKind::RegularExpression { flags, .. } = regexp.node(left).unwrap().kind() else {
        panic!("expected a validated regular-expression literal");
    };
    assert!(flags.contains(RegExpFlag::Global));
    assert!(flags.contains(RegExpFlag::Unicode));
}

#[test]
fn calls_optional_members_and_computed_members_use_arena_child_ranges() {
    let input = source("getRecord()?.items[index](first, second,)");
    let tree = parse_js_expression(&input, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::Call {
        callee,
        arguments,
        optional: false,
    } = tree.node(tree.root()).unwrap().kind()
    else {
        panic!("expected a call expression");
    };
    let args = tree.children(arguments).unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(text(&input, &tree, args[0]), "first");
    assert_eq!(text(&input, &tree, args[1]), "second");

    let SyntaxNodeKind::Member {
        object,
        computed: true,
        optional: false,
        ..
    } = tree.node(callee).unwrap().kind()
    else {
        panic!("expected a computed member access");
    };
    assert_eq!(text(&input, &tree, object), "getRecord()?.items");
    let SyntaxNodeKind::Member { optional: true, .. } = tree.node(object).unwrap().kind() else {
        panic!("expected an optional member access");
    };
}

#[test]
fn tagged_templates_keep_segments_and_substitutions_in_source_order() {
    let input = source("tag`head${value + 1}tail`");
    let tree = parse_js_expression(&input, 0, ParseLimits::default()).unwrap();
    let SyntaxNodeKind::TaggedTemplate { tag, template } = tree.node(tree.root()).unwrap().kind()
    else {
        panic!("expected a tagged template");
    };
    assert_eq!(text(&input, &tree, tag), "tag");
    let SyntaxNodeKind::TemplateLiteral { parts } = tree.node(template).unwrap().kind() else {
        panic!("expected a template literal");
    };
    let parts = tree.template_parts(parts).unwrap();
    assert_eq!(parts.len(), 3);
    let TemplatePart::Segment(head) = parts[0] else {
        panic!("expected the template head");
    };
    assert_eq!(head.kind, TemplateKind::Head);
    assert_eq!(input.slice(head.content_span).unwrap(), "head");
    let TemplatePart::Expression(expression) = parts[1] else {
        panic!("expected a substitution expression");
    };
    assert_eq!(text(&input, &tree, expression), "value + 1");
    let TemplatePart::Segment(tail) = parts[2] else {
        panic!("expected the template tail");
    };
    assert_eq!(tail.kind, TemplateKind::Tail);
    assert_eq!(input.slice(tail.content_span).unwrap(), "tail");
}

#[test]
fn unary_exponent_and_nullish_grammar_errors_fail_closed() {
    for (source_text, offset, kind) in [
        ("-2 ** 2", 3, ParseErrorKind::InvalidExponentiation),
        ("a ?? b || c", 2, ParseErrorKind::MixedNullishAndLogical),
        ("a && b ?? c", 7, ParseErrorKind::MixedNullishAndLogical),
        ("if", 0, ParseErrorKind::ReservedWordRequiresContext),
        (r"\u0069f", 0, ParseErrorKind::EscapedReservedWord),
        (
            "await value",
            0,
            ParseErrorKind::ContextualKeywordRequiresContext,
        ),
        ("f({ value() {} })", 9, ParseErrorKind::UnsupportedSyntax),
    ] {
        let input = source(source_text);
        let error = failure(&input, 0, ParseLimits::default());
        assert_eq!((error.offset, error.kind), (offset, kind), "{source_text}");
    }
    let input = source("a ?? (b || c)");
    let tree = parse_js_expression(&input, 0, ParseLimits::default()).unwrap();
    assert_eq!(input.slice(tree.consumed_span()).unwrap(), "a ?? (b || c)");
}

#[test]
fn optional_chains_cannot_become_tagged_templates_through_member_or_call_suffixes() {
    for source_text in ["tag?.part.next`x`", "tag?.part().next`x`"] {
        let input = source(source_text);
        let error = failure(&input, 0, ParseLimits::default());
        assert_eq!(
            error.kind,
            ParseErrorKind::OptionalChainTag,
            "{source_text}"
        );
        assert_eq!(error.offset, source_text.find('`').unwrap());
    }
}

#[test]
fn node_token_nesting_and_comment_limits_are_explicit() {
    let input = source("1 + 2");
    let error = failure(
        &input,
        0,
        ParseLimits {
            max_nodes: 1,
            ..ParseLimits::default()
        },
    );
    assert_eq!(
        (error.offset, error.kind),
        (4, ParseErrorKind::ResourceLimit)
    );

    let error = failure(
        &input,
        0,
        ParseLimits {
            max_tokens: 1,
            ..ParseLimits::default()
        },
    );
    assert_eq!(
        (error.offset, error.kind),
        (2, ParseErrorKind::ResourceLimit)
    );

    let nested = source("((1))");
    let error = failure(
        &nested,
        0,
        ParseLimits {
            max_nesting_depth: 1,
            ..ParseLimits::default()
        },
    );
    assert_eq!(
        (error.offset, error.kind),
        (1, ParseErrorKind::ResourceLimit)
    );

    let commented = source("/* one */ 1");
    let error = failure(
        &commented,
        0,
        ParseLimits {
            lexical: LexicalLimits {
                max_comments: 0,
                ..LexicalLimits::default()
            },
            ..ParseLimits::default()
        },
    );
    assert_eq!(
        (error.offset, error.kind),
        (0, ParseErrorKind::Lexical(LexicalErrorKind::CommentLimit))
    );
}

#[test]
fn trailing_tokens_and_invalid_source_offsets_do_not_return_partial_trees() {
    let input = source("1;");
    let error = failure(&input, 0, ParseLimits::default());
    assert_eq!(
        (error.offset, error.kind),
        (1, ParseErrorKind::TrailingInput)
    );

    let input = source("");
    let error = failure(&input, 0, ParseLimits::default());
    assert_eq!(
        (error.offset, error.kind),
        (0, ParseErrorKind::ExpectedExpression)
    );

    let input = source("é");
    let error = failure(&input, 1, ParseLimits::default());
    assert!(matches!(
        error.kind,
        ParseErrorKind::Lexical(LexicalErrorKind::InvalidOffset(_))
    ));
}
