//! Class and module parsing contracts through the public facade, using
//! fictional text.

use frameshift_engine::{
    MethodKind, NodeId, ParseErrorKind, ParseLimits, SourceGoal, SourceText, SyntaxNodeKind,
    SyntaxTree, TextLimits, VariableKind, parse_js_program,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn parse(goal: SourceGoal, input: &SourceText) -> SyntaxTree<'_> {
    match parse_js_program(input, goal, ParseLimits::default()) {
        Ok(tree) => {
            assert_reachable_and_nested(input, &tree);
            tree
        }
        Err(error) => panic!(
            "expected {:?} to parse as {goal:?}: {error}",
            input.as_str()
        ),
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

fn assert_reachable_and_nested(input: &SourceText, tree: &SyntaxTree<'_>) {
    let mut seen = vec![false; tree.nodes().len()];
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        assert!(!seen[id.index()], "node {id:?} is shared");
        seen[id.index()] = true;
        let span = tree.node(id).unwrap().span();
        let mut previous_end = span.start();
        for child in tree.child_nodes(id).unwrap() {
            let child_span = tree.node(child).unwrap().span();
            assert!(
                previous_end <= child_span.start() && child_span.end() <= span.end(),
                "child span {child_span:?} misplaced in {:?}",
                input.as_str()
            );
            previous_end = child_span.end();
            stack.push(child);
        }
    }
    assert!(seen.iter().all(|&reached| reached), "unreachable node");
}

fn assert_valid(goal: SourceGoal, cases: &[&str]) {
    for text in cases {
        parse(goal, &source(text));
    }
}

fn assert_failures(goal: SourceGoal, cases: &[(&str, usize, ParseErrorKind)]) {
    for (text, offset, expected) in cases {
        let input = source(text);
        let Err(error) = parse_js_program(&input, goal, ParseLimits::default()) else {
            panic!("expected {text:?} to fail as {goal:?}");
        };
        assert_eq!(
            (error.offset, &error.kind),
            (*offset, expected),
            "{goal:?} {text:?}"
        );
    }
}

const SCRIPT: SourceGoal = SourceGoal::Script;
const MODULE: SourceGoal = SourceGoal::Module;

#[test]
fn class_elements_keep_their_kinds_keys_and_placement() {
    let input = source(
        "class A extends B { #x = 1; static y; constructor() { super(); } \
         get #z() { return this.#x } set #z(v) {} static async *m() {} [k]() {} static { this.y = 1 } }",
    );
    let tree = parse(SCRIPT, &input);
    let SyntaxNodeKind::ClassDeclaration {
        name: Some(name),
        super_class: Some(super_class),
        body: class_body,
    } = kind(&tree, body(&tree)[0])
    else {
        panic!("expected a derived class declaration");
    };
    assert_eq!(text(&input, &tree, name), "A");
    assert_eq!(text(&input, &tree, super_class), "B");
    let SyntaxNodeKind::ClassBody { elements } = kind(&tree, class_body) else {
        panic!("expected a class body");
    };
    let elements: Vec<_> = tree
        .children(elements)
        .unwrap()
        .iter()
        .map(|&id| kind(&tree, id))
        .collect();
    assert!(matches!(
        elements[0],
        SyntaxNodeKind::FieldDefinition {
            value: Some(_),
            is_static: false,
            ..
        }
    ));
    assert!(matches!(
        elements[1],
        SyntaxNodeKind::FieldDefinition {
            value: None,
            is_static: true,
            ..
        }
    ));
    assert!(matches!(
        elements[2],
        SyntaxNodeKind::MethodDefinition {
            kind: MethodKind::Constructor,
            ..
        }
    ));
    let SyntaxNodeKind::MethodDefinition {
        key,
        kind: MethodKind::Getter,
        ..
    } = elements[3]
    else {
        panic!("expected a private getter");
    };
    assert!(matches!(
        kind(&tree, key),
        SyntaxNodeKind::PrivateIdentifier { .. }
    ));
    assert!(matches!(
        elements[5],
        SyntaxNodeKind::MethodDefinition {
            kind: MethodKind::AsyncGenerator,
            is_static: true,
            ..
        }
    ));
    assert!(matches!(
        elements[6],
        SyntaxNodeKind::MethodDefinition { computed: true, .. }
    ));
    assert!(matches!(elements[7], SyntaxNodeKind::StaticBlock { .. }));

    assert_valid(
        SCRIPT,
        &[
            "class A { static() {} get() {} set() {} async() {} static static() {} }",
            "class A { get\nx() {} static\ny }",
            "class A { a\nb = 1\n'c'\n[d] }",
            "class A { m() { return #p in this } #p }",
            "class A { #p; m() { class B { n(o) { return o.#p } } } }",
            "class A { get #a() {} set #a(v) {} }",
            "class A { static { var await_ = 1; } x = () => arguments_ }",
            "class A { x = await } ",
            "class A { constructor() {} ['constructor']() {} }",
            "(class extends (class {}) {})",
            "class A extends B { constructor() { (() => super())() } }",
            "class A { 'constructor'() {} }",
            "class A { static x = () => await }",
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            (
                "class A { constructor() {} constructor() {} }",
                27,
                ParseErrorKind::DuplicateConstructor,
            ),
            (
                "class A { get constructor() {} }",
                14,
                ParseErrorKind::InvalidConstructor,
            ),
            (
                "class A { async constructor() {} }",
                16,
                ParseErrorKind::InvalidConstructor,
            ),
            (
                "class A { constructor = 1 }",
                10,
                ParseErrorKind::InvalidClassElementName,
            ),
            (
                "class A { static prototype() {} }",
                17,
                ParseErrorKind::InvalidClassElementName,
            ),
            (
                "class A { static constructor = 1 }",
                17,
                ParseErrorKind::InvalidClassElementName,
            ),
            (
                "class A { #constructor }",
                10,
                ParseErrorKind::InvalidClassElementName,
            ),
            (
                "class A { #a; #a }",
                14,
                ParseErrorKind::DuplicatePrivateName,
            ),
            (
                "class A { get #a() {} static set #a(v) {} }",
                33,
                ParseErrorKind::DuplicatePrivateName,
            ),
            (
                "class A { m() { this.#b } }",
                21,
                ParseErrorKind::UndeclaredPrivateName,
            ),
            ("this.#a", 5, ParseErrorKind::UndeclaredPrivateName),
            (
                "class A { #a; m() { delete this.#a } }",
                20,
                ParseErrorKind::DeletePrivateName,
            ),
            (
                "class A { #a; m() { return #a } }",
                27,
                ParseErrorKind::UnexpectedPrivateName,
            ),
            (
                "class A { #a; m() { return 1 + #a in this } }",
                31,
                ParseErrorKind::UnexpectedPrivateName,
            ),
            (
                "class A { constructor() { super() } }",
                26,
                ParseErrorKind::InvalidSuper,
            ),
            (
                "class A extends B { m() { super() } }",
                26,
                ParseErrorKind::InvalidSuper,
            ),
            (
                "class A extends B { x = super() }",
                24,
                ParseErrorKind::InvalidSuper,
            ),
            (
                "class A { x = arguments }",
                14,
                ParseErrorKind::InvalidArguments,
            ),
            (
                "class A { static { arguments } }",
                19,
                ParseErrorKind::InvalidArguments,
            ),
            (
                "class A { static { await } }",
                19,
                ParseErrorKind::ReservedWord,
            ),
            (
                "class A { static x = await }",
                21,
                ParseErrorKind::ReservedWord,
            ),
            (
                "class A { static { return } }",
                19,
                ParseErrorKind::InvalidReturn,
            ),
            (
                "class A { m() { with (a) {} } }",
                16,
                ParseErrorKind::StrictMode(frameshift_engine::StrictModeError::With),
            ),
            ("class let {}", 6, ParseErrorKind::ReservedWord),
            (
                "class eval {}",
                6,
                ParseErrorKind::StrictMode(frameshift_engine::StrictModeError::RestrictedName),
            ),
            ("class A { a b }", 12, ParseErrorKind::ExpectedSemicolon),
            (
                "class A { get x }",
                16,
                ParseErrorKind::ExpectedPunctuator(frameshift_engine::Punctuator::LeftParen),
            ),
            (
                "class A extends a, b {}",
                17,
                ParseErrorKind::ExpectedPunctuator(frameshift_engine::Punctuator::LeftBrace),
            ),
            ("class {}", 6, ParseErrorKind::ExpectedIdentifier),
        ],
    );
}

#[test]
fn imports_record_specifiers_attributes_and_bindings() {
    let input = source(
        "import def, * as ns from 'a';\nimport { b, c as d, 'e f' as g, default as h } from \"b\" with { type: 'json' };\nimport 'c';",
    );
    let tree = parse(MODULE, &input);
    let items = body(&tree);
    let SyntaxNodeKind::ImportDeclaration {
        specifiers,
        source: specifier,
        attributes: None,
    } = kind(&tree, items[0])
    else {
        panic!("expected an import declaration");
    };
    assert_eq!(text(&input, &tree, specifier), "'a'");
    let specifiers: Vec<_> = tree
        .children(specifiers)
        .unwrap()
        .iter()
        .map(|&id| kind(&tree, id))
        .collect();
    assert!(matches!(
        specifiers[0],
        SyntaxNodeKind::ImportDefaultSpecifier { .. }
    ));
    assert!(matches!(
        specifiers[1],
        SyntaxNodeKind::ImportNamespaceSpecifier { .. }
    ));
    let SyntaxNodeKind::ImportDeclaration {
        specifiers,
        attributes: Some(attributes),
        ..
    } = kind(&tree, items[1])
    else {
        panic!("expected an import with attributes");
    };
    assert_eq!(text(&input, &tree, attributes), "with { type: 'json' }");
    let named = tree.children(specifiers).unwrap();
    assert!(matches!(
        kind(&tree, named[0]),
        SyntaxNodeKind::ImportSpecifier { imported: None, .. }
    ));
    let SyntaxNodeKind::ImportSpecifier {
        imported: Some(imported),
        local,
    } = kind(&tree, named[2])
    else {
        panic!("expected a renamed import");
    };
    assert_eq!(text(&input, &tree, imported), "'e f'");
    assert_eq!(text(&input, &tree, local), "g");
    assert!(matches!(
        kind(&tree, items[2]),
        SyntaxNodeKind::ImportDeclaration { .. }
    ));

    assert_valid(
        MODULE,
        &[
            "import {} from 'a'",
            "import { a, } from 'a'",
            "import a from 'a'\nwith { type: 'json' }",
            "import a from 'a' with {}",
            "import { if as x } from 'a'",
            "import.meta.url",
            "await import('a')",
        ],
    );
    assert_failures(
        MODULE,
        &[
            (
                "import a from 'a'; import a from 'b'",
                26,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "import a from 'a'; var a",
                23,
                ParseErrorKind::DuplicateDeclaration,
            ),
            (
                "import { default } from 'a'",
                9,
                ParseErrorKind::ReservedWord,
            ),
            ("import { 's' } from 'a'", 13, ParseErrorKind::ExpectedAs),
            ("import * from 'a'", 9, ParseErrorKind::ExpectedAs),
            ("import a 'a'", 9, ParseErrorKind::ExpectedFrom),
            (
                "import a from b",
                14,
                ParseErrorKind::ExpectedModuleSpecifier,
            ),
            (
                "import a from 'a' with { type: 'x', type: 'y' }",
                36,
                ParseErrorKind::DuplicateImportAttribute,
            ),
            (
                "import a from 'a' with { type: 1 }",
                31,
                ParseErrorKind::ExpectedModuleSpecifier,
            ),
            (
                "import a from 'a' assert { type: 'json' }",
                18,
                ParseErrorKind::ExpectedSemicolon,
            ),
            (
                "import { eval } from 'a'",
                9,
                ParseErrorKind::StrictMode(frameshift_engine::StrictModeError::RestrictedName),
            ),
            (
                "{ import a from 'a' }",
                2,
                ParseErrorKind::ModuleDeclarationOutsideModule,
            ),
            ("x <!-- y", 2, ParseErrorKind::HtmlCommentInModule),
        ],
    );
    assert_failures(
        SCRIPT,
        &[
            (
                "import a from 'a'",
                0,
                ParseErrorKind::ModuleDeclarationOutsideModule,
            ),
            (
                "export var a",
                0,
                ParseErrorKind::ModuleDeclarationOutsideModule,
            ),
            ("import.meta", 0, ParseErrorKind::ImportMetaOutsideModule),
        ],
    );
    assert_valid(SCRIPT, &["import('a')", "x <!-- y"]);
}

#[test]
fn exports_track_names_bindings_and_defaults() {
    let input = source(
        "export const [a, { b }] = c, d = 1;\nexport function e() {}\nexport default class {}\nexport { a as 'f', b as g } \nexport * as h from 'm';\nexport { x as y } from 'n';",
    );
    let tree = parse(MODULE, &input);
    let items = body(&tree);
    let SyntaxNodeKind::ExportDeclaration { declaration } = kind(&tree, items[0]) else {
        panic!("expected an exported declaration");
    };
    assert!(matches!(
        kind(&tree, declaration),
        SyntaxNodeKind::VariableDeclaration {
            kind: VariableKind::Const,
            ..
        }
    ));
    let SyntaxNodeKind::ExportDefault { declaration } = kind(&tree, items[2]) else {
        panic!("expected a default export");
    };
    assert!(matches!(
        kind(&tree, declaration),
        SyntaxNodeKind::ClassDeclaration { name: None, .. }
    ));
    let SyntaxNodeKind::ExportNamed {
        specifiers,
        source: None,
        ..
    } = kind(&tree, items[3])
    else {
        panic!("expected a local export list");
    };
    let first = tree.children(specifiers).unwrap()[0];
    let SyntaxNodeKind::ExportSpecifier {
        local,
        exported: Some(_),
    } = kind(&tree, first)
    else {
        panic!("expected a renamed export");
    };
    // Without `from`, the local name is a reference to a binding.
    assert!(matches!(
        kind(&tree, local),
        SyntaxNodeKind::Identifier { .. }
    ));
    assert!(matches!(
        kind(&tree, items[4]),
        SyntaxNodeKind::ExportAll {
            exported: Some(_),
            ..
        }
    ));
    let SyntaxNodeKind::ExportNamed {
        specifiers,
        source: Some(_),
        ..
    } = kind(&tree, items[5])
    else {
        panic!("expected a re-export");
    };
    let first = tree.children(specifiers).unwrap()[0];
    let SyntaxNodeKind::ExportSpecifier { local, .. } = kind(&tree, first) else {
        panic!("expected an export specifier");
    };
    assert!(matches!(
        kind(&tree, local),
        SyntaxNodeKind::IdentifierName { .. }
    ));

    assert_valid(
        MODULE,
        &[
            "export default function () {}",
            "export default async function () {}",
            "export default function* g() {}",
            "export default (1, 2);",
            "export default async () => 1;",
            "var a; export { a as default }",
            "export { if, 'x y' as z } from 'm'",
            "export {}",
            "export { a }; { var a }",
            "export async function f() {}",
            "export * from 'm' with { type: 'json' }",
        ],
    );
    assert_failures(
        MODULE,
        &[
            (
                "export default 1; export default 2",
                25,
                ParseErrorKind::DuplicateExport,
            ),
            (
                "var a; export { a, a }",
                19,
                ParseErrorKind::DuplicateExport,
            ),
            (
                "export var a; export { a }; ",
                23,
                ParseErrorKind::DuplicateExport,
            ),
            (
                "export * as default from 'm'; export default 1",
                37,
                ParseErrorKind::DuplicateExport,
            ),
            ("export { a }", 9, ParseErrorKind::UndeclaredExport),
            (
                "export { a }; { let a }",
                9,
                ParseErrorKind::UndeclaredExport,
            ),
            ("export { 'a' }", 9, ParseErrorKind::InvalidLocalExport),
            ("export { if }", 9, ParseErrorKind::InvalidLocalExport),
            (
                "export { let }; var let",
                9,
                ParseErrorKind::InvalidLocalExport,
            ),
            (
                "var a; export { a as '\\uD800' }",
                21,
                ParseErrorKind::InvalidModuleExportName,
            ),
            (
                "export using a = b",
                7,
                ParseErrorKind::InvalidUsingDeclaration,
            ),
            ("export a", 7, ParseErrorKind::ExpectedExpression),
            (
                "export * from m",
                14,
                ParseErrorKind::ExpectedModuleSpecifier,
            ),
            (
                "export async\nfunction f() {}",
                7,
                ParseErrorKind::ExpectedExpression,
            ),
            (
                "function f() {} export { f }; function f() {}",
                39,
                ParseErrorKind::DuplicateDeclaration,
            ),
        ],
    );
}

#[test]
fn module_code_is_strict_and_permits_top_level_await() {
    let input = source("await a; for await (const b of c) ; await using d = e;");
    let tree = parse(MODULE, &input);
    assert!(matches!(
        kind(&tree, tree.root()),
        SyntaxNodeKind::Program { strict: true, .. }
    ));
    assert!(matches!(
        kind(&tree, body(&tree)[0]),
        SyntaxNodeKind::ExpressionStatement { .. }
    ));
    assert_failures(
        MODULE,
        &[
            ("var await", 4, ParseErrorKind::ReservedWord),
            ("function f() { await a }", 15, ParseErrorKind::ReservedWord),
            (
                "with (a) b",
                0,
                ParseErrorKind::StrictMode(frameshift_engine::StrictModeError::With),
            ),
            ("let = 1", 0, ParseErrorKind::ReservedWord),
            (
                "017",
                0,
                ParseErrorKind::StrictMode(frameshift_engine::StrictModeError::LegacyOctalLiteral),
            ),
            (
                "if (a) function f() {}",
                7,
                ParseErrorKind::DeclarationInStatementPosition,
            ),
            // Without `using` on the same line, `await using` is an expression.
            ("await\nusing a = b", 12, ParseErrorKind::ExpectedSemicolon),
        ],
    );
    assert_valid(SCRIPT, &["var await; await = 1; function f() { await }"]);
}
