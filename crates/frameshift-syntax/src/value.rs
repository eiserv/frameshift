//! StringValue comparisons for tokens that a scanner has already accepted.
//!
//! Early errors compare names after escape decoding, so `eval` is `eval`.
//! These helpers decode lazily and allocate nothing. Their inputs must be
//! spellings returned by a successful scan; they do not validate.

use std::str::Chars;

/// Iterates the StringValue code points of an `IdentifierName` spelling
/// accepted by `scan_js_identifier`, decoding `\u` escapes.
pub(crate) fn identifier_chars(spelling: &str) -> IdentifierChars<'_> {
    IdentifierChars {
        chars: spelling.chars(),
    }
}

/// Lazy decoder returned by [`identifier_chars`].
#[derive(Clone)]
pub(crate) struct IdentifierChars<'a> {
    chars: Chars<'a>,
}

impl Iterator for IdentifierChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        if ch != '\\' {
            return Some(ch);
        }
        // The scanner accepted `\u` with four digits or a braced code point
        // that is a permitted identifier character, never a surrogate.
        self.chars.next();
        let mut value = 0u32;
        if self.chars.clone().next() == Some('{') {
            self.chars.next();
            for digit in self.chars.by_ref() {
                let Some(digit) = digit.to_digit(16) else {
                    break;
                };
                value = (value * 16 + digit).min(0x11_0000);
            }
        } else {
            for _ in 0..4 {
                value = value * 16 + self.chars.next().and_then(|d| d.to_digit(16)).unwrap_or(0);
            }
        }
        Some(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER))
    }
}

/// Reports whether an accepted identifier spelling has the StringValue `target`.
pub(crate) fn identifier_equals(spelling: &str, target: &str) -> bool {
    identifier_chars(spelling).eq(target.chars())
}

/// Reports whether a string literal accepted by `scan_js_string`, including its
/// quotes, has the StringValue `target`. `target` must be ASCII, because
/// escapes are compared as UTF-16 code units, which equal code points only there.
pub(crate) fn string_literal_equals(literal: &str, target: &str) -> bool {
    debug_assert!(target.is_ascii());
    let content = literal
        .get(1..literal.len().saturating_sub(1))
        .unwrap_or_default();
    let mut chars = content.chars();
    let mut expected = target.bytes();
    while let Some(ch) = chars.next() {
        let unit = if ch == '\\' {
            match escape_unit(&mut chars) {
                Some(unit) => unit,
                // Line continuations contribute no code units.
                None => continue,
            }
        } else {
            u32::from(ch)
        };
        if expected.next().map(u32::from) != Some(unit) {
            return false;
        }
    }
    expected.next().is_none()
}

// Returns the code unit or code point of one escape after its backslash, or
// `None` for a line continuation. Astral code points never match ASCII.
fn escape_unit(chars: &mut Chars<'_>) -> Option<u32> {
    let ch = chars.next()?;
    let hex = |chars: &mut Chars<'_>, count: usize| {
        (0..count).fold(0u32, |value, _| {
            value * 16 + chars.next().and_then(|d| d.to_digit(16)).unwrap_or(0)
        })
    };
    Some(match ch {
        'b' => 0x08,
        'f' => 0x0c,
        'n' => 0x0a,
        'r' => 0x0d,
        't' => 0x09,
        'v' => 0x0b,
        '0' => 0,
        'x' => hex(chars, 2),
        'u' if chars.clone().next() == Some('{') => {
            chars.next();
            let mut value = 0u32;
            for digit in chars.by_ref() {
                let Some(digit) = digit.to_digit(16) else {
                    break;
                };
                value = (value * 16 + digit).min(0x11_0000);
            }
            value
        }
        'u' => hex(chars, 4),
        '\r' => {
            if chars.clone().next() == Some('\n') {
                chars.next();
            }
            return None;
        }
        '\n' | '\u{2028}' | '\u{2029}' => return None,
        other => u32::from(other),
    })
}
