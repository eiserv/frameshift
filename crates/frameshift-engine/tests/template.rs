//! Template lexical contracts through the public facade, using fictional text.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, GreaterThanMode, JsLexicalGoal, LegacyOctal, LexicalError,
    LexicalErrorKind, LexicalLimits, Punctuator, PunctuatorContext, SourceGoal, SourceText, Span,
    TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TextLimits, apply_edits,
    scan_js_number, scan_js_punctuator, scan_js_string, scan_js_template, scan_js_trivia,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn context(position: TemplatePosition, mode: TemplateMode) -> TemplateContext {
    TemplateContext { position, mode }
}

fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

fn scan(
    text: &str,
    position: TemplatePosition,
    mode: TemplateMode,
) -> frameshift_engine::TemplateToken {
    scan_js_template(
        &source(text),
        0,
        context(position, mode),
        LexicalLimits::default(),
    )
    .unwrap()
}

#[test]
fn all_four_components_have_exact_delimiter_and_content_spans() {
    for (position, ending, kind) in [
        (TemplatePosition::Start, "`", TemplateKind::NoSubstitution),
        (TemplatePosition::Start, "${", TemplateKind::Head),
        (TemplatePosition::Continuation, "${", TemplateKind::Middle),
        (TemplatePosition::Continuation, "`", TemplateKind::Tail),
    ] {
        for content in [
            "",
            "a",
            "$",
            "{}",
            "🦀é",
            "\r\n\u{2028}\u{2029}",
            "/* text */ // text",
        ] {
            let opening = match position {
                TemplatePosition::Start => "`",
                TemplatePosition::Continuation => "}",
            };
            let offset = "\u{feff}🦀 ".len();
            let segment = format!("{opening}{content}{ending}");
            let text = format!("\u{feff}🦀 {segment}ignored");
            let input = source(&text);
            for mode in [TemplateMode::Untagged, TemplateMode::Tagged] {
                let token = scan_js_template(
                    &input,
                    offset,
                    context(position, mode),
                    LexicalLimits::default(),
                )
                .unwrap();
                assert_eq!(token.kind, kind);
                assert_eq!(
                    token.span,
                    Span::new(offset, offset + segment.len()).unwrap()
                );
                assert_eq!(
                    token.content_span,
                    Span::new(offset + 1, offset + 1 + content.len()).unwrap()
                );
                assert_eq!(input.slice(token.span).unwrap(), segment);
                assert_eq!(input.slice(token.content_span).unwrap(), content);
                assert_eq!(token.invalid_escape, None);
                assert_eq!(
                    token.has_line_terminator,
                    content.contains(['\r', '\n', '\u{2028}', '\u{2029}'])
                );
                assert_eq!(input.as_str(), text);
            }
        }
    }
}

#[test]
fn valid_escapes_preserve_spelling_surrogates_and_line_endings() {
    for content in [
        r"\b\f\n\r\t\v\0",
        r"\x00\xFf",
        r"\u0000\uD800\uDFFF",
        r"\u{0}\u{10ffff}\u{D800}\u{00000000000000000000000041}",
        r"\q\é\🦀\'",
        "\\\"",
        r"\`\$\{\}",
        "a\r\nb\rc\nd\u{2028}e\u{2029}",
        "\\\r\n\\\r\\\n\\\u{2028}\\\u{2029}",
    ] {
        let text = format!("`{content}`");
        for mode in [TemplateMode::Tagged, TemplateMode::Untagged] {
            let token = scan(&text, TemplatePosition::Start, mode);
            assert_eq!(token.span.end(), text.len(), "{text:?}");
            assert_eq!(source(&text).slice(token.content_span).unwrap(), content);
            assert_eq!(token.invalid_escape, None);
            assert_eq!(
                token.has_line_terminator,
                content.contains(['\r', '\n', '\u{2028}', '\u{2029}'])
            );
        }
    }
}

#[test]
fn escaped_delimiters_and_backslash_parity_do_not_start_substitutions() {
    for (text, expected, kind) in [
        (
            r"`\${still text}`tail",
            r"`\${still text}`",
            TemplateKind::NoSubstitution,
        ),
        (
            r"`$\{still text}`tail",
            r"`$\{still text}`",
            TemplateKind::NoSubstitution,
        ),
        (
            r"`\`still text`tail",
            r"`\`still text`",
            TemplateKind::NoSubstitution,
        ),
        (r"`\\${expression}", r"`\\${", TemplateKind::Head),
        (
            r"`\\\${still text}`",
            r"`\\\${still text}`",
            TemplateKind::NoSubstitution,
        ),
        (
            r"`\u0024{still text}`",
            r"`\u0024{still text}`",
            TemplateKind::NoSubstitution,
        ),
        (
            r"`\x24{still text}`",
            r"`\x24{still text}`",
            TemplateKind::NoSubstitution,
        ),
    ] {
        let token = scan(text, TemplatePosition::Start, TemplateMode::Untagged);
        assert_eq!(token.kind, kind);
        assert_eq!(source(text).slice(token.span).unwrap(), expected);
    }
}

