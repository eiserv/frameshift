//! Identifier and reserved-word contracts through the public facade, using fictional text.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, IdentifierKind, LegacyOctal, LexicalError, LexicalErrorKind,
    LexicalLimits, ReservedWord, ReservedWordClass, SourceText, Span, TextLimits, apply_edits,
    scan_js_identifier, scan_js_number,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

fn reserved_words() -> Vec<(&'static str, ReservedWord, ReservedWordClass)> {
    use ReservedWord::*;
    use ReservedWordClass::{Always, Contextual, Strict};
    vec![
        ("await", Await, Contextual),
        ("break", Break, Always),
        ("case", Case, Always),
        ("catch", Catch, Always),
        ("class", Class, Always),
        ("const", Const, Always),
        ("continue", Continue, Always),
        ("debugger", Debugger, Always),
        ("default", Default, Always),
        ("delete", Delete, Always),
        ("do", Do, Always),
        ("else", Else, Always),
        ("enum", Enum, Always),
        ("export", Export, Always),
        ("extends", Extends, Always),
        ("false", False, Always),
        ("finally", Finally, Always),
        ("for", For, Always),
        ("function", Function, Always),
        ("if", If, Always),
        ("import", Import, Always),
        ("in", In, Always),
        ("instanceof", Instanceof, Always),
        ("new", New, Always),
        ("null", Null, Always),
        ("return", Return, Always),
        ("super", Super, Always),
        ("switch", Switch, Always),
        ("this", This, Always),
        ("throw", Throw, Always),
        ("true", True, Always),
        ("try", Try, Always),
        ("typeof", Typeof, Always),
        ("var", Var, Always),
        ("void", Void, Always),
        ("while", While, Always),
        ("with", With, Always),
        ("yield", Yield, Contextual),
        ("implements", Implements, Strict),
        ("interface", Interface, Strict),
        ("let", Let, Strict),
        ("package", Package, Strict),
        ("private", Private, Strict),
        ("protected", Protected, Strict),
        ("public", Public, Strict),
        ("static", Static, Strict),
    ]
}

#[test]
fn names_keep_exact_unicode_spelling_and_end_at_non_parts() {
    for name in [
        "x",
        "$",
        "_",
        "_private$1",
        "café",
        "変数",
        "a\u{200c}b\u{200d}",
        "\u{2118}x",
        "x\u{b7}y",
        "e\u{301}",
        "\u{3d000}",
    ] {
        for suffix in [
            "", " ", "-b", ".b", "(", "€", "🦀", "\u{2028}", "\u{2e2f}", "\u{a0}", "'s'",
        ] {
            let text = format!("é\r\n{name}{suffix}");
            let input = source(&text);
            let offset = "é\r\n".len();
            let token = scan_js_identifier(&input, offset, LexicalLimits::default()).unwrap();
            let expected = Span::new(offset, offset + name.len()).unwrap();
            assert_eq!(token.kind, IdentifierKind::Name);
            assert_eq!(token.span, expected, "{name:?}{suffix:?}");
            assert_eq!(token.name_span, expected);
            assert!(!token.has_escape);
            assert_eq!(token.reserved_word, None);
            assert_eq!(input.as_str(), text);
        }
    }
}

