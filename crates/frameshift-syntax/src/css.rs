//! CSS tokenization as CSS Syntax Level 3 specifies it (`D-077`).
//!
//! The tokenizer follows the Editor's Draft at csswg-drafts revision
//! `c410ee5e4d2afe6d8b2e8ce53723b8d1eb3b5b67` (2026-10-20). Input
//! preprocessing is applied while reading, without copying: CR, FF, and
//! CR LF read as one LF, and NUL reads as U+FFFD. Tokens and comments keep
//! exact spans in the original text and together cover it without gaps or
//! overlap; whitespace is a token, and comments, which the specification
//! drops, are listed separately. Parse errors never stop tokenization: they
//! are recorded with their offsets, as the specification's error recovery
//! defines the resulting tokens. Values are decoded on demand from the
//! source, and numbers keep their exact source text.

use std::borrow::Cow;
use std::fmt;

use frameshift_core::{SourceText, Span, TextError};

/// Explicit resource limits for one tokenization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssLimits {
    /// Maximum number of tokens and comments together, default 4,194,304.
    pub max_tokens: usize,
}

impl Default for CssLimits {
    fn default() -> Self {
        Self {
            max_tokens: 4_194_304,
        }
    }
}

/// The parser-selected context of one token scan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CssTokenContext {
    /// Whether `U+` followed by a hex digit or `?` starts a
    /// `<unicode-range-token>`. Only the value of the `unicode-range`
    /// descriptor sets it; ordinary tokenization does not.
    pub unicode_ranges_allowed: bool,
}

/// The type flag of a `<hash-token>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssHashKind {
    /// The name after `#` is an ident sequence, so it can be an ID selector.
    Id,
    /// Any other name.
    Unrestricted,
}

/// The type flag of a numeric token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssNumberKind {
    /// Digits without a fraction or exponent.
    Integer,
    /// A fraction or an exponent is present.
    Number,
}

/// The sign character of a numeric token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssSign {
    /// `+`.
    Plus,
    /// `-`.
    Minus,
}

/// The number of a `<number-token>`, `<percentage-token>`, or
/// `<dimension-token>`. Its value is the exact source text: Frameshift
/// does not convert it to floating point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssNumber {
    /// The type flag. Percentages keep it, although the specification
    /// does not distinguish them.
    pub kind: CssNumberKind,
    /// The sign character, if any.
    pub sign: Option<CssSign>,
    /// The number text, including the sign and the exponent.
    pub span: Span,
}

/// The kind of a CSS token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssTokenKind {
    /// `<ident-token>`.
    Ident,
    /// `<function-token>`: a name followed by `(`, which the span includes.
    /// For `url(` with a quoted argument, the span also includes the
    /// whitespace the tokenizer consumed before the quote, all but one.
    Function,
    /// `<at-keyword-token>`.
    AtKeyword,
    /// `<hash-token>`.
    Hash(CssHashKind),
    /// `<string-token>`, possibly unterminated at EOF.
    String,
    /// `<bad-string-token>`: a string that a newline ends.
    BadString,
    /// `<url-token>`: an unquoted `url(...)`, possibly unterminated at EOF.
    Url,
    /// `<bad-url-token>`.
    BadUrl,
    /// `<delim-token>` with its code point.
    Delim(char),
    /// `<number-token>`.
    Number(CssNumber),
    /// `<percentage-token>`; the number excludes the `%`.
    Percentage(CssNumber),
    /// `<dimension-token>`.
    Dimension {
        /// The number before the unit.
        number: CssNumber,
        /// The unit, an ident sequence.
        unit: Span,
    },
    /// `<unicode-range-token>`, only with
    /// [`CssTokenContext::unicode_ranges_allowed`]. The range is inclusive
    /// and empty when `end` is below `start`.
    UnicodeRange {
        /// The first code point.
        start: u32,
        /// The last code point.
        end: u32,
    },
    /// `<whitespace-token>`.
    Whitespace,
    /// `<CDO-token>`: `<!--`.
    Cdo,
    /// `<CDC-token>`: `-->`.
    Cdc,
    /// `<colon-token>`.
    Colon,
    /// `<semicolon-token>`.
    Semicolon,
    /// `<comma-token>`.
    Comma,
    /// `<[-token>`.
    OpenSquare,
    /// `<]-token>`.
    CloseSquare,
    /// `<(-token>`.
    OpenParen,
    /// `<)-token>`.
    CloseParen,
    /// `<{-token>`.
    OpenCurly,
    /// `<}-token>`.
    CloseCurly,
}

