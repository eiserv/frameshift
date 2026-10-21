//! CSS tokenization through the public facade (`D-077`).
//!
//! Inputs are fictional. The opt-in `external_css_parsing_tests` test
//! compares tokenization with `component_value_list.json` of the public
//! css-parsing-tests suite, kept outside the repository.

use std::fmt::Write as _;

use frameshift_engine::foundation::{SourceText, Span, TextLimits};
use frameshift_engine::syntax::{
    CssError, CssHashKind, CssLimits, CssNumberKind, CssParseErrorKind, CssSign, CssToken,
    CssTokenContext, CssTokenKind, CssTokens, JsonDialect, JsonDocument, JsonId, JsonLimits,
    JsonValue, css_dimension_unit, css_token_value, parse_json, scan_css_token, tokenize_css,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn tokenize(source: &SourceText) -> CssTokens {
    let tokens = tokenize_css(source, CssLimits::default()).unwrap();
    assert_partition(source, &tokens);
    tokens
}

/// The byte order mark, tokens, and comments cover the text exactly once.
fn assert_partition(source: &SourceText, tokens: &CssTokens) {
    let mut spans: Vec<Span> = tokens.tokens().iter().map(|token| token.span).collect();
    spans.extend(tokens.comments());
    spans.sort();
    let mut offset = if tokens.has_bom() { 3 } else { 0 };
    for span in spans {
        assert_eq!(span.start(), offset, "gap or overlap at {offset}");
        assert!(!span.is_empty());
        offset = span.end();
    }
    assert_eq!(offset, source.len());
}

/// A compact rendering of tokens with decoded values, for assertions.
fn render(source: &SourceText, tokens: &[CssToken]) -> String {
    let mut out = String::new();
    for token in tokens {
        if !out.is_empty() {
            out.push(' ');
        }
        let value = css_token_value(source, token).unwrap();
        let value = value.as_deref().unwrap_or_default();
        let text = source.slice(token.span).unwrap();
        match token.kind {
            CssTokenKind::Ident => write!(out, "ident({value:?})"),
            CssTokenKind::Function => write!(out, "function({value:?})"),
            CssTokenKind::AtKeyword => write!(out, "at({value:?})"),
            CssTokenKind::Hash(CssHashKind::Id) => write!(out, "hash-id({value:?})"),
            CssTokenKind::Hash(CssHashKind::Unrestricted) => write!(out, "hash({value:?})"),
            CssTokenKind::String => write!(out, "string({value:?})"),
            CssTokenKind::BadString => write!(out, "bad-string"),
            CssTokenKind::Url => write!(out, "url({value:?})"),
            CssTokenKind::BadUrl => write!(out, "bad-url"),
            CssTokenKind::Delim(code_point) => write!(out, "delim({code_point:?})"),
            CssTokenKind::Number(number) => write!(out, "number({:?},{:?})", text, number.kind),
            CssTokenKind::Percentage(number) => {
                write!(out, "percentage({:?})", source.slice(number.span).unwrap())
            }
            CssTokenKind::Dimension { number, .. } => write!(
                out,
                "dimension({:?},{:?},{:?})",
                source.slice(number.span).unwrap(),
                number.kind,
                css_dimension_unit(source, token).unwrap().unwrap()
            ),
            CssTokenKind::UnicodeRange { start, end } => write!(out, "range({start:x},{end:x})"),
            CssTokenKind::Whitespace => write!(out, "ws"),
            CssTokenKind::Cdo => write!(out, "cdo"),
            CssTokenKind::Cdc => write!(out, "cdc"),
            CssTokenKind::Colon => write!(out, ":"),
            CssTokenKind::Semicolon => write!(out, ";"),
            CssTokenKind::Comma => write!(out, ","),
            CssTokenKind::OpenSquare => write!(out, "["),
            CssTokenKind::CloseSquare => write!(out, "]"),
            CssTokenKind::OpenParen => write!(out, "("),
            CssTokenKind::CloseParen => write!(out, ")"),
            CssTokenKind::OpenCurly => write!(out, "{{"),
            CssTokenKind::CloseCurly => write!(out, "}}"),
        }
        .unwrap();
    }
    out
}

fn tokens_of(text: &str) -> String {
    let source = source(text);
    let tokens = tokenize(&source);
    render(&source, tokens.tokens())
}

fn errors_of(text: &str) -> Vec<(CssParseErrorKind, usize)> {
    let source = source(text);
    tokenize(&source)
        .errors()
        .iter()
        .map(|error| (error.kind, error.offset))
        .collect()
}

#[test]
fn rules_declarations_and_comments() {
    let text = "/* fictional */ .card > a:hover { color: #fff; margin: -1.5em 0 }\n@media (width >= 40rem) {}";
    let source = source(text);
    let tokens = tokenize(&source);
    assert_eq!(tokens.comments(), [Span::new(0, 15).unwrap()]);
    assert!(tokens.errors().is_empty());
    assert_eq!(
        render(&source, tokens.tokens()),
        concat!(
            "ws delim('.') ident(\"card\") ws delim('>') ws ident(\"a\") : ident(\"hover\") ws { ws ",
            "ident(\"color\") : ws hash-id(\"fff\") ; ws ident(\"margin\") : ws ",
            "dimension(\"-1.5\",Number,\"em\") ws number(\"0\",Integer) ws } ws at(\"media\") ws ( ",
            "ident(\"width\") ws delim('>') delim('=') ws dimension(\"40\",Integer,\"rem\") ) ws { }"
        )
    );
}

#[test]
fn preprocessing_reads_newlines_and_nul_without_copying() {
    // CR LF, CR, and FF read as one LF each; NUL reads as U+FFFD, an ident
    // code point.
    let source = source("a\r\n\r\x0cb\0c");
    let tokens = tokenize(&source);
    assert_eq!(
        render(&source, tokens.tokens()),
        "ident(\"a\") ws ident(\"b\u{FFFD}c\")"
    );
    assert_eq!(tokens.tokens()[1].span, Span::new(1, 5).unwrap());
    // An escape's single trailing whitespace may be a CR LF pair.
    assert_eq!(tokens_of("\\41\r\nB"), "ident(\"AB\")");
    assert_eq!(tokens_of("\\41\r\n B"), "ident(\"A\") ws ident(\"B\")");
}

#[test]
fn escapes_follow_the_replacement_rules() {
    assert_eq!(tokens_of("\\31 23"), "ident(\"123\")");
    assert_eq!(tokens_of("\\0 x"), "ident(\"\u{FFFD}x\")");
    assert_eq!(tokens_of("\\D800"), "ident(\"\u{FFFD}\")");
    assert_eq!(tokens_of("\\110000"), "ident(\"\u{FFFD}\")");
    assert_eq!(tokens_of("\\10FFFF"), "ident(\"\\u{10ffff}\")");
    // At most six hex digits form one escape.
    assert_eq!(tokens_of("\\1234567"), "ident(\"\u{FFFD}7\")");
    assert_eq!(tokens_of("\\10fffff"), "ident(\"\\u{10ffff}f\")");
    assert_eq!(tokens_of("a\\:b"), "ident(\"a:b\")");
    // A backslash before a newline is no escape.
    assert_eq!(tokens_of("\\\nx"), "delim('\\\\') ws ident(\"x\")");
    assert_eq!(errors_of("\\\nx"), [(CssParseErrorKind::InvalidEscape, 0)]);
    // A backslash at EOF is a valid escape that reads as U+FFFD.
    assert_eq!(tokens_of("a\\"), "ident(\"a\u{FFFD}\")");
    assert_eq!(errors_of("a\\"), [(CssParseErrorKind::EscapeAtEof, 2)]);
}

#[test]
fn strings_bad_strings_and_unterminated_strings() {
    assert_eq!(
        tokens_of("'a\\'b' \"c\\\r\nd\" \"\\22\""),
        "string(\"a'b\") ws string(\"cd\") ws string(\"\\\"\")"
    );
    assert_eq!(
        tokens_of("'a\nb'"),
        "bad-string ws ident(\"b\") string(\"\")"
    );
    assert_eq!(
        errors_of("'a\nb'"),
        [
            (CssParseErrorKind::NewlineInString, 2),
            (CssParseErrorKind::UnterminatedString, 5)
        ]
    );
    assert_eq!(tokens_of("'open\\"), "string(\"open\")");
    assert_eq!(
        errors_of("'open\\"),
        [(CssParseErrorKind::UnterminatedString, 6)]
    );
}

#[test]
fn urls_and_quoted_url_functions() {
    assert_eq!(tokens_of("url(a.png)"), "url(\"a.png\")");
    assert_eq!(tokens_of("URL(  a.png  )"), "url(\"a.png\")");
    assert_eq!(tokens_of("u\\72l(x)"), "url(\"x\")");
    assert_eq!(tokens_of("url(a\\)b)"), "url(\"a)b\")");
    // A quoted argument makes a function; all but one whitespace before the
    // quote belong to the function token.
    let source = source("url(   \"a.png\")");
    let tokens = tokenize(&source);
    assert_eq!(
        render(&source, tokens.tokens()),
        "function(\"url\") ws string(\"a.png\") )"
    );
    assert_eq!(tokens.tokens()[0].span, Span::new(0, 6).unwrap());
    assert_eq!(tokens_of("url(a b) x"), "bad-url ws ident(\"x\")");
    assert_eq!(tokens_of("url(a\"b\\)c) x"), "bad-url ws ident(\"x\")");
    assert_eq!(
        errors_of("url(a\"b)"),
        [(CssParseErrorKind::InvalidUrlCodePoint, 5)]
    );
    assert_eq!(
        errors_of("url(a\\\n)"),
        [(CssParseErrorKind::InvalidUrlEscape, 5)]
    );
    assert_eq!(
        errors_of("url(a\u{1})"),
        [(CssParseErrorKind::InvalidUrlCodePoint, 5)]
    );
    assert_eq!(tokens_of("url(a"), "url(\"a\")");
    assert_eq!(
        errors_of("url(a "),
        [(CssParseErrorKind::UnterminatedUrl, 6)]
    );
}

#[test]
fn numbers_percentages_and_dimensions() {
    assert_eq!(
        tokens_of("+.5e-3 -0 1e 1e+ 10% 12px 1--x 7.0E2"),
        concat!(
            "number(\"+.5e-3\",Number) ws number(\"-0\",Integer) ws dimension(\"1\",Integer,\"e\") ws ",
            "dimension(\"1\",Integer,\"e\") delim('+') ws percentage(\"10\") ws ",
            "dimension(\"12\",Integer,\"px\") ws dimension(\"1\",Integer,\"--x\") ws ",
            "number(\"7.0E2\",Number)"
        )
    );
    let source = source("-12.5%");
    let tokens = tokenize(&source);
    let CssTokenKind::Percentage(number) = tokens.tokens()[0].kind else {
        panic!("not a percentage");
    };
    assert_eq!(number.sign, Some(CssSign::Minus));
    assert_eq!(number.kind, CssNumberKind::Number);
    assert_eq!(number.span, Span::new(0, 5).unwrap());
    assert_eq!(
        tokens_of("+-1 .x 1.x"),
        "delim('+') number(\"-1\",Integer) ws delim('.') ident(\"x\") ws number(\"1\",Integer) delim('.') ident(\"x\")"
    );
}

#[test]
fn hashes_at_keywords_cdo_and_cdc() {
    assert_eq!(
        tokens_of("#abc #1a #-a #\\31 # @--x @- <!-- -->"),
        concat!(
            "hash-id(\"abc\") ws hash(\"1a\") ws hash-id(\"-a\") ws hash-id(\"1\") delim('#') ws ",
            "at(\"--x\") ws delim('@') delim('-') ws cdo ws cdc"
        )
    );
}

#[test]
fn unicode_ranges_only_in_their_context() {
    assert_eq!(
        tokens_of("U+0-7F"),
        "ident(\"U\") number(\"+0\",Integer) dimension(\"-7\",Integer,\"F\")"
    );
    let source = source("U+0-7F, u+4??, U+1F600");
    let context = CssTokenContext {
        unicode_ranges_allowed: true,
    };
    let mut offset = 0;
    let mut ranges = Vec::new();
    while let Some(token) = scan_css_token(&source, offset, context).unwrap().token {
        if let CssTokenKind::UnicodeRange { start, end } = token.kind {
            ranges.push((start, end));
        }
        offset = token.span.end();
    }
    assert_eq!(ranges, [(0, 0x7f), (0x400, 0x4ff), (0x1f600, 0x1f600)]);
}

#[test]
fn byte_order_mark_comments_and_limits() {
    let source = source("\u{FEFF}a/* x */b/* open");
    let tokens = tokenize(&source);
    assert!(tokens.has_bom());
    assert_eq!(
        render(&source, tokens.tokens()),
        "ident(\"a\") ident(\"b\")"
    );
    assert_eq!(tokens.comments().len(), 2);
    assert_eq!(
        tokens.errors()[0].kind,
        CssParseErrorKind::UnterminatedComment
    );
    // Two tokens and two comments.
    let exact = CssLimits { max_tokens: 4 };
    assert!(tokenize_css(&source, exact).is_ok());
    let lower = CssLimits { max_tokens: 3 };
    assert_eq!(tokenize_css(&source, lower), Err(CssError::TokenLimit));
}

#[test]
fn values_reject_foreign_tokens_and_scans_reject_bad_offsets() {
    let source = source("é x");
    let foreign = CssToken {
        kind: CssTokenKind::Ident,
        span: Span::new(0, 1).unwrap(),
    };
    assert!(matches!(
        css_token_value(&source, &foreign),
        Err(CssError::InvalidOffset(_) | CssError::ForeignToken)
    ));
    assert!(matches!(
        scan_css_token(&source, 1, CssTokenContext::default()),
        Err(CssError::InvalidOffset(_))
    ));
    // Scanning at every token start reproduces the token.
    let tokens = tokenize(&source);
    for token in tokens.tokens() {
        let scan = scan_css_token(&source, token.span.start(), CssTokenContext::default()).unwrap();
        assert_eq!(scan.token.as_ref(), Some(token));
    }
}

/// Component values in the css-parsing-tests representation, with numbers
/// compared as their decimal value. Each token may carry the suite's marker
/// for a parse error at EOF, which follows the token.
fn component_values(
    source: &SourceText,
    tokens: &[(CssToken, Option<&str>)],
    index: &mut usize,
    close: Option<CssTokenKind>,
) -> Vec<String> {
    let mut values = Vec::new();
    while let Some((token, marker)) = tokens.get(*index) {
        *index += 1;
        if Some(token.kind) == close {
            return values;
        }
        let value = || {
            css_token_value(source, token)
                .unwrap()
                .unwrap()
                .into_owned()
        };
        let number = |span: Span, kind: CssNumberKind| {
            let text = source.slice(span).unwrap();
            let kind = match kind {
                CssNumberKind::Integer => "integer",
                CssNumberKind::Number => "number",
            };
            (text.to_owned(), number_value(text), kind)
        };
        values.push(match token.kind {
            CssTokenKind::Ident => format!("[ident {:?}]", value()),
            CssTokenKind::AtKeyword => format!("[at-keyword {:?}]", value()),
            CssTokenKind::Hash(kind) => format!(
                "[hash {:?} {}]",
                value(),
                match kind {
                    CssHashKind::Id => "id",
                    CssHashKind::Unrestricted => "unrestricted",
                }
            ),
            CssTokenKind::String => format!("[string {:?}]", value()),
            CssTokenKind::BadString => "[error \"bad-string\"]".to_owned(),
            CssTokenKind::Url => format!("[url {:?}]", value()),
            CssTokenKind::BadUrl => "[error \"bad-url\"]".to_owned(),
            CssTokenKind::Delim(code_point) => format!("{:?}", code_point.to_string()),
            CssTokenKind::Number(n) => {
                let (text, value, kind) = number(n.span, n.kind);
                format!("[number {text:?} {value} {kind}]")
            }
            CssTokenKind::Percentage(n) => {
                let (text, value, kind) = number(n.span, n.kind);
                format!("[percentage {text:?} {value} {kind}]")
            }
            CssTokenKind::Dimension { number: n, .. } => {
                let (text, value, kind) = number(n.span, n.kind);
                let unit = css_dimension_unit(source, token).unwrap().unwrap();
                format!("[dimension {text:?} {value} {kind} {unit:?}]")
            }
            CssTokenKind::UnicodeRange { start, end } => format!("[unicode-range {start} {end}]"),
            CssTokenKind::Whitespace => "\" \"".to_owned(),
            CssTokenKind::Cdo => "\"<!--\"".to_owned(),
            CssTokenKind::Cdc => "\"-->\"".to_owned(),
            CssTokenKind::Colon => "\":\"".to_owned(),
            CssTokenKind::Semicolon => "\";\"".to_owned(),
            CssTokenKind::Comma => "\",\"".to_owned(),
            CssTokenKind::CloseSquare => "[error \"]\"]".to_owned(),
            CssTokenKind::CloseParen => "[error \")\"]".to_owned(),
            CssTokenKind::CloseCurly => "[error \"}\"]".to_owned(),
            CssTokenKind::OpenSquare => block(
                "[]",
                component_values(source, tokens, index, Some(CssTokenKind::CloseSquare)),
            ),
            CssTokenKind::OpenParen => block(
                "()",
                component_values(source, tokens, index, Some(CssTokenKind::CloseParen)),
            ),
            CssTokenKind::OpenCurly => block(
                "{}",
                component_values(source, tokens, index, Some(CssTokenKind::CloseCurly)),
            ),
            CssTokenKind::Function => {
                let name = value();
                let arguments =
                    component_values(source, tokens, index, Some(CssTokenKind::CloseParen));
                block(&format!("function {name:?}"), arguments)
            }
        });
        if let Some(marker) = marker {
            values.push(format!("[error {marker:?}]"));
        }
    }
    values
}

fn block(head: &str, contents: Vec<String>) -> String {
    let mut out = format!("[{head:?}");
    if head.starts_with("function") {
        out = format!("[{head}");
    }
    for value in contents {
        out.push(' ');
        out.push_str(&value);
    }
    out.push(']');
    out
}

/// The decimal value of a number's text, as the specification computes it
/// for the representation of the suite, rounded to a double.
fn number_value(text: &str) -> String {
    let value: f64 = text.parse().unwrap();
    format!("{value:?}")
}

fn raw_string(document: &JsonDocument<'_>, id: JsonId) -> String {
    match document.value(id).unwrap() {
        JsonValue::String(text) => text.to_owned(),
        JsonValue::Number(number) => number.text().to_owned(),
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Array(_) | JsonValue::Object(_) => {
            panic!("unexpected value in the suite")
        }
    }
}

/// An expected component value of the suite in the same representation.
fn expected(document: &JsonDocument<'_>, id: JsonId) -> String {
    let JsonValue::Array(items) = document.value(id).unwrap() else {
        // The draft reads the Level 2 match and column tokens as two
        // delimiters.
        let text = raw_string(document, id);
        return match text.as_str() {
            "~=" | "|=" | "^=" | "$=" | "*=" | "||" => text
                .chars()
                .map(|code_point| format!("{:?}", code_point.to_string()))
                .collect::<Vec<_>>()
                .join(" "),
            _ => format!("{text:?}"),
        };
    };
    let items: Vec<JsonId> = items.iter().collect();
    let head = raw_string(document, items[0]);
    let text = |index: usize| raw_string(document, items[index]);
    match head.as_str() {
        "{}" | "[]" | "()" | "function" => {
            let mut out = if head == "function" {
                format!("[function {:?}", text(1))
            } else {
                format!("[{head:?}")
            };
            let start = if head == "function" { 2 } else { 1 };
            for item in &items[start..] {
                out.push(' ');
                out.push_str(&expected(document, *item));
            }
            out + "]"
        }
        "number" | "percentage" => format!(
            "[{head} {:?} {} {}]",
            text(1),
            number_value(&text(1)),
            text(3)
        ),
        "dimension" => format!(
            "[dimension {:?} {} {} {:?}]",
            text(1),
            number_value(&text(1)),
            text(3),
            text(4)
        ),
        "hash" => format!("[hash {:?} {}]", text(1), text(2)),
        "unicode-range" => format!("[unicode-range {} {}]", text(1), text(2)),
        "error" => format!("[error {:?}]", text(1)),
        other => format!("[{other} {:?}]", text(1)),
    }
}

/// Compares tokenization with `component_value_list.json` of
/// css-parsing-tests in `FRAMESHIFT_CSS_PARSING_TESTS`. The suite predates
/// the Editor's Draft in two ways, which the comparison applies: it
/// tokenizes unicode ranges everywhere, so the scan allows them, and it
/// keeps the Level 2 attribute-match and column tokens, which the draft
/// reads as two delimiters.
#[test]
#[ignore = "external conformance: set FRAMESHIFT_CSS_PARSING_TESTS to a css-parsing-tests checkout"]
fn external_css_parsing_tests() {
    let directory =
        std::env::var_os("FRAMESHIFT_CSS_PARSING_TESTS").expect("set FRAMESHIFT_CSS_PARSING_TESTS");
    let path = std::path::Path::new(&directory).join("component_value_list.json");
    let text = std::fs::read_to_string(path).unwrap();
    let suite = source(&text);
    let document = parse_json(&suite, JsonDialect::Json, JsonLimits::default()).unwrap();
    let Some(JsonValue::Array(items)) = document.value(document.root()) else {
        panic!("the suite is not an array");
    };
    let items: Vec<JsonId> = items.iter().collect();
    let mut failures = 0;
    let mut draft_differences = 0;
    for pair in items.chunks(2) {
        let input = raw_string(&document, pair[0]);
        let Some(JsonValue::Array(values)) = document.value(pair[1]) else {
            panic!("output is not an array");
        };
        let want: Vec<String> = values
            .iter()
            .map(|value| expected(&document, value))
            .collect();
        let want = want.join(" ");
        let input_source = source(&input);
        let context = CssTokenContext {
            unicode_ranges_allowed: true,
        };
        let mut tokens = Vec::new();
        let mut offset = 0;
        loop {
            let scan = scan_css_token(&input_source, offset, context).unwrap();
            let Some(token) = scan.token else {
                break;
            };
            let marker = scan.errors.iter().find_map(|error| match error.kind {
                CssParseErrorKind::UnterminatedString => Some("eof-in-string"),
                CssParseErrorKind::UnterminatedUrl => Some("eof-in-url"),
                CssParseErrorKind::UnterminatedComment
                | CssParseErrorKind::NewlineInString
                | CssParseErrorKind::InvalidUrlCodePoint
                | CssParseErrorKind::InvalidUrlEscape
                | CssParseErrorKind::InvalidEscape
                | CssParseErrorKind::EscapeAtEof => None,
            });
            offset = token.span.end();
            tokens.push((token, marker));
        }
        let mut index = 0;
        let got = component_values(&input_source, &tokens, &mut index, None).join(" ");
        if got == want {
            continue;
        }
        // The suite counts every code point from U+0080 as an ident code
        // point, as the 2021 Candidate Recommendation did; the draft's
        // non-ASCII ident code points exclude U+0080 and U+0081.
        if input.contains("\u{80}\u{81}") {
            draft_differences += 1;
            continue;
        }
        failures += 1;
        eprintln!("input {input:?}\n  got  {got:?}\n  want {want:?}");
    }
    eprintln!(
        "{} cases, {draft_differences} draft differences, {failures} failures",
        items.len() / 2
    );
    assert_eq!((draft_differences, failures), (1, 0));
}
