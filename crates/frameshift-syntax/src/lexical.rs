use std::fmt;

use frameshift_core::{SourceText, Span, TextError};

use crate::regexp_pattern::RegExpPatternError;

/// Explicit resource bounds for individual lexical scans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexicalLimits {
    /// Maximum comments retained by one trivia scan, default 100,000.
    pub max_comments: usize,
    /// Maximum bytes in one string, including quotes, default 1 MiB.
    pub max_string_bytes: usize,
    /// Maximum bytes in one numeric literal, including separators and suffix, default 1 MiB.
    pub max_number_bytes: usize,
    /// Maximum bytes in one template segment, including delimiters, default 1 MiB.
    pub max_template_bytes: usize,
    /// Maximum bytes in one identifier, including `#` and escapes, default 1 MiB.
    pub max_identifier_bytes: usize,
    /// Maximum bytes in one regular-expression literal, including both
    /// slashes and the flags, default 1 MiB.
    pub max_regexp_bytes: usize,
}

impl Default for LexicalLimits {
    fn default() -> Self {
        Self {
            max_comments: 100_000,
            max_string_bytes: 1024 * 1024,
            max_number_bytes: 1024 * 1024,
            max_template_bytes: 1024 * 1024,
            max_identifier_bytes: 1024 * 1024,
            max_regexp_bytes: 1024 * 1024,
        }
    }
}

/// A JavaScript comment delimiter form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommentKind {
    /// A `//` comment, excluding its terminating line break.
    Line,
    /// A `/* */` comment, including both delimiters.
    Block,
}

/// An original-source comment, without copied content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Comment delimiter form.
    pub kind: CommentKind,
    /// Half-open source span, including delimiters.
    pub span: Span,
}

/// Consecutive whitespace and comments at a parser-selected offset.
#[derive(Debug, PartialEq, Eq)]
pub struct Trivia {
    /// Consumed source range, empty if no trivia was found.
    pub span: Span,
    /// Whether the range contains an ECMAScript line terminator.
    pub has_line_terminator: bool,
    /// Comments in source order, including those containing line terminators.
    pub comments: Vec<Comment>,
}

/// Reason a lexical scan cannot return a trustworthy result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LexicalErrorKind {
    /// The starting offset is outside the text or divides a UTF-8 scalar.
    InvalidOffset(TextError),
    /// A string scan did not start at a single or double quote.
    ExpectedQuote,
    /// A quoted string reached EOF before its closing quote.
    UnterminatedString,
    /// An unescaped CR or LF occurred inside a string.
    UnescapedLineBreak,
    /// A hexadecimal or Unicode escape is incomplete or out of range.
    InvalidEscape,
    /// A legacy decimal escape is outside the supported strict-string subset.
    LegacyDecimalEscape,
    /// A block comment reached EOF before `*/`.
    UnterminatedComment,
    /// The trivia scan would retain more than the allowed number of comments.
    CommentLimit,
    /// The string scan would consume more than the allowed number of bytes.
    StringLimit,
    /// A numeric scan did not start at a digit or a dot followed by a digit.
    ExpectedNumber,
    /// A radix prefix or exponent was not followed by a required digit.
    MissingNumericDigit,
    /// A numeric separator was not between two digits of the same part and radix.
    InvalidNumericSeparator,
    /// An identifier start, backslash, extra decimal digit, or invalid BigInt
    /// suffix follows a number.
    InvalidNumericContinuation,
    /// A leading-zero integer is outside the supported strict-mode subset.
    LegacyLeadingZero,
    /// The numeric scan would consume more than the allowed number of bytes.
    NumberLimit,
    /// A template scan did not start at the delimiter selected by its context.
    ExpectedTemplateDelimiter,
    /// A template segment reached EOF before a backtick or `${` terminator.
    UnterminatedTemplate,
    /// An untagged template contains an escape with an undefined cooked value.
    InvalidTemplateEscape,
    /// The template scan would consume more than the allowed number of bytes.
    TemplateLimit,
    /// An identifier scan did not start at an identifier start or a private name.
    ExpectedIdentifier,
    /// A backslash in or after a name is not a `\u` escape of a code point
    /// permitted at that position.
    InvalidIdentifierEscape,
    /// The identifier scan would consume more than the allowed number of bytes.
    IdentifierLimit,
    /// A regular-expression scan did not start at a slash, or the slash
    /// begins a comment.
    ExpectedRegExp,
    /// A regular-expression literal reached EOF before its closing slash.
    UnterminatedRegExp,
    /// A line terminator occurred inside a regular-expression literal.
    RegExpLineTerminator,
    /// A flag is not one of `dgimsuvy`, or a backslash follows the flags.
    InvalidRegExpFlag,
    /// A flag occurs more than once.
    DuplicateRegExpFlag,
    /// The flags contain both `u` and `v`.
    IncompatibleRegExpFlags,
    /// The pattern violates the grammar selected by the flags.
    InvalidRegExpPattern(RegExpPatternError),
    /// The regular-expression scan would consume more than the allowed number of bytes.
    RegExpLimit,
    /// Scanner storage could not be reserved.
    AllocationFailed,
    /// A scanner produced an invalid range, indicating an internal defect.
    InternalInvariant(TextError),
}

