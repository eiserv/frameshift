//! Lexical contracts exercised through the engine facade, using fictional text.

use frameshift_engine::{
    CommentKind, Edit, EditLimits, EditOrder, LegacyOctal, LexicalError, LexicalErrorKind,
    LexicalLimits, SourceGoal, SourceText, Span, TextError, TextLimits, apply_edits,
    scan_js_string, scan_js_trivia,
};

fn make_source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

#[test]
fn trivia_preserves_bytes_comments_and_line_break_evidence() {
    let text = "\u{feff}\t // é🦀\r\n/* first\u{2028}second */  'value'";
    let source = make_source(text);
    let result = scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
    assert_eq!(result.span, Span::new(0, text.find('\'').unwrap()).unwrap());
    assert!(result.has_line_terminator);
    assert_eq!(result.comments.len(), 2);
    assert_eq!(result.comments[0].kind, CommentKind::Line);
    assert_eq!(source.slice(result.comments[0].span).unwrap(), "// é🦀");
    assert_eq!(result.comments[1].kind, CommentKind::Block);
    assert_eq!(
        source.slice(result.comments[1].span).unwrap(),
        "/* first\u{2028}second */"
    );
    assert_eq!(source.as_str(), text);
}

#[test]
fn every_ecmascript_line_terminator_affects_trivia() {
    for line_break in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        for text in [
            format!(" {line_break}x"),
            format!("// line{line_break}x"),
            format!("/*{line_break}*/x"),
        ] {
            let source = make_source(&text);
            let result =
                scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
            assert!(result.has_line_terminator);
            assert_eq!(result.span.end(), text.len() - 1);
            for comment in result.comments {
                let slice = source.slice(comment.span).unwrap();
                if comment.kind == CommentKind::Line {
                    assert_eq!(slice, "// line");
                }
            }
        }
    }
    let source = make_source("/**/\t");
    assert!(
        !scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default())
            .unwrap()
            .has_line_terminator
    );
}

#[test]
fn consumes_ecmascript_whitespace_without_using_unicode_white_space_property() {
    let whitespace = "\t\u{b}\u{c} \u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{202f}\u{205f}\u{3000}\u{feff}";
    let source = make_source(whitespace);
    let result = scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
    assert_eq!(result.span.end(), source.len());
    assert!(!result.has_line_terminator);
    for text in [
        "\u{85}", "\u{180e}", "\u{200b}", "\0", "/", "/=", "/regex/", "#!x", "<!--x",
    ] {
        let source = make_source(text);
        let result =
            scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
        assert!(result.span.is_empty(), "{text:?}");
        assert!(result.comments.is_empty());
    }
}

#[test]
fn block_comments_do_not_nest_and_stop_at_first_closing_delimiter() {
    let source = make_source("/* a /* b */tail */");
    let result = scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
    assert_eq!(
        source.slice(result.comments[0].span).unwrap(),
        "/* a /* b */"
    );
    assert_eq!(&source.as_str()[result.span.end()..], "tail */");
    for text in ["//", "// at EOF", "/*/**/*/"] {
        assert!(
            scan_js_trivia(
                &make_source(text),
                0,
                SourceGoal::Module,
                LexicalLimits::default()
            )
            .is_ok()
        );
    }
}

#[test]
fn trivia_errors_are_explicit_without_partial_results_or_source_content() {
    let source = make_source("/**/ /* secret");
    assert_eq!(
        scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()),
        Err(error(5, LexicalErrorKind::UnterminatedComment))
    );
    let limits = LexicalLimits {
        max_comments: 1,
        ..LexicalLimits::default()
    };
    assert_eq!(
        scan_js_trivia(&source, 0, SourceGoal::Module, limits),
        Err(error(5, LexicalErrorKind::CommentLimit))
    );
    assert_eq!(source.as_str(), "/**/ /* secret");
    assert!(
        !error(5, LexicalErrorKind::UnterminatedComment)
            .to_string()
            .contains("secret")
    );
    let source = make_source("/**/");
    assert!(scan_js_trivia(&source, 0, SourceGoal::Module, limits).is_ok());
    assert!(
        scan_js_trivia(
            &source,
            source.len(),
            SourceGoal::Module,
            LexicalLimits {
                max_comments: 0,
                ..limits
            }
        )
        .unwrap()
        .span
        .is_empty()
    );
    assert_eq!(
        scan_js_trivia(
            &source,
            0,
            SourceGoal::Module,
            LexicalLimits {
                max_comments: 0,
                ..limits
            }
        ),
        Err(error(0, LexicalErrorKind::CommentLimit))
    );
}

