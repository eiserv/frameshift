//! Regular-expression literal contracts through the public facade, using fictional text.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, LexicalError, LexicalErrorKind, LexicalLimits, RegExpFlag,
    RegExpPatternError, SourceText, Span, TextError, TextLimits, apply_edits, scan_js_regexp,
    validate_js_regexp,
};

use RegExpPatternError::*;

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}

fn pattern(offset: usize, kind: RegExpPatternError) -> Result<u32, LexicalError> {
    Err(error(offset, LexicalErrorKind::InvalidRegExpPattern(kind)))
}

/// Validates a literal that starts at offset 0 and returns its group count.
fn validate(literal: &str) -> Result<u32, LexicalError> {
    let input = source(literal);
    let result = validate_js_regexp(&input, 0, LexicalLimits::default());
    if let Ok(validated) = result {
        assert_eq!(validated.token.span, Span::new(0, literal.len()).unwrap());
    }
    assert_eq!(input.as_str(), literal);
    result.map(|validated| validated.capturing_groups)
}

fn check(cases: &[(&str, Result<u32, LexicalError>)]) {
    for (literal, expected) in cases {
        assert_eq!(&validate(literal), expected, "{literal}");
    }
}

#[test]
fn lexical_extent_respects_classes_escapes_and_flags() {
    for (text, offset, body, flags) in [
        ("/a/", 0, (1, 2), (3, 3)),
        ("x = /a+b/gi.test(y)", 4, (5, 8), (9, 11)),
        ("/[/]/", 0, (1, 4), (5, 5)),
        (r"/[\]/]/", 0, (1, 6), (7, 7)),
        (r"/\//g;", 0, (1, 3), (4, 5)),
        (r"/a\[/]/", 0, (1, 4), (5, 5)),
        ("/é🦀/u ", 0, (1, 7), (8, 9)),
        ("/a/u\u{200d}x", 0, (1, 2), (3, 8)),
        ("/a/\u{2028}", 0, (1, 2), (3, 3)),
        ("é /[[]]/v", 3, (4, 8), (9, 10)),
    ] {
        let input = source(text);
        let token = scan_js_regexp(&input, offset, LexicalLimits::default()).unwrap();
        assert_eq!(
            token.body_span,
            Span::new(body.0, body.1).unwrap(),
            "{text}"
        );
        assert_eq!(
            token.flags_span,
            Span::new(flags.0, flags.1).unwrap(),
            "{text}"
        );
        assert_eq!(token.span, Span::new(offset, flags.1).unwrap(), "{text}");
        assert_eq!(input.as_str(), text);
    }
}