#[test]
fn invalid_escapes_are_errors_only_in_untagged_templates() {
    for escape in [
        r"\1",
        r"\8",
        r"\9",
        r"\01",
        r"\08",
        r"\00",
        r"\x",
        r"\x0",
        r"\xGG",
        r"\u",
        r"\u0",
        r"\u00",
        r"\u000",
        r"\uGGGG",
        r"\u{}",
        r"\u{xyz}",
        r"\u{41",
        r"\u{41z}",
        r"\u{110000}",
        r"\u{FFFFFFFFFFFFFFFFFFFFFFFFFFFF}",
    ] {
        for (opening, position) in [
            ("`", TemplatePosition::Start),
            ("}", TemplatePosition::Continuation),
        ] {
            for ending in ["`", "${expression}"] {
                let text = format!("{opening}é{escape}{ending}");
                let input = source(&text);
                let escape_offset = "`é".len();
                assert_eq!(
                    scan_js_template(
                        &input,
                        0,
                        context(position, TemplateMode::Untagged),
                        LexicalLimits::default()
                    ),
                    Err(error(
                        escape_offset,
                        LexicalErrorKind::InvalidTemplateEscape
                    )),
                    "{text:?}"
                );
                let token = scan(&text, position, TemplateMode::Tagged);
                assert_eq!(token.invalid_escape, Some(escape_offset), "{text:?}");
                assert_eq!(
                    token.span.end(),
                    opening.len()
                        + "é".len()
                        + escape.len()
                        + if ending.starts_with('`') { 1 } else { 2 }
                );
                assert_eq!(input.as_str(), text);
            }
        }
    }
    let token = scan(
        r"`\xZ\8\u{110000}`",
        TemplatePosition::Start,
        TemplateMode::Tagged,
    );
    assert_eq!(token.invalid_escape, Some(1));
    for (text, expected, kind) in [
        (r"`\x${expr}", r"`\x${", TemplateKind::Head),
        (r"`\u{${expr}", r"`\u{${", TemplateKind::Head),
        (r"`\u{110000${expr}", r"`\u{110000${", TemplateKind::Head),
        (r"`\x\`text`", r"`\x\`text`", TemplateKind::NoSubstitution),
        (
            r"`\u{FF\${text}`",
            r"`\u{FF\${text}`",
            TemplateKind::NoSubstitution,
        ),
    ] {
        let token = scan(text, TemplatePosition::Start, TemplateMode::Tagged);
        assert_eq!(token.invalid_escape, Some(1));
        assert_eq!(token.kind, kind);
        assert_eq!(source(text).slice(token.span).unwrap(), expected);
    }
}

#[test]
fn unterminated_segments_and_wrong_delimiters_return_no_partial_token() {
    for (text, position) in [
        ("`", TemplatePosition::Start),
        ("`text", TemplatePosition::Start),
        ("`\\", TemplatePosition::Start),
        ("`\\`", TemplatePosition::Start),
        ("}", TemplatePosition::Continuation),
        ("}\\\r\n", TemplatePosition::Continuation),
    ] {
        for mode in [TemplateMode::Tagged, TemplateMode::Untagged] {
            assert_eq!(
                scan_js_template(
                    &source(text),
                    0,
                    context(position, mode),
                    LexicalLimits::default()
                ),
                Err(error(0, LexicalErrorKind::UnterminatedTemplate))
            );
        }
    }
    for text in ["", "x", "${", "'text'", "\"text\"", "é"] {
        for position in [TemplatePosition::Start, TemplatePosition::Continuation] {
            assert_eq!(
                scan_js_template(
                    &source(text),
                    0,
                    context(position, TemplateMode::Tagged),
                    LexicalLimits::default()
                ),
                Err(error(0, LexicalErrorKind::ExpectedTemplateDelimiter))
            );
        }
    }
    assert_eq!(
        scan_js_template(
            &source("`"),
            0,
            context(TemplatePosition::Continuation, TemplateMode::Tagged),
            LexicalLimits::default()
        ),
        Err(error(0, LexicalErrorKind::ExpectedTemplateDelimiter))
    );
    assert_eq!(
        scan_js_template(
            &source("}"),
            0,
            context(TemplatePosition::Start, TemplateMode::Tagged),
            LexicalLimits::default()
        ),
        Err(error(0, LexicalErrorKind::ExpectedTemplateDelimiter))
    );
    let failure = error(1, LexicalErrorKind::InvalidTemplateEscape);
    assert_eq!(
        failure.to_string(),
        "invalid untagged template escape at byte offset 1"
    );
    assert!(std::error::Error::source(&failure).is_none());
}

