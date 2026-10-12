//! Literal, assignment, destructuring, `new`, and arrow function contracts
//! through the public facade, using fictional text.

use frameshift_engine::{
    AssignmentOperator, BinaryOperator, JsDialect, NodeId, ParseError, ParseErrorKind, ParseLimits,
    SourceText, SyntaxNodeKind, SyntaxTree, TextLimits, UpdateOperator, parse_js_expression,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn parse(input: &SourceText) -> SyntaxTree<'_> {
    match parse_js_expression(input, 0, JsDialect::JavaScript, ParseLimits::default()) {
        Ok(tree) => {
            assert_tree_invariants(input, &tree);
            tree
        }
        Err(error) => panic!("expected {:?} to parse: {error}", input.as_str()),
    }
}

fn failure(text: &str) -> ParseError {
    let input = source(text);
    match parse_js_expression(&input, 0, JsDialect::JavaScript, ParseLimits::default()) {
        Ok(_) => panic!("expected expression parsing to fail for {text:?}"),
        Err(error) => error,
    }
}

fn kind(tree: &SyntaxTree<'_>, id: NodeId) -> SyntaxNodeKind {
    tree.node(id).unwrap().kind()
}

fn text<'a>(input: &'a SourceText, tree: &SyntaxTree<'_>, id: NodeId) -> &'a str {
    input.slice(tree.node(id).unwrap().span()).unwrap()
}

fn list(tree: &SyntaxTree<'_>, list: frameshift_engine::ChildList) -> Vec<NodeId> {
    tree.children(list).unwrap().to_vec()
}

// Every node is reachable exactly once, child spans nest in source order,
// and a second parse yields an identical arena.
fn assert_tree_invariants(input: &SourceText, tree: &SyntaxTree<'_>) {
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
    let again =
        parse_js_expression(input, 0, JsDialect::JavaScript, ParseLimits::default()).unwrap();
    assert_eq!(again.nodes(), tree.nodes());
    assert_eq!(again.root(), tree.root());
}

#[test]
fn array_and_object_literals_keep_holes_spreads_and_property_forms() {
    let input = source("[a, , ...b, c,]");
    let tree = parse(&input);
    let SyntaxNodeKind::ArrayExpression {
        elements,
        trailing_comma: true,
    } = kind(&tree, tree.root())
    else {
        panic!("expected an array literal with a trailing comma");
    };
    let elements = list(&tree, elements);
    assert_eq!(elements.len(), 4);
    assert_eq!(kind(&tree, elements[1]), SyntaxNodeKind::Elision);
    let hole = tree.node(elements[1]).unwrap().span();
    assert_eq!((hole.start(), hole.end()), (4, 4));
    assert!(matches!(
        kind(&tree, elements[2]),
        SyntaxNodeKind::Spread { .. }
    ));
    assert_eq!(text(&input, &tree, elements[2]), "...b");

    let holes = source("[a,,]");
    let tree = parse(&holes);
    let SyntaxNodeKind::ArrayExpression {
        elements,
        trailing_comma: false,
    } = kind(&tree, tree.root())
    else {
        panic!("a hole is not a trailing comma");
    };
    assert_eq!(elements.len(), 2);

    let input = source("{ a, b: 1, [k]: v, 'c': 2, 3: d, ...rest, }");
    let tree = parse(&input);
    let SyntaxNodeKind::ObjectExpression {
        properties,
        trailing_comma: true,
    } = kind(&tree, tree.root())
    else {
        panic!("expected an object literal with a trailing comma");
    };
    let properties = list(&tree, properties);
    assert_eq!(properties.len(), 6);
    let SyntaxNodeKind::ShorthandProperty { identifier } = kind(&tree, properties[0]) else {
        panic!("expected a shorthand property");
    };
    assert!(matches!(
        kind(&tree, identifier),
        SyntaxNodeKind::Identifier { .. }
    ));
    assert_eq!(
        tree.node(identifier).unwrap().span(),
        tree.node(properties[0]).unwrap().span()
    );
    let keys: Vec<_> = properties[1..5]
        .iter()
        .map(|&property| {
            let SyntaxNodeKind::Property { key, computed, .. } = kind(&tree, property) else {
                panic!("expected a keyed property");
            };
            (kind(&tree, key), computed)
        })
        .collect();
    assert!(matches!(
        keys[0],
        (SyntaxNodeKind::IdentifierName { .. }, false)
    ));
    assert!(matches!(keys[1], (SyntaxNodeKind::Identifier { .. }, true)));
    assert!(matches!(keys[2], (SyntaxNodeKind::StringLiteral, false)));
    assert!(matches!(
        keys[3],
        (SyntaxNodeKind::NumberLiteral { .. }, false)
    ));
    assert_eq!(text(&input, &tree, properties[2]), "[k]: v");
    assert!(matches!(
        kind(&tree, properties[5]),
        SyntaxNodeKind::Spread { .. }
    ));

    let keywords = source("{ if: 1, new: 2, get: 3, set, async }");
    parse(&keywords);
    let template = source("`${ {a: 1}.a }${ x => ({}) }`");
    parse(&template);
}

