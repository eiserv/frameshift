//! Punctuation grammar contracts through the engine facade, using fictional text.

use frameshift_engine::{
    GreaterThanMode, JsLexicalGoal, LegacyOctal, LexicalError, LexicalErrorKind, LexicalLimits,
    Punctuator, PunctuatorContext, SourceGoal, SourceText, Span, TextLimits, scan_js_number,
    scan_js_punctuator, scan_js_trivia,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn context(goal: JsLexicalGoal) -> PunctuatorContext {
    PunctuatorContext {
        goal,
        greater_than: GreaterThanMode::LongestMatch,
    }
}

fn punctuation() -> Vec<(&'static str, Punctuator)> {
    use Punctuator::*;
    vec![
        ("{", LeftBrace),
        ("(", LeftParen),
        (")", RightParen),
        ("[", LeftBracket),
        ("]", RightBracket),
        (".", Dot),
        ("...", Ellipsis),
        (";", Semicolon),
        (",", Comma),
        ("<", LessThan),
        (">", GreaterThan),
        ("<=", LessThanEqual),
        (">=", GreaterThanEqual),
        ("==", Equal),
        ("!=", NotEqual),
        ("===", StrictEqual),
        ("!==", StrictNotEqual),
        ("+", Plus),
        ("-", Minus),
        ("*", Star),
        ("%", Percent),
        ("**", Exponent),
        ("++", Increment),
        ("--", Decrement),
        ("<<", ShiftLeft),
        (">>", ShiftRight),
        (">>>", ShiftRightUnsigned),
        ("&", BitAnd),
        ("|", BitOr),
        ("^", BitXor),
        ("!", Bang),
        ("~", Tilde),
        ("&&", LogicalAnd),
        ("||", LogicalOr),
        ("??", Nullish),
        ("?", Question),
        (":", Colon),
        ("=", Assign),
        ("+=", PlusAssign),
        ("-=", MinusAssign),
        ("*=", StarAssign),
        ("%=", PercentAssign),
        ("**=", ExponentAssign),
        ("<<=", ShiftLeftAssign),
        (">>=", ShiftRightAssign),
        (">>>=", ShiftRightUnsignedAssign),
        ("&=", BitAndAssign),
        ("|=", BitOrAssign),
        ("^=", BitXorAssign),
        ("&&=", LogicalAndAssign),
        ("||=", LogicalOrAssign),
        ("??=", NullishAssign),
        ("=>", Arrow),
        ("?.", OptionalChain),
        ("/", Slash),
        ("/=", SlashAssign),
        ("}", RightBrace),
    ]
}

#[test]
fn every_ecmascript_punctuator_has_an_exact_kind_and_span() {
    for (spelling, kind) in punctuation() {
        let text = format!("é\r\n{spelling} tail");
        let input = source(&text);
        let token = scan_js_punctuator(&input, 4, context(JsLexicalGoal::Div))
            .unwrap()
            .unwrap();
        assert_eq!(token.kind, kind, "{spelling}");
        assert_eq!(token.span, Span::new(4, 4 + spelling.len()).unwrap());
        assert_eq!(input.slice(token.span).unwrap(), spelling);
        assert_eq!(kind.spelling(), spelling);
        assert_eq!(input.as_str(), text);
    }
}

#[test]
fn operator_pairs_choose_the_longest_grammar_spelling() {
    let grammar = punctuation();
    for (left, _) in &grammar {
        for (right, _) in &grammar {
            let text = format!("{left}{right}");
            let input = source(&text);
            let result = scan_js_punctuator(&input, 0, context(JsLexicalGoal::Div)).unwrap();
            if text.starts_with("//") || text.starts_with("/*") {
                assert!(result.is_none(), "{text}");
                continue;
            }
            let &(spelling, kind) = grammar
                .iter()
                .filter(|(spelling, _)| text.starts_with(spelling))
                .max_by_key(|(spelling, _)| spelling.len())
                .unwrap();
            let token = result.unwrap();
            assert_eq!(token.kind, kind, "{text}");
            assert_eq!(input.slice(token.span).unwrap(), spelling);
        }
    }
}

#[test]
fn parser_goals_control_division_and_right_brace_without_guessing() {
    for (goal, division, brace) in [
        (JsLexicalGoal::Div, true, true),
        (JsLexicalGoal::RegExp, false, true),
        (JsLexicalGoal::RegExpOrTemplateTail, false, false),
        (JsLexicalGoal::TemplateTail, true, false),
    ] {
        for (text, kind) in [
            ("/", Punctuator::Slash),
            ("/=", Punctuator::SlashAssign),
            ("/expression/", Punctuator::Slash),
        ] {
            let input = source(text);
            let token = scan_js_punctuator(&input, 0, context(goal)).unwrap();
            assert_eq!(token.is_some(), division, "{goal:?}: {text}");
            if let Some(token) = token {
                assert_eq!(token.kind, kind);
            }
        }
        for text in ["}", "}template`", "}${tail}"] {
            let token = scan_js_punctuator(&source(text), 0, context(goal)).unwrap();
            assert_eq!(token.is_some(), brace, "{goal:?}: {text}");
            if let Some(token) = token {
                assert_eq!(token.kind, Punctuator::RightBrace);
            }
        }
        for text in ["//", "//comment", "/*", "/*comment*/"] {
            assert!(
                scan_js_punctuator(&source(text), 0, context(goal))
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(
            scan_js_punctuator(&source("+"), 0, context(goal))
                .unwrap()
                .unwrap()
                .kind,
            Punctuator::Plus
        );
    }
}

#[test]
fn optional_chaining_and_numeric_dots_obey_decimal_lookahead() {
    for digit in '0'..='9' {
        let text = format!("?.{digit}");
        let input = source(&text);
        let question = scan_js_punctuator(&input, 0, context(JsLexicalGoal::Div))
            .unwrap()
            .unwrap();
        assert_eq!(question.kind, Punctuator::Question);
        assert_eq!(question.span.end(), 1);
        assert!(
            scan_js_punctuator(&input, 1, context(JsLexicalGoal::Div))
                .unwrap()
                .is_none()
        );
        let number =
            scan_js_number(&input, 1, LegacyOctal::Reject, LexicalLimits::default()).unwrap();
        assert_eq!(input.slice(number.span).unwrap(), format!(".{digit}"));
    }
    for text in [
        "?.",
        "?.field",
        "?.[index]",
        "?.(arg)",
        "?.\u{660}",
        "?.\u{2028}",
    ] {
        let token = scan_js_punctuator(&source(text), 0, context(JsLexicalGoal::Div))
            .unwrap()
            .unwrap();
        assert_eq!(token.kind, Punctuator::OptionalChain, "{text}");
        assert_eq!(token.span.end(), 2);
    }
    for (text, kind) in [
        (".", Punctuator::Dot),
        ("..", Punctuator::Dot),
        ("...", Punctuator::Ellipsis),
        ("...1", Punctuator::Ellipsis),
        (".é", Punctuator::Dot),
    ] {
        assert_eq!(
            scan_js_punctuator(&source(text), 0, context(JsLexicalGoal::Div))
                .unwrap()
                .unwrap()
                .kind,
            kind
        );
    }
}

#[test]
fn type_closing_mode_splits_one_greater_than_then_can_resume_normal_scanning() {
    let type_context = PunctuatorContext {
        goal: JsLexicalGoal::Div,
        greater_than: GreaterThanMode::Single,
    };
    for text in [">", ">=", ">>", ">>=", ">>>", ">>>="] {
        let input = source(text);
        let token = scan_js_punctuator(&input, 0, type_context)
            .unwrap()
            .unwrap();
        assert_eq!(token.kind, Punctuator::GreaterThan);
        assert_eq!(token.span.end(), 1);
    }
    let input = source(">>>=");
    let first = scan_js_punctuator(&input, 0, type_context)
        .unwrap()
        .unwrap();
    let second = scan_js_punctuator(&input, first.span.end(), type_context)
        .unwrap()
        .unwrap();
    let final_token = scan_js_punctuator(&input, second.span.end(), context(JsLexicalGoal::Div))
        .unwrap()
        .unwrap();
    assert_eq!(first.kind, Punctuator::GreaterThan);
    assert_eq!(second.kind, Punctuator::GreaterThan);
    assert_eq!(final_token.kind, Punctuator::GreaterThanEqual);
    assert_eq!(final_token.span, Span::new(2, 4).unwrap());
    assert_eq!(
        scan_js_punctuator(&source("<<="), 0, type_context)
            .unwrap()
            .unwrap()
            .kind,
        Punctuator::ShiftLeftAssign
    );
}

#[test]
fn non_punctuation_and_invalid_offsets_return_explicit_results() {
    for text in [
        "",
        " ",
        "\u{feff}",
        "\r\n",
        "é",
        "🦀",
        "word",
        "1",
        ".1",
        "'x'",
        "`x`",
        "#name",
        "@decorator",
        "\\u0061",
    ] {
        assert!(
            scan_js_punctuator(&source(text), 0, context(JsLexicalGoal::Div))
                .unwrap()
                .is_none(),
            "{text}"
        );
    }
    let input = source("é>=");
    for offset in [1, input.len() + 1, usize::MAX] {
        assert_eq!(
            scan_js_punctuator(&input, offset, context(JsLexicalGoal::Div)),
            Err(LexicalError {
                offset,
                kind: LexicalErrorKind::InvalidOffset(input.validate_offset(offset).unwrap_err())
            })
        );
    }
    assert!(
        scan_js_punctuator(&input, input.len(), context(JsLexicalGoal::Div))
            .unwrap()
            .is_none()
    );
}

#[test]
fn trivia_numbers_and_punctuation_share_source_coordinates() {
    let input = source("/* keep */\r\n(1_0 + .5) >= 0xA;");
    let mut cursor = 0;
    for spelling in ["(", "1_0", "+", ".5", ")", ">=", "0xA", ";"] {
        cursor = scan_js_trivia(&input, cursor, SourceGoal::Module, LexicalLimits::default())
            .unwrap()
            .span
            .end();
        let span = if spelling.as_bytes()[0].is_ascii_digit() || spelling.starts_with('.') {
            scan_js_number(
                &input,
                cursor,
                LegacyOctal::Reject,
                LexicalLimits::default(),
            )
            .unwrap()
            .span
        } else {
            scan_js_punctuator(&input, cursor, context(JsLexicalGoal::Div))
                .unwrap()
                .unwrap()
                .span
        };
        assert_eq!(input.slice(span).unwrap(), spelling);
        cursor = span.end();
    }
    assert_eq!(cursor, input.len());
    assert_eq!(input.as_str(), "/* keep */\r\n(1_0 + .5) >= 0xA;");
}

#[test]
fn generated_punctuation_is_deterministic_in_every_goal() {
    let alphabet = ['>', '/', '.', '*', '?', '0', '}', '=', 'é', '\u{2028}'];
    for length in 0..=4 {
        for mut index in 0..alphabet.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push(alphabet[index % alphabet.len()]);
                index /= alphabet.len();
            }
            let input = source(&text);
            for offset in text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([text.len()])
            {
                for goal in [
                    JsLexicalGoal::Div,
                    JsLexicalGoal::RegExp,
                    JsLexicalGoal::RegExpOrTemplateTail,
                    JsLexicalGoal::TemplateTail,
                ] {
                    for greater_than in [GreaterThanMode::LongestMatch, GreaterThanMode::Single] {
                        let context = PunctuatorContext { goal, greater_than };
                        let result = scan_js_punctuator(&input, offset, context);
                        assert_eq!(result, scan_js_punctuator(&input, offset, context));
                        if let Some(token) = result.unwrap() {
                            assert_eq!(token.span.start(), offset);
                            assert!((1..=4).contains(&token.span.len()));
                            assert_eq!(input.slice(token.span).unwrap(), token.kind.spelling());
                        }
                    }
                }
            }
            assert_eq!(input.as_str(), text);
        }
    }
}