/// A CSS token with its exact source span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssToken {
    /// The token kind.
    pub kind: CssTokenKind,
    /// The half-open source span.
    pub span: Span,
}

/// A parse error that the specification defines during tokenization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssParseErrorKind {
    /// A comment reaches EOF before `*/`.
    UnterminatedComment,
    /// A string reaches EOF before its closing quote.
    UnterminatedString,
    /// A newline ends a string, which becomes a `<bad-string-token>`.
    NewlineInString,
    /// An unquoted URL reaches EOF before `)`.
    UnterminatedUrl,
    /// A quote, `(`, or a non-printable code point occurs in an unquoted
    /// URL, which becomes a `<bad-url-token>`.
    InvalidUrlCodePoint,
    /// A backslash that does not start a valid escape occurs in an unquoted
    /// URL, which becomes a `<bad-url-token>`.
    InvalidUrlEscape,
    /// A backslash followed by a newline starts a token; it becomes a
    /// `<delim-token>`.
    InvalidEscape,
    /// An escape reaches EOF; it reads as U+FFFD.
    EscapeAtEof,
}

/// A tokenization parse error at a byte offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssParseError {
    /// What is wrong.
    pub kind: CssParseErrorKind,
    /// The byte offset of the code point at which the error occurs, or the
    /// text length at EOF.
    pub offset: usize,
}

/// A failure that ends tokenization without a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CssError {
    /// The starting offset is outside the text or divides a UTF-8 scalar.
    InvalidOffset(TextError),
    /// More tokens and comments than [`CssLimits::max_tokens`].
    TokenLimit,
    /// A token passed for decoding is not the token that this source
    /// produces at its offset.
    ForeignToken,
}

impl fmt::Display for CssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOffset(error) => write!(f, "invalid CSS scan offset: {error}"),
            Self::TokenLimit => f.write_str("the CSS token limit was reached"),
            Self::ForeignToken => f.write_str("the CSS token does not belong to this source"),
        }
    }
}

impl std::error::Error for CssError {}

/// The result of one parser-directed token scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssScan {
    /// Comments consumed before the token, in source order.
    pub comments: Vec<Span>,
    /// The token, or `None` at EOF.
    pub token: Option<CssToken>,
    /// Parse errors of the comments and the token, in source order.
    pub errors: Vec<CssParseError>,
}

/// A tokenized style sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssTokens {
    bom: bool,
    tokens: Vec<CssToken>,
    comments: Vec<Span>,
    errors: Vec<CssParseError>,
}

impl CssTokens {
    /// Whether the text starts with a UTF-8 byte order mark, which
    /// decoding removes, so no token covers its three bytes.
    pub fn has_bom(&self) -> bool {
        self.bom
    }

    /// The tokens in source order, without an EOF token.
    pub fn tokens(&self) -> &[CssToken] {
        &self.tokens
    }

    /// The comments in source order, including `/*` and `*/`.
    pub fn comments(&self) -> &[Span] {
        &self.comments
    }

    /// The parse errors in source order.
    pub fn errors(&self) -> &[CssParseError] {
        &self.errors
    }
}