#[test]
fn assignments_are_right_associative_and_require_simple_targets() {
    let input = source("a = b.c += d ??= e");
    let tree = parse(&input);
    let SyntaxNodeKind::Assignment {
        operator: AssignmentOperator::Assign,
        target,
        value,
    } = kind(&tree, tree.root())
    else {
        panic!("expected a plain assignment");
    };
    assert_eq!(text(&input, &tree, target), "a");
    let SyntaxNodeKind::Assignment {
        operator: AssignmentOperator::AddAssign,
        target,
        value,
    } = kind(&tree, value)
    else {
        panic!("expected an addition assignment");
    };
    assert!(matches!(kind(&tree, target), SyntaxNodeKind::Member { .. }));
    assert!(matches!(
        kind(&tree, value),
        SyntaxNodeKind::Assignment {
            operator: AssignmentOperator::NullishAssign,
            ..
        }
    ));

    for valid in [
        "(a) = 1",
        "((a.b)) **= 2",
        "a[0] >>>= 1",
        "x ? y = 1 : z = 2",
        "a = b, c = d",
        "f(a = 1)",
        "++a ** 2",
        "a++ ** 2",
        "-x++",
        "--(a)",
        "a\n= b",
    ] {
        parse(&source(valid));
    }

    for (text, offset, expected) in [
        ("a + b = c", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("a?.b = c", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("a?.b.c += 1", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("f() = 1", 0, ParseErrorKind::StrictModeRequiresContext),
        ("(f()) += 1", 0, ParseErrorKind::StrictModeRequiresContext),
        ("f()++", 0, ParseErrorKind::StrictModeRequiresContext),
        ("++f()", 2, ParseErrorKind::StrictModeRequiresContext),
        ("f() &&= 1", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("[f()] = 1", 1, ParseErrorKind::InvalidAssignmentTarget),
        ("a?.() = 1", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("([a]) = 1", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("[a] += 1", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("1 = a", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("(a => b) = c", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("++a++", 2, ParseErrorKind::InvalidAssignmentTarget),
        ("this++", 0, ParseErrorKind::InvalidAssignmentTarget),
        ("a\n++b", 2, ParseErrorKind::TrailingInput),
    ] {
        let error = failure(text);
        assert_eq!((error.offset, error.kind), (offset, expected), "{text}");
    }
}

#[test]
fn update_expressions_record_operator_and_position() {
    let input = source("i++ + --j");
    let tree = parse(&input);
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = kind(&tree, tree.root())
    else {
        panic!("expected addition");
    };
    assert!(matches!(
        kind(&tree, left),
        SyntaxNodeKind::Update {
            operator: UpdateOperator::Increment,
            prefix: false,
            ..
        }
    ));
    assert!(matches!(
        kind(&tree, right),
        SyntaxNodeKind::Update {
            operator: UpdateOperator::Decrement,
            prefix: true,
            ..
        }
    ));
    assert_eq!(text(&input, &tree, left), "i++");
}

#[test]
fn destructuring_assignment_reinterprets_literals_as_patterns_in_place() {
    let input = source("[a, [b = 1], , c.d, ...e] = f");
    let tree = parse(&input);
    let node_count = tree.nodes().len();
    let SyntaxNodeKind::Assignment { target, .. } = kind(&tree, tree.root()) else {
        panic!("expected an assignment");
    };
    let SyntaxNodeKind::ArrayPattern { elements, .. } = kind(&tree, target) else {
        panic!("expected an array pattern");
    };
    let elements = list(&tree, elements);
    // Assignment targets stay references; only parameters declare names.
    assert!(matches!(
        kind(&tree, elements[0]),
        SyntaxNodeKind::Identifier { .. }
    ));
    let SyntaxNodeKind::ArrayPattern {
        elements: nested, ..
    } = kind(&tree, elements[1])
    else {
        panic!("expected a nested array pattern");
    };
    let nested = list(&tree, nested);
    let SyntaxNodeKind::AssignmentPattern { target, default } = kind(&tree, nested[0]) else {
        panic!("expected a default value");
    };
    assert_eq!(text(&input, &tree, target), "b");
    assert_eq!(text(&input, &tree, default), "1");
    assert_eq!(kind(&tree, elements[2]), SyntaxNodeKind::Elision);
    assert!(matches!(
        kind(&tree, elements[3]),
        SyntaxNodeKind::Member { .. }
    ));
    assert!(matches!(
        kind(&tree, elements[4]),
        SyntaxNodeKind::RestElement { .. }
    ));
    assert_eq!(node_count, tree.nodes().len());

    let input = source("({ a, b: { c }, d = 1, [k]: [e], ...f } = g)");
    let tree = parse(&input);
    let SyntaxNodeKind::Parenthesized { expression } = kind(&tree, tree.root()) else {
        panic!("expected parentheses");
    };
    let SyntaxNodeKind::Assignment { target, .. } = kind(&tree, expression) else {
        panic!("expected an assignment");
    };
    let SyntaxNodeKind::ObjectPattern { properties, .. } = kind(&tree, target) else {
        panic!("expected an object pattern");
    };
    let properties: Vec<_> = list(&tree, properties)
        .into_iter()
        .map(|id| kind(&tree, id))
        .collect();
    assert!(matches!(
        properties[0],
        SyntaxNodeKind::ShorthandPatternProperty { default: None, .. }
    ));
    let SyntaxNodeKind::PatternProperty { value, .. } = properties[1] else {
        panic!("expected a pattern property");
    };
    assert!(matches!(
        kind(&tree, value),
        SyntaxNodeKind::ObjectPattern { .. }
    ));
    assert!(matches!(
        properties[2],
        SyntaxNodeKind::ShorthandPatternProperty {
            default: Some(_),
            ..
        }
    ));
    assert!(matches!(
        properties[3],
        SyntaxNodeKind::PatternProperty { computed: true, .. }
    ));
    assert!(matches!(properties[4], SyntaxNodeKind::RestElement { .. }));

    for valid in [
        "[(a), (b.c)] = d",
        "({ a: (b) } = c)",
        "[{ a = 1 }] = b",
        "({ a: { b = 1 } } = c)",
        "[a = { b = 1 } = c] = d",
        "({ __proto__: a, __proto__: b } = c)",
        "[...[a, b]] = c",
        "[...a.b] = c",
        "({ ...a.b } = c)",
        "[,] = a",
    ] {
        parse(&source(valid));
    }
}

#[test]
fn invalid_destructuring_targets_fail_at_the_offending_element() {
    for (text, offset, expected) in [
        ("[...a, b] = c", 1, ParseErrorKind::InvalidRestElement),
        ("[...a,] = c", 1, ParseErrorKind::InvalidRestElement),
        ("[...a = 1] = c", 4, ParseErrorKind::InvalidRestElement),
        ("({ ...{ a } } = b)", 6, ParseErrorKind::InvalidRestElement),
        ("({ ...a, } = b)", 3, ParseErrorKind::InvalidRestElement),
        ("[a + 1] = b", 1, ParseErrorKind::InvalidAssignmentTarget),
        ("[(a = 1)] = b", 1, ParseErrorKind::InvalidAssignmentTarget),
        ("[([a])] = b", 1, ParseErrorKind::InvalidAssignmentTarget),
        ("[a += 1] = b", 1, ParseErrorKind::InvalidAssignmentTarget),
        ("({ a: 1 } = b)", 6, ParseErrorKind::InvalidAssignmentTarget),
        ("[a?.b] = c", 1, ParseErrorKind::InvalidAssignmentTarget),
        (
            "({ a = 1 })",
            3,
            ParseErrorKind::ShorthandInitializerOutsidePattern,
        ),
        (
            "f({ a = 1 }, { b = 2 })",
            4,
            ParseErrorKind::ShorthandInitializerOutsidePattern,
        ),
        (
            "({ a = 1 }).b = 2",
            3,
            ParseErrorKind::ShorthandInitializerOutsidePattern,
        ),
        (
            "({ a = { b = 1 } }) => a",
            9,
            ParseErrorKind::ShorthandInitializerOutsidePattern,
        ),
    ] {
        let error = failure(text);
        assert_eq!((error.offset, error.kind), (offset, expected), "{text}");
    }
}

#[test]
fn duplicate_proto_properties_are_rejected_only_in_expressions() {
    for (text, offset) in [
        ("({ __proto__: a, \"__proto__\": b })", 17),
        ("({ __proto__: a, '__pr\\x6fto__': b })", 17),
        ("({ __prot\\u006f__: a, __proto__: b })", 22),
        ("({ __proto__: a, __proto__: b, __proto__: c })", 17),
    ] {
        let error = failure(text);
        assert_eq!(
            (error.offset, error.kind),
            (offset, ParseErrorKind::DuplicateProto),
            "{text}"
        );
    }
    for valid in [
        "({ __proto__: a, __proto__ })",
        "({ __proto__: a, ['__proto__']: b })",
        "({ __proto__: a, __proto: b })",
    ] {
        parse(&source(valid));
    }
}

#[test]
fn arrow_functions_reinterpret_parameters_as_bindings() {
    let input = source("(a, b = 1, [c], { d }, ...e) => a + d");
    let tree = parse(&input);
    let SyntaxNodeKind::ArrowFunction {
        parameters,
        body,
        is_async: false,
    } = kind(&tree, tree.root())
    else {
        panic!("expected an arrow function");
    };
    assert_eq!(text(&input, &tree, body), "a + d");
    let SyntaxNodeKind::FormalParameters { parameters } = kind(&tree, parameters) else {
        panic!("expected a parameter list");
    };
    let parameters = list(&tree, parameters);
    assert_eq!(parameters.len(), 5);
    assert_eq!(
        kind(&tree, parameters[0]),
        SyntaxNodeKind::BindingIdentifier { has_escape: false }
    );
    let SyntaxNodeKind::AssignmentPattern { target, .. } = kind(&tree, parameters[1]) else {
        panic!("expected a default parameter");
    };
    assert!(matches!(
        kind(&tree, target),
        SyntaxNodeKind::BindingIdentifier { .. }
    ));
    let SyntaxNodeKind::ArrayPattern { elements, .. } = kind(&tree, parameters[2]) else {
        panic!("expected an array parameter pattern");
    };
    assert!(matches!(
        kind(&tree, list(&tree, elements)[0]),
        SyntaxNodeKind::BindingIdentifier { .. }
    ));
    let SyntaxNodeKind::ObjectPattern { properties, .. } = kind(&tree, parameters[3]) else {
        panic!("expected an object parameter pattern");
    };
    let SyntaxNodeKind::ShorthandPatternProperty { target, .. } =
        kind(&tree, list(&tree, properties)[0])
    else {
        panic!("expected a shorthand parameter property");
    };
    assert!(matches!(
        kind(&tree, target),
        SyntaxNodeKind::BindingIdentifier { .. }
    ));
    let SyntaxNodeKind::RestElement { argument } = kind(&tree, parameters[4]) else {
        panic!("expected a rest parameter");
    };
    assert!(matches!(
        kind(&tree, argument),
        SyntaxNodeKind::BindingIdentifier { .. }
    ));

    let input = source("a => b => a, c");
    let tree = parse(&input);
    let SyntaxNodeKind::Binary {
        operator: BinaryOperator::Comma,
        left,
        ..
    } = kind(&tree, tree.root())
    else {
        panic!("a concise body ends before a comma");
    };
    let SyntaxNodeKind::ArrowFunction { body, .. } = kind(&tree, left) else {
        panic!("expected an arrow function");
    };
    assert!(matches!(
        kind(&tree, body),
        SyntaxNodeKind::ArrowFunction { .. }
    ));

    for valid in [
        "() => ({})",
        "async => async",
        "(a,) => a",
        "([a, b] = [1, 2], { c: [d] } = {}) => d",
        "a ? b => c : d => e",
        "f(a => a, ...rest)",
        "x = y => y",
        "(\na\n) => a",
        "async(a)",
        "async in x",
        "(a => a)(1)",
    ] {
        parse(&source(valid));
    }
}

#[test]
fn invalid_arrow_functions_fail_closed_with_exact_offsets() {
    for (text, offset, expected) in [
        ("(a, a) => 1", 4, ParseErrorKind::DuplicateParameter),
        ("(a, \\u0061) => 1", 4, ParseErrorKind::DuplicateParameter),
        ("({ a }, [a]) => 1", 9, ParseErrorKind::DuplicateParameter),
        ("(b, a, b, a) => 1", 7, ParseErrorKind::DuplicateParameter),
        ("(a.b) => 1", 1, ParseErrorKind::InvalidArrowParameters),
        ("((a)) => 1", 1, ParseErrorKind::InvalidArrowParameters),
        ("([a.b]) => 1", 2, ParseErrorKind::InvalidArrowParameters),
        ("(a += 1) => 1", 1, ParseErrorKind::InvalidArrowParameters),
        ("(1) => 1", 1, ParseErrorKind::InvalidArrowParameters),
        ("true => 1", 0, ParseErrorKind::InvalidArrowParameters),
        ("(a => a) => 1", 1, ParseErrorKind::InvalidArrowParameters),
        ("(...a, b) => 1", 1, ParseErrorKind::InvalidRestElement),
        ("(...a,) => 1", 1, ParseErrorKind::InvalidRestElement),
        ("(...a = 1) => 1", 4, ParseErrorKind::InvalidRestElement),
        ("(a)\n=> 1", 4, ParseErrorKind::LineTerminatorBeforeArrow),
        (
            "a /* \n */ => 1",
            10,
            ParseErrorKind::LineTerminatorBeforeArrow,
        ),
        ("x + a => 1", 6, ParseErrorKind::UnexpectedArrow),
        ("x + (a) => 1", 8, ParseErrorKind::UnexpectedArrow),
        ("!a => 1", 3, ParseErrorKind::UnexpectedArrow),
        ("f(a) => 1", 5, ParseErrorKind::UnexpectedArrow),
        ("()", 2, ParseErrorKind::ExpectedArrow),
        ("(a,)", 4, ParseErrorKind::ExpectedArrow),
        ("(...a)", 6, ParseErrorKind::ExpectedArrow),
        ("async\nx => x", 6, ParseErrorKind::TrailingInput),
        (
            "async\n(x) => x",
            10,
            ParseErrorKind::LineTerminatorBeforeArrow,
        ),
        (
            "async (await) => 1",
            7,
            ParseErrorKind::ContextualKeywordRequiresContext,
        ),
        ("async await => 1", 6, ParseErrorKind::AwaitInParameters),
        ("let => 1", 0, ParseErrorKind::ReservedWordRequiresContext),
        (
            "await => 1",
            0,
            ParseErrorKind::ContextualKeywordRequiresContext,
        ),
    ] {
        let error = failure(text);
        assert_eq!((error.offset, error.kind), (offset, expected), "{text}");
    }
}

#[test]
fn new_expressions_bind_the_first_argument_list() {
    let input = source("new new Outer.Inner(a)(...b).c");
    let tree = parse(&input);
    let SyntaxNodeKind::Member { object, .. } = kind(&tree, tree.root()) else {
        panic!("expected member access on the outer new expression");
    };
    let SyntaxNodeKind::New {
        callee,
        arguments: Some(outer_arguments),
    } = kind(&tree, object)
    else {
        panic!("expected the outer new expression with arguments");
    };
    assert_eq!(text(&input, &tree, object), "new new Outer.Inner(a)(...b)");
    assert_eq!(outer_arguments.len(), 1);
    let SyntaxNodeKind::New {
        callee: inner,
        arguments: Some(inner_arguments),
    } = kind(&tree, callee)
    else {
        panic!("expected the inner new expression");
    };
    assert_eq!(text(&input, &tree, inner), "Outer.Inner");
    assert_eq!(inner_arguments.len(), 1);

    let input = source("new Widget");
    let tree = parse(&input);
    assert!(matches!(
        kind(&tree, tree.root()),
        SyntaxNodeKind::New {
            arguments: None,
            ..
        }
    ));

    let input = source("new Factory().build()");
    let tree = parse(&input);
    let SyntaxNodeKind::Call { callee, .. } = kind(&tree, tree.root()) else {
        panic!("expected a call on the constructed value");
    };
    let SyntaxNodeKind::Member { object, .. } = kind(&tree, callee) else {
        panic!("expected member access");
    };
    assert!(matches!(kind(&tree, object), SyntaxNodeKind::New { .. }));

    parse(&source("new tag`x`()"));
    parse(&source("new (a?.b)()"));
    parse(&source("new a()?.b"));

    for (text, offset, expected) in [
        ("new a?.b()", 5, ParseErrorKind::OptionalChainInNew),
        ("new.target", 0, ParseErrorKind::MetaPropertyRequiresContext),
        ("new", 3, ParseErrorKind::ExpectedExpression),
        ("new (a) => 1", 8, ParseErrorKind::UnexpectedArrow),
    ] {
        let error = failure(text);
        assert_eq!((error.offset, error.kind), (offset, expected), "{text}");
    }
}

#[test]
fn strict_mode_dependent_and_unsupported_forms_fail_closed() {
    for (text, offset, expected) in [
        ("eval = 1", 0, ParseErrorKind::StrictModeRequiresContext),
        ("arguments++", 0, ParseErrorKind::StrictModeRequiresContext),
        ("--eval", 2, ParseErrorKind::StrictModeRequiresContext),
        (
            "[ev\\u0061l] = x",
            1,
            ParseErrorKind::StrictModeRequiresContext,
        ),
        (
            "({ arguments } = x)",
            3,
            ParseErrorKind::StrictModeRequiresContext,
        ),
        ("(eval) => 1", 1, ParseErrorKind::StrictModeRequiresContext),
        ("eval => 1", 0, ParseErrorKind::StrictModeRequiresContext),
        ("delete x", 0, ParseErrorKind::StrictModeRequiresContext),
        ("delete ((x))", 0, ParseErrorKind::StrictModeRequiresContext),
        (
            "function () { with (a) {} }",
            14,
            ParseErrorKind::StrictModeRequiresContext,
        ),
        (
            "function (a, a) {}",
            13,
            ParseErrorKind::StrictModeRequiresContext,
        ),
        ("({ m() { super() } })", 9, ParseErrorKind::InvalidSuper),
        ("super.x", 0, ParseErrorKind::SuperRequiresContext),
        ("import.meta", 0, ParseErrorKind::SourceGoalRequiresContext),
        ("a <!-- b", 2, ParseErrorKind::SourceGoalRequiresContext),
        ("a\n--> b", 2, ParseErrorKind::SourceGoalRequiresContext),
        (
            "this.#x",
            5,
            ParseErrorKind::PrivateIdentifierRequiresContext,
        ),
        (
            "({ #x: 1 })",
            3,
            ParseErrorKind::PrivateIdentifierRequiresContext,
        ),
        ("({ if })", 3, ParseErrorKind::InvalidPropertyDefinition),
        ("({ let })", 3, ParseErrorKind::ReservedWordRequiresContext),
        ("({ a b })", 5, ParseErrorKind::InvalidPropertyDefinition),
        ("({ `a`: 1 })", 3, ParseErrorKind::InvalidPropertyDefinition),
        ("[a", 2, ParseErrorKind::ExpectedClosingBracket),
        ("({ a: 1", 7, ParseErrorKind::ExpectedClosingBrace),
        ("({", 2, ParseErrorKind::ExpectedClosingBrace),
    ] {
        let error = failure(text);
        assert_eq!((error.offset, error.kind), (offset, expected), "{text}");
    }
    parse(&source("delete x.y"));
    parse(&source("evaluate = 1"));
    for valid in [
        "a => {}",
        "async x => x",
        "async (x) => await x",
        "async function () { await x }",
        "({ m() {}, get x() {}, set x(v) {}, *g() {}, async m() {}, 'k'() {}, [k]() {} })",
        "function () { 'use strict'; with_ = 1 }",
        "function () {}",
        "class { #x; m() { return this.#x } }",
        "import('x', { with: {} })",
        "function () { return new.target }",
        "({ m() { return super.m() } })",
    ] {
        parse(&source(valid));
    }
}

#[test]
fn generated_short_inputs_never_return_inconsistent_trees() {
    const ALPHABET: [char; 11] = ['a', '(', ')', '[', ']', '{', '}', '=', '>', ',', '.'];
    let mut accepted = 0usize;
    let mut buffer = String::new();
    for length in 0..=5u32 {
        for mut index in 0..ALPHABET.len().pow(length) {
            buffer.clear();
            for _ in 0..length {
                buffer.push(ALPHABET[index % ALPHABET.len()]);
                index /= ALPHABET.len();
            }
            let input = source(&buffer);
            match parse_js_expression(&input, 0, JsDialect::JavaScript, ParseLimits::default()) {
                Ok(tree) => {
                    assert_tree_invariants(&input, &tree);
                    accepted += 1;
                }
                Err(error) => {
                    assert!(error.offset <= buffer.len(), "{buffer:?}: {error}");
                    assert!(
                        !matches!(
                            error.kind,
                            ParseErrorKind::InternalInvariant(_)
                                | ParseErrorKind::AllocationFailed
                                | ParseErrorKind::ResourceLimit
                        ),
                        "{buffer:?}: {error}"
                    );
                }
            }
        }
    }
    assert!(accepted > 0);
}