#[test]
fn escapes_must_denote_code_points_permitted_at_their_position() {
    for (name, length) in [
        (r"\u0061bc", 8),
        (r"a\u{62}c", 8),
        (r"\u{00000000061}", 15),
        (r"\u{2118}", 8),
        (r"x\u200C\u{200d}", 15),
        (r"\u{3D000}\u0301", 15),
        (r"\u0024\u005f", 12),
    ] {
        let input = source(&format!("{name}+"));
        let token = scan_js_identifier(&input, 0, LexicalLimits::default()).unwrap();
        assert_eq!(token.span, Span::new(0, length).unwrap(), "{name}");
        assert!(token.has_escape);
    }
    for (text, offset) in [
        (r"\u0030abc", 0),
        (r"\u{301}", 0),
        (r"\u{200c}", 0),
        (r"\x61", 0),
        (r"\", 0),
        (r"a\u0020", 1),
        (r"a\u{2e2f}", 1),
        (r"a\x61", 1),
        (r"a\u", 1),
        (r"a\u00", 1),
        (r"a\u00g0", 1),
        (r"a\u{}", 1),
        (r"a\u{61", 1),
        (r"a\u{110000}", 1),
        (r"a\u{FFFFFFFFFFFF}", 1),
        (r"a\uD800", 1),
        (r"a\u{dfff}", 1),
        (r"a\u{20}b", 1),
        (r"ab\\", 2),
        (r"a\u0062\n", 7),
    ] {
        assert_eq!(
            scan_js_identifier(&source(text), 0, LexicalLimits::default()),
            Err(error(offset, LexicalErrorKind::InvalidIdentifierEscape)),
            "{text}"
        );
    }
    let zeros = format!(r"\u{{{}61}}", "0".repeat(100_000));
    let input = source(&zeros);
    assert_eq!(
        scan_js_identifier(&input, 0, LexicalLimits::default())
            .unwrap()
            .span
            .len(),
        zeros.len()
    );
}

#[test]
fn private_names_exclude_the_hash_from_the_name_span() {
    for (text, name_end) in [("#x", 2), ("#if", 3), (r"#\u0078", 7), ("#é$", 4)] {
        let input = source(&format!("{text}()"));
        let token = scan_js_identifier(&input, 0, LexicalLimits::default()).unwrap();
        assert_eq!(token.kind, IdentifierKind::PrivateName);
        assert_eq!(token.span, Span::new(0, name_end).unwrap());
        assert_eq!(token.name_span, Span::new(1, name_end).unwrap());
        assert_eq!(token.reserved_word, None, "{text}");
    }
    for text in ["#", "# x", "#1", "#-", "#\u{200c}", "##x"] {
        assert_eq!(
            scan_js_identifier(&source(text), 0, LexicalLimits::default()),
            Err(error(1, LexicalErrorKind::ExpectedIdentifier)),
            "{text}"
        );
    }
    assert_eq!(
        scan_js_identifier(&source(r"#\u0031"), 0, LexicalLimits::default()),
        Err(error(1, LexicalErrorKind::InvalidIdentifierEscape))
    );
}

#[test]
fn reserved_words_follow_string_values_and_report_escapes() {
    for (spelling, word, class) in reserved_words() {
        assert_eq!(word.spelling(), spelling);
        assert_eq!(word.class(), class);
        let input = source(&format!("{spelling} "));
        let token = scan_js_identifier(&input, 0, LexicalLimits::default()).unwrap();
        assert_eq!(token.reserved_word, Some(word));
        assert!(!token.has_escape);
        assert_eq!(input.slice(token.span).unwrap(), spelling);
        // Escaping the first letter keeps the StringValue, which is what early
        // errors compare, while the flag tells the parser it is no keyword.
        let first = spelling.chars().next().unwrap();
        let escaped = format!(r"\u{{{:x}}}{}", u32::from(first), &spelling[1..]);
        let token = scan_js_identifier(&source(&escaped), 0, LexicalLimits::default()).unwrap();
        assert_eq!(token.reserved_word, Some(word), "{escaped}");
        assert!(token.has_escape);
    }
    for text in [
        "If",
        "iff",
        "instanceofx",
        "implementsx",
        "async",
        "of",
        "get",
        "set",
        "as",
        "from",
        "meta",
        "target",
        "undefined",
        "arguments",
        "eval",
        "type",
        "declare",
        "ifé",
        "i\u{200c}f",
        r"i\u0066\u200c",
    ] {
        assert_eq!(
            scan_js_identifier(&source(text), 0, LexicalLimits::default())
                .unwrap()
                .reserved_word,
            None,
            "{text}"
        );
    }
    let token = scan_js_identifier(&source(r"els\u{65}"), 0, LexicalLimits::default()).unwrap();
    assert_eq!(token.reserved_word, Some(ReservedWord::Else));
}

#[test]
fn only_exact_spellings_are_reserved_words() {
    let words = reserved_words();
    let expected = |candidate: &str| {
        words
            .iter()
            .find(|(spelling, _, _)| *spelling == candidate)
            .map(|&(_, word, _)| word)
    };
    for (spelling, _, _) in &words {
        let mut candidates = (1..=spelling.len())
            .map(|end| spelling[..end].to_owned())
            .collect::<Vec<_>>();
        candidates.push(format!("{spelling}s"));
        candidates.push(format!("_{spelling}"));
        candidates.push(format!("{spelling}{spelling}"));
        candidates.push(spelling.to_uppercase());
        let mut capitalized = String::from(*spelling);
        capitalized[..1].make_ascii_uppercase();
        candidates.push(capitalized);
        for candidate in candidates {
            let token =
                scan_js_identifier(&source(&candidate), 0, LexicalLimits::default()).unwrap();
            assert_eq!(token.reserved_word, expected(&candidate), "{candidate}");
        }
    }
}

#[test]
fn offsets_starts_and_byte_limits_are_checked_in_order() {
    for text in [
        "", " ", "1", "+", "-x", "'x'", "\u{200c}", "\u{301}", "€", "🦀",
    ] {
        assert_eq!(
            scan_js_identifier(&source(text), 0, LexicalLimits::default()),
            Err(error(0, LexicalErrorKind::ExpectedIdentifier)),
            "{text}"
        );
    }
    let input = source("éx");
    for offset in [1, input.len() + 1, usize::MAX] {
        assert_eq!(
            scan_js_identifier(
                &input,
                offset,
                LexicalLimits {
                    max_identifier_bytes: 0,
                    ..LexicalLimits::default()
                }
            ),
            Err(error(
                offset,
                LexicalErrorKind::InvalidOffset(input.validate_offset(offset).unwrap_err())
            ))
        );
    }
    for name in ["abc", "#ab", r"\u0061b", r"a\u{62}", "ab$"] {
        let text = format!("é{name};");
        let input = source(&text);
        for limit in 0..name.len() {
            let limits = LexicalLimits {
                max_identifier_bytes: limit,
                ..LexicalLimits::default()
            };
            assert_eq!(
                scan_js_identifier(&input, 2, limits),
                Err(error(2 + limit, LexicalErrorKind::IdentifierLimit)),
                "{name}, limit {limit}"
            );
        }
        let limits = LexicalLimits {
            max_identifier_bytes: name.len(),
            ..LexicalLimits::default()
        };
        assert_eq!(
            scan_js_identifier(&input, 2, limits).unwrap().span.end(),
            2 + name.len()
        );
    }
    // A multi-byte scalar that would cross the limit is not consumed.
    let limits = LexicalLimits {
        max_identifier_bytes: 2,
        ..LexicalLimits::default()
    };
    assert_eq!(
        scan_js_identifier(&source("aé"), 0, limits),
        Err(error(1, LexicalErrorKind::IdentifierLimit))
    );
    let failure =
        scan_js_identifier(&source(r"secret\x"), 0, LexicalLimits::default()).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "invalid identifier escape at byte offset 6"
    );
    assert!(!format!("{failure:?}").contains("secret"));
}