/// Tokenizes a whole style sheet with the default context, as the parser's
/// entry points do. A leading byte order mark is skipped.
pub fn tokenize_css(source: &SourceText, limits: CssLimits) -> Result<CssTokens, CssError> {
    let text = source.as_str();
    let bom = text.starts_with('\u{FEFF}');
    let mut scanner = Scanner::new(text, if bom { 3 } else { 0 });
    let mut tokens = Vec::new();
    let mut comments = Vec::new();
    let mut errors = Vec::new();
    loop {
        let token = scanner.token(CssTokenContext::default(), &mut comments, &mut errors);
        if tokens.len() + comments.len() + usize::from(token.is_some()) > limits.max_tokens {
            return Err(CssError::TokenLimit);
        }
        match token {
            Some(token) => tokens.push(token),
            None => break,
        }
    }
    Ok(CssTokens {
        bom,
        tokens,
        comments,
        errors,
    })
}

/// Consumes comments and one token at a parser-selected offset, as
/// "consume a token" does. A byte order mark is not skipped here.
pub fn scan_css_token(
    source: &SourceText,
    offset: usize,
    context: CssTokenContext,
) -> Result<CssScan, CssError> {
    scan_css_token_until(source, offset, source.len(), context)
}

// The unicode-range descriptor is retokenized from a source segment. Its
// tokenizer must see EOF at that segment's boundary, not the stylesheet's.
pub(crate) fn scan_css_token_until(
    source: &SourceText,
    offset: usize,
    end: usize,
    context: CssTokenContext,
) -> Result<CssScan, CssError> {
    source
        .validate_offset(offset)
        .map_err(CssError::InvalidOffset)?;
    source
        .validate_offset(end)
        .map_err(CssError::InvalidOffset)?;
    if offset > end {
        return Err(CssError::ForeignToken);
    }
    let mut scanner = Scanner::new(&source.as_str()[..end], offset);
    let mut comments = Vec::new();
    let mut errors = Vec::new();
    let token = scanner.token(context, &mut comments, &mut errors);
    Ok(CssScan {
        comments,
        token,
        errors,
    })
}

/// The decoded value of a token: the name of an ident, function, at-keyword,
/// or hash token, the content of a string or URL, or the code point of a
/// delimiter. Other tokens have no value. The token must be one that this
/// source produces at its offset.
pub fn css_token_value<'a>(
    source: &'a SourceText,
    token: &CssToken,
) -> Result<Option<Cow<'a, str>>, CssError> {
    verify(source, token)?;
    let text = source.as_str();
    let raw = &text[token.span.start()..token.span.end()];
    let start = token.span.start();
    let end = token.span.end();
    let value = match token.kind {
        CssTokenKind::Ident => Some(name_value(text, start, end)),
        CssTokenKind::Function => {
            let mut scanner = Scanner::new(text, start);
            let mut value = String::new();
            scanner.ident_sequence(&mut value);
            Some(borrow_if_equal(raw, value))
        }
        CssTokenKind::AtKeyword | CssTokenKind::Hash(_) => Some(name_value(text, start + 1, end)),
        CssTokenKind::String | CssTokenKind::BadString => {
            let mut scanner = Scanner::new(text, start);
            let mut value = String::new();
            if let Some(quote) = scanner.read() {
                scanner.string(quote, &mut value, &mut Vec::new());
            }
            Some(Cow::Owned(value))
        }
        CssTokenKind::Url | CssTokenKind::BadUrl => {
            let mut scanner = Scanner::new(text, start);
            let mut name = String::new();
            scanner.ident_sequence(&mut name);
            scanner.read();
            let mut value = String::new();
            scanner.url(&mut value, &mut Vec::new());
            Some(Cow::Owned(value))
        }
        CssTokenKind::Delim(code_point) => Some(Cow::Owned(code_point.to_string())),
        CssTokenKind::Number(_)
        | CssTokenKind::Percentage(_)
        | CssTokenKind::Dimension { .. }
        | CssTokenKind::UnicodeRange { .. }
        | CssTokenKind::Whitespace
        | CssTokenKind::Cdo
        | CssTokenKind::Cdc
        | CssTokenKind::Colon
        | CssTokenKind::Semicolon
        | CssTokenKind::Comma
        | CssTokenKind::OpenSquare
        | CssTokenKind::CloseSquare
        | CssTokenKind::OpenParen
        | CssTokenKind::CloseParen
        | CssTokenKind::OpenCurly
        | CssTokenKind::CloseCurly => None,
    };
    Ok(value)
}