#[test]
fn strings_preserve_quotes_unicode_escapes_and_line_continuations() {
    for literal in [
        "''",
        "\"\"",
        "'é🦀'",
        "'\u{2028}\u{2029}'",
        r#"'\'\"\\\b\f\n\r\t\v\0\z'"#,
        r#"'\x00\xFF\u0000\uD800\uDC00\u{D800}\u{10FFFF}\u{000000000000041}'"#,
        "'a\\\nb'",
        "'a\\\rb'",
        "'a\\\r\nb'",
        "'a\\\u{2028}b'",
        "'a\\\u{2029}b'",
    ] {
        let text = format!("prefix{literal}suffix");
        let source = make_source(&text);
        let span = scan_js_string(&source, 6, LegacyOctal::Reject, LexicalLimits::default())
            .unwrap()
            .span;
        assert_eq!(span, Span::new(6, 6 + literal.len()).unwrap());
        assert_eq!(source.slice(span).unwrap(), literal);
        assert_eq!(source.as_str(), text);
    }
}

#[test]
fn rejects_malformed_and_legacy_escapes_with_exact_offsets() {
    for literal in [
        r#"'\x'"#,
        r#"'\x0g'"#,
        r#"'\u123'"#,
        r#"'\u{}'"#,
        r#"'\u{110000}'"#,
        r#"'\u{FFFFFFFF}'"#,
        r#"'\u{1x}'"#,
        r#"'\u{12'"#,
        "'\\",
    ] {
        assert_eq!(
            scan_js_string(
                &make_source(literal),
                0,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(error(1, LexicalErrorKind::InvalidEscape)),
            "{literal:?}"
        );
    }
    for digit in '0'..='9' {
        let literal = format!("'\\{digit}1'");
        assert_eq!(
            scan_js_string(
                &make_source(&literal),
                0,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(error(1, LexicalErrorKind::LegacyDecimalEscape))
        );
    }
}

#[test]
fn sloppy_strings_report_the_first_legacy_octal_escape() {
    for (literal, first) in [
        (r#"'\1'"#, Some(1)),
        (r#"'a\08'"#, Some(2)),
        (r#"'\377\8'"#, Some(1)),
        (r#"'\x41\9'"#, Some(5)),
        (r#"'\0'"#, None),
        (r#"'\0a0'"#, None),
    ] {
        let token = scan_js_string(
            &make_source(literal),
            0,
            LegacyOctal::Permit,
            LexicalLimits::default(),
        )
        .unwrap();
        assert_eq!(
            token.span,
            Span::new(0, literal.len()).unwrap(),
            "{literal}"
        );
        assert_eq!(token.legacy_octal_escape, first, "{literal}");
        let strict = scan_js_string(
            &make_source(literal),
            0,
            LegacyOctal::Reject,
            LexicalLimits::default(),
        );
        match first {
            Some(offset) => assert_eq!(
                strict,
                Err(error(offset, LexicalErrorKind::LegacyDecimalEscape)),
                "{literal}"
            ),
            None => assert_eq!(strict, Ok(token), "{literal}"),
        }
    }
}

#[test]
fn script_goal_consumes_annex_b_html_like_comments() {
    let text = "<!-- open é\n  /* one line */ --> close\r\n/* a\nb */ -->tail\n-->";
    let source = make_source(text);
    let result = scan_js_trivia(&source, 0, SourceGoal::Script, LexicalLimits::default()).unwrap();
    assert_eq!(result.span, Span::new(0, text.len()).unwrap());
    let comments: Vec<_> = result
        .comments
        .iter()
        .map(|comment| (comment.kind, source.slice(comment.span).unwrap()))
        .collect();
    assert_eq!(
        comments,
        [
            (CommentKind::HtmlOpen, "<!-- open é"),
            (CommentKind::Block, "/* one line */"),
            (CommentKind::HtmlClose, "--> close"),
            (CommentKind::Block, "/* a\nb */"),
            (CommentKind::HtmlClose, "-->tail"),
            (CommentKind::HtmlClose, "-->"),
        ]
    );

    // A close comment needs a preceding line terminator in the same trivia,
    // so it is never one at the start of the source or after a token.
    for (text, end) in [("-->", 0), (" /* x */ -->", 9), ("x\n-->", 0)] {
        let result = scan_js_trivia(
            &make_source(text),
            0,
            SourceGoal::Script,
            LexicalLimits::default(),
        )
        .unwrap();
        assert_eq!(result.span.end(), end, "{text:?}");
    }
    // Module code has no HTML-like comments.
    for text in ["<!-- x", "\n--> x"] {
        let source = make_source(text);
        let module =
            scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
        assert!(module.comments.is_empty(), "{text:?}");
        assert_eq!(module.span.end(), text.find(['<', '-']).unwrap());
    }
    let limits = LexicalLimits {
        max_comments: 1,
        ..LexicalLimits::default()
    };
    assert_eq!(
        scan_js_trivia(&make_source("<!--\n-->"), 0, SourceGoal::Script, limits),
        Err(error(5, LexicalErrorKind::CommentLimit))
    );
}

#[test]
fn rejects_unterminated_strings_and_raw_cr_or_lf() {
    for literal in ["'", "'secret", "\"secret'"] {
        assert_eq!(
            scan_js_string(
                &make_source(literal),
                0,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(error(0, LexicalErrorKind::UnterminatedString))
        );
    }
    for literal in ["'é\nx'", "'é\rx'", "'é\r\nx'"] {
        assert_eq!(
            scan_js_string(
                &make_source(literal),
                0,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(error(3, LexicalErrorKind::UnescapedLineBreak))
        );
    }
    for text in ["", "`template`", "unquoted"] {
        assert_eq!(
            scan_js_string(
                &make_source(text),
                0,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(error(0, LexicalErrorKind::ExpectedQuote))
        );
    }
}

#[test]
fn validates_offsets_and_string_byte_limits_including_delimiters() {
    for literal in [r#"'\x41'"#, r#"'\u0041'"#, r#"'\u{000041}'"#, "'\\\r\n'"] {
        let source = make_source(literal);
        for limit in 0..literal.len() {
            let result = scan_js_string(
                &source,
                0,
                LegacyOctal::Reject,
                LexicalLimits {
                    max_string_bytes: limit,
                    ..LexicalLimits::default()
                },
            );
            assert!(matches!(
                result,
                Err(LexicalError {
                    kind: LexicalErrorKind::StringLimit,
                    ..
                })
            ));
        }
        assert_eq!(
            scan_js_string(
                &source,
                0,
                LegacyOctal::Reject,
                LexicalLimits {
                    max_string_bytes: literal.len(),
                    ..LexicalLimits::default()
                }
            )
            .unwrap()
            .span
            .end(),
            literal.len()
        );
    }
    let long_escape = format!("'\\u{{{}41}}'", "0".repeat(16_384));
    assert_eq!(
        scan_js_string(
            &make_source(&long_escape),
            0,
            LegacyOctal::Reject,
            LexicalLimits::default()
        )
        .unwrap()
        .span
        .end(),
        long_escape.len()
    );
    let source = make_source("'é🦀'");
    for offset in [2, 4, source.len() + 1, usize::MAX] {
        let cause = source.validate_offset(offset).unwrap_err();
        let expected = error(offset, LexicalErrorKind::InvalidOffset(cause));
        assert_eq!(
            scan_js_trivia(
                &source,
                offset,
                SourceGoal::Module,
                LexicalLimits::default()
            ),
            Err(expected.clone())
        );
        assert_eq!(
            scan_js_string(
                &source,
                offset,
                LegacyOctal::Reject,
                LexicalLimits::default()
            ),
            Err(expected)
        );
    }
    for (limit, offset) in [(0, 0), (1, 1), (2, 1), (3, 3), (6, 3), (7, 7)] {
        assert_eq!(
            scan_js_string(
                &source,
                0,
                LegacyOctal::Reject,
                LexicalLimits {
                    max_string_bytes: limit,
                    ..LexicalLimits::default()
                }
            ),
            Err(error(offset, LexicalErrorKind::StringLimit))
        );
    }
    assert_eq!(
        scan_js_string(
            &source,
            0,
            LegacyOctal::Reject,
            LexicalLimits {
                max_string_bytes: 8,
                ..LexicalLimits::default()
            }
        )
        .unwrap()
        .span
        .end(),
        8
    );
    assert_eq!(
        scan_js_string(
            &source,
            2,
            LegacyOctal::Reject,
            LexicalLimits {
                max_string_bytes: 0,
                ..LexicalLimits::default()
            }
        ),
        Err(error(
            2,
            LexicalErrorKind::InvalidOffset(TextError::NotCharBoundary { offset: 2 })
        ))
    );
}

#[test]
fn parser_selected_string_span_drives_a_provenance_preserving_edit() {
    let source = make_source("/* keep */\r\n'old é'; // keep too");
    let trivia = scan_js_trivia(&source, 0, SourceGoal::Module, LexicalLimits::default()).unwrap();
    let literal = scan_js_string(
        &source,
        trivia.span.end(),
        LegacyOctal::Reject,
        LexicalLimits::default(),
    )
    .unwrap()
    .span;
    let result = apply_edits(
        &source,
        &[Edit {
            span: literal,
            replacement: "'new'",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        }],
        EditLimits::default(),
    )
    .unwrap();
    assert_eq!(result.as_str(), "/* keep */\r\n'new'; // keep too");
    assert_eq!(result.applied_edits()[0].source, literal);
    assert_eq!(result.provenance().len(), 3);
    assert_eq!(source.as_str(), "/* keep */\r\n'old é'; // keep too");
}

#[test]
fn generated_html_like_comment_inputs_extend_module_trivia_deterministically() {
    let alphabet = ['<', '!', '-', '>', '\n', '/', '*', ' ', 'x'];
    let limits = LexicalLimits {
        max_comments: 3,
        ..LexicalLimits::default()
    };
    for length in 0..=5 {
        for mut index in 0..alphabet.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push(alphabet[index % alphabet.len()]);
                index /= alphabet.len();
            }
            let source = make_source(&text);
            for offset in 0..=text.len() {
                let script = scan_js_trivia(&source, offset, SourceGoal::Script, limits);
                assert_eq!(
                    script,
                    scan_js_trivia(&source, offset, SourceGoal::Script, limits)
                );
                let module = scan_js_trivia(&source, offset, SourceGoal::Module, limits);
                if let Ok(module) = &module {
                    assert!(module.comments.iter().all(|comment| matches!(
                        comment.kind,
                        CommentKind::Line | CommentKind::Block
                    )));
                }
                if let Ok(script) = script {
                    for comment in &script.comments {
                        let spelling = source.slice(comment.span).unwrap();
                        let prefix = match comment.kind {
                            CommentKind::Line => "//",
                            CommentKind::Block => "/*",
                            CommentKind::HtmlOpen => "<!--",
                            CommentKind::HtmlClose => "-->",
                            CommentKind::Hashbang => "#!",
                        };
                        assert!(spelling.starts_with(prefix), "{text:?}");
                    }
                    if let Ok(module) = module {
                        assert!(script.span.end() >= module.span.end(), "{text:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn generated_short_inputs_have_deterministic_errors_and_valid_spans() {
    let alphabet = ['\'', '"', '\\', '/', '*', '\r', '\u{2028}', 'é', '0'];
    for length in 0..=4 {
        for mut index in 0..alphabet.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push(alphabet[index % alphabet.len()]);
                index /= alphabet.len();
            }
            let source = make_source(&text);
            for offset in text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain([text.len()])
            {
                let limits = LexicalLimits {
                    max_comments: 2,
                    max_string_bytes: 8,
                    ..LexicalLimits::default()
                };
                let trivia = scan_js_trivia(&source, offset, SourceGoal::Module, limits);
                assert_eq!(
                    trivia,
                    scan_js_trivia(&source, offset, SourceGoal::Module, limits)
                );
                if let Ok(trivia) = trivia {
                    assert_eq!(trivia.span.start(), offset);
                    source.slice(trivia.span).unwrap();
                    for comment in trivia.comments {
                        assert!(!comment.span.is_empty());
                        source.slice(comment.span).unwrap();
                    }
                }
                let string = scan_js_string(&source, offset, LegacyOctal::Reject, limits);
                assert_eq!(
                    string,
                    scan_js_string(&source, offset, LegacyOctal::Reject, limits)
                );
                if let Ok(token) = string {
                    let span = token.span;
                    let literal = source.slice(span).unwrap();
                    assert_eq!(span.start(), offset);
                    assert!(literal.len() >= 2);
                    assert_eq!(literal.as_bytes().first(), literal.as_bytes().last());
                }
            }
            assert_eq!(source.as_str(), text);
        }
    }
}

// The trivia grammar transcribed over Unicode scalars, independent of the
// scanner's byte-level paths: comment text and line breaks, including U+2028
// and U+2029 inside comments, must be classified exactly as by scalar.
type ReferenceTrivia = (usize, bool, Vec<(CommentKind, usize, usize)>);

fn reference_trivia(
    text: &str,
    offset: usize,
    goal: SourceGoal,
    max_comments: usize,
) -> Result<ReferenceTrivia, LexicalError> {
    let is_whitespace = |ch: char| {
        matches!(
            ch,
            '\t' | '\u{b}' | '\u{c}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    };
    let is_line_terminator = |ch: char| matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let html = goal == SourceGoal::Script;
    let mut cursor = offset;
    let mut line_break = false;
    let mut comments = Vec::new();
    while let Some(ch) = text[cursor..].chars().next() {
        if is_whitespace(ch) || is_line_terminator(ch) {
            line_break |= is_line_terminator(ch);
            cursor += ch.len_utf8();
            continue;
        }
        let rest = &text[cursor..];
        let (kind, delimiter) = if rest.starts_with("//") {
            (CommentKind::Line, 2)
        } else if rest.starts_with("/*") {
            (CommentKind::Block, 2)
        } else if html && rest.starts_with("<!--") {
            (CommentKind::HtmlOpen, 4)
        } else if html && line_break && rest.starts_with("-->") {
            (CommentKind::HtmlClose, 3)
        } else {
            break;
        };
        if comments.len() == max_comments {
            return Err(error(cursor, LexicalErrorKind::CommentLimit));
        }
        let start = cursor;
        cursor += delimiter;
        if kind == CommentKind::Block {
            loop {
                if text[cursor..].starts_with("*/") {
                    cursor += 2;
                    break;
                }
                let Some(ch) = text[cursor..].chars().next() else {
                    return Err(error(start, LexicalErrorKind::UnterminatedComment));
                };
                line_break |= is_line_terminator(ch);
                cursor += ch.len_utf8();
            }
        } else {
            while let Some(ch) = text[cursor..].chars().next() {
                if is_line_terminator(ch) {
                    break;
                }
                cursor += ch.len_utf8();
            }
        }
        comments.push((kind, start, cursor));
    }
    Ok((cursor, line_break, comments))
}

#[test]
fn trivia_scanning_agrees_with_a_scalar_reference_model() {
    // U+2027, U+202A, and U+20AC share the UTF-8 lead byte of U+2028 and
    // U+2029 but are not line terminators; U+180E is not whitespace.
    let alphabet = [
        ' ', '\t', '\n', '\r', '\u{2028}', '\u{2029}', '\u{2027}', '\u{202a}', '\u{20ac}',
        '\u{a0}', '\u{3000}', '\u{180e}', '/', '*', '<', '!', '-', '>', 'x',
    ];
    let max_comments = 3;
    let limits = LexicalLimits {
        max_comments,
        ..LexicalLimits::default()
    };
    let check = |text: &str| {
        let source = make_source(text);
        for offset in text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain([text.len()])
        {
            for goal in [SourceGoal::Script, SourceGoal::Module] {
                let actual = scan_js_trivia(&source, offset, goal, limits).map(|trivia| {
                    assert_eq!(trivia.span.start(), offset);
                    let comments = trivia
                        .comments
                        .iter()
                        .map(|comment| (comment.kind, comment.span.start(), comment.span.end()))
                        .collect::<Vec<_>>();
                    (trivia.span.end(), trivia.has_line_terminator, comments)
                });
                let expected = reference_trivia(text, offset, goal, max_comments);
                assert_eq!(actual, expected, "{text:?} at {offset} in {goal:?}");
            }
        }
    };
    for length in 0..=3 {
        for mut index in 0..alphabet.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push(alphabet[index % alphabet.len()]);
                index /= alphabet.len();
            }
            check(&text);
        }
    }
    // Longer inputs from a fixed SplitMix64 sequence.
    let mut state: u64 = 0x7121_5EED;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    };
    for _ in 0..20_000 {
        let length = 4 + (next() % 9) as usize;
        let text: String = (0..length)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .collect();
        check(&text);
    }
}
