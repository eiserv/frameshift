//! JSON and JSONC grammar, value, duplicate-key, and limit contracts.

use frameshift_core::TextLimits;

use super::*;

fn text(source: &str) -> SourceText {
    SourceText::new(source.to_owned(), TextLimits::default()).unwrap()
}

fn parse(source: &SourceText, dialect: JsonDialect) -> Result<JsonDocument<'_>, JsonError> {
    parse_json(source, dialect, JsonLimits::default())
}

fn accepts(source: &str, dialect: JsonDialect) -> bool {
    parse(&text(source), dialect).is_ok()
}

fn error(source: &str, dialect: JsonDialect) -> (JsonErrorKind, usize, usize) {
    let source = text(source);
    let error = parse(&source, dialect).unwrap_err();
    (error.kind(), error.span().start(), error.span().end())
}

/// Renders a value as canonical JSON for comparisons.
fn render(document: &JsonDocument<'_>, id: JsonId) -> String {
    match document.value(id).unwrap() {
        JsonValue::Null => "null".to_owned(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(number) => number.text().to_owned(),
        JsonValue::String(value) => format!("{value:?}"),
        JsonValue::Array(array) => {
            let items: Vec<String> = array.iter().map(|item| render(document, item)).collect();
            format!("[{}]", items.join(","))
        }
        JsonValue::Object(object) => {
            let members: Vec<String> = object
                .members()
                .map(|member| format!("{:?}:{}", member.key(), render(document, member.value())))
                .collect();
            format!("{{{}}}", members.join(","))
        }
    }
}

fn rendered(source: &str, dialect: JsonDialect) -> String {
    let source = text(source);
    let document = parse(&source, dialect).unwrap();
    render(&document, document.root())
}

#[test]
fn values_spans_and_identities_follow_the_source() {
    let source = text(" {\"a\": [1, true, null], \"b\": {\"c\": \"d\"}} ");
    let document = parse(&source, JsonDialect::Json).unwrap();
    let root = document.root();
    assert_eq!(root.index(), 0);
    assert_eq!(document.span(root).unwrap(), Span::new(1, 40).unwrap());
    let object = document.value(root).unwrap().as_object().unwrap();
    assert_eq!(object.len(), 2);
    let keys: Vec<&str> = object.members().map(JsonMember::key).collect();
    assert_eq!(keys, ["a", "b"]);
    let a = object.get("a").unwrap();
    let array = document.value(a).unwrap().as_array().unwrap();
    assert_eq!(array.len(), 3);
    let spans: Vec<&str> = array
        .iter()
        .map(|id| {
            let span = document.span(id).unwrap();
            &source.as_str()[span.start()..span.end()]
        })
        .collect();
    assert_eq!(spans, ["1", "true", "null"]);
    let member = object.member(1).unwrap();
    assert_eq!(member.key_span(), Span::new(24, 27).unwrap());
    assert_eq!(document.value(member.key_id()).unwrap().as_str(), Some("b"));
    // Identities ascend with the values' starts: keys and values in order.
    let starts: Vec<usize> = (0..document.node_count())
        .map(|index| document.span(JsonId(index as u32)).unwrap().start())
        .collect();
    assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(document.node_count(), 10);
    assert!(document.value(JsonId(10)).is_none());
    assert!(document.span(JsonId(10)).is_none());
    assert!(!document.has_bom());

    for (source, expected) in [
        ("0", "0"),
        ("\"x\"", "\"x\""),
        ("[]", "[]"),
        ("{}", "{}"),
        ("[[[]],{}]", "[[[]],{}]"),
        (" \t\r\nnull\n", "null"),
        ("false", "false"),
    ] {
        assert_eq!(rendered(source, JsonDialect::Json), expected, "{source}");
    }
}

#[test]
fn a_byte_order_mark_is_skipped_and_recorded() {
    let source = text("\u{FEFF}{\"k\": 1}");
    let document = parse(&source, JsonDialect::Json).unwrap();
    assert!(document.has_bom());
    assert_eq!(
        document.span(document.root()).unwrap(),
        Span::new(3, 11).unwrap()
    );
    // Only a leading mark is skipped.
    assert_eq!(
        error("[\u{FEFF}]", JsonDialect::Json),
        (
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Value
            },
            1,
            4
        )
    );
}

