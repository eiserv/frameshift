//! Script parsing contracts through the public facade: statements,
//! declarations, functions, strictness, and early errors, using fictional text.

use frameshift_engine::{
    CommentKind, FunctionKind, NodeId, ParseError, ParseErrorKind, ParseLimits, SourceGoal,
    SourceText, StrictModeError, SyntaxNodeKind, SyntaxTree, TextLimits, VariableKind,
    parse_js_program,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn parse(goal: SourceGoal, input: &SourceText) -> SyntaxTree<'_> {
    match parse_js_program(input, goal, ParseLimits::default()) {
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
    match parse_js_program(&input, goal, ParseLimits::default()) {
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

fn body(tree: &SyntaxTree<'_>) -> Vec<NodeId> {
    let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root()) else {
        panic!("expected a program root");
    };
    tree.children(body).unwrap().to_vec()
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
    let again = parse_js_program(input, goal, ParseLimits::default()).unwrap();
    assert_eq!(again.nodes(), tree.nodes());
    assert_eq!(again.comments(), tree.comments());
}

fn assert_failures(goal: SourceGoal, cases: &[(&str, usize, ParseErrorKind)]) {
    for (text, offset, expected) in cases {
        let error = failure(goal, text);
        assert_eq!(
            (error.offset, &error.kind),
            (*offset, expected),
            "{goal:?} {text:?}"
        );
    }
}

fn assert_valid(goal: SourceGoal, cases: &[&str]) {
    for text in cases {
        parse(goal, &source(text));
    }
}

const SCRIPT: SourceGoal = SourceGoal::Script;
const MODULE: SourceGoal = SourceGoal::Module;

#[test]
fn programs_keep_statement_spans_comments_and_strictness() {
    let program =
        "#!/usr/bin/env node\n'use strict'; // note\nlet a = 1, [b] = c;\nif (a) b(); else { c }\n";
    let input = source(program);
    let tree = parse(SCRIPT, &input);
    let SyntaxNodeKind::Program {
        goal, strict: true, ..
    } = kind(&tree, tree.root())
    else {
        panic!("expected a strict script");
    };
    assert_eq!(goal, SCRIPT);
    assert_eq!(text(&input, &tree, tree.root()), program);
    let comments: Vec<_> = tree.comments().iter().map(|comment| comment.kind).collect();
    assert_eq!(comments, [CommentKind::Hashbang, CommentKind::Line]);
    let items = body(&tree);
    assert_eq!(items.len(), 3);
    assert!(matches!(
        kind(&tree, items[0]),
        SyntaxNodeKind::Directive { .. }
    ));
    assert_eq!(text(&input, &tree, items[0]), "'use strict';");
    let SyntaxNodeKind::VariableDeclaration {
        kind: VariableKind::Let,
        declarators,
    } = kind(&tree, items[1])
    else {
        panic!("expected a let declaration");
    };
    assert_eq!(text(&input, &tree, items[1]), "let a = 1, [b] = c;");
    let declarators = tree.children(declarators).unwrap();
    let SyntaxNodeKind::VariableDeclarator { target, init } = kind(&tree, declarators[1]) else {
        panic!("expected a declarator");
    };
    assert!(matches!(
        kind(&tree, target),
        SyntaxNodeKind::ArrayPattern { .. }
    ));
    assert_eq!(text(&input, &tree, init.unwrap()), "c");
    let SyntaxNodeKind::IfStatement {
        alternate: Some(alternate),
        ..
    } = kind(&tree, items[2])
    else {
        panic!("expected an if statement with else");
    };
    assert!(matches!(
        kind(&tree, alternate),
        SyntaxNodeKind::BlockStatement { .. }
    ));

    // A byte order mark is an encoding signature, so a hashbang may follow it.
    let input = source("\u{feff}#!x\nlet a");
    let tree = parse(MODULE, &input);
    assert_eq!(tree.comments()[0].kind, CommentKind::Hashbang);
    assert_failures(
        SCRIPT,
        &[(
            " #!x",
            2,
            ParseErrorKind::Lexical(frameshift_engine::LexicalErrorKind::ExpectedIdentifier),
        )],
    );
}

#[test]
fn automatic_semicolon_insertion_follows_restricted_productions() {
    let input = source("a\nb\n++c\nfunction f() { return\nd }\ndo x; while (y) z");
    let tree = parse(SCRIPT, &input);
    let items = body(&tree);
    assert_eq!(items.len(), 6);
    assert_eq!(text(&input, &tree, items[2]), "++c");
    let SyntaxNodeKind::FunctionDeclaration { body: function, .. } = kind(&tree, items[3]) else {
        panic!("expected a function declaration");
    };
    let SyntaxNodeKind::FunctionBody {
        body: statements, ..
    } = kind(&tree, function)
    else {
        panic!("expected a function body");
    };
    let statements = tree.children(statements).unwrap();
    assert_eq!(statements.len(), 2);
    assert_eq!(
        kind(&tree, statements[0]),
        SyntaxNodeKind::ReturnStatement { argument: None }
    );
    assert_valid(
        SCRIPT,
        &[
            "x\n(y)",
            "a = b\n/c/g.exec(d)",
            "var a = function () {}\n(1)",
            "x => {}\n+1",
            // Only a comma continues an AssignmentExpression, so the line
            // break ends the statement and `/` starts a regular expression.
            "a = () => {}\n/b/g.exec(c)",
            "a ? b : () => {}\n/c/.d",
            "{ a } b",
            "for (;;) break\nfoo",
            "l: { break l }",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            ("a b", 2, ParseErrorKind::ExpectedSemicolon),
            ("throw\nerror", 6, ParseErrorKind::LineTerminatorAfterThrow),
            (
                // Semicolons in a `for` head are never inserted.
                "for (a\n b) c",
                8,
                ParseErrorKind::ExpectedPunctuator(frameshift_engine::Punctuator::Semicolon),
            ),
            ("x => {} + 1", 8, ParseErrorKind::ExpectedSemicolon),
            ("a = () => {} + 1", 13, ParseErrorKind::ExpectedSemicolon),
            (
                "a ? b : () => {} / 1",
                17,
                ParseErrorKind::ExpectedSemicolon,
            ),
            ("if (a)", 6, ParseErrorKind::ExpectedExpression),
        ],
    );
}

#[test]
fn loops_switches_and_try_statements_build_their_nodes() {
    let input = source(
        "for (let i = 0; i < n; i++) ; for (const k in o) ; for (x of y) ; \
         switch (v) { case 1: a; default: b } try { t } catch ({ e }) { h } finally { f } \
         while (c) d; do e; while (f)",
    );
    let tree = parse(SCRIPT, &input);
    let kinds: Vec<_> = body(&tree).iter().map(|&id| kind(&tree, id)).collect();
    assert!(matches!(
        kinds[0],
        SyntaxNodeKind::ForStatement {
            init: Some(_),
            test: Some(_),
            update: Some(_),
            ..
        }
    ));
    assert!(matches!(kinds[1], SyntaxNodeKind::ForInStatement { .. }));
    assert!(matches!(
        kinds[2],
        SyntaxNodeKind::ForOfStatement {
            is_await: false,
            ..
        }
    ));
    let SyntaxNodeKind::SwitchStatement { cases, .. } = kinds[3] else {
        panic!("expected a switch");
    };
    let cases = tree.children(cases).unwrap();
    assert!(matches!(
        kind(&tree, cases[1]),
        SyntaxNodeKind::SwitchCase { test: None, .. }
    ));
    let SyntaxNodeKind::TryStatement {
        handler: Some(handler),
        finalizer: Some(_),
        ..
    } = kinds[4]
    else {
        panic!("expected try, catch, and finally");
    };
    let SyntaxNodeKind::CatchClause {
        parameter: Some(parameter),
        ..
    } = kind(&tree, handler)
    else {
        panic!("expected a catch parameter");
    };
    assert!(matches!(
        kind(&tree, parameter),
        SyntaxNodeKind::ObjectPattern { .. }
    ));
    assert!(matches!(kinds[5], SyntaxNodeKind::WhileStatement { .. }));
    assert!(matches!(kinds[6], SyntaxNodeKind::DoWhileStatement { .. }));

    assert_valid(
        SCRIPT,
        &[
            "for (var a = 0, b; ;) ;",
            "for (let [a, b] of c) ;",
            "for (let in x) ;",
            "for (async of => {}; ;) ;",
            "for ((async) of x) ;",
            "for (using of x) ;",
            "for ([a, b.c] of d) ;",
            "for (a in b, c) ;",
            "for (a = (b in c); ;) ;",
            "try {} catch {}",
            "try {} finally {}",
            "switch (a) {}",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            (
                "for (let a, b of c) ;",
                12,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            (
                "for (const a = 1 of b) ;",
                15,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            (
                "for (let [a] = 1 in b) ;",
                15,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            (
                "for (let.x of y) ;",
                5,
                ParseErrorKind::InvalidForOfLeftHandSide,
            ),
            (
                "for (async of x) ;",
                5,
                ParseErrorKind::InvalidForOfLeftHandSide,
            ),
            (
                "for (let of x) ;",
                12,
                ParseErrorKind::ExpectedPunctuator(frameshift_engine::Punctuator::Semicolon),
            ),
            (
                "for (a + b of c) ;",
                5,
                ParseErrorKind::InvalidAssignmentTarget,
            ),
            ("for (const a;;) ;", 12, ParseErrorKind::MissingInitializer),
            (
                "switch (a) { default: default: }",
                22,
                ParseErrorKind::DuplicateDefaultClause,
            ),
            ("try {}", 6, ParseErrorKind::ExpectedCatchOrFinally),
            ("do a; b", 6, ParseErrorKind::ExpectedWhile),
            ("const a;", 7, ParseErrorKind::MissingInitializer),
            ("let [a];", 7, ParseErrorKind::MissingInitializer),
        ],
    );
}

#[test]
fn labels_break_continue_and_return_respect_their_targets() {
    assert_valid(
        SCRIPT,
        &[
            "a: { break a; }",
            "a: b: while (1) continue a;",
            "a: while (1) { b: { continue a; } }",
            "while (1) { break; continue; }",
            "switch (x) { case 1: break; }",
            "a: { function f() { b: while (1) break b; } }",
            "a: ; a: ;",
            "function f() { return 1 }",
            "yield: ;",
            "await: ;",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            ("a: a: ;", 3, ParseErrorKind::DuplicateLabel),
            ("a: { b: a: ; }", 8, ParseErrorKind::DuplicateLabel),
            ("break;", 0, ParseErrorKind::InvalidBreak),
            ("continue;", 0, ParseErrorKind::InvalidContinue),
            (
                "switch (x) { case 1: continue; }",
                21,
                ParseErrorKind::InvalidContinue,
            ),
            ("a: { continue a; }", 14, ParseErrorKind::InvalidContinue),
            (
                "a: { b: while (1) continue a; }",
                27,
                ParseErrorKind::InvalidContinue,
            ),
            ("while (1) break b;", 16, ParseErrorKind::UndefinedLabel),
            (
                "a: { function f() { break a; } }",
                26,
                ParseErrorKind::UndefinedLabel,
            ),
            (
                "while (1) { function f() { break; } }",
                27,
                ParseErrorKind::InvalidBreak,
            ),
            ("return;", 0, ParseErrorKind::InvalidReturn),
            ("{ return }", 2, ParseErrorKind::InvalidReturn),
        ],
    );
    assert_failures(
        MODULE,
        &[
            ("await: ;", 0, ParseErrorKind::ReservedWord),
            ("yield: ;", 0, ParseErrorKind::ReservedWord),
        ],
    );
}

#[test]
fn functions_of_every_form_record_their_kind_and_context() {
    let input = source(
        "function f(a, b, c) { 'use strict'; return a } \
         async function* g() { yield await 1; for await (const x of y) ; } \
         var h = async (x) => { await x }, i = async x => x",
    );
    let tree = parse(SCRIPT, &input);
    let items = body(&tree);
    let SyntaxNodeKind::FunctionDeclaration {
        name: Some(name),
        parameters,
        body: function,
        kind: FunctionKind::Normal,
    } = kind(&tree, items[0])
    else {
        panic!("expected a function declaration");
    };
    assert_eq!(text(&input, &tree, name), "f");
    assert_eq!(text(&input, &tree, parameters), "(a, b, c)");
    assert!(matches!(
        kind(&tree, function),
        SyntaxNodeKind::FunctionBody { strict: true, .. }
    ));
    assert!(matches!(
        kind(&tree, items[1]),
        SyntaxNodeKind::FunctionDeclaration {
            kind: FunctionKind::AsyncGenerator,
            ..
        }
    ));

    assert_valid(
        SCRIPT,
        &[
            "function* g() { yield; yield* a; yield\na; (yield); [yield]; }",
            "function* g() { function y() {} }",
            "async function f() { await a; (await b) ** 2; }",
            "async function await() {}",
            "function* yield() {}",
            "(function await() {})",
            "function f() { return new.target }",
            "function f() { () => new.target }",
            "({ m() { super.x }, get a() { return 1 }, set a(v) {} })",
            "(async () => await 1)",
            "async function f() { () => 1 }",
            "function f(a, a) {}",
            "async function f(a, a) {}",
            "function f() { arguments; eval; }",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            (
                "function* g() { function yield() {} }",
                25,
                ParseErrorKind::ReservedWord,
            ),
            ("(function* yield() {})", 11, ParseErrorKind::ReservedWord),
            (
                "(async function await() {})",
                16,
                ParseErrorKind::ReservedWord,
            ),
            (
                "function* g(a = yield) {}",
                16,
                ParseErrorKind::YieldInParameters,
            ),
            (
                "async function f(a = await 1) {}",
                21,
                ParseErrorKind::AwaitInParameters,
            ),
            (
                "async function f() { (a = await 1) => 1 }",
                26,
                ParseErrorKind::AwaitInParameters,
            ),
            (
                "function f(a = 1) { 'use strict' }",
                20,
                ParseErrorKind::UseStrictWithNonSimpleParameters,
            ),
            (
                "function eval() { 'use strict' }",
                9,
                ParseErrorKind::StrictMode(StrictModeError::RestrictedName),
            ),
            (
                "function f(a, a) { 'use strict' }",
                14,
                ParseErrorKind::DuplicateParameter,
            ),
            (
                "function f(static) { 'use strict' }",
                11,
                ParseErrorKind::ReservedWord,
            ),
            ("(a, a) => 1", 4, ParseErrorKind::DuplicateParameter),
            ("({ m(a, a) {} })", 8, ParseErrorKind::DuplicateParameter),
            (
                "function f([a], a) {}",
                16,
                ParseErrorKind::DuplicateParameter,
            ),
            (
                "function f(a) { let a }",
                20,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "({ get a(b) {} })",
                8,
                ParseErrorKind::InvalidAccessorParameters,
            ),
            (
                "({ set a(...b) {} })",
                8,
                ParseErrorKind::InvalidAccessorParameters,
            ),
            (
                "({ set a(b, c) {} })",
                8,
                ParseErrorKind::InvalidAccessorParameters,
            ),
            ("({ m() { super() } })", 9, ParseErrorKind::InvalidSuper),
            ("function f() { super.x }", 15, ParseErrorKind::InvalidSuper),
            ("super.x", 0, ParseErrorKind::InvalidSuper),
            ("new.target", 0, ParseErrorKind::InvalidNewTarget),
            ("new.tar\\u0067et", 4, ParseErrorKind::InvalidMetaProperty),
            (
                "function f() { new.foo }",
                19,
                ParseErrorKind::InvalidMetaProperty,
            ),
            ("import.meta", 0, ParseErrorKind::ImportMetaOutsideModule),
            ("a => {} ()", 8, ParseErrorKind::ExpectedSemicolon),
            (
                "async function f() { for await (x;;) ; }",
                21,
                ParseErrorKind::InvalidForAwait,
            ),
            (
                "\\u0061sync function f() {}",
                11,
                ParseErrorKind::ExpectedSemicolon,
            ),
        ],
    );
}

#[test]
fn redeclarations_follow_scope_rules_and_annex_b() {
    assert_valid(
        SCRIPT,
        &[
            "var a; var a; function a() {}",
            "function f() {} function f() {}",
            "{ function f() {} function f() {} }",
            "switch (x) { case 1: function f() {} default: function f() {} }",
            "let a; { let a; }",
            "try {} catch (e) { var e; }",
            "try {} catch (e) { for (var e of []) ; }",
            "for (let a;;) { let a; }",
            "function f(a) { var a; }",
            "(function f() { let f; })",
            "{ var a; } var a;",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            ("let a; var a;", 11, ParseErrorKind::DuplicateDeclaration),
            ("var a; let a;", 11, ParseErrorKind::DuplicateDeclaration),
            ("let a; let a;", 11, ParseErrorKind::DuplicateDeclaration),
            (
                "let a; { var a; }",
                13,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "{ let a; { var a; } }",
                15,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "const a = 1; function a() {}",
                22,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "{ function f() {} let f; }",
                22,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "{ function f() {} var f; }",
                22,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "{ function f() {} async function f() {} }",
                33,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "'use strict'; { function f() {} function f() {} }",
                41,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "try {} catch (e) { let e; }",
                23,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "try {} catch ([e]) { var e; }",
                25,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "try {} catch ([e, e]) {}",
                18,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "for (let a of b) { var a; }",
                23,
                ParseErrorKind::DuplicateDeclaration,
            ),
            ("let let = 1;", 4, ParseErrorKind::LetBindingName),
            ("const [let] = a;", 7, ParseErrorKind::LetBindingName),
            (
                "class C {} class C {}",
                17,
                ParseErrorKind::DuplicateDeclaration,
            ),
        ],
    );
    assert_failures(
        MODULE,
        &[
            (
                "function f() {} function f() {}",
                25,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "var f; function f() {}",
                16,
                ParseErrorKind::DuplicateDeclaration,
            ),
        ],
    );
}

#[test]
fn sloppy_scripts_accept_annex_b_and_non_reserved_names() {
    assert_valid(
        SCRIPT,
        &[
            "with (a) b;",
            "var x = 017 + 08 + 09.5 + '\\1\\8';",
            "let = 1; let\nx = 2; static = 3; yield = 4; await = 5;",
            "var let, static, implements, package, private, protected, public, interface;",
            "eval = 1; arguments++; delete x; [eval] = a;",
            "f() = 1; f() += 1; for (f() in a) ;",
            "if (a) function f() {} else function g() {}",
            "l: function f() {}",
            "a: b: function f() {}",
            "for (var a = 1 in b) ;",
            "function f(a, a) {}",
            "x = a <!-- comment\n--> also a comment\n/* x\n */ --> also",
            "function f() { 'a'; 'use strict'; }",
            "if (a) let\nx = 1",
            "var yield; function f() { var yield; }",
            "({ __proto__: a, ['__proto__']: b })",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            (
                "'use strict'; with (a) b",
                14,
                ParseErrorKind::StrictMode(StrictModeError::With),
            ),
            (
                "'use strict'; 017",
                14,
                ParseErrorKind::StrictMode(StrictModeError::LegacyOctalLiteral),
            ),
            (
                "'use strict'; '\\08'",
                15,
                ParseErrorKind::StrictMode(StrictModeError::LegacyOctalEscape),
            ),
            (
                "function f() { '\\1'; 'use strict' }",
                16,
                ParseErrorKind::StrictMode(StrictModeError::LegacyOctalEscape),
            ),
            (
                "'use strict'; delete x",
                14,
                ParseErrorKind::StrictMode(StrictModeError::DeleteIdentifier),
            ),
            (
                "'use strict'; eval = 1",
                14,
                ParseErrorKind::StrictMode(StrictModeError::RestrictedName),
            ),
            (
                "'use strict'; f() = 1",
                14,
                ParseErrorKind::StrictMode(StrictModeError::CallAssignmentTarget),
            ),
            ("'use strict'; let = 1", 14, ParseErrorKind::ReservedWord),
            (
                "'use strict'; if (a) function f() {}",
                21,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "'use strict'; l: function f() {}",
                17,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "'use strict'; for (var a = 1 in b) ;",
                27,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            (
                "if (a) function* f() {}",
                7,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "if (a) async function f() {}",
                7,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "while (a) function f() {}",
                10,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "while (a) l: function f() {}",
                13,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "if (a) l: function f() {}",
                10,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "if (a) let [x] = 1",
                7,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "if (a) const x = 1",
                7,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            (
                "while (a) class C {}",
                10,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            ("f() &&= 1", 0, ParseErrorKind::InvalidAssignmentTarget),
            (
                "for (var [a] = 1 in b) ;",
                15,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            // At the start of the source, `-->` is not a comment.
            ("-->x", 2, ParseErrorKind::ExpectedExpression),
        ],
    );
}

#[test]
fn using_declarations_are_limited_to_blocks_and_loops() {
    assert_valid(
        SCRIPT,
        &[
            "{ using a = b, c = d; }",
            "function f() { using a = b; }",
            "for (using a of b) ;",
            "for (using a = b; ;) ;",
            "switch (a) { case 1: { using b = c; } }",
            "using\nx = 1",
            "using[x] = 1",
            "if (a) using\nb = c",
            "async function f() { await using a = b; for (await using c of d) ; }",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            ("using a = b;", 0, ParseErrorKind::InvalidUsingDeclaration),
            (
                "switch (a) { case 1: using b = c; }",
                21,
                ParseErrorKind::InvalidUsingDeclaration,
            ),
            ("{ using a; }", 9, ParseErrorKind::MissingInitializer),
            (
                "for (using a in b) ;",
                11,
                ParseErrorKind::InvalidForInOfDeclaration,
            ),
            (
                "{ using a = 1; let a; }",
                19,
                ParseErrorKind::DuplicateDeclaration,
            ),
        ],
    );
    assert_valid(
        MODULE,
        &[
            "using a = b;",
            "await using a = b;",
            "for await (using a of b) ;",
        ],
    );
}

// Unoptimized builds use far more stack per nesting level than release
// builds, so the bound is exercised on a thread with an explicit stack.
#[test]
fn nesting_token_and_node_limits_fail_closed_without_overflowing() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(check_nesting_token_and_node_limits)
        .unwrap()
        .join()
        .unwrap();
}

fn check_nesting_token_and_node_limits() {
    let limits = ParseLimits::default();
    let depth = limits.max_nesting_depth;
    for (open, close) in [
        ("{", "}"),
        ("if (a) ", ""),
        ("function f() {", "}"),
        ("(", ")"),
        ("[", "]"),
        ("() => ", "a"),
        ("class C { m() {", "} }"),
        ("`${", "}`"),
    ] {
        let deep = format!("{}a{}", open.repeat(depth * 2), close.repeat(depth * 2));
        let error = failure(SCRIPT, &deep);
        assert_eq!(error.kind, ParseErrorKind::ResourceLimit, "{open}");
        let shallow = format!("{}a{}", open.repeat(8), close.repeat(8));
        parse(SCRIPT, &source(&shallow));
    }
    let input = source("a; b; c;");
    let error = parse_js_program(
        &input,
        SCRIPT,
        ParseLimits {
            max_tokens: 4,
            ..ParseLimits::default()
        },
    )
    .err()
    .unwrap();
    assert_eq!(
        (error.offset, error.kind),
        (6, ParseErrorKind::ResourceLimit)
    );
    let error = parse_js_program(
        &input,
        SCRIPT,
        ParseLimits {
            max_nodes: 3,
            ..ParseLimits::default()
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ParseErrorKind::ResourceLimit);
}

#[test]
fn generated_short_programs_never_return_inconsistent_trees() {
    const ALPHABET: [&str; 12] = [
        "a", "let", "=", ";", "{", "}", "(", ")", "=>", "\n", "function", ":",
    ];
    let mut accepted = 0usize;
    for length in 0..=4u32 {
        for mut index in 0..ALPHABET.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push_str(ALPHABET[index % ALPHABET.len()]);
                text.push(' ');
                index /= ALPHABET.len();
            }
            let input = source(&text);
            for goal in [SCRIPT, MODULE] {
                match parse_js_program(&input, goal, ParseLimits::default()) {
                    Ok(tree) => {
                        assert_tree_invariants(&input, goal, &tree);
                        accepted += 1;
                    }
                    Err(error) => {
                        assert!(error.offset <= text.len(), "{text:?}: {error}");
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
    assert!(accepted > 500, "{accepted}");
}