/// A lexical error with a canonical byte offset and no source content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexicalError {
    /// Original-source byte offset associated with the failure.
    pub offset: usize,
    /// Typed failure reason.
    pub kind: LexicalErrorKind,
}

impl fmt::Display for LexicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match &self.kind {
            LexicalErrorKind::InvalidOffset(error) => {
                return write!(f, "invalid lexical offset {}: {error}", self.offset);
            }
            LexicalErrorKind::ExpectedQuote => "expected a string quote",
            LexicalErrorKind::UnterminatedString => "unterminated string",
            LexicalErrorKind::UnescapedLineBreak => "unescaped string line break",
            LexicalErrorKind::InvalidEscape => "invalid string escape",
            LexicalErrorKind::LegacyDecimalEscape => "legacy decimal escape is unsupported",
            LexicalErrorKind::UnterminatedComment => "unterminated block comment",
            LexicalErrorKind::CommentLimit => "comment count exceeds lexical limit",
            LexicalErrorKind::StringLimit => "string size exceeds lexical limit",
            LexicalErrorKind::ExpectedNumber => "expected a numeric literal",
            LexicalErrorKind::MissingNumericDigit => "expected a numeric digit",
            LexicalErrorKind::InvalidNumericSeparator => "invalid numeric separator",
            LexicalErrorKind::InvalidNumericContinuation => "invalid numeric continuation",
            LexicalErrorKind::LegacyLeadingZero => "legacy leading-zero number is unsupported",
            LexicalErrorKind::NumberLimit => "number size exceeds lexical limit",
            LexicalErrorKind::ExpectedTemplateDelimiter => "expected a template delimiter",
            LexicalErrorKind::UnterminatedTemplate => "unterminated template segment",
            LexicalErrorKind::InvalidTemplateEscape => "invalid untagged template escape",
            LexicalErrorKind::TemplateLimit => "template segment size exceeds lexical limit",
            LexicalErrorKind::ExpectedIdentifier => "expected an identifier",
            LexicalErrorKind::InvalidIdentifierEscape => "invalid identifier escape",
            LexicalErrorKind::IdentifierLimit => "identifier size exceeds lexical limit",
            LexicalErrorKind::ExpectedRegExp => "expected a regular expression",
            LexicalErrorKind::UnterminatedRegExp => "unterminated regular expression",
            LexicalErrorKind::RegExpLineTerminator => "line terminator in regular expression",
            LexicalErrorKind::InvalidRegExpFlag => "invalid regular-expression flag",
            LexicalErrorKind::DuplicateRegExpFlag => "duplicate regular-expression flag",
            LexicalErrorKind::IncompatibleRegExpFlags => {
                "regular-expression flags u and v are incompatible"
            }
            LexicalErrorKind::InvalidRegExpPattern(pattern) => {
                return write!(
                    f,
                    "invalid regular expression: {} at byte offset {}",
                    pattern.message(),
                    self.offset
                );
            }
            LexicalErrorKind::RegExpLimit => "regular expression size exceeds lexical limit",
            LexicalErrorKind::AllocationFailed => "could not allocate lexical storage",
            LexicalErrorKind::InternalInvariant(error) => {
                return write!(f, "internal lexical error at {}: {error}", self.offset);
            }
        };
        write!(f, "{message} at byte offset {}", self.offset)
    }
}

