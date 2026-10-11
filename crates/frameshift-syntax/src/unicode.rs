use crate::unicode_tables::{ID_CONTINUE, ID_START};

/// Reports whether `ch` is an ECMAScript `IdentifierStartChar`: a code point
/// with the Unicode `ID_Start` property, `$`, or `_`. Uses the tables pinned to
/// [`UNICODE_VERSION`](crate::UNICODE_VERSION), never Rust's own Unicode data.
/// Constant time for ASCII, otherwise one binary search over static ranges.
pub fn is_js_identifier_start(ch: char) -> bool {
    if ch.is_ascii() {
        return ch.is_ascii_alphabetic() || ch == '$' || ch == '_';
    }
    contains(ID_START, ch)
}

/// Reports whether `ch` is an ECMAScript `IdentifierPartChar`: a code point
/// with the Unicode `ID_Continue` property, or `$`. Since Unicode 15.1,
/// `ID_Continue` includes U+200C ZWNJ and U+200D ZWJ, so they need no special
/// case. Uses the same pinned tables as [`is_js_identifier_start`].
pub fn is_js_identifier_part(ch: char) -> bool {
    if ch.is_ascii() {
        return ch.is_ascii_alphanumeric() || ch == '$' || ch == '_';
    }
    contains(ID_CONTINUE, ch)
}

fn contains(table: &[(u32, u32)], ch: char) -> bool {
    let value = u32::from(ch);
    let index = table.partition_point(|&(_, end)| end < value);
    table.get(index).is_some_and(|&(start, _)| start <= value)
}
