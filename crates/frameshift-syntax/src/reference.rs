//! Character references with pinned tables: as JSX compilers decode them in
//! JSX text and attribute strings, and as HTML decodes them in templates.

use frameshift_core::{Span, TextError};

use crate::{
    entity_tables::{HTML_REFERENCES, XHTML_ENTITIES},
    lexical::{LexicalError, LexicalErrorKind, error, span},
};

/// A character reference that JSX compilers decode in JSX text and strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsxCharacterReference {
    /// Span from `&` through the terminating `;`.
    pub span: Span,
    /// The decoded code point, at most U+10FFFF. It can be zero or a
    /// surrogate, which compilers emit as a lone UTF-16 code unit.
    pub code_point: u32,
}

impl JsxCharacterReference {
    /// Returns the decoded value as a Unicode scalar value, or `None` for a
    /// surrogate code point.
    pub fn as_char(self) -> Option<char> {
        char::from_u32(self.code_point)
    }
}

/// Where an HTML character reference occurs, which decides how a legacy
/// reference without a semicolon is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlReferenceContext {
    /// Text content.
    Text,
    /// An attribute value, where `&amp=` or `&ampx` stays literal text.
    Attribute,
}

/// The decoded value of an HTML character reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlReferenceValue {
    /// One or two characters of a named reference.
    Named(&'static str),
    /// The character of a numeric reference after HTML's replacements.
    Numeric(char),
}

/// A character reference as an HTML parser decodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HtmlCharacterReference {
    /// Span from `&` through the last consumed character.
    pub span: Span,
    /// Decoded value.
    pub value: HtmlReferenceValue,
    /// Whether a `;` ended the reference. HTML decodes some references
    /// without one but reports a missing-semicolon parse error.
    pub terminated: bool,
}

/// Decodes the character reference at `offset` the way the JSX compilers
/// Babel, TypeScript, and SWC decode JSX text and attribute strings.
///
/// A reference is `&#` decimal digits `;`, `&#x` hexadecimal digits `;` (a
/// lowercase `x` only), or `&` name `;` with one of the 253 XHTML 1.0 entity
/// names, compared case-sensitively. Numeric values above U+10FFFF are not
/// references; zero and surrogates are. Anything else, including a missing
/// semicolon, is not a reference and returns `None`, so the `&` is literal
/// text. `offset` must lie on a scalar boundary of `text`; a byte other than
/// `&` returns `None`. Decoding allocates nothing.
pub fn decode_jsx_character_reference(
    text: &str,
    offset: usize,
) -> Result<Option<JsxCharacterReference>, LexicalError> {
    validate(text, offset)?;
    let bytes = text.as_bytes();
    if bytes.get(offset) != Some(&b'&') {
        return Ok(None);
    }
    let start = offset + 1;
    if bytes.get(start) == Some(&b'#') {
        let hex = bytes.get(start + 1) == Some(&b'x');
        let digits_start = start + 1 + usize::from(hex);
        let (value, digits_end) = digits(bytes, digits_start, hex);
        if digits_end == digits_start || value > 0x10ffff || bytes.get(digits_end) != Some(&b';') {
            return Ok(None);
        }
        return Ok(Some(JsxCharacterReference {
            span: span(offset, digits_end + 1)?,
            code_point: value,
        }));
    }
    // Babel looks for the semicolon within ten characters; every XHTML name
    // is shorter, so any longer name fails the lookup as well.
    let Some(length) = bytes[start..]
        .iter()
        .take(10)
        .position(|&byte| byte == b';')
    else {
        return Ok(None);
    };
    let name = &text[start..start + length];
    let Ok(index) = XHTML_ENTITIES.binary_search_by(|(entry, _)| entry.cmp(&name)) else {
        return Ok(None);
    };
    Ok(Some(JsxCharacterReference {
        span: span(offset, start + length + 1)?,
        code_point: u32::from(XHTML_ENTITIES[index].1),
    }))
}

/// Decodes the character reference at `offset` as an HTML parser does in
/// `context`, following the character reference states of the WHATWG HTML
/// Standard's tokenizer.
///
/// A named reference is the longest of the 2,231 WHATWG names that the text
/// continues with after `&`, including legacy names without a semicolon, so
/// `&notit;` decodes `&not` to U+00AC. In an attribute, a match without a
/// semicolon that is followed by `=` or an ASCII alphanumeric stays literal.
/// A numeric reference is `&#` with decimal or `&#x` or `&#X` with
/// hexadecimal digits and an optional `;`; zero, surrogates, and values above
/// U+10FFFF become U+FFFD, and the C1 controls U+0080 to U+009F that
/// Windows-1252 assigns become those characters. Anything else returns
/// `None`, so the `&` is literal text. `offset` must lie on a scalar boundary
/// of `text`; a byte other than `&` returns `None`. Decoding allocates
/// nothing and inspects at most the longest name or the digit run.
pub fn decode_html_character_reference(
    text: &str,
    offset: usize,
    context: HtmlReferenceContext,
) -> Result<Option<HtmlCharacterReference>, LexicalError> {
    validate(text, offset)?;
    let bytes = text.as_bytes();
    if bytes.get(offset) != Some(&b'&') {
        return Ok(None);
    }
    let start = offset + 1;
    match bytes.get(start) {
        Some(b'#') => numeric_html_reference(bytes, offset),
        Some(byte) if byte.is_ascii_alphanumeric() => named_html_reference(text, offset, context),
        Some(_) | None => Ok(None),
    }
}