impl std::error::Error for LexicalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            LexicalErrorKind::InvalidOffset(error) | LexicalErrorKind::InternalInvariant(error) => {
                Some(error)
            }
            LexicalErrorKind::ExpectedQuote
            | LexicalErrorKind::UnterminatedString
            | LexicalErrorKind::UnescapedLineBreak
            | LexicalErrorKind::InvalidEscape
            | LexicalErrorKind::LegacyDecimalEscape
            | LexicalErrorKind::UnterminatedComment
            | LexicalErrorKind::CommentLimit
            | LexicalErrorKind::StringLimit
            | LexicalErrorKind::ExpectedNumber
            | LexicalErrorKind::MissingNumericDigit
            | LexicalErrorKind::InvalidNumericSeparator
            | LexicalErrorKind::InvalidNumericContinuation
            | LexicalErrorKind::LegacyLeadingZero
            | LexicalErrorKind::NumberLimit
            | LexicalErrorKind::ExpectedTemplateDelimiter
            | LexicalErrorKind::UnterminatedTemplate
            | LexicalErrorKind::InvalidTemplateEscape
            | LexicalErrorKind::TemplateLimit
            | LexicalErrorKind::ExpectedIdentifier
            | LexicalErrorKind::InvalidIdentifierEscape
            | LexicalErrorKind::IdentifierLimit
            | LexicalErrorKind::ExpectedRegExp
            | LexicalErrorKind::UnterminatedRegExp
            | LexicalErrorKind::RegExpLineTerminator
            | LexicalErrorKind::InvalidRegExpFlag
            | LexicalErrorKind::DuplicateRegExpFlag
            | LexicalErrorKind::IncompatibleRegExpFlags
            | LexicalErrorKind::InvalidRegExpPattern(_)
            | LexicalErrorKind::RegExpLimit
            | LexicalErrorKind::AllocationFailed => None,
        }
    }
}

/// Scans ECMAScript whitespace, line terminators, and `//` or `/* */` comments.
/// The caller must select a JavaScript lexical context, never JSX text or a
/// literal body. Hashbang and legacy HTML comments are not consumed. A slash
/// that does not begin a comment is left for the parser's lexical goal.
/// Time is linear in consumed bytes; storage is bounded by `max_comments`.
pub fn scan_js_trivia(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<Trivia, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let mut cursor = offset;
    let mut comments = Vec::new();
    let mut has_line_terminator = false;
    while let Some(ch) = text[cursor..].chars().next() {
        if is_whitespace(ch) || is_line_terminator(ch) {
            has_line_terminator |= is_line_terminator(ch);
            cursor += ch.len_utf8();
            continue;
        }
        let rest = &text[cursor..];
        let kind = if rest.starts_with("//") {
            CommentKind::Line
        } else if rest.starts_with("/*") {
            CommentKind::Block
        } else {
            break;
        };
        if comments.len() == limits.max_comments {
            return Err(error(cursor, LexicalErrorKind::CommentLimit));
        }
        let start = cursor;
        cursor += 2;
        match kind {
            CommentKind::Line => {
                while let Some(ch) = text[cursor..].chars().next() {
                    if is_line_terminator(ch) {
                        break;
                    }
                    cursor += ch.len_utf8();
                }
            }
            CommentKind::Block => loop {
                if text[cursor..].starts_with("*/") {
                    cursor += 2;
                    break;
                }
                let Some(ch) = text[cursor..].chars().next() else {
                    return Err(error(start, LexicalErrorKind::UnterminatedComment));
                };
                has_line_terminator |= is_line_terminator(ch);
                cursor += ch.len_utf8();
            },
        }
        comments
            .try_reserve(1)
            .map_err(|_| error(start, LexicalErrorKind::AllocationFailed))?;
        comments.push(Comment {
            kind,
            span: span(start, cursor)?,
        });
    }
    Ok(Trivia {
        span: span(offset, cursor)?,
        has_line_terminator,
        comments,
    })
}