#[test]
fn numbers_follow_the_grammar_and_keep_their_text() {
    for valid in [
        "0",
        "-0",
        "1",
        "-12",
        "1.5",
        "0.25",
        "1e5",
        "1E+5",
        "1e-05",
        "-0.0e0",
        "12345678901234567890123",
    ] {
        assert_eq!(rendered(valid, JsonDialect::Json), valid);
    }
    for (invalid, end) in [
        ("01", 2),
        ("-01", 3),
        ("1.", 2),
        ("1.e5", 3),
        (".5", 1),
        ("-", 1),
        ("-a", 2),
        ("+1", 1),
        ("1e", 2),
        ("1e+", 3),
        ("1E-x", 4),
    ] {
        let (kind, start, actual_end) = error(invalid, JsonDialect::Jsonc);
        if invalid.starts_with(['.', '+']) {
            assert_eq!(
                kind,
                JsonErrorKind::UnexpectedCharacter {
                    expected: JsonExpected::Value
                },
                "{invalid}"
            );
        } else {
            assert_eq!(kind, JsonErrorKind::InvalidNumber, "{invalid}");
        }
        assert_eq!((start, actual_end), (0, end), "{invalid}");
    }
    let number = |source: &str| {
        let source = text(source);
        let document = parse(&source, JsonDialect::Json).unwrap();
        let number = document
            .value(document.root())
            .unwrap()
            .as_number()
            .unwrap();
        (number.is_integer(), number.as_u64(), number.as_i64())
    };
    assert_eq!(number("42"), (true, Some(42), Some(42)));
    assert_eq!(number("-0"), (true, None, Some(0)));
    assert_eq!(number("-7"), (true, None, Some(-7)));
    assert_eq!(number("18446744073709551615"), (true, Some(u64::MAX), None));
    assert_eq!(number("18446744073709551616"), (true, None, None));
    assert_eq!(number("1.0"), (false, None, None));
    assert_eq!(number("1e2"), (false, None, None));
    // A number directly followed by a letter is not a number and a word.
    assert_eq!(
        error("1a", JsonDialect::Json),
        (
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::End
            },
            1,
            2
        )
    );
}

#[test]
fn strings_are_validated_and_decoded() {
    let decoded = |source: &str| {
        let source = text(source);
        let document = parse(&source, JsonDialect::Json).unwrap();
        document
            .value(document.root())
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(decoded("\"plain é\""), "plain é");
    assert_eq!(
        decoded("\"\\\" \\\\ \\/ \\b \\f \\n \\r \\t\""),
        "\" \\ / \u{8} \u{c} \n \r \t"
    );
    assert_eq!(decoded("\"\\u0041\\u00e9\\u20AC\""), "A\u{e9}\u{20ac}");
    assert_eq!(decoded("\"x\\ud83d\\ude00y\""), "x\u{1f600}y");
    assert_eq!(decoded("\"\\u0000\""), "\0");
    // Line and paragraph separators and DEL need no escape in JSON.
    assert_eq!(
        decoded("\"\u{2028}\u{2029}\u{7f}\""),
        "\u{2028}\u{2029}\u{7f}"
    );

    for (source, kind, start, end) in [
        ("\"\\ud800\"", JsonErrorKind::LoneSurrogate, 1, 7),
        ("\"\\udc00\"", JsonErrorKind::LoneSurrogate, 1, 7),
        ("\"\\ud800\\u0041\"", JsonErrorKind::LoneSurrogate, 1, 7),
        ("\"\\ud800\\ud800\"", JsonErrorKind::LoneSurrogate, 1, 7),
        ("\"\\uDBFF\"", JsonErrorKind::LoneSurrogate, 1, 7),
        ("\"\\u12\"", JsonErrorKind::InvalidEscape, 1, 3),
        ("\"\\uZZZZ\"", JsonErrorKind::InvalidEscape, 1, 3),
        ("\"\\x\"", JsonErrorKind::InvalidEscape, 1, 3),
        ("\"\\é\"", JsonErrorKind::InvalidEscape, 1, 4),
        ("\"\\'\"", JsonErrorKind::InvalidEscape, 1, 3),
        ("\"a\tb\"", JsonErrorKind::ControlCharacter, 2, 3),
        ("\"a\nb\"", JsonErrorKind::ControlCharacter, 2, 3),
        ("\"open", JsonErrorKind::UnterminatedString, 0, 5),
        ("\"open\\", JsonErrorKind::UnterminatedString, 0, 6),
        (
            "'single'",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Value,
            },
            0,
            1,
        ),
    ] {
        assert_eq!(
            error(source, JsonDialect::Jsonc),
            (kind, start, end),
            "{source:?}"
        );
    }
}

#[test]
fn only_the_four_whitespace_characters_separate_tokens() {
    for space in [
        "\u{a0}", "\u{c}", "\u{b}", "\u{2028}", "\u{feff}", "\u{3000}",
    ] {
        let source = format!("[1,{space}2]");
        let (kind, start, end) = error(&source, JsonDialect::Jsonc);
        assert_eq!(
            kind,
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Value
            },
            "{space:?}"
        );
        assert_eq!((start, end), (3, 3 + space.len()));
    }
}

