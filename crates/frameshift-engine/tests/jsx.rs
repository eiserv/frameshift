//! JSX scanner and parser contracts through the public facade, using fictional text.

use frameshift_engine::{
    CommentKind, Edit, EditLimits, EditOrder, JsDialect, LexicalError, LexicalErrorKind,
    LexicalLimits, NodeId, ParseError, ParseErrorKind, ParseLimits, Punctuator, SourceGoal,
    SourceText, Span, SyntaxNodeKind, SyntaxTree, TextLimits, apply_edits, parse_js_expression,
    parse_js_program, scan_jsx_identifier, scan_jsx_string, scan_jsx_text,
};

const SCRIPT: SourceGoal = SourceGoal::Script;
const MODULE: SourceGoal = SourceGoal::Module;

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn lexical(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

fn span(start: usize, end: usize) -> Span {
    Span::new(start, end).unwrap()
}

fn parse(goal: SourceGoal, input: &SourceText) -> SyntaxTree<'_> {
    match parse_js_program(input, goal, JsDialect::Jsx, ParseLimits::default()) {
        Ok(tree) => {
            assert_tree_invariants(input, goal, &tree);
            tree
        }
        Err(error) => panic!(
            "expected {:?} to parse as {goal:?}: {error}",
            input.as_str()
        ),
    }
}

fn failure(goal: SourceGoal, text: &str) -> ParseError {
    let input = source(text);
    match parse_js_program(&input, goal, JsDialect::Jsx, ParseLimits::default()) {
        Ok(_) => panic!("expected {text:?} to fail as {goal:?}"),
        Err(error) => error,
    }
}

fn kind(tree: &SyntaxTree<'_>, id: NodeId) -> SyntaxNodeKind {
    tree.node(id).unwrap().kind()
}

fn text<'a>(input: &'a SourceText, tree: &SyntaxTree<'_>, id: NodeId) -> &'a str {
    input.slice(tree.node(id).unwrap().span()).unwrap()
}

fn children(tree: &SyntaxTree<'_>, id: NodeId) -> Vec<NodeId> {
    tree.child_nodes(id).unwrap().collect()
}

// The first JSX element or fragment in source order.
fn element(tree: &SyntaxTree<'_>) -> NodeId {
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        if matches!(
            kind(tree, id),
            SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
        ) {
            return id;
        }
        stack.extend(children(tree, id).into_iter().rev());
    }
    panic!("expected a JSX element or fragment");
}

// Every node is reachable exactly once, child spans nest in source order,
// and a second parse yields an identical arena.
fn assert_tree_invariants(input: &SourceText, goal: SourceGoal, tree: &SyntaxTree<'_>) {
    let mut seen = vec![false; tree.nodes().len()];
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        assert!(!seen[id.index()], "node {id:?} is shared");
        seen[id.index()] = true;
        let node = tree.node(id).unwrap();
        input.slice(node.span()).unwrap();
        let mut previous_end = node.span().start();
        for child in tree.child_nodes(id).unwrap() {
            let span = tree.node(child).unwrap().span();
            assert!(
                previous_end <= span.start() && span.end() <= node.span().end(),
                "child span {span:?} is outside or before its sibling in {:?}",
                input.as_str()
            );
            previous_end = span.end();
            stack.push(child);
        }
    }
    assert!(seen.iter().all(|&reached| reached), "unreachable node");
    let again = parse_js_program(input, goal, JsDialect::Jsx, ParseLimits::default()).unwrap();
    assert_eq!(again.nodes(), tree.nodes());
    assert_eq!(again.comments(), tree.comments());
}

