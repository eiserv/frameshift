//! Character reference decoding contracts through the public facade, using fictional text.

use frameshift_engine::{
    HtmlCharacterReference, HtmlReferenceContext, HtmlReferenceValue, JsxCharacterReference,
    LexicalErrorKind, Span, decode_html_character_reference, decode_jsx_character_reference,
};

const TEXT: HtmlReferenceContext = HtmlReferenceContext::Text;
const ATTRIBUTE: HtmlReferenceContext = HtmlReferenceContext::Attribute;

fn span(start: usize, end: usize) -> Span {
    Span::new(start, end).unwrap()
}

fn jsx(text: &str) -> Option<(usize, u32)> {
    decode_jsx_character_reference(text, 0)
        .unwrap()
        .map(|reference| (reference.span.end(), reference.code_point))
}

fn html(text: &str, context: HtmlReferenceContext) -> Option<(usize, String, bool)> {
    decode_html_character_reference(text, 0, context)
        .unwrap()
        .map(|reference| {
            let value = match reference.value {
                HtmlReferenceValue::Named(value) => value.to_owned(),
                HtmlReferenceValue::Numeric(ch) => ch.to_string(),
            };
            (reference.span.end(), value, reference.terminated)
        })
}

#[test]
fn jsx_references_follow_the_compilers() {
    for (text, expected) in [
        ("&amp;", Some((5, 0x26))),
        ("&nbsp; x", Some((6, 0xa0))),
        ("&thetasym;", Some((10, 0x3d1))),
        ("&apos;", Some((6, 0x27))),
        ("&lang;", Some((6, 0x2329))),
        ("&#65;", Some((5, 65))),
        ("&#00065;", Some((8, 65))),
        ("&#x41;", Some((6, 0x41))),
        ("&#x1F600;", Some((9, 0x1f600))),
        ("&#0;", Some((4, 0))),
        ("&#xD800;", Some((8, 0xd800))),
        ("&#1114111;", Some((10, 0x10ffff))),
        // Not references: the `&` stays literal text.
        ("&#1114112;", None),
        ("&#99999999999999999999;", None),
        ("&#X41;", None),
        ("&#x;", None),
        ("&#;", None),
        ("&#65", None),
        ("&#1_0;", None),
        ("&amp", None),
        ("&Amp;", None),
        ("&check;", None),
        ("&;", None),
        ("&", None),
        ("a&amp;", None),
    ] {
        assert_eq!(jsx(text), expected, "{text:?}");
    }
    let reference = decode_jsx_character_reference("x&#xDC00;", 1)
        .unwrap()
        .unwrap();
    assert_eq!(
        reference,
        JsxCharacterReference {
            span: span(1, 9),
            code_point: 0xdc00
        }
    );
    assert_eq!(reference.as_char(), None);
    assert_eq!(
        jsx("&euro;").and_then(|(_, code_point)| char::from_u32(code_point)),
        Some('\u{20ac}')
    );
}

#[test]
fn html_named_references_take_the_longest_match() {
    for (text, expected) in [
        ("&amp;", Some((5, "&", true))),
        ("&amp", Some((4, "&", false))),
        ("&AMP;", Some((5, "&", true))),
        ("&notit;", Some((4, "\u{ac}", false))),
        ("&notin;", Some((7, "\u{2209}", true))),
        ("&check;", Some((7, "\u{2713}", true))),
        ("&lang;", Some((6, "\u{27e8}", true))),
        ("&NotEqualTilde;", Some((15, "\u{2242}\u{338}", true))),
        (
            "&CounterClockwiseContourIntegral;",
            Some((33, "\u{2233}", true)),
        ),
        ("&nbspx", Some((5, "\u{a0}", false))),
        ("&ampx", Some((4, "&", false))),
        ("&amp=", Some((4, "&", false))),
        ("&dagger=", None),
        ("&Amp;", None),
        ("&bogus;", None),
        ("&;", None),
        ("& amp;", None),
        ("&\u{e9};", None),
    ] {
        assert_eq!(
            html(text, TEXT),
            expected.map(|(end, value, terminated)| (end, value.to_owned(), terminated)),
            "{text:?}"
        );
    }
    // In attributes, an unterminated match before `=` or an alphanumeric
    // stays literal, but a terminated one or another follower decodes.
    assert_eq!(html("&ampx", ATTRIBUTE), None);
    assert_eq!(html("&amp=", ATTRIBUTE), None);
    assert_eq!(html("&notit;", ATTRIBUTE), None);
    assert_eq!(html("&amp;=", ATTRIBUTE), Some((5, "&".to_owned(), true)));
    assert_eq!(html("&amp ", ATTRIBUTE), Some((4, "&".to_owned(), false)));
    assert_eq!(html("&amp", ATTRIBUTE), Some((4, "&".to_owned(), false)));
}

