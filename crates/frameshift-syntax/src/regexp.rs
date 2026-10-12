use frameshift_core::{SourceText, Span};

use crate::lexical::{
    LexicalError, LexicalErrorKind, LexicalLimits, error, is_line_terminator, span, validate,
};
use crate::regexp_pattern::validate_pattern;
use crate::unicode::is_js_identifier_part;

macro_rules! define_flags {
    ($($flag:ident => $letter:literal, $doc:literal;)+) => {
        /// A regular-expression flag permitted by ECMAScript section 13.2.7.2.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum RegExpFlag {
            $(#[doc = $doc]
            $flag,)+
        }

        impl RegExpFlag {
            /// Every flag, in the order of its bit in [`RegExpFlags`].
            pub const ALL: &[Self] = &[$(Self::$flag,)+];

            /// Returns the flag's source letter.
            pub const fn letter(self) -> char {
                match self {
                    $(Self::$flag => $letter,)+
                }
            }

            fn from_letter(letter: char) -> Option<Self> {
                match letter {
                    $($letter => Some(Self::$flag),)+
                    _ => None,
                }
            }
        }
    };
}

define_flags! {
    HasIndices => 'd', "`d`: match indices are reported.";
    Global => 'g', "`g`: matching continues from the last index.";
    IgnoreCase => 'i', "`i`: matching is case-insensitive.";
    Multiline => 'm', "`m`: `^` and `$` also match at line terminators.";
    DotAll => 's', "`s`: `.` also matches line terminators.";
    Unicode => 'u', "`u`: the pattern uses the Unicode-mode grammar.";
    UnicodeSets => 'v', "`v`: the pattern uses the UnicodeSets-mode grammar.";
    Sticky => 'y', "`y`: matching is anchored at the last index.";
}

/// A set of distinct, mutually compatible regular-expression flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegExpFlags {
    bits: u8,
}

impl RegExpFlags {
    /// Reports whether the set contains `flag`.
    pub const fn contains(self, flag: RegExpFlag) -> bool {
        self.bits & Self::bit(flag) != 0
    }

    /// Reports whether the pattern grammar has the `[UnicodeMode]`
    /// parameter, which is the case with either `u` or `v`.
    pub const fn is_unicode_mode(self) -> bool {
        self.contains(RegExpFlag::Unicode) || self.contains(RegExpFlag::UnicodeSets)
    }

    const fn bit(flag: RegExpFlag) -> u8 {
        1 << flag as u8
    }
}

/// The lexical extent of one regular-expression literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegExpToken {
    /// Exact source span, from the opening slash through the last flag.
    pub span: Span,
    /// The `BodyText`: the pattern between the slashes, as spelled.
    pub body_span: Span,
    /// The `FlagText` after the closing slash, possibly empty and not yet validated.
    pub flags_span: Span,
}

/// A regular-expression literal that satisfies `IsValidRegularExpressionLiteral`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegExpLiteral {
    /// The literal's lexical extent.
    pub token: RegExpToken,
    /// The validated flags.
    pub flags: RegExpFlags,
    /// The number of capturing groups in the pattern.
    pub capturing_groups: u32,
}

