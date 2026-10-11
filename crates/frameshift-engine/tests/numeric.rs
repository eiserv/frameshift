//! Numeric scanning contracts through the public facade, using fictional text.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, LexicalError, LexicalErrorKind, LexicalLimits, NumericBase,
    NumericKind, SourceText, Span, TextLimits, apply_edits, scan_js_number,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

#[test]
fn decimal_spelling_is_preserved_without_evaluation() {
    for literal in [
        "0",
        "1",
        "1234567890",
        "1_234_567",
        "1.",
        "0.",
        ".5",
        ".0_1",
        "0.01",
        "12.34_56",
        "1e0",
        "0E-1",
        "1e+1_000",
        "1.e2",
        ".5E-2",
        "1_0.0_1e2_3",
        "1e99999999999999999999999999999",
        "9999999999999999999999999999999999999999999999999999",
    ] {
        let text = format!("🦀/* prefix */{literal}; // tail\r\n");
        let input = source(&text);
        let offset = "🦀/* prefix */".len();
        let result = scan_js_number(&input, offset, LexicalLimits::default()).unwrap();
        assert_eq!(result.base, NumericBase::Decimal, "{literal}");
        assert_eq!(result.kind, NumericKind::Number);
        assert_eq!(
            result.span,
            Span::new(offset, offset + literal.len()).unwrap()
        );
        assert_eq!(input.slice(result.span).unwrap(), literal);
        assert_eq!(input.as_str(), text);
    }
}

#[test]
fn radix_integers_and_bigints_retain_their_categories() {
    for (literal, base) in [
        ("0", NumericBase::Decimal),
        ("1_234", NumericBase::Decimal),
        ("0b10_01", NumericBase::Binary),
        ("0B00_11", NumericBase::Binary),
        ("0o07_65", NumericBase::Octal),
        ("0O7", NumericBase::Octal),
        ("0x00_Ff", NumericBase::Hexadecimal),
        ("0XAB_cd_EF", NumericBase::Hexadecimal),
    ] {
        for (suffix, kind) in [("", NumericKind::Number), ("n", NumericKind::BigInt)] {
            let text = format!("{literal}{suffix}");
            let input = source(&text);
            let result = scan_js_number(&input, 0, LexicalLimits::default()).unwrap();
            assert_eq!(result.base, base);
            assert_eq!(result.kind, kind);
            assert_eq!(input.slice(result.span).unwrap(), text);
        }
    }
}

#[test]
fn malformed_parts_fail_at_precise_offsets_without_partial_results() {
    use LexicalErrorKind::{
        InvalidNumericContinuation, InvalidNumericSeparator, MissingNumericDigit,
    };
    for (text, offset, kind) in [
        ("0x", 2, MissingNumericDigit),
        ("0b2", 2, MissingNumericDigit),
        ("0o8", 2, MissingNumericDigit),
        ("0Xg", 2, MissingNumericDigit),
        ("1e", 2, MissingNumericDigit),
        ("1e+", 3, MissingNumericDigit),
        ("1e-;", 3, MissingNumericDigit),
        ("1_", 1, InvalidNumericSeparator),
        ("1__2", 1, InvalidNumericSeparator),
        ("0_1", 1, InvalidNumericSeparator),
        ("0x_1", 2, InvalidNumericSeparator),
        ("0b1_2", 3, InvalidNumericSeparator),
        ("0o7_8", 3, InvalidNumericSeparator),
        ("0xF_n", 3, InvalidNumericSeparator),
        ("1._0", 2, InvalidNumericSeparator),
        ("1_.0", 1, InvalidNumericSeparator),
        ("1e_2", 2, InvalidNumericSeparator),
        ("1e+_2", 3, InvalidNumericSeparator),
        ("1e2_", 3, InvalidNumericSeparator),
        ("0b102", 4, InvalidNumericContinuation),
        ("0o78", 3, InvalidNumericContinuation),
        ("0x1g", 3, InvalidNumericContinuation),
        ("1.0n", 3, InvalidNumericContinuation),
        ("1.n", 2, InvalidNumericContinuation),
        (".1n", 2, InvalidNumericContinuation),
        ("1e2n", 3, InvalidNumericContinuation),
        ("1nn", 2, InvalidNumericContinuation),
        ("1n2", 2, InvalidNumericContinuation),
        ("1n_", 2, InvalidNumericContinuation),
        ("3in", 1, InvalidNumericContinuation),
        ("1$", 1, InvalidNumericContinuation),
        (r"1\u0061", 1, InvalidNumericContinuation),
    ] {
        let input = source(text);
        assert_eq!(
            scan_js_number(&input, 0, LexicalLimits::default()),
            Err(error(offset, kind)),
            "{text}"
        );
        assert_eq!(input.as_str(), text);
    }
}

#[test]
fn strict_mode_rejects_legacy_leading_zero_forms() {
    for text in ["00", "01", "077", "08", "09.5", "00e2", "01n"] {
        assert_eq!(
            scan_js_number(&source(text), 0, LexicalLimits::default()),
            Err(error(1, LexicalErrorKind::LegacyLeadingZero)),
            "{text}"
        );
    }
}