#[test]
fn offsets_and_each_segment_byte_boundary_are_checked() {
    let input = source("é`");
    for offset in [1, input.len() + 1, usize::MAX] {
        assert_eq!(
            scan_js_template(
                &input,
                offset,
                context(TemplatePosition::Start, TemplateMode::Tagged),
                LexicalLimits {
                    max_template_bytes: 0,
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
        scan_js_template(
            &input,
            input.len(),
            context(TemplatePosition::Start, TemplateMode::Tagged),
            LexicalLimits::default()
        ),
        Err(error(
            input.len(),
            LexicalErrorKind::ExpectedTemplateDelimiter
        ))
    );
    for (text, position) in [
        ("``", TemplatePosition::Start),
        ("`${", TemplatePosition::Start),
        ("}`", TemplatePosition::Continuation),
        ("}${", TemplatePosition::Continuation),
        ("`é🦀\r\n${", TemplatePosition::Start),
        (r"`\u{00000041}\xFF\0`", TemplatePosition::Start),
        ("}\\\r\n`", TemplatePosition::Continuation),
        (r"`\u{FFFFFFFFFF}\x`", TemplatePosition::Start),
    ] {
        let input = source(&format!("🦀{text}tail"));
        let offset = "🦀".len();
        for max_template_bytes in 0..=text.len() {
            let result = scan_js_template(
                &input,
                offset,
                context(position, TemplateMode::Tagged),
                LexicalLimits {
                    max_template_bytes,
                    ..LexicalLimits::default()
                },
            );
            if max_template_bytes == text.len() {
                assert_eq!(result.unwrap().span.end(), offset + text.len());
            } else {
                let first_exceeded = text
                    .char_indices()
                    .find(|(index, ch)| index + ch.len_utf8() > max_template_bytes)
                    .unwrap()
                    .0;
                assert_eq!(
                    result,
                    Err(error(
                        offset + first_exceeded,
                        LexicalErrorKind::TemplateLimit
                    )),
                    "{text:?}, {max_template_bytes}"
                );
            }
        }
    }
    assert_eq!(LexicalLimits::default().max_template_bytes, 1024 * 1024);
}

#[test]
fn parser_selected_segments_cooperate_with_other_lexical_goals() {
    let text = "`outer ${ {s: '}', /* } */ n: 1} + `inner ${2}` } end`";
    let input = source(text);
    let limits = LexicalLimits::default();
    let head = scan_js_template(
        &input,
        0,
        context(TemplatePosition::Start, TemplateMode::Untagged),
        limits,
    )
    .unwrap();
    assert_eq!(input.slice(head.span).unwrap(), "`outer ${");
    let string_offset = text.find("'}'").unwrap();
    assert_eq!(
        input
            .slice(
                scan_js_string(&input, string_offset, LegacyOctal::Reject, limits)
                    .unwrap()
                    .span
            )
            .unwrap(),
        "'}'"
    );
    let trivia =
        scan_js_trivia(&input, text.find("/*").unwrap(), SourceGoal::Module, limits).unwrap();
    assert_eq!(input.slice(trivia.comments[0].span).unwrap(), "/* } */");
    let object_brace = text.find("} +").unwrap();
    let punctuation = scan_js_punctuator(
        &input,
        object_brace,
        PunctuatorContext {
            goal: JsLexicalGoal::Div,
            greater_than: GreaterThanMode::LongestMatch,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(punctuation.kind, Punctuator::RightBrace);
    let nested_offset = text.find("`inner").unwrap();
    let nested = scan_js_template(
        &input,
        nested_offset,
        context(TemplatePosition::Start, TemplateMode::Untagged),
        limits,
    )
    .unwrap();
    assert_eq!(nested.kind, TemplateKind::Head);
    let number = scan_js_number(&input, nested.span.end(), LegacyOctal::Reject, limits).unwrap();
    assert_eq!(input.slice(number.span).unwrap(), "2");
    let nested_tail = scan_js_template(
        &input,
        number.span.end(),
        context(TemplatePosition::Continuation, TemplateMode::Untagged),
        limits,
    )
    .unwrap();
    assert_eq!(input.slice(nested_tail.span).unwrap(), "}`");
    let outer_offset = text.find("} end").unwrap();
    for goal in [
        JsLexicalGoal::TemplateTail,
        JsLexicalGoal::RegExpOrTemplateTail,
    ] {
        assert!(
            scan_js_punctuator(
                &input,
                outer_offset,
                PunctuatorContext {
                    goal,
                    greater_than: GreaterThanMode::LongestMatch
                }
            )
            .unwrap()
            .is_none()
        );
    }
    let outer_tail = scan_js_template(
        &input,
        outer_offset,
        context(TemplatePosition::Continuation, TemplateMode::Untagged),
        limits,
    )
    .unwrap();
    assert_eq!(input.slice(outer_tail.span).unwrap(), "} end`");
    assert_eq!(outer_tail.span.end(), input.len());
}

#[test]
fn scanned_content_can_drive_a_byte_preserving_edit() {
    let text = "\u{feff}const greeting = `Hello ${name}🦀`; // keep\r\n";
    let input = source(text);
    let token = scan_js_template(
        &input,
        text.find('`').unwrap(),
        context(TemplatePosition::Start, TemplateMode::Untagged),
        LexicalLimits::default(),
    )
    .unwrap();
    let order = EditOrder {
        operation: 0,
        edit: 0,
    };
    let output = apply_edits(
        &input,
        &[Edit {
            span: token.content_span,
            replacement: "Welcome ",
            order,
        }],
        EditLimits::default(),
    )
    .unwrap();
    assert_eq!(
        output.as_str(),
        "\u{feff}const greeting = `Welcome ${name}🦀`; // keep\r\n"
    );
    assert_eq!(input.as_str(), text);
    assert_eq!(output.applied_edits()[0].source, token.content_span);
    assert_eq!(
        output
            .provenance()
            .iter()
            .map(|region| region.output.len())
            .sum::<usize>(),
        output.as_str().len()
    );
}

#[test]
fn long_code_point_escapes_remain_bounded_without_integer_overflow() {
    for (digits, invalid) in [
        ("0".repeat(100_000) + "41", false),
        ("F".repeat(100_000), true),
    ] {
        let text = format!("`\\u{{{digits}}}`");
        let token = scan(&text, TemplatePosition::Start, TemplateMode::Tagged);
        assert_eq!(token.span.end(), text.len());
        assert_eq!(token.invalid_escape.is_some(), invalid);
    }
    let limits = LexicalLimits {
        max_template_bytes: 16,
        ..LexicalLimits::default()
    };
    let input = source(&format!("`\\u{{{}`", "F".repeat(100_000)));
    assert_eq!(
        scan_js_template(
            &input,
            0,
            context(TemplatePosition::Start, TemplateMode::Tagged),
            limits
        ),
        Err(error(16, LexicalErrorKind::TemplateLimit))
    );
}

#[test]
fn generated_templates_have_deterministic_spans_and_escape_policy() {
    let alphabet = ['`', '}', '$', '{', '\\', 'u', 'x', '0', 'F', 'é', '\n'];
    for length in 0..=4 {
        for mut number in 0..alphabet.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push(alphabet[number % alphabet.len()]);
                number /= alphabet.len();
            }
            let input = source(&text);
            for offset in text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(text.len()))
            {
                for position in [TemplatePosition::Start, TemplatePosition::Continuation] {
                    for max_template_bytes in [0, 3, 16] {
                        let limits = LexicalLimits {
                            max_template_bytes,
                            ..LexicalLimits::default()
                        };
                        let tagged = scan_js_template(
                            &input,
                            offset,
                            context(position, TemplateMode::Tagged),
                            limits,
                        );
                        assert_eq!(
                            tagged,
                            scan_js_template(
                                &input,
                                offset,
                                context(position, TemplateMode::Tagged),
                                limits
                            )
                        );
                        let untagged = scan_js_template(
                            &input,
                            offset,
                            context(position, TemplateMode::Untagged),
                            limits,
                        );
                        match tagged {
                            Ok(token) => {
                                assert_eq!(token.span.start(), offset);
                                assert!(token.span.end() > offset);
                                assert!(token.span.len() <= max_template_bytes);
                                assert!(input.slice(token.span).is_ok());
                                assert!(input.slice(token.content_span).is_ok());
                                assert_eq!(token.content_span.start(), offset + 1);
                                assert!(token.content_span.end() < token.span.end());
                                match token.invalid_escape {
                                    Some(escape) => {
                                        assert!(
                                            escape >= token.content_span.start()
                                                && escape < token.content_span.end()
                                        );
                                        assert!(text[escape..].starts_with('\\'));
                                        assert_eq!(
                                            untagged,
                                            Err(error(
                                                escape,
                                                LexicalErrorKind::InvalidTemplateEscape
                                            ))
                                        );
                                    }
                                    None => assert_eq!(untagged, Ok(token)),
                                }
                            }
                            Err(failure) => {
                                assert!(input.validate_offset(failure.offset).is_ok());
                            }
                        }
                        assert_eq!(input.as_str(), text);
                    }
                }
            }
        }
    }
}