/// The decoded unit of a `<dimension-token>`.
pub fn css_dimension_unit<'a>(
    source: &'a SourceText,
    token: &CssToken,
) -> Result<Option<Cow<'a, str>>, CssError> {
    let CssTokenKind::Dimension { unit, .. } = token.kind else {
        return Ok(None);
    };
    verify(source, token)?;
    Ok(Some(name_value(source.as_str(), unit.start(), unit.end())))
}

/// Checks that the source produces the token at its offset. Only a
/// `<unicode-range-token>` depends on the context, and it has no value.
fn verify(source: &SourceText, token: &CssToken) -> Result<(), CssError> {
    let start = token.span.start();
    source
        .validate_offset(start)
        .map_err(CssError::InvalidOffset)?;
    let mut scanner = Scanner::new(source.as_str(), start);
    let mut comments = Vec::new();
    let scanned = scanner.token(
        CssTokenContext {
            unicode_ranges_allowed: matches!(token.kind, CssTokenKind::UnicodeRange { .. }),
        },
        &mut comments,
        &mut Vec::new(),
    );
    if comments.is_empty() && scanned.as_ref() == Some(token) {
        Ok(())
    } else {
        Err(CssError::ForeignToken)
    }
}

/// The value of an ident sequence between two offsets: a slice when it has
/// no escape and no NUL, otherwise the decoded code points.
fn name_value(text: &str, start: usize, end: usize) -> Cow<'_, str> {
    let raw = &text[start..end];
    if !raw.bytes().any(|byte| byte == b'\\' || byte == 0) {
        return Cow::Borrowed(raw);
    }
    let mut scanner = Scanner::new(text, start);
    let mut value = String::new();
    scanner.ident_sequence(&mut value);
    Cow::Owned(value)
}

fn borrow_if_equal(raw: &str, value: String) -> Cow<'_, str> {
    match raw.strip_suffix('(') {
        Some(name) if name == value => Cow::Borrowed(name),
        Some(_) | None => Cow::Owned(value),
    }
}

/// A sink for decoded code points; scanning without decoding discards them.
trait Sink {
    fn push(&mut self, code_point: char);
}

impl Sink for String {
    fn push(&mut self, code_point: char) {
        String::push(self, code_point);
    }
}

struct Discard;

impl Sink for Discard {
    fn push(&mut self, _: char) {}
}

/// Whether the code points pushed so far are an ASCII case-insensitive
/// match for `url`.
struct UrlName {
    length: usize,
    matches: bool,
}

impl Sink for UrlName {
    fn push(&mut self, code_point: char) {
        self.matches &=
            self.length < 3 && code_point.to_ascii_lowercase() == ['u', 'r', 'l'][self.length];
        self.length += 1;
    }
}

impl UrlName {
    fn is_url(&self) -> bool {
        self.matches && self.length == 3
    }
}

/// Reads preprocessed code points from the original text.
struct Scanner<'a> {
    text: &'a str,
    position: usize,
}

const REPLACEMENT: char = '\u{FFFD}';

/// The preprocessed code point at a byte offset and its length in the
/// original text.
fn code_point_at(text: &str, offset: usize) -> Option<(char, usize)> {
    let bytes = text.as_bytes();
    let byte = *bytes.get(offset)?;
    Some(match byte {
        b'\r' if bytes.get(offset + 1) == Some(&b'\n') => ('\n', 2),
        b'\r' | 0x0c => ('\n', 1),
        0 => (REPLACEMENT, 1),
        0x01..=0x7f => (char::from(byte), 1),
        _ => {
            let code_point = text[offset..].chars().next()?;
            (code_point, code_point.len_utf8())
        }
    })
}