/// Scans one single- or double-quoted ECMAScript strict-mode string literal.
/// Returns its original span, without decoding or normalizing its value.
/// Supports character, hexadecimal, Unicode, and line-continuation escapes.
/// Legacy decimal escapes are explicitly unsupported. Raw U+2028 and U+2029
/// are legal string characters. This is not a JSX attribute or JSON scanner.
/// Scanning is iterative, allocation-free, and linear in consumed bytes.
pub fn scan_js_string(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<Span, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let Some(quote @ ('\'' | '"')) = text[offset..].chars().next() else {
        return Err(error(offset, LexicalErrorKind::ExpectedQuote));
    };
    let mut cursor = offset;
    advance(text, &mut cursor, offset, limits)?;
    while let Some(ch) = text[cursor..].chars().next() {
        if ch == '\r' || ch == '\n' {
            return Err(error(cursor, LexicalErrorKind::UnescapedLineBreak));
        }
        let start = cursor;
        advance(text, &mut cursor, offset, limits)?;
        if ch == quote {
            return span(offset, cursor);
        }
        if ch == '\\' {
            scan_escape(text, &mut cursor, start, offset, limits)?;
        }
    }
    Err(error(offset, LexicalErrorKind::UnterminatedString))
}

fn scan_escape(
    text: &str,
    cursor: &mut usize,
    escape: usize,
    start: usize,
    limits: LexicalLimits,
) -> Result<(), LexicalError> {
    let Some(ch) = text[*cursor..].chars().next() else {
        return Err(error(escape, LexicalErrorKind::InvalidEscape));
    };
    advance(text, cursor, start, limits)?;
    match ch {
        'x' => hex_digits(text, cursor, 2, escape, start, limits),
        'u' if text[*cursor..].starts_with('{') => {
            advance(text, cursor, start, limits)?;
            let mut value = 0u32;
            let mut has_digit = false;
            while let Some(ch) = text[*cursor..].chars().next() {
                if ch == '}' && has_digit {
                    return advance(text, cursor, start, limits);
                }
                let digit = ch
                    .to_digit(16)
                    .ok_or_else(|| error(escape, LexicalErrorKind::InvalidEscape))?;
                value = value * 16 + digit;
                if value > 0x10ffff {
                    return Err(error(escape, LexicalErrorKind::InvalidEscape));
                }
                has_digit = true;
                advance(text, cursor, start, limits)?;
            }
            Err(error(escape, LexicalErrorKind::InvalidEscape))
        }
        'u' => hex_digits(text, cursor, 4, escape, start, limits),
        '0' if !text.as_bytes().get(*cursor).is_some_and(u8::is_ascii_digit) => Ok(()),
        '0'..='9' => Err(error(escape, LexicalErrorKind::LegacyDecimalEscape)),
        '\r' if text[*cursor..].starts_with('\n') => advance(text, cursor, start, limits),
        _ => Ok(()),
    }
}

fn hex_digits(
    text: &str,
    cursor: &mut usize,
    count: usize,
    escape: usize,
    start: usize,
    limits: LexicalLimits,
) -> Result<(), LexicalError> {
    for _ in 0..count {
        if !text
            .as_bytes()
            .get(*cursor)
            .is_some_and(u8::is_ascii_hexdigit)
        {
            return Err(error(escape, LexicalErrorKind::InvalidEscape));
        }
        advance(text, cursor, start, limits)?;
    }
    Ok(())
}

fn advance(
    text: &str,
    cursor: &mut usize,
    start: usize,
    limits: LexicalLimits,
) -> Result<(), LexicalError> {
    let Some(ch) = text[*cursor..].chars().next() else {
        return Err(error(*cursor, LexicalErrorKind::UnterminatedString));
    };
    if *cursor - start + ch.len_utf8() > limits.max_string_bytes {
        return Err(error(*cursor, LexicalErrorKind::StringLimit));
    }
    *cursor += ch.len_utf8();
    Ok(())
}

pub(crate) fn is_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\u{000b}' | '\u{000c}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

pub(crate) fn is_line_terminator(ch: char) -> bool {
    matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn validate(source: &SourceText, offset: usize) -> Result<(), LexicalError> {
    source
        .validate_offset(offset)
        .map_err(|cause| error(offset, LexicalErrorKind::InvalidOffset(cause)))
}

pub(crate) fn span(start: usize, end: usize) -> Result<Span, LexicalError> {
    Span::new(start, end).map_err(|cause| error(start, LexicalErrorKind::InternalInvariant(cause)))
}

pub(crate) fn error(offset: usize, kind: LexicalErrorKind) -> LexicalError {
    LexicalError { offset, kind }
}