#[test]
fn jsx_names_accept_hyphens_unicode_and_reserved_words() {
    let limits = LexicalLimits::default();
    let astral = char::from_u32(0x1d465).unwrap();
    for (text, end) in [
        ("div>", 3),
        ("data-id=", 7),
        ("aria-", 5),
        ("a--b c", 4),
        ("class/", 5),
        ("$_1:", 3),
        ("\u{e9}t\u{e9}.", 5),
        (&format!("{astral}y "), 5),
    ] {
        assert_eq!(
            scan_jsx_identifier(&source(text), 0, limits),
            Ok(span(0, end)),
            "{text:?}"
        );
    }
    let backslash = '\\';
    for (text, error) in [
        ("-a", lexical(0, LexicalErrorKind::ExpectedIdentifier)),
        ("1a", lexical(0, LexicalErrorKind::ExpectedIdentifier)),
        ("", lexical(0, LexicalErrorKind::ExpectedIdentifier)),
        (
            &format!("a{backslash}u0062"),
            lexical(1, LexicalErrorKind::InvalidIdentifierEscape),
        ),
        (
            &format!("{backslash}u0061"),
            lexical(0, LexicalErrorKind::InvalidIdentifierEscape),
        ),
    ] {
        assert_eq!(
            scan_jsx_identifier(&source(text), 0, limits),
            Err(error),
            "{text:?}"
        );
    }
    let small = LexicalLimits {
        max_identifier_bytes: 3,
        ..limits
    };
    assert_eq!(
        scan_jsx_identifier(&source("abcd"), 0, small),
        Err(lexical(3, LexicalErrorKind::IdentifierLimit))
    );
    assert_eq!(
        scan_jsx_identifier(&source("ab\u{e9}"), 0, small),
        Err(lexical(2, LexicalErrorKind::IdentifierLimit))
    );
    assert_eq!(
        scan_jsx_identifier(&source("abc"), 0, small),
        Ok(span(0, 3))
    );
    assert!(matches!(
        scan_jsx_identifier(&source("\u{e9}"), 1, limits),
        Err(LexicalError {
            offset: 1,
            kind: LexicalErrorKind::InvalidOffset(_)
        })
    ));
}

#[test]
fn jsx_strings_have_no_escapes_and_may_span_lines() {
    let limits = LexicalLimits::default();
    for (text, end) in [
        ("\"a\\\" b", 4),
        ("'x\ny' z", 5),
        ("\"&amp;\"", 7),
        ("''", 2),
        ("'\u{e9}'", 4),
    ] {
        let token = scan_jsx_string(&source(text), 0, limits).unwrap();
        assert_eq!(
            (token.span, token.content_span),
            (span(0, end), span(1, end - 1)),
            "{text:?}"
        );
    }
    assert_eq!(
        scan_jsx_string(&source("\"abc"), 0, limits),
        Err(lexical(0, LexicalErrorKind::UnterminatedString))
    );
    assert_eq!(
        scan_jsx_string(&source("a"), 0, limits),
        Err(lexical(0, LexicalErrorKind::ExpectedQuote))
    );
    let small = LexicalLimits {
        max_string_bytes: 4,
        ..limits
    };
    assert_eq!(
        scan_jsx_string(&source("\"abcd\""), 0, small),
        Err(lexical(4, LexicalErrorKind::StringLimit))
    );
    assert_eq!(
        scan_jsx_string(&source("\"ab\""), 0, small).map(|token| token.span),
        Ok(span(0, 4))
    );
    // A limit error precedes the missing quote, and points to the scalar
    // that would exceed the bound.
    let smaller = LexicalLimits {
        max_string_bytes: 3,
        ..limits
    };
    assert_eq!(
        scan_jsx_string(&source("\"a\u{e9}bc"), 0, smaller),
        Err(lexical(2, LexicalErrorKind::StringLimit))
    );
}

