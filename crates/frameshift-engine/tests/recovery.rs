//! Bounded JavaScript diagnostic recovery through the public syntax facade.

use frameshift_engine::syntax::{
    JsDialect, ParseErrorKind, ParseLimits, RecoveryLimits, RecoveryStopKind, SourceGoal,
    StrictModeError, parse_js_program, parse_js_program_with_recovery,
};
use frameshift_engine::{SourceText, TextLimits};

fn make_source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

#[test]
fn valid_programs_keep_the_strict_tree_in_every_dialect() {
    for goal in [SourceGoal::Script, SourceGoal::Module] {
        for (dialect, text) in [
            (
                JsDialect::JavaScript,
                "#!/bin/js\n'use strict'; const x = /;/; f(`a${x}`);",
            ),
            (JsDialect::Jsx, "const x = <p title=';'>a</p>;"),
            (JsDialect::TypeScript, "const x: Map<string, number> = f();"),
            (JsDialect::Tsx, "const x: unknown = <P/>;"),
            (JsDialect::TypeScriptDeclaration, "declare const x: string;"),
        ] {
            let source = make_source(text);
            let strict = parse_js_program(&source, goal, dialect, ParseLimits::default()).unwrap();
            let result = parse_js_program_with_recovery(
                &source,
                goal,
                dialect,
                ParseLimits::default(),
                RecoveryLimits::default(),
            )
            .unwrap();
            assert!(result.diagnostics().is_empty());
            assert_eq!(result.stop(), None);
            let tree = result.tree().unwrap();
            assert_eq!(tree.nodes(), strict.nodes());
            assert_eq!(tree.comments(), strict.comments());
            let mut stack = vec![tree.root()];
            while let Some(id) = stack.pop() {
                let children = tree.child_nodes(id).unwrap();
                assert!(children.clone().eq(strict.child_nodes(id).unwrap()));
                stack.extend(children);
                if let frameshift_engine::SyntaxNodeKind::TemplateLiteral { parts } =
                    tree.node(id).unwrap().kind()
                {
                    assert_eq!(tree.template_parts(parts), strict.template_parts(parts));
                }
            }
            assert_eq!(result.into_tree().unwrap().nodes(), strict.nodes());
        }
    }
}

#[test]
fn multiple_errors_have_original_offsets_and_never_expose_a_partial_tree() {
    for goal in [SourceGoal::Script, SourceGoal::Module] {
        for dialect in [
            JsDialect::JavaScript,
            JsDialect::Jsx,
            JsDialect::TypeScript,
            JsDialect::Tsx,
        ] {
            let text = "const 色 = ;\r\n/* ; */ const b = ;\nvalid(); const c = ;";
            let source = make_source(text);
            let result = parse_js_program_with_recovery(
                &source,
                goal,
                dialect,
                ParseLimits::default(),
                RecoveryLimits::default(),
            )
            .unwrap();
            assert_eq!(result.diagnostics().len(), 3, "{dialect:?}");
            assert_eq!(
                result.diagnostics()[0],
                parse_js_program(&source, goal, dialect, ParseLimits::default())
                    .err()
                    .unwrap()
            );
            let offsets: Vec<_> = result.diagnostics().iter().map(|e| e.offset).collect();
            assert_eq!(
                offsets,
                [
                    text.find(";\r").unwrap(),
                    text.find(";\n").unwrap(),
                    text.len() - 1
                ]
            );
            for error in result.diagnostics() {
                source.validate_offset(error.offset).unwrap();
            }
            assert!(result.tree().is_none());
            assert!(result.into_tree().is_none());
        }
    }
}