#[test]
fn html_numeric_references_apply_replacements() {
    for (text, expected) in [
        ("&#65;", Some((5, '\u{41}', true))),
        ("&#65", Some((4, '\u{41}', false))),
        ("&#X41;", Some((6, '\u{41}', true))),
        ("&#x41g", Some((5, '\u{41}', false))),
        ("&#0;", Some((4, '\u{fffd}', true))),
        ("&#xD800;", Some((8, '\u{fffd}', true))),
        ("&#x110000;", Some((10, '\u{fffd}', true))),
        ("&#99999999999999999999;", Some((23, '\u{fffd}', true))),
        ("&#128;", Some((6, '\u{20ac}', true))),
        ("&#x9F;", Some((6, '\u{178}', true))),
        ("&#x81;", Some((6, '\u{81}', true))),
        ("&#13;", Some((5, '\u{d}', true))),
        ("&#xFFFE;", Some((8, '\u{fffe}', true))),
        ("&#;", None),
        ("&#x;", None),
        ("&#", None),
        ("&#a;", None),
    ] {
        assert_eq!(
            html(text, TEXT),
            expected.map(|(end, value, terminated)| (end, value.to_string(), terminated)),
            "{text:?}"
        );
        assert_eq!(html(text, TEXT), html(text, ATTRIBUTE), "{text:?}");
    }
    assert_eq!(
        decode_html_character_reference("a&#65;", 1, TEXT),
        Ok(Some(HtmlCharacterReference {
            span: span(1, 6),
            value: HtmlReferenceValue::Numeric('A'),
            terminated: true,
        }))
    );
}

#[test]
fn offsets_are_validated_and_other_bytes_are_not_references() {
    for offset in [1, 9] {
        let error = decode_jsx_character_reference("\u{e9}&amp;", offset).unwrap_err();
        assert!(matches!(error.kind, LexicalErrorKind::InvalidOffset(_)));
        assert_eq!(error.offset, offset);
        let error = decode_html_character_reference("\u{e9}&amp;", offset, TEXT).unwrap_err();
        assert!(matches!(error.kind, LexicalErrorKind::InvalidOffset(_)));
    }
    assert_eq!(decode_jsx_character_reference("&amp;", 1), Ok(None));
    assert_eq!(decode_html_character_reference("&amp;", 5, TEXT), Ok(None));
    assert_eq!(decode_jsx_character_reference("", 0), Ok(None));
}

// Every JSX reference is an HTML reference with the same span, and agrees
// with it on every value that HTML does not replace.
#[test]
fn generated_strings_decode_consistently() {
    const ALPHABET: [&str; 12] = ["&", "#", "x", "X", "4", "1", "0", ";", "a", "m", "p", "="];
    let mut references = 0usize;
    for length in 0..=6u32 {
        for mut index in 0..ALPHABET.len().pow(length) {
            let mut text = String::new();
            for _ in 0..length {
                text.push_str(ALPHABET[index % ALPHABET.len()]);
                index /= ALPHABET.len();
            }
            for offset in 0..=text.len() {
                let jsx = decode_jsx_character_reference(&text, offset).unwrap();
                for context in [TEXT, ATTRIBUTE] {
                    let html = decode_html_character_reference(&text, offset, context).unwrap();
                    if let Some(reference) = html {
                        assert_eq!(reference.span.start(), offset, "{text:?}");
                        assert!(reference.span.len() >= 3, "{text:?}");
                        references += 1;
                    }
                    let Some(jsx) = jsx else {
                        continue;
                    };
                    let html = html.unwrap_or_else(|| panic!("{text:?} at {offset}"));
                    assert_eq!(jsx.span, html.span, "{text:?}");
                    if let (Some(ch), HtmlReferenceValue::Numeric(value)) =
                        (jsx.as_char(), html.value)
                        && ch != '\0'
                        && !('\u{80}'..='\u{9f}').contains(&ch)
                    {
                        assert_eq!(ch, value, "{text:?}");
                    }
                }
            }
        }
    }
    assert!(references > 10_000, "{references}");
}