#[test]
fn jsx_text_stops_at_tags_and_containers() {
    let limits = LexicalLimits::default();
    for (text, end) in [
        ("a b{", 3),
        ("x<", 1),
        ("", 0),
        ("<", 0),
        ("// x /* y */{", 12),
        ("line\r\nbreak &amp; \u{e9}", 20),
    ] {
        assert_eq!(
            scan_jsx_text(&source(text), 0, limits),
            Ok(span(0, end)),
            "{text:?}"
        );
    }
    assert_eq!(
        scan_jsx_text(&source("a>b"), 0, limits),
        Err(lexical(1, LexicalErrorKind::InvalidJsxTextCharacter))
    );
    assert_eq!(
        scan_jsx_text(&source("a}b"), 0, limits),
        Err(lexical(1, LexicalErrorKind::InvalidJsxTextCharacter))
    );
    let small = |max_jsx_text_bytes| LexicalLimits {
        max_jsx_text_bytes,
        ..limits
    };
    assert_eq!(
        scan_jsx_text(&source("abc>"), 0, small(2)),
        Err(lexical(2, LexicalErrorKind::JsxTextLimit))
    );
    assert_eq!(
        scan_jsx_text(&source("abc>"), 0, small(3)),
        Err(lexical(3, LexicalErrorKind::InvalidJsxTextCharacter))
    );
    assert_eq!(
        scan_jsx_text(&source("a\u{e9}{"), 0, small(2)),
        Err(lexical(1, LexicalErrorKind::JsxTextLimit))
    );
    assert_eq!(
        scan_jsx_text(&source("a\u{e9}{"), 0, small(3)),
        Ok(span(0, 3))
    );
}

#[test]
fn elements_attributes_and_children_build_span_precise_nodes() {
    let program = "const x = <a b=\"1\" c={d} {...e} f>t{g}<h.i.j/><k:l m/>{/* c */}{...n}</a>;";
    let input = source(program);
    let tree = parse(MODULE, &input);
    let root = element(&tree);
    let SyntaxNodeKind::JsxElement {
        opening,
        children: child_list,
        closing: Some(closing),
    } = kind(&tree, root)
    else {
        panic!("expected an element with a closing tag");
    };
    assert_eq!(text(&input, &tree, root), &program[10..program.len() - 1]);
    assert_eq!(text(&input, &tree, opening), "<a b=\"1\" c={d} {...e} f>");
    assert_eq!(text(&input, &tree, closing), "</a>");
    let SyntaxNodeKind::JsxOpeningElement {
        name,
        attributes,
        self_closing: false,
    } = kind(&tree, opening)
    else {
        panic!("expected an opening element");
    };
    assert_eq!(kind(&tree, name), SyntaxNodeKind::JsxIdentifier);
    let attributes = tree.children(attributes).unwrap();
    let attribute_texts: Vec<_> = attributes
        .iter()
        .map(|&id| text(&input, &tree, id))
        .collect();
    assert_eq!(attribute_texts, ["b=\"1\"", "c={d}", "{...e}", "f"]);
    let SyntaxNodeKind::JsxAttribute {
        value: Some(string),
        ..
    } = kind(&tree, attributes[0])
    else {
        panic!("expected a string attribute");
    };
    assert_eq!(kind(&tree, string), SyntaxNodeKind::JsxStringLiteral);
    assert!(matches!(
        kind(&tree, attributes[1]),
        SyntaxNodeKind::JsxAttribute { value: Some(_), .. }
    ));
    assert!(matches!(
        kind(&tree, attributes[2]),
        SyntaxNodeKind::JsxSpreadAttribute { .. }
    ));
    assert!(matches!(
        kind(&tree, attributes[3]),
        SyntaxNodeKind::JsxAttribute { value: None, .. }
    ));
    let child_list = tree.children(child_list).unwrap();
    let child_texts: Vec<_> = child_list
        .iter()
        .map(|&id| text(&input, &tree, id))
        .collect();
    assert_eq!(
        child_texts,
        ["t", "{g}", "<h.i.j/>", "<k:l m/>", "{/* c */}", "{...n}"]
    );
    assert_eq!(kind(&tree, child_list[0]), SyntaxNodeKind::JsxText);
    assert!(matches!(
        kind(&tree, child_list[1]),
        SyntaxNodeKind::JsxExpressionContainer {
            expression: Some(_)
        }
    ));
    assert_eq!(
        kind(&tree, child_list[4]),
        SyntaxNodeKind::JsxExpressionContainer { expression: None }
    );
    assert!(matches!(
        kind(&tree, child_list[5]),
        SyntaxNodeKind::JsxSpreadChild { .. }
    ));
    // `h.i.j` nests to the left; `k:l` is a namespaced name.
    let SyntaxNodeKind::JsxElement {
        opening: member_opening,
        closing: None,
        ..
    } = kind(&tree, child_list[2])
    else {
        panic!("expected a self-closing element");
    };
    let SyntaxNodeKind::JsxOpeningElement {
        name: member,
        self_closing: true,
        ..
    } = kind(&tree, member_opening)
    else {
        panic!("expected a self-closing opening element");
    };
    let SyntaxNodeKind::JsxMemberExpression { object, property } = kind(&tree, member) else {
        panic!("expected a member name");
    };
    assert_eq!(
        (text(&input, &tree, object), text(&input, &tree, property)),
        ("h.i", "j")
    );
    let namespaced = children(&tree, children(&tree, child_list[3])[0])[0];
    let SyntaxNodeKind::JsxNamespacedName { namespace, name } = kind(&tree, namespaced) else {
        panic!("expected a namespaced name");
    };
    assert_eq!(
        (text(&input, &tree, namespace), text(&input, &tree, name)),
        ("k", "l")
    );
    assert_eq!(tree.comments().len(), 1);
    assert_eq!(tree.comments()[0].kind, CommentKind::Block);
    let SyntaxNodeKind::Program { dialect, .. } = kind(&tree, tree.root()) else {
        panic!("expected a program");
    };
    assert_eq!(dialect, JsDialect::Jsx);
}