#[test]
fn numbers_and_names_share_the_identifier_start_boundary() {
    let input = source("3in x1 0x\u{2118}");
    assert_eq!(
        scan_js_number(&input, 0, LegacyOctal::Reject, LexicalLimits::default()),
        Err(error(1, LexicalErrorKind::InvalidNumericContinuation))
    );
    let name = scan_js_identifier(&input, 4, LexicalLimits::default()).unwrap();
    assert_eq!(input.slice(name.span).unwrap(), "x1");
    assert_eq!(
        scan_js_number(&input, 7, LegacyOctal::Reject, LexicalLimits::default()),
        Err(error(9, LexicalErrorKind::MissingNumericDigit))
    );
}

#[test]
fn an_escaped_name_span_can_drive_an_exact_edit() {
    let input = source("/* keep */\r\nconst \\u{61}bc = 1; // retained\r\n");
    let token = scan_js_identifier(&input, 18, LexicalLimits::default()).unwrap();
    assert!(token.has_escape);
    let rewritten = apply_edits(
        &input,
        &[Edit {
            span: token.span,
            replacement: "abc",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        }],
        EditLimits::default(),
    )
    .unwrap();
    assert_eq!(
        rewritten.as_str(),
        "/* keep */\r\nconst abc = 1; // retained\r\n"
    );
    assert_eq!(rewritten.applied_edits()[0].source, token.span);
    assert_eq!(rewritten.provenance().len(), 3);
}

#[test]
fn generated_names_have_deterministic_results_and_bounded_spans() {
    let alphabet = [
        'a', '\\', 'u', '0', '{', '}', '#', 'é', '\u{200c}', '1', '$',
    ];
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
                let limits = LexicalLimits {
                    max_identifier_bytes: 5,
                    ..LexicalLimits::default()
                };
                let result = scan_js_identifier(&input, offset, limits);
                assert_eq!(result, scan_js_identifier(&input, offset, limits));
                match result {
                    Ok(token) => {
                        let spelling = input.slice(token.span).unwrap();
                        assert_eq!(token.span.start(), offset);
                        assert!(token.span.len() <= limits.max_identifier_bytes);
                        assert!(!token.name_span.is_empty());
                        assert_eq!(token.name_span.end(), token.span.end());
                        assert_eq!(token.has_escape, spelling.contains('\\'));
                        assert_eq!(
                            token.kind == IdentifierKind::PrivateName,
                            spelling.starts_with('#')
                        );
                    }
                    Err(failure) => {
                        assert!(failure.offset >= offset && failure.offset <= text.len());
                        input.validate_offset(failure.offset).unwrap();
                    }
                }
            }
            assert_eq!(input.as_str(), text);
        }
    }
}