#[test]
fn numeric_boundaries_use_pinned_unicode_identifier_starts() {
    for suffix in ["é", "中", "\u{2118}", "\u{309b}", "\u{3d000}"] {
        assert_eq!(
            scan_js_number(&source(&format!("1{suffix}")), 0, LexicalLimits::default()),
            Err(error(1, LexicalErrorKind::InvalidNumericContinuation)),
            "{suffix}"
        );
    }
    // Continue-only, format, and symbol scalars end the literal; whatever token
    // follows is the parser's concern.
    for suffix in [
        "🦀", "\u{301}", "\u{660}", "\u{b7}", "\u{85}", "\u{200b}", "\u{200c}", "\u{200d}",
        "\u{180e}", "\u{2e2f}", "€",
    ] {
        let input = source(&format!("0x1n{suffix}"));
        let result = scan_js_number(&input, 0, LexicalLimits::default()).unwrap();
        assert_eq!(result.span, Span::new(0, 4).unwrap(), "{suffix}");
    }
    for suffix in [
        " ",
        "\t",
        "\r",
        "\n",
        "\r\n",
        "\u{2028}",
        "\u{2029}",
        "\u{a0}",
        "\u{1680}",
        "\u{2000}",
        "\u{200a}",
        "\u{202f}",
        "\u{205f}",
        "\u{3000}",
        "\u{feff}",
        ";",
        ",",
        ")",
        "]",
        "}",
        "+1",
        "-1",
        "/* comment */",
        "// comment",
        "?.field",
    ] {
        for literal in ["1", "0b1", "0xF", "1n"] {
            let input = source(&format!("{literal}{suffix}"));
            let result = scan_js_number(&input, 0, LexicalLimits::default()).unwrap();
            assert_eq!(input.slice(result.span).unwrap(), literal);
        }
    }
    for (text, expected) in [("1..field", "1."), ("0x1.field", "0x1"), ("1n.field", "1n")] {
        let input = source(text);
        let result = scan_js_number(&input, 0, LexicalLimits::default()).unwrap();
        assert_eq!(input.slice(result.span).unwrap(), expected);
    }
}

#[test]
fn offsets_and_byte_limits_are_checked_before_consumption() {
    for text in [
        "", ".", "...", "+1", "-1", "Infinity", "NaN", "_1", "é", ".x",
    ] {
        assert_eq!(
            scan_js_number(&source(text), 0, LexicalLimits::default()),
            Err(error(0, LexicalErrorKind::ExpectedNumber))
        );
    }
    let input = source("é1");
    for offset in [1, input.len() + 1, usize::MAX] {
        assert_eq!(
            scan_js_number(
                &input,
                offset,
                LexicalLimits {
                    max_number_bytes: 0,
                    ..LexicalLimits::default()
                }
            ),
            Err(error(
                offset,
                LexicalErrorKind::InvalidOffset(input.validate_offset(offset).unwrap_err())
            ))
        );
    }
    assert_eq!(
        scan_js_number(&input, input.len(), LexicalLimits::default()),
        Err(error(input.len(), LexicalErrorKind::ExpectedNumber))
    );
    for literal in [
        "0",
        "1_234.5_6e+7_8",
        ".1",
        "1.",
        "0xF_Fn",
        "0b10_01",
        "0O7n",
    ] {
        let text = format!("é{literal};");
        let input = source(&text);
        for limit in 0..literal.len() {
            assert_eq!(
                scan_js_number(
                    &input,
                    2,
                    LexicalLimits {
                        max_number_bytes: limit,
                        ..LexicalLimits::default()
                    }
                ),
                Err(error(2 + limit, LexicalErrorKind::NumberLimit)),
                "{literal}, limit {limit}"
            );
        }
        assert_eq!(
            scan_js_number(
                &input,
                2,
                LexicalLimits {
                    max_number_bytes: literal.len(),
                    ..LexicalLimits::default()
                }
            )
            .unwrap()
            .span
            .end(),
            2 + literal.len()
        );
    }
    let huge = "1".repeat(32_768);
    assert_eq!(
        scan_js_number(&source(&huge), 0, LexicalLimits::default())
            .unwrap()
            .span
            .len(),
        huge.len()
    );
    let failure =
        scan_js_number(&source("1privateSecret"), 0, LexicalLimits::default()).unwrap_err();
    assert_eq!(
        failure.to_string(),
        "invalid numeric continuation at byte offset 1"
    );
    assert!(!format!("{failure:?}").contains("privateSecret"));
}

#[test]
fn a_parser_selected_numeric_span_can_drive_an_exact_edit() {
    let input = source("/* keep */\r\n0xF_Fn; // retained\r\n");
    let literal = scan_js_number(&input, 12, LexicalLimits::default()).unwrap();
    let rewritten = apply_edits(
        &input,
        &[Edit {
            span: literal.span,
            replacement: "255n",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        }],
        EditLimits::default(),
    )
    .unwrap();
    assert_eq!(rewritten.as_str(), "/* keep */\r\n255n; // retained\r\n");
    assert_eq!(rewritten.applied_edits()[0].source, literal.span);
    assert_eq!(rewritten.provenance().len(), 3);
    assert_eq!(input.as_str(), "/* keep */\r\n0xF_Fn; // retained\r\n");
}

#[test]
fn generated_numbers_have_deterministic_results_and_bounded_spans() {
    let alphabet = [
        '0', '1', '8', 'x', 'b', 'e', 'n', '_', '.', '+', 'é', '\u{2028}',
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
                    max_number_bytes: 3,
                    ..LexicalLimits::default()
                };
                let result = scan_js_number(&input, offset, limits);
                assert_eq!(result, scan_js_number(&input, offset, limits));
                match result {
                    Ok(literal) => {
                        assert_eq!(literal.span.start(), offset);
                        assert!(!literal.span.is_empty());
                        assert!(literal.span.len() <= limits.max_number_bytes);
                        assert!(input.slice(literal.span).unwrap().is_ascii());
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