#[test]
fn jsonc_adds_comments_and_one_trailing_comma() {
    let source = "// leading\n{\n  /* block\n  */ \"a\": [1, 2,], // after\n  \"b\": {\"c\": 3,},\r\n}\n// end";
    assert_eq!(
        rendered(source, JsonDialect::Jsonc),
        "{\"a\":[1,2],\"b\":{\"c\":3}}"
    );
    assert_eq!(
        rendered("[1 /* a */ , /**/ 2]", JsonDialect::Jsonc),
        "[1,2]"
    );
    assert_eq!(
        rendered("1 // é\u{2028}still comment", JsonDialect::Jsonc),
        "1"
    );
    assert_eq!(rendered("/***/0/*/ */", JsonDialect::Jsonc), "0");

    for (source, kind, start, end) in [
        (
            "[,]",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Value,
            },
            1,
            2,
        ),
        (
            "[1,,]",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Value,
            },
            3,
            4,
        ),
        (
            "{,}",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Key,
            },
            1,
            2,
        ),
        (
            "{\"a\":1,,}",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::Key,
            },
            7,
            8,
        ),
        ("[1] /* open", JsonErrorKind::UnterminatedComment, 4, 11),
        (
            "[1 / 2]",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::ArrayContinuation,
            },
            3,
            4,
        ),
        (
            "[1]/",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::End,
            },
            3,
            4,
        ),
        (
            "1,",
            JsonErrorKind::UnexpectedCharacter {
                expected: JsonExpected::End,
            },
            1,
            2,
        ),
    ] {
        assert_eq!(
            error(source, JsonDialect::Jsonc),
            (kind, start, end),
            "{source}"
        );
    }

    for (source, kind, start, end) in [
        ("[1, 2,]", JsonErrorKind::TrailingCommaNotAllowed, 5, 6),
        ("{\"a\": 1 ,}", JsonErrorKind::TrailingCommaNotAllowed, 8, 9),
        ("// c\n1", JsonErrorKind::CommentNotAllowed, 0, 2),
        ("[1 /* c */]", JsonErrorKind::CommentNotAllowed, 3, 5),
    ] {
        assert_eq!(
            error(source, JsonDialect::Json),
            (kind, start, end),
            "{source}"
        );
    }
}

#[test]
fn structural_errors_name_what_was_expected() {
    let unexpected = |expected| JsonErrorKind::UnexpectedCharacter { expected };
    let end = |expected| JsonErrorKind::UnexpectedEnd { expected };
    for (source, kind, start, stop) in [
        ("", end(JsonExpected::Value), 0, 0),
        ("   ", end(JsonExpected::Value), 3, 3),
        ("[", end(JsonExpected::Value), 1, 1),
        ("[1", end(JsonExpected::ArrayContinuation), 2, 2),
        ("{", end(JsonExpected::Key), 1, 1),
        ("{\"a\"", end(JsonExpected::Colon), 4, 4),
        ("{\"a\":", end(JsonExpected::Value), 5, 5),
        ("{\"a\":1", end(JsonExpected::ObjectContinuation), 6, 6),
        ("{1:2}", unexpected(JsonExpected::Key), 1, 2),
        ("{\"a\" 1}", unexpected(JsonExpected::Colon), 5, 6),
        ("[1 2]", unexpected(JsonExpected::ArrayContinuation), 3, 4),
        (
            "{\"a\":1 \"b\":2}",
            unexpected(JsonExpected::ObjectContinuation),
            7,
            8,
        ),
        ("[1}", unexpected(JsonExpected::ArrayContinuation), 2, 3),
        ("{]", unexpected(JsonExpected::Key), 1, 2),
        ("1 2", unexpected(JsonExpected::End), 2, 3),
        ("tru", end(JsonExpected::Value), 3, 3),
        ("trux", unexpected(JsonExpected::Value), 3, 4),
        ("nul é", unexpected(JsonExpected::Value), 3, 4),
        ("[é]", unexpected(JsonExpected::Value), 1, 3),
        ("True", unexpected(JsonExpected::Value), 0, 1),
        ("NaN", unexpected(JsonExpected::Value), 0, 1),
        ("undefined", unexpected(JsonExpected::Value), 0, 1),
    ] {
        assert_eq!(
            error(source, JsonDialect::Jsonc),
            (kind, start, stop),
            "{source:?}"
        );
    }
    let message = parse(&text("[1 2]"), JsonDialect::Json)
        .unwrap_err()
        .to_string();
    assert_eq!(
        message,
        "unexpected character, expected ',' or ']' at bytes 3..4"
    );
}