const fn is_whitespace(code_point: char) -> bool {
    matches!(code_point, '\n' | '\t' | ' ')
}

const fn is_hex_digit(code_point: char) -> bool {
    code_point.is_ascii_hexdigit()
}

const fn is_non_ascii_ident(code_point: char) -> bool {
    matches!(
        code_point,
        '\u{B7}'
            | '\u{C0}'..='\u{D6}'
            | '\u{D8}'..='\u{F6}'
            | '\u{F8}'..='\u{37D}'
            | '\u{37F}'..='\u{1FFF}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{203F}'
            | '\u{2040}'
            | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}'
            | '\u{3001}'..='\u{D7FF}'
            | '\u{F900}'..='\u{FDCF}'
            | '\u{FDF0}'..='\u{FFFD}'
            | '\u{10000}'..
    )
}

const fn is_ident_start(code_point: char) -> bool {
    code_point.is_ascii_alphabetic() || code_point == '_' || is_non_ascii_ident(code_point)
}

const fn is_ident(code_point: char) -> bool {
    is_ident_start(code_point) || code_point.is_ascii_digit() || code_point == '-'
}

const fn is_non_printable(code_point: char) -> bool {
    matches!(code_point, '\u{0}'..='\u{8}' | '\u{B}' | '\u{E}'..='\u{1F}' | '\u{7F}')
}

/// Whether two code points are a valid escape.
fn valid_escape(first: Option<char>, second: Option<char>) -> bool {
    first == Some('\\') && second != Some('\n')
}

/// Whether three code points would start an ident sequence.
fn starts_ident(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    match first {
        Some('-') => {
            second.is_some_and(|code_point| is_ident_start(code_point) || code_point == '-')
                || valid_escape(second, third)
        }
        Some('\\') => valid_escape(first, second),
        Some(code_point) => is_ident_start(code_point),
        None => false,
    }
}