#[test]
fn fragments_attribute_elements_and_tag_trivia_are_supported() {
    let input = source("x = <>a<>b</>{c}</>;");
    let tree = parse(MODULE, &input);
    let fragment = element(&tree);
    let SyntaxNodeKind::JsxFragment {
        opening,
        children: list,
        closing,
    } = kind(&tree, fragment)
    else {
        panic!("expected a fragment");
    };
    assert_eq!(kind(&tree, opening), SyntaxNodeKind::JsxOpeningFragment);
    assert_eq!(kind(&tree, closing), SyntaxNodeKind::JsxClosingFragment);
    let texts: Vec<_> = tree
        .children(list)
        .unwrap()
        .iter()
        .map(|&id| text(&input, &tree, id))
        .collect();
    assert_eq!(texts, ["a", "<>b</>", "{c}"]);

    let input = source("x = <a b=<c/> d=<>e</> />;");
    let tree = parse(MODULE, &input);
    let opening = children(&tree, element(&tree))[0];
    let values: Vec<_> = children(&tree, opening)[1..]
        .iter()
        .map(|&attribute| kind(&tree, children(&tree, attribute)[1]))
        .collect();
    assert!(matches!(values[0], SyntaxNodeKind::JsxElement { .. }));
    assert!(matches!(values[1], SyntaxNodeKind::JsxFragment { .. }));

    // Tags hold JavaScript trivia between tokens; children never do.
    let input = source("x = < a /* c */ b = \"1\" // d\n / >;\ny = <a>// e\n/* f */</a >;");
    let tree = parse(SCRIPT, &input);
    let kinds: Vec<_> = tree.comments().iter().map(|comment| comment.kind).collect();
    assert_eq!(kinds, [CommentKind::Block, CommentKind::Line]);
    assert!(tree.nodes().iter().any(|node| {
        node.kind() == SyntaxNodeKind::JsxText
            && input.slice(node.span()).unwrap() == "// e\n/* f */"
    }));
    // Closing names match by structure and spelling, ignoring trivia.
    parse(MODULE, &source("<a.b-c></ a . b-c >;\n<x:y></x : y>;"));
    // Reserved words are JSX names, and a leading HTML-like comment in a
    // script is trivia before the element.
    parse(SCRIPT, &source("<!-- c\n<this.if class=\"x\"></this.if>"));
}