#[test]
fn synchronization_keeps_strings_comments_and_balanced_groups_whole() {
    for text in [
        "const a = [';', (1 + )]; /* ; */ const b = ;",
        "const a = {x: ';', y: (1 + )}; // ;\nconst b = ;",
        "call(';', (1 + )); const b = ;",
        "const a = ['a\\\r\nb', (1 + )]; const b = ;",
    ] {
        let source = make_source(text);
        let result = parse_js_program_with_recovery(
            &source,
            SourceGoal::Module,
            JsDialect::JavaScript,
            ParseLimits::default(),
            RecoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(result.diagnostics().len(), 2, "{text}: {:?}", result.stop());
        assert_eq!(result.diagnostics()[1].offset, text.len() - 1);
        assert_eq!(result.stop(), None);
    }
}

#[test]
fn unknown_lexical_or_statement_context_stops_without_guessing() {
    for (text, dialect) in [
        ("const a = /;/ + ; const b = ;", JsDialect::JavaScript),
        ("const a = `;` + ; const b = ;", JsDialect::JavaScript),
        ("const a = (1 + ; const b = ;", JsDialect::JavaScript),
        (
            "if (x) call(1 + ); else other(); const b = ;",
            JsDialect::JavaScript,
        ),
        ("{ const a = ; } const b = ;", JsDialect::JavaScript),
        ("a: call(1 + ); const b = ;", JsDialect::JavaScript),
        (
            "x +\nif (x) a(); else b(); const c = ;",
            JsDialect::JavaScript,
        ),
        ("const a = <P></Q>; const b = ;", JsDialect::Jsx),
        ("const a: T<U> = ; const b = ;", JsDialect::TypeScript),
        ("const a = '\n; const b = ;", JsDialect::JavaScript),
    ] {
        let source = make_source(text);
        let result = parse_js_program_with_recovery(
            &source,
            SourceGoal::Module,
            dialect,
            ParseLimits::default(),
            RecoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(result.diagnostics().len(), 1, "{text}");
        assert_eq!(
            result.stop().unwrap().kind,
            RecoveryStopKind::UnsafeBoundary,
            "{text}"
        );
        assert!(result.tree().is_none());
    }
}

#[test]
fn root_strictness_survives_and_later_strings_are_not_directives() {
    let source = make_source("'use strict'; const a = ; with (o) {} const b = ;");
    let result = parse_js_program_with_recovery(
        &source,
        SourceGoal::Script,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(result.diagnostics().len(), 2);
    assert_eq!(
        result.diagnostics()[1].kind,
        ParseErrorKind::StrictMode(StrictModeError::With)
    );
    let source = make_source("const a = ; 'use strict'; with (o) {} const b = ;");
    let result = parse_js_program_with_recovery(
        &source,
        SourceGoal::Script,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(result.diagnostics().len(), 2);
    assert_eq!(
        result.diagnostics()[1].kind,
        ParseErrorKind::ExpectedExpression
    );
}

#[test]
fn fresh_scopes_do_not_invent_undeclared_export_diagnostics() {
    let text = "const known = 1; const bad = ; export { known }; const later = ;";
    let source = make_source(text);
    let result = parse_js_program_with_recovery(
        &source,
        SourceGoal::Module,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(result.diagnostics().len(), 2);
    assert!(
        result
            .diagnostics()
            .iter()
            .all(|e| e.kind == ParseErrorKind::ExpectedExpression)
    );
    assert_eq!(result.stop(), None);
    // A deferred error pointing behind the current item cannot be resumed.
    let source = make_source("export {missing}; const later = 1;");
    let result = parse_js_program_with_recovery(
        &source,
        SourceGoal::Module,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(
        result.diagnostics()[0].kind,
        ParseErrorKind::UndeclaredExport
    );
    assert_eq!(
        result.stop().unwrap().kind,
        RecoveryStopKind::UnsafeBoundary
    );
}

#[test]
fn diagnostic_work_and_parse_budgets_are_independent_and_cumulative() {
    let source = make_source("const a=;const b=;const c=;");
    let parse = |limits, recovery| {
        parse_js_program_with_recovery(
            &source,
            SourceGoal::Module,
            JsDialect::JavaScript,
            limits,
            recovery,
        )
        .unwrap()
    };
    let result = parse(
        ParseLimits::default(),
        RecoveryLimits {
            max_diagnostics: 2,
            ..RecoveryLimits::default()
        },
    );
    assert_eq!(result.diagnostics().len(), 2);
    assert_eq!(
        result.stop().unwrap().kind,
        RecoveryStopKind::DiagnosticLimit
    );
    let result = parse(
        ParseLimits::default(),
        RecoveryLimits {
            max_work: 9,
            ..RecoveryLimits::default()
        },
    );
    assert_eq!(result.diagnostics().len(), 2);
    assert_eq!(result.stop().unwrap().kind, RecoveryStopKind::WorkLimit);
    assert_eq!(result.stop().unwrap().offset, 9);
    let result = parse(
        ParseLimits {
            max_tokens: 3,
            ..ParseLimits::default()
        },
        RecoveryLimits::default(),
    );
    assert_eq!(result.stop().unwrap().kind, RecoveryStopKind::ParseFailure);
    assert_eq!(
        result.diagnostics().last().unwrap().kind,
        ParseErrorKind::ResourceLimit
    );
    assert!(
        parse_js_program_with_recovery(
            &source,
            SourceGoal::Module,
            JsDialect::JavaScript,
            ParseLimits::default(),
            RecoveryLimits {
                max_diagnostics: 0,
                ..RecoveryLimits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn many_errors_are_deterministic_and_bounded() {
    let source = make_source(&"const bad=;".repeat(1_000));
    let result = parse_js_program_with_recovery(
        &source,
        SourceGoal::Module,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(result.diagnostics().len(), 256);
    assert_eq!(
        result.stop().unwrap().kind,
        RecoveryStopKind::DiagnosticLimit
    );
    let repeat = parse_js_program_with_recovery(
        &source,
        SourceGoal::Module,
        JsDialect::JavaScript,
        ParseLimits::default(),
        RecoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(result.diagnostics(), repeat.diagnostics());
    assert_eq!(result.stop(), repeat.stop());
}