/// Whether three code points would start a number.
fn starts_number(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    let digit = |code_point: Option<char>| code_point.is_some_and(|c| c.is_ascii_digit());
    match first {
        Some('+' | '-') => digit(second) || (second == Some('.') && digit(third)),
        Some('.') => digit(second),
        first => digit(first),
    }
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str, position: usize) -> Self {
        Self { text, position }
    }

    /// The `n`th next code point, zero-based, without consuming it.
    fn peek(&self, n: usize) -> Option<char> {
        let mut offset = self.position;
        for _ in 0..n {
            offset += code_point_at(self.text, offset)?.1;
        }
        code_point_at(self.text, offset).map(|(code_point, _)| code_point)
    }

    fn read(&mut self) -> Option<char> {
        let (code_point, length) = code_point_at(self.text, self.position)?;
        self.position += length;
        Some(code_point)
    }

    fn span(&self, start: usize) -> Span {
        // The scanner only moves forward, so the span is ordered.
        Span::new(start, self.position).unwrap_or_default()
    }

    /// Consumes comments, then one token.
    fn token(
        &mut self,
        context: CssTokenContext,
        comments: &mut Vec<Span>,
        errors: &mut Vec<CssParseError>,
    ) -> Option<CssToken> {
        self.comments(comments, errors);
        let start = self.position;
        let current = self.read()?;
        let kind = match current {
            '\n' | '\t' | ' ' => {
                while self.peek(0).is_some_and(is_whitespace) {
                    self.read();
                }
                CssTokenKind::Whitespace
            }
            '"' | '\'' => self.string(current, &mut Discard, errors),
            '#' => {
                if self.peek(0).is_some_and(is_ident) || valid_escape(self.peek(0), self.peek(1)) {
                    let kind = if starts_ident(self.peek(0), self.peek(1), self.peek(2)) {
                        CssHashKind::Id
                    } else {
                        CssHashKind::Unrestricted
                    };
                    self.ident_sequence_with_errors(&mut Discard, errors);
                    CssTokenKind::Hash(kind)
                } else {
                    CssTokenKind::Delim('#')
                }
            }
            '(' => CssTokenKind::OpenParen,
            ')' => CssTokenKind::CloseParen,
            ',' => CssTokenKind::Comma,
            ':' => CssTokenKind::Colon,
            ';' => CssTokenKind::Semicolon,
            '[' => CssTokenKind::OpenSquare,
            ']' => CssTokenKind::CloseSquare,
            '{' => CssTokenKind::OpenCurly,
            '}' => CssTokenKind::CloseCurly,
            '+' | '.' => {
                if starts_number(Some(current), self.peek(0), self.peek(1)) {
                    self.position = start;
                    self.numeric(errors)
                } else {
                    CssTokenKind::Delim(current)
                }
            }
            '-' => {
                if starts_number(Some(current), self.peek(0), self.peek(1)) {
                    self.position = start;
                    self.numeric(errors)
                } else if self.peek(0) == Some('-') && self.peek(1) == Some('>') {
                    self.read();
                    self.read();
                    CssTokenKind::Cdc
                } else if starts_ident(Some(current), self.peek(0), self.peek(1)) {
                    self.position = start;
                    self.ident_like(errors)
                } else {
                    CssTokenKind::Delim('-')
                }
            }
            '<' => {
                if self.peek(0) == Some('!')
                    && self.peek(1) == Some('-')
                    && self.peek(2) == Some('-')
                {
                    self.read();
                    self.read();
                    self.read();
                    CssTokenKind::Cdo
                } else {
                    CssTokenKind::Delim('<')
                }
            }
            '@' => {
                if starts_ident(self.peek(0), self.peek(1), self.peek(2)) {
                    self.ident_sequence_with_errors(&mut Discard, errors);
                    CssTokenKind::AtKeyword
                } else {
                    CssTokenKind::Delim('@')
                }
            }
            '\\' => {
                if valid_escape(Some(current), self.peek(0)) {
                    self.position = start;
                    self.ident_like(errors)
                } else {
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::InvalidEscape,
                        offset: start,
                    });
                    CssTokenKind::Delim('\\')
                }
            }
            '0'..='9' => {
                self.position = start;
                self.numeric(errors)
            }
            'U' | 'u'
                if context.unicode_ranges_allowed
                    && self.peek(0) == Some('+')
                    && self.peek(1).is_some_and(|c| c == '?' || is_hex_digit(c)) =>
            {
                self.unicode_range()
            }
            code_point if is_ident_start(code_point) => {
                self.position = start;
                self.ident_like(errors)
            }
            code_point => CssTokenKind::Delim(code_point),
        };
        Some(CssToken {
            kind,
            span: self.span(start),
        })
    }

    fn comments(&mut self, comments: &mut Vec<Span>, errors: &mut Vec<CssParseError>) {
        while self.text[self.position..].starts_with("/*") {
            let start = self.position;
            match self.text[start + 2..].find("*/") {
                Some(index) => self.position = start + 2 + index + 2,
                None => {
                    self.position = self.text.len();
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::UnterminatedComment,
                        offset: self.position,
                    });
                }
            }
            comments.push(self.span(start));
        }
    }

    /// Consumes a string token after its opening quote.
    fn string(
        &mut self,
        quote: char,
        value: &mut impl Sink,
        errors: &mut Vec<CssParseError>,
    ) -> CssTokenKind {
        loop {
            let at = self.position;
            match self.peek(0) {
                None => {
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::UnterminatedString,
                        offset: at,
                    });
                    return CssTokenKind::String;
                }
                Some('\n') => {
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::NewlineInString,
                        offset: at,
                    });
                    return CssTokenKind::BadString;
                }
                Some(code_point) => {
                    self.read();
                    if code_point == quote {
                        return CssTokenKind::String;
                    }
                    if code_point == '\\' {
                        match self.peek(0) {
                            None => {}
                            Some('\n') => {
                                self.read();
                            }
                            Some(_) => value.push(self.escaped(errors)),
                        }
                    } else {
                        value.push(code_point);
                    }
                }
            }
        }
    }

    /// Consumes an escaped code point after its backslash.
    fn escaped(&mut self, errors: &mut Vec<CssParseError>) -> char {
        let Some(first) = self.read() else {
            errors.push(CssParseError {
                kind: CssParseErrorKind::EscapeAtEof,
                offset: self.position,
            });
            return REPLACEMENT;
        };
        if !is_hex_digit(first) {
            return first;
        }
        let mut number = first.to_digit(16).unwrap_or_default();
        for _ in 0..5 {
            match self.peek(0).and_then(|c| c.to_digit(16)) {
                Some(digit) => {
                    number = number * 16 + digit;
                    self.read();
                }
                None => break,
            }
        }
        if self.peek(0).is_some_and(is_whitespace) {
            self.read();
        }
        match char::from_u32(number) {
            Some('\u{0}') | None => REPLACEMENT,
            Some(code_point) => code_point,
        }
    }

    /// Consumes an ident sequence, decoding escapes into `value`.
    fn ident_sequence(&mut self, value: &mut impl Sink) {
        self.ident_sequence_with_errors(value, &mut Vec::new());
    }

    fn ident_sequence_with_errors(
        &mut self,
        value: &mut impl Sink,
        errors: &mut Vec<CssParseError>,
    ) {
        loop {
            match self.peek(0) {
                Some(code_point) if is_ident(code_point) => {
                    self.read();
                    value.push(code_point);
                }
                first @ Some('\\') if valid_escape(first, self.peek(1)) => {
                    self.read();
                    value.push(self.escaped(errors));
                }
                Some(_) | None => return,
            }
        }
    }

    fn numeric(&mut self, errors: &mut Vec<CssParseError>) -> CssTokenKind {
        let number = self.number();
        if starts_ident(self.peek(0), self.peek(1), self.peek(2)) {
            let unit_start = self.position;
            self.ident_sequence_with_errors(&mut Discard, errors);
            CssTokenKind::Dimension {
                number,
                unit: self.span(unit_start),
            }
        } else if self.peek(0) == Some('%') {
            self.read();
            CssTokenKind::Percentage(number)
        } else {
            CssTokenKind::Number(number)
        }
    }

    fn number(&mut self) -> CssNumber {
        let start = self.position;
        let mut kind = CssNumberKind::Integer;
        let sign = match self.peek(0) {
            Some('+') => Some(CssSign::Plus),
            Some('-') => Some(CssSign::Minus),
            Some(_) | None => None,
        };
        if sign.is_some() {
            self.read();
        }
        self.digits();
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.read();
            self.digits();
            kind = CssNumberKind::Number;
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
            let signed = matches!(self.peek(1), Some('+' | '-'));
            if digit(self.peek(1)) || (signed && digit(self.peek(2))) {
                self.read();
                if signed {
                    self.read();
                }
                self.digits();
                kind = CssNumberKind::Number;
            }
        }
        CssNumber {
            kind,
            sign,
            span: self.span(start),
        }
    }

    fn digits(&mut self) {
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.read();
        }
    }

    fn ident_like(&mut self, errors: &mut Vec<CssParseError>) -> CssTokenKind {
        let mut name = UrlName {
            length: 0,
            matches: true,
        };
        self.ident_sequence_with_errors(&mut name, errors);
        if self.peek(0) != Some('(') {
            return CssTokenKind::Ident;
        }
        self.read();
        if !name.is_url() {
            return CssTokenKind::Function;
        }
        while self.peek(0).is_some_and(is_whitespace) && self.peek(1).is_some_and(is_whitespace) {
            self.read();
        }
        let quote = |c: Option<char>| matches!(c, Some('"' | '\''));
        if quote(self.peek(0)) || (self.peek(0).is_some_and(is_whitespace) && quote(self.peek(1))) {
            return CssTokenKind::Function;
        }
        self.url(&mut Discard, errors)
    }

    /// Consumes an unquoted URL after `url(`.
    fn url(&mut self, value: &mut impl Sink, errors: &mut Vec<CssParseError>) -> CssTokenKind {
        while self.peek(0).is_some_and(is_whitespace) {
            self.read();
        }
        loop {
            let at = self.position;
            let Some(code_point) = self.read() else {
                errors.push(CssParseError {
                    kind: CssParseErrorKind::UnterminatedUrl,
                    offset: at,
                });
                return CssTokenKind::Url;
            };
            match code_point {
                ')' => return CssTokenKind::Url,
                '\n' | '\t' | ' ' => {
                    while self.peek(0).is_some_and(is_whitespace) {
                        self.read();
                    }
                    match self.peek(0) {
                        Some(')') => {
                            self.read();
                            return CssTokenKind::Url;
                        }
                        None => {
                            errors.push(CssParseError {
                                kind: CssParseErrorKind::UnterminatedUrl,
                                offset: self.position,
                            });
                            return CssTokenKind::Url;
                        }
                        Some(_) => {
                            self.bad_url_remnants(errors);
                            return CssTokenKind::BadUrl;
                        }
                    }
                }
                '"' | '\'' | '(' => {
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::InvalidUrlCodePoint,
                        offset: at,
                    });
                    self.bad_url_remnants(errors);
                    return CssTokenKind::BadUrl;
                }
                code_point if is_non_printable(code_point) => {
                    errors.push(CssParseError {
                        kind: CssParseErrorKind::InvalidUrlCodePoint,
                        offset: at,
                    });
                    self.bad_url_remnants(errors);
                    return CssTokenKind::BadUrl;
                }
                '\\' => {
                    if valid_escape(Some('\\'), self.peek(0)) {
                        value.push(self.escaped(errors));
                    } else {
                        errors.push(CssParseError {
                            kind: CssParseErrorKind::InvalidUrlEscape,
                            offset: at,
                        });
                        self.bad_url_remnants(errors);
                        return CssTokenKind::BadUrl;
                    }
                }
                code_point => value.push(code_point),
            }
        }
    }

    fn bad_url_remnants(&mut self, errors: &mut Vec<CssParseError>) {
        loop {
            match self.peek(0) {
                None => return,
                Some(')') => {
                    self.read();
                    return;
                }
                first @ Some('\\') if valid_escape(first, self.peek(1)) => {
                    self.read();
                    self.escaped(errors);
                }
                Some(_) => {
                    self.read();
                }
            }
        }
    }

    /// Consumes a unicode-range token after its `U` or `u`.
    fn unicode_range(&mut self) -> CssTokenKind {
        self.read();
        let mut digits = String::new();
        while digits.len() < 6 && self.peek(0).is_some_and(is_hex_digit) {
            digits.extend(self.read());
        }
        let mut questions = 0;
        while digits.len() + questions < 6 && self.peek(0) == Some('?') {
            self.read();
            questions += 1;
        }
        let hex = |text: &str| u32::from_str_radix(text, 16).unwrap_or_default();
        if questions > 0 {
            let low = format!("{digits}{}", "0".repeat(questions));
            let high = format!("{digits}{}", "F".repeat(questions));
            return CssTokenKind::UnicodeRange {
                start: hex(&low),
                end: hex(&high),
            };
        }
        let start = hex(&digits);
        if self.peek(0) == Some('-') && self.peek(1).is_some_and(is_hex_digit) {
            self.read();
            let mut end = String::new();
            while end.len() < 6 && self.peek(0).is_some_and(is_hex_digit) {
                end.extend(self.read());
            }
            return CssTokenKind::UnicodeRange {
                start,
                end: hex(&end),
            };
        }
        CssTokenKind::UnicodeRange { start, end: start }
    }
}