#[test]
fn jsx_errors_fail_closed_at_exact_offsets() {
    let backslash = '\\';
    let escaped = format!("<a{backslash}u0062 />");
    let cases: Vec<(&str, usize, ParseErrorKind)> = vec![
        ("<a></b>", 3, ParseErrorKind::JsxClosingTagMismatch),
        ("<a.b></a.c>", 5, ParseErrorKind::JsxClosingTagMismatch),
        ("<a:b></a>", 5, ParseErrorKind::JsxClosingTagMismatch),
        ("<a.b></a:b>", 5, ParseErrorKind::JsxClosingTagMismatch),
        ("<a></>", 3, ParseErrorKind::JsxClosingTagMismatch),
        ("<></a>", 2, ParseErrorKind::JsxClosingTagMismatch),
        ("<a><b></a>", 6, ParseErrorKind::JsxClosingTagMismatch),
        ("<a>", 0, ParseErrorKind::UnclosedJsxElement),
        ("x = <a><>b", 7, ParseErrorKind::UnclosedJsxElement),
        ("<a b=c />", 5, ParseErrorKind::ExpectedJsxAttributeValue),
        ("<a b= />", 6, ParseErrorKind::ExpectedJsxAttributeValue),
        ("<a b={} />", 5, ParseErrorKind::EmptyJsxAttributeValue),
        (
            "<a b={/* c */} />",
            5,
            ParseErrorKind::EmptyJsxAttributeValue,
        ),
        ("<a b.c />", 4, ParseErrorKind::ExpectedJsxTagEnd),
        ("<a:b.c />", 4, ParseErrorKind::ExpectedJsxTagEnd),
        ("<a / b>", 5, ParseErrorKind::ExpectedJsxTagEnd),
        ("<a b", 4, ParseErrorKind::ExpectedJsxTagEnd),
        ("<a></a b>", 7, ParseErrorKind::ExpectedJsxTagEnd),
        (
            "<a>>",
            3,
            ParseErrorKind::Lexical(LexicalErrorKind::InvalidJsxTextCharacter),
        ),
        (
            "<a>}</a>",
            3,
            ParseErrorKind::Lexical(LexicalErrorKind::InvalidJsxTextCharacter),
        ),
        ("<a>{a, b}</a>", 5, ParseErrorKind::ExpectedClosingBrace),
        ("<a>{...a, b}</a>", 8, ParseErrorKind::ExpectedClosingBrace),
        (
            "<a {b} />",
            4,
            ParseErrorKind::ExpectedPunctuator(Punctuator::Ellipsis),
        ),
        ("<a/> < b", 5, ParseErrorKind::AdjacentJsxElements),
        ("<a/>\n<b/>", 5, ParseErrorKind::AdjacentJsxElements),
        ("<></> < b", 6, ParseErrorKind::AdjacentJsxElements),
        (
            "<1/>",
            1,
            ParseErrorKind::Lexical(LexicalErrorKind::ExpectedIdentifier),
        ),
        (
            "</a>",
            1,
            ParseErrorKind::Lexical(LexicalErrorKind::ExpectedIdentifier),
        ),
        (
            "<a><!-- b --></a>",
            4,
            ParseErrorKind::Lexical(LexicalErrorKind::ExpectedIdentifier),
        ),
        (
            "<a b=\"x />",
            5,
            ParseErrorKind::Lexical(LexicalErrorKind::UnterminatedString),
        ),
        (
            &escaped,
            2,
            ParseErrorKind::Lexical(LexicalErrorKind::InvalidIdentifierEscape),
        ),
        ("<!-- a", 0, ParseErrorKind::HtmlCommentInModule),
    ];
    for (text, offset, expected) in cases {
        let error = failure(MODULE, text);
        assert_eq!((error.offset, &error.kind), (offset, &expected), "{text:?}");
    }
    // Only a lone `<` after an element is rejected; other operators parse.
    for text in [
        "<a/> <= b;",
        "<a/> << b;",
        "<a/> > b;",
        "<a/>.b;",
        "<a/>`t`;",
    ] {
        parse(MODULE, &source(text));
    }
}