fn named_html_reference(
    text: &str,
    offset: usize,
    context: HtmlReferenceContext,
) -> Result<Option<HtmlCharacterReference>, LexicalError> {
    let bytes = text.as_bytes();
    let start = offset + 1;
    let alphanumeric = bytes[start..]
        .iter()
        .take(LONGEST_HTML_NAME)
        .take_while(|byte| byte.is_ascii_alphanumeric())
        .count();
    let semicolon = usize::from(bytes.get(start + alphanumeric) == Some(&b';'));
    // A semicolon ends every name that contains one, so only prefixes of the
    // alphanumeric run, and the whole run with its semicolon, can match.
    let longest = (1..=alphanumeric + semicolon).rev().find_map(|length| {
        let name = &text[start..start + length];
        HTML_REFERENCES
            .binary_search_by(|(entry, _)| entry.cmp(&name))
            .ok()
            .map(|index| (length, HTML_REFERENCES[index].1))
    });
    let Some((length, value)) = longest else {
        return Ok(None);
    };
    let end = start + length;
    let terminated = bytes[end - 1] == b';';
    if context == HtmlReferenceContext::Attribute
        && !terminated
        && bytes
            .get(end)
            .is_some_and(|&byte| byte == b'=' || byte.is_ascii_alphanumeric())
    {
        return Ok(None);
    }
    Ok(Some(HtmlCharacterReference {
        span: span(offset, end)?,
        value: HtmlReferenceValue::Named(value),
        terminated,
    }))
}

fn numeric_html_reference(
    bytes: &[u8],
    offset: usize,
) -> Result<Option<HtmlCharacterReference>, LexicalError> {
    let hex = matches!(bytes.get(offset + 2), Some(b'x' | b'X'));
    let digits_start = offset + 2 + usize::from(hex);
    let (value, digits_end) = digits(bytes, digits_start, hex);
    if digits_end == digits_start {
        return Ok(None);
    }
    let terminated = bytes.get(digits_end) == Some(&b';');
    let end = digits_end + usize::from(terminated);
    let replacement = C1_REPLACEMENTS
        .iter()
        .find(|(control, _)| *control == value)
        .map(|&(_, replacement)| replacement);
    let character = match (value, replacement) {
        (_, Some(replacement)) => replacement,
        (0, None) => '\u{fffd}',
        (value, None) => char::from_u32(value).unwrap_or('\u{fffd}'),
    };
    Ok(Some(HtmlCharacterReference {
        span: span(offset, end)?,
        value: HtmlReferenceValue::Numeric(character),
        terminated,
    }))
}

// The length of the longest WHATWG name, `CounterClockwiseContourIntegral;`.
const LONGEST_HTML_NAME: usize = 32;

// Values of numeric references to C1 controls that HTML replaces with the
// Windows-1252 characters at those positions; the other C1 values remain.
const C1_REPLACEMENTS: [(u32, char); 27] = [
    (0x80, '\u{20ac}'),
    (0x82, '\u{201a}'),
    (0x83, '\u{0192}'),
    (0x84, '\u{201e}'),
    (0x85, '\u{2026}'),
    (0x86, '\u{2020}'),
    (0x87, '\u{2021}'),
    (0x88, '\u{02c6}'),
    (0x89, '\u{2030}'),
    (0x8a, '\u{0160}'),
    (0x8b, '\u{2039}'),
    (0x8c, '\u{0152}'),
    (0x8e, '\u{017d}'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201c}'),
    (0x94, '\u{201d}'),
    (0x95, '\u{2022}'),
    (0x96, '\u{2013}'),
    (0x97, '\u{2014}'),
    (0x98, '\u{02dc}'),
    (0x99, '\u{2122}'),
    (0x9a, '\u{0161}'),
    (0x9b, '\u{203a}'),
    (0x9c, '\u{0153}'),
    (0x9e, '\u{017e}'),
    (0x9f, '\u{0178}'),
];

// Reads ASCII digits of the radix and returns their value, capped above
// U+10FFFF so that long digit runs cannot overflow, and the end offset.
fn digits(bytes: &[u8], start: usize, hex: bool) -> (u32, usize) {
    let radix = if hex { 16 } else { 10 };
    let mut value = 0u32;
    let mut end = start;
    while let Some(digit) = bytes
        .get(end)
        .and_then(|&byte| char::from(byte).to_digit(radix))
    {
        value = (value * radix + digit).min(0x11_0000);
        end += 1;
    }
    (value, end)
}

fn validate(text: &str, offset: usize) -> Result<(), LexicalError> {
    let cause = if offset > text.len() {
        TextError::OutOfBounds {
            offset,
            len: text.len(),
        }
    } else if !text.is_char_boundary(offset) {
        TextError::NotCharBoundary { offset }
    } else {
        return Ok(());
    };
    Err(error(offset, LexicalErrorKind::InvalidOffset(cause)))
}

// The generated tables are sorted and complete; lookups depend on it.
const _: () = {
    assert!(HTML_REFERENCES.len() == 2231);
    let mut index = 1;
    while index < HTML_REFERENCES.len() {
        let previous = HTML_REFERENCES[index - 1].0.as_bytes();
        assert!(less(previous, HTML_REFERENCES[index].0.as_bytes()));
        index += 1;
    }
    assert!(XHTML_ENTITIES.len() == 253);
    let mut index = 1;
    while index < XHTML_ENTITIES.len() {
        let previous = XHTML_ENTITIES[index - 1].0.as_bytes();
        assert!(less(previous, XHTML_ENTITIES[index].0.as_bytes()));
        index += 1;
    }
};

const fn less(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0;
    while index < left.len() && index < right.len() {
        if left[index] != right[index] {
            return left[index] < right[index];
        }
        index += 1;
    }
    left.len() < right.len()
}