/// Scans the lexical extent of one ECMAScript regular-expression literal
/// (section 12.9.5) at a slash that the parser has identified in a
/// regular-expression lexical goal. Inside a class, `/` does not end the
/// body; a backslash escapes any non-terminator. Flags are the following
/// `IdentifierPartChar`s; a backslash directly after them is an error, because
/// flags cannot contain escapes and no token may follow the literal there.
/// Neither flags nor pattern are validated; use [`validate_js_regexp`] for
/// that. A token without validation is evidence of extent, not of validity.
/// Offsets are validated first, then the opening slash, then the byte limit
/// before each consumed scalar. Iterative, allocation-free, and linear.
pub fn scan_js_regexp(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<RegExpToken, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let rest = &text[offset..];
    // `//` and `/*` begin comments in every lexical goal.
    if !rest.starts_with('/') || rest[1..].starts_with(['/', '*']) {
        return Err(error(offset, LexicalErrorKind::ExpectedRegExp));
    }
    let mut scan = Scan {
        text,
        start: offset,
        cursor: offset,
        max_bytes: limits.max_regexp_bytes,
    };
    scan.advance('/')?;
    let body_start = scan.cursor;
    let mut in_class = false;
    loop {
        let ch = scan.body_char()?;
        match ch {
            '/' if !in_class => break,
            '[' => in_class = true,
            ']' => in_class = false,
            '\\' => {
                scan.advance(ch)?;
                let escaped = scan.body_char()?;
                scan.advance(escaped)?;
                continue;
            }
            _ => {}
        }
        scan.advance(ch)?;
    }
    let body_end = scan.cursor;
    scan.advance('/')?;
    let flags_start = scan.cursor;
    while let Some(ch) = scan.peek().filter(|&ch| is_js_identifier_part(ch)) {
        scan.advance(ch)?;
    }
    if scan.peek() == Some('\\') {
        return Err(error(scan.cursor, LexicalErrorKind::InvalidRegExpFlag));
    }
    Ok(RegExpToken {
        span: span(offset, scan.cursor)?,
        body_span: span(body_start, body_end)?,
        flags_span: span(flags_start, scan.cursor)?,
    })
}

/// Scans a regular-expression literal like [`scan_js_regexp`] and checks it
/// with `IsValidRegularExpressionLiteral` (section 13.2.7.2): only the flags
/// `dgimsuvy`, each at most once and not `u` with `v`, and a pattern that
/// parses under the grammar of section 22.2.1 with all early errors. With `u`
/// or `v`, the Unicode-mode grammar applies; otherwise the web-compatible
/// grammar of Annex B.1.2, reparsed with named groups if the pattern has one
/// (`D-037`). Property escapes use the pinned Unicode tables. Errors point to
/// the offending flag or pattern position; lexical errors take precedence.
/// Patterns are never compiled or executed, so validation cannot backtrack
/// catastrophically: time is linear in literal bytes, apart from logarithmic
/// lookups of group names and properties. Storage is proportional to the
/// pattern and reserved fallibly.
pub fn validate_js_regexp(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<RegExpLiteral, LexicalError> {
    let token = scan_js_regexp(source, offset, limits)?;
    let text = source.as_str();
    let flags = validate_flags(text, token.flags_span)?;
    let capturing_groups = validate_pattern(text, token.body_span, flags)?;
    Ok(RegExpLiteral {
        token,
        flags,
        capturing_groups,
    })
}

fn validate_flags(text: &str, flags_span: Span) -> Result<RegExpFlags, LexicalError> {
    let mut flags = RegExpFlags::default();
    for (index, letter) in text[flags_span.start()..flags_span.end()].char_indices() {
        let offset = flags_span.start() + index;
        let flag = RegExpFlag::from_letter(letter)
            .ok_or_else(|| error(offset, LexicalErrorKind::InvalidRegExpFlag))?;
        if flags.contains(flag) {
            return Err(error(offset, LexicalErrorKind::DuplicateRegExpFlag));
        }
        flags.bits |= RegExpFlags::bit(flag);
        if flags.contains(RegExpFlag::Unicode) && flags.contains(RegExpFlag::UnicodeSets) {
            return Err(error(offset, LexicalErrorKind::IncompatibleRegExpFlags));
        }
    }
    Ok(flags)
}

struct Scan<'a> {
    text: &'a str,
    start: usize,
    cursor: usize,
    max_bytes: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.cursor..].chars().next()
    }

    /// Peeks a body character, which must exist and must not end the line.
    fn body_char(&self) -> Result<char, LexicalError> {
        match self.peek() {
            None => Err(error(self.start, LexicalErrorKind::UnterminatedRegExp)),
            Some(ch) if is_line_terminator(ch) => {
                Err(error(self.cursor, LexicalErrorKind::RegExpLineTerminator))
            }
            Some(ch) => Ok(ch),
        }
    }

    /// Consumes `ch`, which the caller has just peeked.
    fn advance(&mut self, ch: char) -> Result<(), LexicalError> {
        if self.cursor - self.start + ch.len_utf8() > self.max_bytes {
            return Err(error(self.cursor, LexicalErrorKind::RegExpLimit));
        }
        self.cursor += ch.len_utf8();
        Ok(())
    }
}
