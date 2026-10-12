//! Parser-directed JSX lexical scanners: element and attribute names,
//! attribute strings, and text between tags.

use frameshift_core::{SourceText, Span};

use crate::{
    lexical::{LexicalError, LexicalErrorKind, LexicalLimits, error, span, validate},
    unicode::{is_js_identifier_part, is_js_identifier_start},
};

/// One JSX attribute string accepted by [`scan_jsx_string`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsxStringToken {
    /// Exact source span, including both quotes.
    pub span: Span,
    /// The original spelling between the quotes, possibly empty.
    pub content_span: Span,
}

/// Scans one `JSXIdentifier`: an `IdentifierStartChar` followed by
/// `IdentifierPartChar`s and hyphens, such as `div`, `data-id`, or `aria-`.
///
/// The caller selects a JSX tag context. Unicode escapes are not part of the
/// JSX name grammar: a backslash at the start of, inside, or directly after
/// the name returns `InvalidIdentifierEscape` at the backslash. Reserved words
/// are ordinary JSX names. `max_identifier_bytes` bounds the name. Scanning is
/// allocation-free and linear in consumed bytes.
pub fn scan_jsx_identifier(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<Span, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let bytes = text.as_bytes();
    let mut cursor = offset;
    while let Some(&byte) = bytes.get(cursor) {
        let (permitted, width) = if byte.is_ascii() {
            let permitted = if cursor == offset {
                byte.is_ascii_alphabetic() || matches!(byte, b'$' | b'_')
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'_' | b'-')
            };
            (permitted, 1)
        } else {
            match text[cursor..].chars().next() {
                Some(ch) if cursor == offset => (is_js_identifier_start(ch), ch.len_utf8()),
                Some(ch) => (is_js_identifier_part(ch), ch.len_utf8()),
                None => (false, 0),
            }
        };
        if !permitted {
            if byte == b'\\' {
                return Err(error(cursor, LexicalErrorKind::InvalidIdentifierEscape));
            }
            break;
        }
        if cursor - offset + width > limits.max_identifier_bytes {
            return Err(error(cursor, LexicalErrorKind::IdentifierLimit));
        }
        cursor += width;
    }
    if cursor == offset {
        return Err(error(offset, LexicalErrorKind::ExpectedIdentifier));
    }
    span(offset, cursor)
}

/// Scans one single- or double-quoted JSX attribute string.
///
/// JSX strings have no escapes: a backslash is an ordinary character, and the
/// string ends at the first matching quote. Line terminators are permitted.
/// HTML character references such as `&amp;` are part of the original
/// spelling; the scanner neither decodes nor validates them. Missing quotes
/// return `ExpectedQuote`, EOF returns `UnterminatedString` at the opening
/// quote, and `max_string_bytes`, including both quotes, bounds the token.
/// Scanning is allocation-free and linear in consumed bytes.
pub fn scan_jsx_string(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<JsxStringToken, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let bytes = text.as_bytes();
    let Some(&quote @ (b'\'' | b'"')) = bytes.get(offset) else {
        return Err(error(offset, LexicalErrorKind::ExpectedQuote));
    };
    // Quotes are ASCII, so a byte search cannot stop inside a UTF-8 sequence.
    let close = bytes[offset + 1..]
        .iter()
        .position(|&byte| byte == quote)
        .map(|index| offset + 1 + index);
    let end = close.map_or(text.len(), |close| close + 1);
    if end - offset > limits.max_string_bytes {
        return Err(error(
            text.floor_char_boundary(offset + limits.max_string_bytes),
            LexicalErrorKind::StringLimit,
        ));
    }
    let Some(close) = close else {
        return Err(error(offset, LexicalErrorKind::UnterminatedString));
    };
    Ok(JsxStringToken {
        span: span(offset, end)?,
        content_span: span(offset + 1, close)?,
    })
}

/// Scans JSX text between tags and expression containers.
///
/// The text extends from `offset` to the next `{` or `<`, or to EOF, which
/// the caller reports as an unclosed element. It may be empty. JSX text cannot
/// contain `>` or `}`: the first one returns `InvalidJsxTextCharacter` at its
/// offset. Comments, whitespace, line terminators, and character references
/// are ordinary text; nothing is decoded, trimmed, or normalized.
/// `max_jsx_text_bytes` bounds the text: a limit error reports the first
/// scalar that would exceed it, which takes precedence over a later invalid
/// character. Scanning is allocation-free and linear in consumed bytes.
pub fn scan_jsx_text(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<Span, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    // The delimiters are ASCII, so a byte scan cannot stop inside a scalar.
    let end = text.as_bytes()[offset..]
        .iter()
        .position(|byte| matches!(byte, b'{' | b'<' | b'>' | b'}'))
        .map_or(text.len(), |index| offset + index);
    if end - offset > limits.max_jsx_text_bytes {
        return Err(error(
            text.floor_char_boundary(offset + limits.max_jsx_text_bytes),
            LexicalErrorKind::JsxTextLimit,
        ));
    }
    if matches!(text.as_bytes().get(end), Some(b'>' | b'}')) {
        return Err(error(end, LexicalErrorKind::InvalidJsxTextCharacter));
    }
    span(offset, end)
}