#[test]
fn lexical_errors_have_exact_offsets_and_precedence() {
    let limits = LexicalLimits::default();
    for (text, offset, expected) in [
        ("a/", 0, error(0, LexicalErrorKind::ExpectedRegExp)),
        ("//a", 0, error(0, LexicalErrorKind::ExpectedRegExp)),
        ("/*a*/", 0, error(0, LexicalErrorKind::ExpectedRegExp)),
        ("", 0, error(0, LexicalErrorKind::ExpectedRegExp)),
        ("/abc", 0, error(0, LexicalErrorKind::UnterminatedRegExp)),
        ("x /[/]", 2, error(2, LexicalErrorKind::UnterminatedRegExp)),
        (r"/a\", 0, error(0, LexicalErrorKind::UnterminatedRegExp)),
        (
            "/a\nb/",
            0,
            error(2, LexicalErrorKind::RegExpLineTerminator),
        ),
        (
            "/a\r\n/",
            0,
            error(2, LexicalErrorKind::RegExpLineTerminator),
        ),
        (
            "/a\\\n/",
            0,
            error(3, LexicalErrorKind::RegExpLineTerminator),
        ),
        (
            "/[\u{2028}]/",
            0,
            error(2, LexicalErrorKind::RegExpLineTerminator),
        ),
        (
            "/\u{2029}/",
            0,
            error(1, LexicalErrorKind::RegExpLineTerminator),
        ),
        (
            "/a/g\\u0069",
            0,
            error(4, LexicalErrorKind::InvalidRegExpFlag),
        ),
        (
            "/a/\\u0067",
            0,
            error(3, LexicalErrorKind::InvalidRegExpFlag),
        ),
    ] {
        let input = source(text);
        assert_eq!(
            scan_js_regexp(&input, offset, limits),
            Err(expected.clone())
        );
        assert_eq!(validate_js_regexp(&input, offset, limits), Err(expected));
    }
    let input = source("é/a/");
    assert_eq!(
        scan_js_regexp(&input, 1, limits),
        Err(error(
            1,
            LexicalErrorKind::InvalidOffset(TextError::NotCharBoundary { offset: 1 })
        ))
    );
    assert_eq!(
        scan_js_regexp(&input, 9, limits),
        Err(error(
            9,
            LexicalErrorKind::InvalidOffset(TextError::OutOfBounds { offset: 9, len: 5 })
        ))
    );
}

#[test]
fn each_byte_limit_boundary_is_checked_before_consumption() {
    let text = "/[é]/gu";
    let input = source(text);
    for max_regexp_bytes in 0..=text.len() + 1 {
        let limits = LexicalLimits {
            max_regexp_bytes,
            ..LexicalLimits::default()
        };
        let result = scan_js_regexp(&input, 0, limits);
        if max_regexp_bytes >= text.len() {
            assert_eq!(result.unwrap().span, Span::new(0, text.len()).unwrap());
            continue;
        }
        // The two-byte scalar `é` occupies offsets 2 and 3.
        let offset = if max_regexp_bytes == 3 {
            2
        } else {
            max_regexp_bytes
        };
        assert_eq!(result, Err(error(offset, LexicalErrorKind::RegExpLimit)));
        assert_eq!(
            validate_js_regexp(&input, 0, limits).map(|literal| literal.token),
            result
        );
    }
}

#[test]
fn flags_are_validated_in_source_order() {
    let input = source("/a/dgimsyu");
    let literal = validate_js_regexp(&input, 0, LexicalLimits::default()).unwrap();
    for &flag in RegExpFlag::ALL {
        assert_eq!(
            literal.flags.contains(flag),
            flag != RegExpFlag::UnicodeSets,
            "{flag:?}"
        );
    }
    assert!(literal.flags.is_unicode_mode());
    let letters: String = RegExpFlag::ALL.iter().map(|flag| flag.letter()).collect();
    assert_eq!(letters, "dgimsuvy");
    let plain = validate_js_regexp(&source("/a/"), 0, LexicalLimits::default()).unwrap();
    assert!(!plain.flags.is_unicode_mode());
    assert!(
        RegExpFlag::ALL
            .iter()
            .all(|&flag| !plain.flags.contains(flag))
    );
    check(&[
        ("/a/v", Ok(0)),
        ("/a/x", Err(error(3, LexicalErrorKind::InvalidRegExpFlag))),
        ("/a/gG", Err(error(4, LexicalErrorKind::InvalidRegExpFlag))),
        ("/a/gé", Err(error(4, LexicalErrorKind::InvalidRegExpFlag))),
        (
            "/a/gig",
            Err(error(5, LexicalErrorKind::DuplicateRegExpFlag)),
        ),
        (
            "/a/uv",
            Err(error(4, LexicalErrorKind::IncompatibleRegExpFlags)),
        ),
        (
            "/a/vgu",
            Err(error(5, LexicalErrorKind::IncompatibleRegExpFlags)),
        ),
        // Flags are checked before the pattern.
        ("/(/x", Err(error(3, LexicalErrorKind::InvalidRegExpFlag))),
    ]);
}

#[test]
fn annex_b_applies_only_without_unicode_flags() {
    // Each body is accepted without flags, as browsers do, and rejected in
    // Unicode mode at the given offset.
    for (literal, offset, kind) in [
        ("/]/", 1, LoneSyntaxCharacter),
        ("/{/", 1, LoneSyntaxCharacter),
        ("/}/", 1, LoneSyntaxCharacter),
        ("/a{1/", 2, LoneSyntaxCharacter),
        ("/a{,1}/", 2, LoneSyntaxCharacter),
        (r"/\z/", 1, InvalidEscape),
        (r"/\-/", 1, InvalidEscape),
        (r"/\8/", 1, InvalidBackreference),
        (r"/\2(a)/", 1, InvalidBackreference),
        (r"/\00/", 1, InvalidEscape),
        (r"/\c1/", 1, InvalidEscape),
        (r"/[\c1]/", 2, InvalidEscape),
        (r"/[\c-]/", 2, InvalidEscape),
        (r"/[\B]/", 2, InvalidEscape),
        (r"/\x4/", 1, InvalidEscape),
        (r"/\u{110000}/", 1, InvalidEscape),
        (r"/\u004/", 1, InvalidEscape),
        (r"/\p/", 1, InvalidPropertyEscape),
        (r"/(?=a)*/", 6, NothingToRepeat),
        (r"/(?!a){2}/", 6, NothingToRepeat),
        (r"/\k/", 1, InvalidEscape),
        (r"/[\k]/", 2, InvalidEscape),
        (r"/\k<a>/", 1, UnresolvedGroupName),
    ] {
        assert!(validate(literal).is_ok(), "{literal}");
        for flag in ["u", "v"] {
            assert_eq!(
                validate(&format!("{literal}{flag}")),
                pattern(offset, kind),
                "{literal}{flag}"
            );
        }
    }
    check(&[
        // A class escape endpoint makes `-` literal only without Unicode mode.
        (r"/[\d-z]/", Ok(0)),
        (r"/[\d-z]/u", pattern(2, InvalidClassRange)),
        (r"/[\d-z]/v", pattern(4, InvalidClassSetSyntax)),
        (r"/[a-\w]/", Ok(0)),
        (r"/[a-\w]/u", pattern(2, InvalidClassRange)),
        (r"/[a-\w]/v", pattern(4, InvalidClassSetSyntax)),
        (r"/\1(a)/", Ok(1)),
        (r"/\1(a)/u", Ok(1)),
        (r"/\u{41}/", Ok(0)),
        (r"/\u{41}/u", Ok(0)),
        (r"/[\0-\8]/", Ok(0)),
        ("/{1}/", pattern(1, NothingToRepeat)),
        ("/a|{1,2}/", pattern(3, NothingToRepeat)),
        ("/(?<=a)*/", pattern(7, NothingToRepeat)),
        ("/^*/", pattern(2, NothingToRepeat)),
        (r"/\b+/", pattern(3, NothingToRepeat)),
        ("/[z-a]/", pattern(2, InvalidClassRange)),
    ]);
}

#[test]
fn annex_b_classes_see_utf16_code_units() {
    check(&[
        // Lead D83D, trail DE00, then the range DE00-D83D is out of order.
        ("/[😀-😂]/", pattern(2, InvalidClassRange)),
        ("/[😀-😂]/u", Ok(0)),
        ("/[😀-😂]/v", Ok(0)),
        (
            "/[\\uD83D\\uDE00-\\uD83D\\uDE02]/",
            pattern(8, InvalidClassRange),
        ),
        ("/[\\uD83D\\uDE00-\\uD83D\\uDE02]/u", Ok(0)),
        ("/[a-😀]/", Ok(0)),
        ("/[😀]/", Ok(0)),
        ("/[\\😀]/", Ok(0)),
        (r"/[\u{1F600}-\u{1F602}]/u", Ok(0)),
        (r"/[\u{1F602}-\u{1F600}]/u", pattern(2, InvalidClassRange)),
        // Legacy octal escapes have values in classes.
        (r"/[\1-\0]/", pattern(2, InvalidClassRange)),
        (r"/[\377-\400]/", pattern(2, InvalidClassRange)),
        (r"/[\08-9]/", Ok(0)),
        (r"/[\c_-\cA]/", pattern(2, InvalidClassRange)),
    ]);
}

#[test]
fn annex_b_reparses_with_named_groups_when_a_group_has_a_name() {
    check(&[
        (r"/\k<a>/", Ok(0)),
        (r"/[\k]/", Ok(0)),
        (r"/\k<a>(?<b>x)/", pattern(1, UnresolvedGroupName)),
        (r"/\k(?<a>x)/", pattern(1, InvalidEscape)),
        (r"/[\k](?<a>x)/", pattern(2, InvalidEscape)),
        (r"/(?<a>.)\k<a>/", Ok(1)),
        (r"/\k<a>(?<a>.)/", Ok(1)),
        // A lookbehind is not a group name, so `\k` stays an identity escape.
        (r"/(?<=a)\k/", Ok(0)),
    ]);
}

#[test]
fn group_names_follow_identifier_rules_and_may_repeat_only_in_alternatives() {
    check(&[
        ("/(?<a>x)|(?<a>y)/", Ok(2)),
        ("/(?<a>x)(?<a>y)/", pattern(8, DuplicateGroupName)),
        (r"/(?:(?<a>x)|(?<a>y))\k<a>/", Ok(2)),
        ("/(?<a>x)|(?:(?<a>y)|(?<a>z))/", Ok(3)),
        ("/(?:(?<a>x)|y)(?<a>z)/", pattern(14, DuplicateGroupName)),
        ("/(?<a>(?<a>x))/", pattern(6, DuplicateGroupName)),
        ("/(?<a>x)|(?<a>y)(?<a>z)/", pattern(16, DuplicateGroupName)),
        ("/(?:(?<a>x)|(?<a>y))|(?<a>z)/", Ok(3)),
        (
            "/(?:(?<a>x)|(?<a>y))(?<a>z)/",
            pattern(20, DuplicateGroupName),
        ),
        ("/(?<a>x)|(?<b>y)\\k<a>/", Ok(2)),
        ("/(?<\\u0061>x)\\k<a>/", Ok(1)),
        (r"/(?<a\u{62}>x)\k<ab>/", Ok(1)),
        ("/(?<\\uD835\\uDC9C>x)/", Ok(1)),
        ("/(?<变量$_\u{200c}>x)/u", Ok(1)),
        (r"/(?<\uD835>x)/", pattern(4, InvalidGroupName)),
        ("/(?<\\u0030>x)/", pattern(4, InvalidGroupName)),
        (r"/(?<a\x62>x)/", pattern(5, InvalidGroupName)),
        ("/(?<1>x)/", pattern(4, InvalidGroupName)),
        ("/(?<>x)/", pattern(4, InvalidGroupName)),
        ("/(?<a/", pattern(5, InvalidGroupName)),
        ("/(?<a-b>x)/", pattern(5, InvalidGroupName)),
        (r"/(?<a>x)\k<b>/", pattern(8, UnresolvedGroupName)),
        (r"/(?<a>x)\k<b>\9/u", pattern(8, UnresolvedGroupName)),
        (r"/\9(?<a>x)\k<b>/u", pattern(1, InvalidBackreference)),
        (r"/\k/u", pattern(1, InvalidEscape)),
    ]);
}

#[test]
fn groups_modifiers_and_quantifiers_have_exact_errors() {
    check(&[
        ("/(a)(?:b)(?<n>c)(?=(d))(?<!(e))/", Ok(4)),
        ("/(?i:a)(?ims-:b)(?-i:c)(?i-s:d)/", Ok(0)),
        ("/(?ii:a)/", pattern(1, InvalidModifiers)),
        ("/(?i-i:a)/", pattern(1, InvalidModifiers)),
        ("/(?-:a)/", pattern(1, InvalidModifiers)),
        ("/(?i-ss:a)/", pattern(1, InvalidModifiers)),
        ("/(?x:a)/", pattern(1, InvalidGroup)),
        ("/(?i)a/", pattern(1, InvalidGroup)),
        ("/(?/", pattern(1, InvalidGroup)),
        ("/(a/", pattern(1, UnterminatedGroup)),
        ("/((a)/", pattern(1, UnterminatedGroup)),
        ("/(a)(b/", pattern(4, UnterminatedGroup)),
        ("/a)/", pattern(2, UnmatchedParenthesis)),
        ("/a{2,1}/", pattern(2, InvalidQuantifierRange)),
        ("/a{2,1}/u", pattern(2, InvalidQuantifierRange)),
        ("/a{1,99999999999999999999}/", Ok(0)),
        (
            "/a{99999999999999999999,1}/",
            pattern(2, InvalidQuantifierRange),
        ),
        ("/a{01,1}?b{3}c{4,}?/u", Ok(0)),
        ("/a**/", pattern(3, NothingToRepeat)),
        ("/a*??/", pattern(4, NothingToRepeat)),
        ("/(*)/", pattern(2, NothingToRepeat)),
        ("/|*/", pattern(2, NothingToRepeat)),
        ("/+/u", pattern(1, NothingToRepeat)),
        ("/a{1}{2}/", pattern(5, NothingToRepeat)),
        ("/(?:a)+(b)*?/u", Ok(1)),
    ]);
}

#[test]
fn unicode_mode_escapes_and_classes_are_strict() {
    check(&[
        ("/\\u{1F600}\\uD83D\\uDE00\\uD83D\\u{41}/u", Ok(0)),
        (r"/\u{0000000041}/u", Ok(0)),
        (r"/\cA\cz\0\f\n\r\t\v\x41\//u", Ok(0)),
        (r"/\^\$\\\.\*\+\?\(\)\[\]\{\}\|/u", Ok(0)),
        (r"/[\-\b\d\W]/u", Ok(0)),
        (r"/\-/u", pattern(1, InvalidEscape)),
        (r"/\0 \01/u", pattern(4, InvalidEscape)),
        (r"/[\1]/u", pattern(2, InvalidEscape)),
        (r"/\u{}/u", pattern(1, InvalidEscape)),
        (r"/\q{a}/v", pattern(1, InvalidEscape)),
        ("/a]/u", pattern(2, LoneSyntaxCharacter)),
    ]);
}

#[test]
fn property_escapes_use_exact_pinned_names() {
    for property in [
        "L",
        "Letter",
        "digit",
        "punct",
        "Combining_Mark",
        "gc=Lu",
        "General_Category=Decimal_Number",
        "sc=Latn",
        "Script=Latin",
        "scx=Grek",
        "Script_Extensions=Greek",
        "sc=Qaai",
        // Listed in PropertyValueAliases.txt, so valid even without code points.
        "sc=Hrkt",
        "Script=Katakana_Or_Hiragana",
        // New in Unicode 18.0.0.
        "sc=Seal",
        "Script_Extensions=Jurchen",
        "ASCII",
        "Any",
        "Assigned",
        "Lowercase",
        "space",
        "XIDS",
    ] {
        for (flags, escape) in [("u", "p"), ("u", "P"), ("v", "p"), ("v", "P")] {
            let literal = format!(r"/\{escape}{{{property}}}[\{escape}{{{property}}}]/{flags}");
            assert_eq!(validate(&literal), Ok(0), "{literal}");
        }
    }
    for property in [
        "l",
        "letter",
        "gc=Latin",
        "sc=L",
        "General_Category=L=L",
        "Foo",
        "ASCII=Y",
        "Script",
        "Basic_Emoji=Y",
        "",
        " L",
        "L ",
        "isL",
    ] {
        let literal = format!(r"/\p{{{property}}}/u");
        assert_eq!(
            validate(&literal),
            pattern(1, InvalidPropertyEscape),
            "{literal}"
        );
    }
    check(&[
        (r"/\pL/u", pattern(1, InvalidPropertyEscape)),
        (r"/\p{L/u", pattern(1, InvalidPropertyEscape)),
        (
            r"/\p{RGI_Emoji}\p{Basic_Emoji}[\p{Emoji_Keycap_Sequence}]/v",
            Ok(0),
        ),
        (r"/\p{RGI_Emoji}/u", pattern(1, InvalidPropertyEscape)),
        (r"/[\p{RGI_Emoji}]/u", pattern(2, InvalidPropertyEscape)),
        (r"/\P{RGI_Emoji}/v", pattern(1, NegatedClassStrings)),
        (r"/[\P{RGI_Emoji}]/v", pattern(2, NegatedClassStrings)),
    ]);
}

#[test]
fn unicode_sets_classes_check_operators_and_strings() {
    check(&[
        (r"/[\p{L}--\p{Lu}--[a-z]]/v", Ok(0)),
        (r"/[\p{L}&&\p{Lu}&&\w]/v", Ok(0)),
        ("/[[a-z]--[aeiou]][^[a]][]/v", Ok(0)),
        (r"/[\q{abc|d|}a-z\-\&\b\x41]/v", Ok(0)),
        (r"/[^\q{a|b}][^\p{RGI_Emoji}&&\p{L}]/v", Ok(0)),
        (r"/[\q{abc}]/v", Ok(0)),
        ("/[a&&b--c]/v", pattern(6, InvalidClassSetSyntax)),
        ("/[ab&&c]/v", pattern(4, InvalidClassSetSyntax)),
        ("/[a-b&&c]/v", pattern(5, InvalidClassSetSyntax)),
        ("/[a&&b-c]/v", pattern(5, InvalidClassSetSyntax)),
        ("/[a&&bc]/v", pattern(6, InvalidClassSetSyntax)),
        ("/[a&&&b]/v", pattern(5, InvalidClassSetSyntax)),
        ("/[&&a]/v", pattern(2, InvalidClassSetSyntax)),
        ("/[a&&]/v", pattern(5, InvalidClassSetSyntax)),
        ("/[a-]/v", pattern(4, InvalidClassSetSyntax)),
        ("/[-a]/v", pattern(2, InvalidClassSetSyntax)),
        ("/[(]/v", pattern(2, InvalidClassSetSyntax)),
        ("/[a!!b]/v", pattern(3, InvalidClassSetSyntax)),
        ("/[^^^]/v", pattern(3, InvalidClassSetSyntax)),
        (r"/[\d-a]/v", pattern(4, InvalidClassSetSyntax)),
        (r"/[a-\d]/v", pattern(4, InvalidClassSetSyntax)),
        (r"/[\q{a]/v", pattern(6, InvalidClassSetSyntax)),
        ("/[z-a]/v", pattern(2, InvalidClassRange)),
        ("/[[a]/v", pattern(1, UnterminatedCharacterClass)),
        (r"/[^\q{abc}]/v", pattern(1, NegatedClassStrings)),
        (r"/[^\q{}]/v", pattern(1, NegatedClassStrings)),
        (r"/[^[\p{RGI_Emoji}]]/v", pattern(1, NegatedClassStrings)),
        (r"/[[^\q{ab}]]/v", pattern(2, NegatedClassStrings)),
        (
            r"/[^\p{RGI_Emoji}--\q{x}]/v",
            pattern(1, NegatedClassStrings),
        ),
        (r"/[^\q{ab}&&\q{cd}]/v", pattern(1, NegatedClassStrings)),
        (r"/[^\q{ab}&&a]/v", Ok(0)),
    ]);
}

#[test]
fn deep_nesting_is_iterative_and_linear() {
    let depth = 200_000;
    let groups = format!("/{}a{}/u", "(".repeat(depth), ")".repeat(depth));
    assert_eq!(validate(&groups), Ok(depth as u32));
    let unclosed = format!("/{}a/", "(?:".repeat(depth));
    assert_eq!(
        validate(&unclosed),
        pattern(1 + 3 * (depth - 1), UnterminatedGroup)
    );
    let classes = format!("/{}{}/v", "[".repeat(depth), "]".repeat(depth));
    assert_eq!(validate(&classes), Ok(0));
    let names: String = (0..20_000).map(|_| "(?<a>x)|").collect();
    assert_eq!(validate(&format!("/{names}(?<a>x)/")), Ok(20_001));
    let escapes = format!(r"/\u{{{}41}}/u", "0".repeat(depth));
    assert_eq!(validate(&escapes), Ok(0));
}

#[test]
fn a_body_span_can_drive_an_exact_edit() {
    let input = source("const r = /a+(b)/gi; // keep");
    let literal = validate_js_regexp(&input, 10, LexicalLimits::default()).unwrap();
    assert_eq!(literal.capturing_groups, 1);
    let rewritten = apply_edits(
        &input,
        &[Edit {
            span: literal.token.body_span,
            replacement: "c*",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        }],
        EditLimits::default(),
    )
    .unwrap();
    assert_eq!(rewritten.as_str(), "const r = /c*/gi; // keep");
    assert_eq!(rewritten.provenance().len(), 3);
}

#[test]
fn generated_literals_have_deterministic_bounded_results() {
    let alphabet = [
        '/', '[', ']', '\\', '(', ')', '?', '{', '1', '}', 'u', 'v', '\n',
    ];
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
                let limits = LexicalLimits::default();
                let scanned = scan_js_regexp(&input, offset, limits);
                assert_eq!(scanned, scan_js_regexp(&input, offset, limits));
                let validated = validate_js_regexp(&input, offset, limits);
                assert_eq!(validated, validate_js_regexp(&input, offset, limits));
                match (scanned, validated) {
                    (Ok(token), Ok(literal)) => {
                        assert_eq!(literal.token, token);
                        assert_eq!(token.span.start(), offset);
                        assert_eq!(token.body_span.start(), offset + 1);
                        assert!(!token.body_span.is_empty());
                        assert_eq!(token.flags_span.start(), token.body_span.end() + 1);
                        assert_eq!(token.flags_span.end(), token.span.end());
                        assert!(!input.slice(token.span).unwrap().contains('\n'));
                    }
                    (Ok(token), Err(failure)) => {
                        // A scanned literal can only fail flag or pattern validation.
                        let flag = matches!(
                            failure.kind,
                            LexicalErrorKind::InvalidRegExpFlag
                                | LexicalErrorKind::DuplicateRegExpFlag
                                | LexicalErrorKind::IncompatibleRegExpFlags
                        );
                        let (start, end) = if flag {
                            (token.flags_span.start(), token.flags_span.end() - 1)
                        } else {
                            assert!(
                                matches!(failure.kind, LexicalErrorKind::InvalidRegExpPattern(_)),
                                "{failure:?} for {text:?} at {offset}"
                            );
                            (token.body_span.start(), token.body_span.end())
                        };
                        assert!((start..=end).contains(&failure.offset));
                    }
                    (Err(failure), validated) => {
                        assert_eq!(validated, Err(failure.clone()));
                        assert!(input.validate_offset(failure.offset).is_ok());
                    }
                }
                assert_eq!(input.as_str(), text);
            }
        }
    }
}