#[test]
fn duplicate_keys_are_kept_marked_and_resolved_to_the_last() {
    let source = text(
        "{\"a\": 1, \"b\": {\"x\": 1, \"x\": 2}, \"a\": 2, \"\\u0061\": 3, \"c\": 4, \"b\": 5}",
    );
    let document = parse(&source, JsonDialect::Json).unwrap();
    let object = document
        .value(document.root())
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(object.len(), 6);
    let shadowed: Vec<(&str, bool)> = object
        .members()
        .map(|member| (member.key(), member.is_shadowed()))
        .collect();
    assert_eq!(
        shadowed,
        [
            ("a", true),
            ("b", true),
            ("a", true),
            ("a", false),
            ("c", false),
            ("b", false)
        ]
    );
    let value = |id: Option<JsonId>| render(&document, id.unwrap());
    assert_eq!(value(object.get("a")), "3");
    assert_eq!(value(object.get("b")), "5");
    assert_eq!(value(object.get("c")), "4");
    assert_eq!(object.get("d"), None);
    assert_eq!(object.get(""), None);
    // Duplicates are listed in source order, across nesting levels.
    let listed: Vec<(&str, usize)> = document
        .duplicate_keys()
        .map(|member| (member.key(), member.key_span().start()))
        .collect();
    assert_eq!(listed, [("a", 1), ("b", 9), ("x", 15), ("a", 32)]);

    let nested = document.value(object.member(1).unwrap().value()).unwrap();
    let nested = nested.as_object().unwrap();
    assert_eq!(render(&document, nested.get("x").unwrap()), "2");

    let unique = text("{\"b\": 1, \"a\": 2, \"ab\": 3, \"\": 4}");
    let unique = parse(&unique, JsonDialect::Json).unwrap();
    assert_eq!(unique.duplicate_keys().len(), 0);
    let object = unique.value(unique.root()).unwrap().as_object().unwrap();
    for (key, expected) in [("b", "1"), ("a", "2"), ("ab", "3"), ("", "4")] {
        assert_eq!(render(&unique, object.get(key).unwrap()), expected);
    }
}

#[test]
fn large_objects_resolve_keys_by_binary_search() {
    let members: Vec<String> = (0..2_000)
        .map(|index| format!("\"k{:04}\": {index}", (index * 7_919) % 2_000))
        .collect();
    let source = text(&format!("{{{}}}", members.join(",")));
    let document = parse(&source, JsonDialect::Json).unwrap();
    let object = document
        .value(document.root())
        .unwrap()
        .as_object()
        .unwrap();
    for index in 0..2_000 {
        let key = format!("k{:04}", (index * 7_919) % 2_000);
        assert_eq!(
            render(&document, object.get(&key).unwrap()),
            index.to_string()
        );
    }
}

#[test]
fn depth_and_node_limits_are_inclusive_and_explicit() {
    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let limits = JsonLimits {
        max_depth: 3,
        max_nodes: 100,
    };
    assert!(parse_json(&text(&nested(3)), JsonDialect::Json, limits).is_ok());
    let error = parse_json(&text(&nested(4)), JsonDialect::Json, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::DepthLimit { limit: 3 });
    assert_eq!(error.span(), Span::new(3, 4).unwrap());

    let limits = JsonLimits {
        max_depth: 3,
        max_nodes: 4,
    };
    // An object with one member has two nodes per member plus itself.
    assert!(parse_json(&text("[{\"a\": 1}]"), JsonDialect::Json, limits).is_ok());
    let error = parse_json(&text("[{\"a\": 1}, 2]"), JsonDialect::Json, limits).unwrap_err();
    assert_eq!(error.kind(), JsonErrorKind::NodeLimit { limit: 4 });
    assert_eq!(error.span(), Span::new(11, 12).unwrap());

    // Iterative parsing: deep input never consumes stack.
    let deep = text(&nested(200_000));
    let limits = JsonLimits {
        max_depth: 200_000,
        max_nodes: 200_000,
    };
    let document = parse_json(&deep, JsonDialect::Json, limits).unwrap();
    assert_eq!(document.node_count(), 200_000);
    assert_eq!(JsonLimits::default().max_depth, 256);
    assert_eq!(JsonLimits::default().max_nodes, 4_194_304);
}

#[test]
fn parsing_is_repeatable_and_jsonc_accepts_every_json_document() {
    for source in [
        "{\"name\": \"fictional\", \"version\": \"1.0.0\", \"private\": true}",
        "[1, [2, [3, {\"a\": [null]}]], \"\\u00e9\"]",
        "{\"x\": 1, \"x\": 2}",
    ] {
        let source = text(source);
        let first = parse(&source, JsonDialect::Json).unwrap();
        let second = parse(&source, JsonDialect::Json).unwrap();
        assert_eq!(first, second);
        let jsonc = parse(&source, JsonDialect::Jsonc).unwrap();
        assert_eq!(render(&first, first.root()), render(&jsonc, jsonc.root()));
    }
    assert!(accepts("[]", JsonDialect::Json));
    assert!(!accepts("[]]", JsonDialect::Jsonc));
}