#[test]
fn jsx_requires_the_jsx_dialect_and_respects_function_context() {
    let input = source("x = <a/>;");
    let error = parse_js_program(
        &input,
        MODULE,
        JsDialect::JavaScript,
        ParseLimits::default(),
    )
    .err()
    .unwrap();
    assert_eq!(
        (error.offset, error.kind),
        (4, ParseErrorKind::ExpectedExpression)
    );
    parse(
        MODULE,
        &source(
            "function* g() { yield <a/>; }\nasync function f() { return await <a>{await b}</a>; }",
        ),
    );
    assert_eq!(
        failure(MODULE, "function f() { return <a>{yield}</a>; }").kind,
        ParseErrorKind::ReservedWord
    );
    let input = source("<a b={c}>{d}</a>");
    let tree = parse_js_expression(&input, 0, JsDialect::Jsx, ParseLimits::default()).unwrap();
    assert!(matches!(
        kind(&tree, tree.root()),
        SyntaxNodeKind::JsxElement { .. }
    ));
    let input = source("<a>{await b}</a>");
    let error = parse_js_expression(&input, 0, JsDialect::Jsx, ParseLimits::default())
        .err()
        .unwrap();
    assert_eq!(
        (error.offset, error.kind),
        (4, ParseErrorKind::ContextualKeywordRequiresContext)
    );
}

#[test]
fn jsx_spans_drive_provenance_preserving_edits() {
    let input = source("const view = <Card title=\"x\">\n  {/* keep */}\n  hi\n</Card>;\n");
    let tree = parse(MODULE, &input);
    let edits: Vec<Edit> = tree
        .nodes()
        .iter()
        .filter(|node| {
            node.kind() == SyntaxNodeKind::JsxIdentifier
                && input.slice(node.span()).unwrap() == "Card"
        })
        .enumerate()
        .map(|(index, node)| Edit {
            span: node.span(),
            replacement: "Panel",
            order: EditOrder {
                operation: 0,
                edit: index as u32,
            },
        })
        .collect();
    assert_eq!(edits.len(), 2);
    let rewritten = apply_edits(&input, &edits, EditLimits::default()).unwrap();
    assert_eq!(
        rewritten.as_str(),
        "const view = <Panel title=\"x\">\n  {/* keep */}\n  hi\n</Panel>;\n"
    );
    // The untouched regions around the names, comment included, are copies.
    assert_eq!(rewritten.provenance().len(), 5);
}

// Unoptimized builds use far more stack per nesting level than release
// builds, so the bound is exercised on a thread with an explicit stack.
#[test]
fn deeply_nested_jsx_fails_closed_without_overflowing() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(check_nesting)
        .unwrap()
        .join()
        .unwrap();
}

fn check_nesting() {
    let depth = ParseLimits::default().max_nesting_depth;
    for (open, close) in [
        ("<a>", "</a>"),
        ("<>", "</>"),
        ("<a b=", " />"),
        ("<a>{", "}</a>"),
        ("<a {...", "} />"),
    ] {
        let deep = format!("{}x{}", open.repeat(depth * 2), close.repeat(depth * 2));
        let error = failure(MODULE, &deep);
        assert_eq!(error.kind, ParseErrorKind::ResourceLimit, "{open}");
    }
    parse(
        MODULE,
        &source(&format!("{}x{}", "<a>".repeat(8), "</a>".repeat(8))),
    );
    parse(
        MODULE,
        &source(&format!("{}<b/>{}", "<a c=".repeat(8), " />".repeat(8))),
    );
}

#[test]
fn generated_short_jsx_programs_never_return_inconsistent_trees() {
    const ALPHABET: [&str; 12] = [
        "<", ">", "/", "a", "{", "}", "=", "\"s\"", ".", ":", "...", "\n",
    ];
    let mut accepted = 0usize;
    for length in 0..=4u32 {
        for mut index in 0..ALPHABET.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push_str(ALPHABET[index % ALPHABET.len()]);
                index /= ALPHABET.len();
            }
            for prefix in ["", "<a>"] {
                let input = source(&format!("{prefix}{text}"));
                for goal in [SCRIPT, MODULE] {
                    match parse_js_program(&input, goal, JsDialect::Jsx, ParseLimits::default()) {
                        Ok(tree) => {
                            assert_tree_invariants(&input, goal, &tree);
                            accepted += 1;
                        }
                        Err(error) => {
                            assert!(error.offset <= input.len(), "{text:?}: {error}");
                            assert!(
                                !matches!(
                                    error.kind,
                                    ParseErrorKind::InternalInvariant(_)
                                        | ParseErrorKind::AllocationFailed
                                        | ParseErrorKind::ResourceLimit
                                ),
                                "{text:?}: {error}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(accepted > 300, "{accepted}");
}
