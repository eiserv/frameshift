//! The Yarn classic lockfile syntax, `yarn.lock` format version 1
//! (`D-072`).
//!
//! The grammar is the one that Yarn 1.22.22's lockfile parser implements,
//! not a YAML subset: keys and values separated by spaces with an optional
//! colon, nested objects indented in steps of two spaces, comma-separated
//! keys that share one value, `#` comments, quoted strings with JSON
//! escapes, unquoted words, digit runs, and the words `true` and `false`.
//! Its quirks are reproduced, such as a word that starts with `true`, or a
//! quoted string that ends at a quote after three backslashes. Where Yarn
//! would read the text differently or drop content silently, the input is
//! rejected instead: Git merge conflict markers, which Yarn resolves by
//! merging both sides, indentation that ends the top-level object early, and
//! escapes of unpaired surrogates.

use std::fmt;

use frameshift_core::{SourceText, Span};

/// Explicit bounds for one Yarn classic lockfile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YarnLimits {
    /// Maximum object nesting, counting the top-level object, default 64.
    /// Lockfile entries nest three levels deep.
    pub max_depth: usize,
    /// Maximum values and keys, default 4,194,304.
    pub max_nodes: usize,
}

impl Default for YarnLimits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_nodes: 4_194_304,
        }
    }
}

/// Why a lockfile was rejected. No variant includes source content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YarnErrorKind {
    /// A character that starts no token, such as a tab or `@`, or a quoted
    /// string that is not a complete JSON string, for example one with a raw
    /// control character, an invalid escape, or an unpaired surrogate escape.
    InvalidToken,
    /// A token that the grammar does not allow at this point.
    UnexpectedToken,
    /// An odd number of leading spaces, or indentation at the top level,
    /// after which Yarn would ignore the rest of the file.
    Indentation,
    /// An empty key.
    EmptyKey,
    /// The file contains the three Git merge conflict markers. Yarn merges
    /// both sides of such a file instead of reading it.
    MergeConflict,
    /// A `yarn lockfile v<n>` comment names a format version above 1.
    UnsupportedVersion,
    /// Objects nest deeper than the limit.
    DepthLimit {
        /// The configured maximum depth.
        limit: usize,
    },
    /// The lockfile has more values and keys than the limit.
    NodeLimit {
        /// The configured maximum node count.
        limit: usize,
    },
    /// Storage could not be reserved.
    AllocationFailed,
}

/// A rejected lockfile. It names a kind and a span, never source content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YarnError {
    kind: YarnErrorKind,
    span: Span,
}

impl YarnError {
    /// Returns why the lockfile was rejected.
    pub const fn kind(self) -> YarnErrorKind {
        self.kind
    }

    /// Returns the source bytes at fault, at character boundaries. An empty
    /// span marks a position, such as the end of the input.
    pub const fn span(self) -> Span {
        self.span
    }
}

impl fmt::Display for YarnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            YarnErrorKind::InvalidToken => f.write_str("invalid token")?,
            YarnErrorKind::UnexpectedToken => f.write_str("unexpected token")?,
            YarnErrorKind::Indentation => f.write_str("invalid indentation")?,
            YarnErrorKind::EmptyKey => f.write_str("empty key")?,
            YarnErrorKind::MergeConflict => f.write_str("merge conflict markers")?,
            YarnErrorKind::UnsupportedVersion => {
                f.write_str("lockfile format version above 1")?;
            }
            YarnErrorKind::DepthLimit { limit } => {
                write!(f, "nesting exceeds the limit of {limit} (yarn.max-depth)")?;
            }
            YarnErrorKind::NodeLimit { limit } => {
                write!(
                    f,
                    "the lockfile exceeds {limit} values and keys (yarn.max-nodes)"
                )?;
            }
            YarnErrorKind::AllocationFailed => {
                f.write_str("Yarn lockfile storage allocation failed")?;
            }
        }
        write!(f, " at bytes {}..{}", self.span.start(), self.span.end())
    }
}

impl std::error::Error for YarnError {}

/// Identifies a value in one [`YarnDocument`]. Identities ascend in source
/// order of the values' starts; the top-level object comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YarnId(u32);

impl YarnId {
    /// Returns the zero-based position in the document's value arena.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where the text of a string or key is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// A range of the source.
    Source(u32, u32),
    /// A range of the decoded buffer.
    Decoded(u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    String(Text),
    Number,
    True,
    False,
    Object {
        first: u32,
        len: u32,
        /// Where the object's keys start in the sorted key order.
        order: u32,
        keys: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node {
    kind: Kind,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Member {
    first_key: u32,
    keys: u32,
    value: u32,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    text: Text,
    start: u32,
    end: u32,
    shadowed: bool,
}

/// A parsed lockfile that borrows its source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YarnDocument<'s> {
    source: &'s str,
    nodes: Vec<Node>,
    members: Vec<Member>,
    keys: Vec<Key>,
    /// Per object, its key indices sorted by text, then by position.
    order: Vec<u32>,
    decoded: String,
    /// Shadowed keys, in source order.
    shadowed: Vec<u32>,
    bom: bool,
}

impl<'s> YarnDocument<'s> {
    /// Returns the top-level object.
    pub const fn root(&self) -> YarnId {
        YarnId(0)
    }

    /// Returns whether the source starts with a UTF-8 byte order mark,
    /// which the parser skipped.
    pub const fn has_bom(&self) -> bool {
        self.bom
    }

    /// Returns the number of values and keys.
    pub fn node_count(&self) -> usize {
        self.nodes.len() + self.keys.len()
    }

    /// Returns the source span of a value. A quoted string includes its
    /// quotes; an object spans its members, or is empty without members.
    pub fn span(&self, id: YarnId) -> Option<Span> {
        let node = self.nodes.get(id.index())?;
        Span::new(node.start as usize, node.end as usize).ok()
    }

    /// Returns a value.
    pub fn value(&self, id: YarnId) -> Option<YarnValue<'_>> {
        let node = self.nodes.get(id.index())?;
        Some(match node.kind {
            Kind::String(text) => YarnValue::String(self.text(text)),
            Kind::Number => YarnValue::Number(&self.source[node.start as usize..node.end as usize]),
            Kind::True => YarnValue::Bool(true),
            Kind::False => YarnValue::Bool(false),
            Kind::Object {
                first,
                len,
                order,
                keys,
            } => YarnValue::Object(YarnObject {
                document: self,
                first: first as usize,
                len: len as usize,
                order: order as usize,
                keys: keys as usize,
            }),
        })
    }

    /// Lists every key that occurs again later in the same object, in
    /// source order, including keys that one member repeats. Yarn uses the
    /// last occurrence.
    pub fn duplicate_keys(&self) -> impl ExactSizeIterator<Item = YarnKey<'_>> {
        self.shadowed.iter().map(|&key| self.key(key as usize))
    }

    fn text(&self, text: Text) -> &str {
        match text {
            Text::Source(start, end) => &self.source[start as usize..end as usize],
            Text::Decoded(start, end) => &self.decoded[start as usize..end as usize],
        }
    }

    fn key(&self, index: usize) -> YarnKey<'_> {
        let key = self.keys[index];
        YarnKey {
            text: self.text(key.text),
            span: Span::new(key.start as usize, key.end as usize).unwrap_or_default(),
            shadowed: key.shadowed,
        }
    }

    fn member(&self, index: usize) -> YarnMember<'_> {
        let member = self.members[index];
        YarnMember {
            document: self,
            first_key: member.first_key as usize,
            keys: member.keys as usize,
            value: YarnId(member.value),
            span: Span::new(member.start as usize, member.end as usize).unwrap_or_default(),
        }
    }
}

/// A value of a lockfile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YarnValue<'a> {
    /// A decoded quoted string or an unquoted word.
    String(&'a str),
    /// A run of ASCII digits, which Yarn reads as a number; the exact text.
    Number(&'a str),
    /// A `true` or `false` token.
    Bool(bool),
    /// An object.
    Object(YarnObject<'a>),
}

impl<'a> YarnValue<'a> {
    /// Returns the string, if this is one.
    pub const fn as_str(self) -> Option<&'a str> {
        match self {
            Self::String(text) => Some(text),
            Self::Number(_) | Self::Bool(_) | Self::Object(_) => None,
        }
    }

    /// Returns the object, if this is one.
    pub const fn as_object(self) -> Option<YarnObject<'a>> {
        match self {
            Self::Object(object) => Some(object),
            Self::String(_) | Self::Number(_) | Self::Bool(_) => None,
        }
    }
}

/// The members of an object, in source order, including shadowed keys.
#[derive(Clone, Copy, Debug)]
pub struct YarnObject<'a> {
    document: &'a YarnDocument<'a>,
    first: usize,
    len: usize,
    order: usize,
    keys: usize,
}

impl PartialEq for YarnObject<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.document, other.document)
            && self.first == other.first
            && self.len == other.len
    }
}

impl Eq for YarnObject<'_> {}

impl<'a> YarnObject<'a> {
    /// Returns the number of members.
    pub const fn len(self) -> usize {
        self.len
    }

    /// Returns whether the object has no members.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Returns the value of the last occurrence of a key, as Yarn does.
    pub fn get(self, key: &str) -> Option<YarnId> {
        let order = &self.document.order[self.order..self.order + self.keys];
        let text = |index: u32| self.document.text(self.document.keys[index as usize].text);
        let after = order.partition_point(|&index| text(index) <= key);
        let last = *order.get(after.checked_sub(1)?)?;
        if text(last) != key {
            return None;
        }
        // The member that holds the key: members own ascending key ranges.
        let members = &self.document.members[self.first..self.first + self.len];
        let position = members.partition_point(|member| member.first_key <= last);
        let member = members.get(position.checked_sub(1)?)?;
        Some(YarnId(member.value))
    }

    /// Returns the member at a source position.
    pub fn member(self, index: usize) -> Option<YarnMember<'a>> {
        (index < self.len).then(|| self.document.member(self.first + index))
    }

    /// Iterates over the members in source order.
    pub fn members(self) -> impl ExactSizeIterator<Item = YarnMember<'a>> + 'a {
        let document = self.document;
        (self.first..self.first + self.len).map(move |index| document.member(index))
    }
}

/// One member of an object: one or more keys that share one value.
#[derive(Clone, Copy, Debug)]
pub struct YarnMember<'a> {
    document: &'a YarnDocument<'a>,
    first_key: usize,
    keys: usize,
    value: YarnId,
    span: Span,
}

impl PartialEq for YarnMember<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.document, other.document)
            && self.first_key == other.first_key
            && self.keys == other.keys
    }
}

impl Eq for YarnMember<'_> {}

impl<'a> YarnMember<'a> {
    /// Returns the number of keys, at least one.
    pub const fn key_count(self) -> usize {
        self.keys
    }

    /// Iterates over the keys in source order.
    pub fn keys(self) -> impl ExactSizeIterator<Item = YarnKey<'a>> + 'a {
        let document = self.document;
        (self.first_key..self.first_key + self.keys).map(move |index| document.key(index))
    }

    /// Returns the shared value.
    pub const fn value(self) -> YarnId {
        self.value
    }

    /// Returns the source span from the first key to the end of the value.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// One key of a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YarnKey<'a> {
    text: &'a str,
    span: Span,
    shadowed: bool,
}

impl<'a> YarnKey<'a> {
    /// Borrows the decoded key.
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// Returns the source span of the key, including any quotes.
    pub const fn span(self) -> Span {
        self.span
    }

    /// Returns whether a later key of the same object has the same text.
    pub const fn is_shadowed(self) -> bool {
        self.shadowed
    }
}

/// Parses one complete Yarn classic lockfile.
///
/// A leading UTF-8 byte order mark is skipped, as Yarn does. Spans refer to
/// the original source, including the mark. The text must be read by the
/// classic grammar as a whole: a file with merge conflict markers, a format
/// version comment above 1, or content that Yarn would ignore is rejected.
pub fn parse_yarn_lockfile(
    source: &SourceText,
    limits: YarnLimits,
) -> Result<YarnDocument<'_>, YarnError> {
    let text = source.as_str();
    let bom = text.starts_with('\u{FEFF}');
    let start = if bom { '\u{FEFF}'.len_utf8() } else { 0 };
    let content = &text[start..];
    if content.contains("<<<<<<<") && content.contains("=======") && content.contains(">>>>>>>") {
        let at = start + content.find("<<<<<<<").unwrap_or(0);
        return Err(YarnError {
            kind: YarnErrorKind::MergeConflict,
            span: Span::new(at, at + 7).unwrap_or_default(),
        });
    }
    let mut parser = Parser {
        lexer: Lexer {
            text,
            bytes: text.as_bytes(),
            pos: start,
            last_newline: false,
        },
        limits,
        document: YarnDocument {
            source: text,
            nodes: Vec::new(),
            members: Vec::new(),
            keys: Vec::new(),
            order: Vec::new(),
            decoded: String::new(),
            shadowed: Vec::new(),
            bom,
        },
        pending: Vec::new(),
    };
    parser.run(start)?;
    Ok(parser.document)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
    Newline,
    Comment,
    Indent(usize),
    String(Text),
    Number,
    Bool(bool),
    Colon,
    Comma,
    Invalid,
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}

struct Lexer<'s> {
    text: &'s str,
    bytes: &'s [u8],
    pos: usize,
    /// Whether the previous token was a newline from LF or CRLF, after
    /// which spaces are indentation. A lone CR is a newline, but Yarn does
    /// not treat the spaces after it as indentation.
    last_newline: bool,
}

impl Lexer<'_> {
    /// Produces the next token as Yarn's tokenizer does. Strings that need
    /// decoding are appended to `decoded`.
    fn next(&mut self, decoded: &mut String) -> Result<Token, YarnError> {
        loop {
            let start = self.pos;
            let Some(&byte) = self.bytes.get(start) else {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    start,
                    end: start,
                });
            };
            let next = self.bytes.get(start + 1).copied();
            let after_newline = self.last_newline;
            self.last_newline = byte == b'\n' || (byte == b'\r' && next == Some(b'\n'));
            let (kind, end) = match byte {
                // Yarn consumes a following LF with any newline, so a blank
                // line of two LFs is one token.
                b'\n' | b'\r' => (
                    TokenKind::Newline,
                    start + 1 + usize::from(next == Some(b'\n')),
                ),
                b'#' => {
                    let end = self.text[start + 1..]
                        .find('\n')
                        .map_or(self.bytes.len(), |offset| start + 1 + offset);
                    (TokenKind::Comment, end)
                }
                b' ' if after_newline => {
                    let count = self.bytes[start..]
                        .iter()
                        .take_while(|&&byte| byte == b' ')
                        .count();
                    if count % 2 == 1 {
                        return Err(error(YarnErrorKind::Indentation, start, start + count));
                    }
                    (TokenKind::Indent(count / 2), start + count)
                }
                b' ' => {
                    self.pos = start + 1;
                    continue;
                }
                b'"' => {
                    let end = self.quoted_end(start);
                    let kind = match decode_json_string(&self.text[start..end], decoded) {
                        Ok(Quoted::Plain) => {
                            TokenKind::String(Text::Source(start as u32 + 1, end as u32 - 1))
                        }
                        Ok(Quoted::Decoded(from, to)) => TokenKind::String(Text::Decoded(from, to)),
                        Ok(Quoted::Invalid) => TokenKind::Invalid,
                        Err(()) => {
                            return Err(error(YarnErrorKind::AllocationFailed, start, end));
                        }
                    };
                    (kind, end)
                }
                b'0'..=b'9' => {
                    let count = self.bytes[start..]
                        .iter()
                        .take_while(|byte| byte.is_ascii_digit())
                        .count();
                    (TokenKind::Number, start + count)
                }
                _ if self.bytes[start..].starts_with(b"true") => (TokenKind::Bool(true), start + 4),
                _ if self.bytes[start..].starts_with(b"false") => {
                    (TokenKind::Bool(false), start + 5)
                }
                b':' => (TokenKind::Colon, start + 1),
                b',' => (TokenKind::Comma, start + 1),
                b'a'..=b'z' | b'A'..=b'Z' | b'/' | b'.' | b'-' => {
                    let count = self.bytes[start..]
                        .iter()
                        .take_while(|&&byte| !matches!(byte, b':' | b' ' | b'\n' | b'\r' | b','))
                        .count();
                    let end = start + count;
                    (
                        TokenKind::String(Text::Source(start as u32, end as u32)),
                        end,
                    )
                }
                _ => {
                    let width = self.text[start..].chars().next().map_or(1, char::len_utf8);
                    (TokenKind::Invalid, start + width)
                }
            };
            self.pos = end;
            return Ok(Token { kind, start, end });
        }
    }

    /// Finds the end of a quoted string as Yarn does: at the first quote
    /// that does not follow exactly one backslash, or at the end of input.
    fn quoted_end(&self, start: usize) -> usize {
        let mut index = start + 1;
        while index < self.bytes.len() {
            if self.bytes[index] == b'"' {
                // A backslash before the quote is at `start + 1` or later,
                // so the second look-behind stays within the token.
                let escaped =
                    self.bytes[index - 1] == BACKSLASH && self.bytes[index - 2] != BACKSLASH;
                if !escaped {
                    return index + 1;
                }
            }
            index += 1;
        }
        self.bytes.len()
    }
}

const BACKSLASH: u8 = b'\\';

/// A quoted string token's content.
enum Quoted {
    /// Valid without escapes: the source between the quotes.
    Plain,
    /// Valid with escapes: a range of the decoded buffer.
    Decoded(u32, u32),
    /// Not exactly one JSON string.
    Invalid,
}

/// Decodes a quoted string as `JSON.parse` does: the text from the opening
/// quote to Yarn's end of the token must be exactly one JSON string. Unlike
/// `JSON.parse`, escapes of unpaired surrogates are invalid, so every value
/// is valid UTF-8. Fails only when storage cannot be reserved.
fn decode_json_string(text: &str, decoded: &mut String) -> Result<Quoted, ()> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[bytes.len() - 1] != b'"' {
        return Ok(Quoted::Invalid);
    }
    let content = &text[1..text.len() - 1];
    if !content.contains('\\') {
        let invalid = content.bytes().any(|byte| byte == b'"' || byte < 0x20);
        return Ok(if invalid {
            Quoted::Invalid
        } else {
            Quoted::Plain
        });
    }
    let from = decoded.len();
    let mut chars = content.char_indices();
    let result = loop {
        let Some((index, ch)) = chars.next() else {
            break Quoted::Decoded(from as u32, decoded.len() as u32);
        };
        let value = match ch {
            '"' | '\u{0}'..='\u{1F}' => break Quoted::Invalid,
            '\\' => match chars.next().map(|(_, escape)| escape) {
                Some('"') => '"',
                Some('\\') => '\\',
                Some('/') => '/',
                Some('b') => '\u{8}',
                Some('f') => '\u{C}',
                Some('n') => '\n',
                Some('r') => '\r',
                Some('t') => '\t',
                Some('u') => {
                    let Some(high) = hex4(content.as_bytes(), index + 2) else {
                        break Quoted::Invalid;
                    };
                    chars.nth(3);
                    let code = if (0xD800..0xDC00).contains(&high) {
                        let low = content.as_bytes()[index + 6..]
                            .starts_with(b"\\u")
                            .then(|| hex4(content.as_bytes(), index + 8))
                            .flatten()
                            .filter(|low| (0xDC00..0xE000).contains(low));
                        let Some(low) = low else {
                            break Quoted::Invalid;
                        };
                        chars.nth(5);
                        0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                    } else {
                        high
                    };
                    match char::from_u32(code) {
                        Some(value) => value,
                        None => break Quoted::Invalid,
                    }
                }
                Some(_) | None => break Quoted::Invalid,
            },
            _ => ch,
        };
        if decoded.try_reserve(value.len_utf8()).is_err() {
            decoded.truncate(from);
            return Err(());
        }
        decoded.push(value);
    };
    if matches!(result, Quoted::Invalid) {
        decoded.truncate(from);
    }
    Ok(result)
}

/// Reads four hexadecimal digits at `at`.
fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4)?.iter().try_fold(0, |value, &byte| {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        Some(value << 4 | u32::from(digit))
    })
}

fn error(kind: YarnErrorKind, start: usize, end: usize) -> YarnError {
    YarnError {
        kind,
        span: Span::new(start, end).unwrap_or_default(),
    }
}

/// Whether `ch` is removed by JavaScript's `String.prototype.trim`.
fn is_js_trim(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\u{B}' | '\u{C}' | ' ' | '\u{A0}' | '\u{FEFF}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\n'
                | '\r'
                | '\u{2028}'
                | '\u{2029}'
    )
}

/// Whether a comment names a lockfile format version above 1, which Yarn
/// rejects: its trimmed text is `yarn lockfile v` and decimal digits.
fn names_newer_version(comment: &str) -> bool {
    let Some(digits) = comment
        .trim_matches(is_js_trim)
        .strip_prefix("yarn lockfile v")
    else {
        return false;
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let significant = digits.trim_start_matches('0');
    significant.len() > 1 || significant > "1"
}

/// An object whose members are being parsed.
struct Frame {
    node: u32,
    indent: usize,
    /// Where this object's pending members start.
    pending: usize,
    /// The keys of the member in the parent object that this object is the
    /// value of: the first key index and the key count.
    keys: (u32, u32),
    /// The start of that member.
    start: usize,
}

struct Parser<'s> {
    lexer: Lexer<'s>,
    limits: YarnLimits,
    document: YarnDocument<'s>,
    pending: Vec<Member>,
}

impl Parser<'_> {
    /// The next token that is not a comment, rejecting a comment that
    /// names a newer format version as Yarn does.
    fn next(&mut self) -> Result<Token, YarnError> {
        loop {
            let token = self.lexer.next(&mut self.document.decoded)?;
            if token.kind != TokenKind::Comment {
                return Ok(token);
            }
            if names_newer_version(&self.lexer.text[token.start + 1..token.end]) {
                return Err(error(
                    YarnErrorKind::UnsupportedVersion,
                    token.start,
                    token.end,
                ));
            }
        }
    }

    fn reserve_node(&self, at: &Token) -> Result<(), YarnError> {
        if self.document.node_count() >= self.limits.max_nodes {
            return Err(error(
                YarnErrorKind::NodeLimit {
                    limit: self.limits.max_nodes,
                },
                at.start,
                at.end,
            ));
        }
        Ok(())
    }

    fn push_node(&mut self, node: Node, at: &Token) -> Result<u32, YarnError> {
        self.reserve_node(at)?;
        push(&mut self.document.nodes, node, at)?;
        Ok(self.document.nodes.len() as u32 - 1)
    }

    fn push_key(&mut self, token: &Token) -> Result<(), YarnError> {
        let TokenKind::String(text) = token.kind else {
            return Err(unexpected(token));
        };
        if matches!(text, Text::Source(start, end) | Text::Decoded(start, end) if start == end) {
            return Err(error(YarnErrorKind::EmptyKey, token.start, token.end));
        }
        self.reserve_node(token)?;
        let key = Key {
            text,
            start: token.start as u32,
            end: token.end as u32,
            shadowed: false,
        };
        push(&mut self.document.keys, key, token)
    }

    fn run(&mut self, start: usize) -> Result<(), YarnError> {
        if self.limits.max_depth == 0 {
            return Err(error(
                YarnErrorKind::DepthLimit {
                    limit: self.limits.max_depth,
                },
                start,
                start,
            ));
        }
        let root = Token {
            kind: TokenKind::Eof,
            start,
            end: start,
        };
        let node = Node {
            kind: Kind::Object {
                first: 0,
                len: 0,
                order: 0,
                keys: 0,
            },
            start: start as u32,
            end: self.lexer.bytes.len() as u32,
        };
        self.push_node(node, &root)?;
        let mut frames = vec![Frame {
            node: 0,
            indent: 0,
            pending: 0,
            keys: (0, 0),
            start,
        }];
        let mut token = self.next()?;
        loop {
            let indent = frames.last().map_or(0, |frame| frame.indent);
            // Whether the current object ends here, as Yarn's `break`.
            let ends = match token.kind {
                TokenKind::Newline => {
                    token = self.next()?;
                    if indent == 0 {
                        continue;
                    }
                    if token.kind == TokenKind::Indent(indent) {
                        token = self.next()?;
                        false
                    } else {
                        true
                    }
                }
                TokenKind::Indent(value) => {
                    if value == indent {
                        token = self.next()?;
                        false
                    } else {
                        true
                    }
                }
                TokenKind::Eof => true,
                TokenKind::String(_) => {
                    let first_key = self.document.keys.len() as u32;
                    let member_start = token.start;
                    self.push_key(&token)?;
                    token = self.next()?;
                    while token.kind == TokenKind::Comma {
                        token = self.next()?;
                        match token.kind {
                            TokenKind::String(_) => self.push_key(&token)?,
                            TokenKind::Invalid => {
                                return Err(error(
                                    YarnErrorKind::InvalidToken,
                                    token.start,
                                    token.end,
                                ));
                            }
                            TokenKind::Newline
                            | TokenKind::Comment
                            | TokenKind::Indent(_)
                            | TokenKind::Number
                            | TokenKind::Bool(_)
                            | TokenKind::Colon
                            | TokenKind::Comma
                            | TokenKind::Eof => return Err(unexpected(&token)),
                        }
                        token = self.next()?;
                    }
                    let keys = (first_key, self.document.keys.len() as u32 - first_key);
                    let colon = token.kind == TokenKind::Colon;
                    if colon {
                        token = self.next()?;
                    }
                    let scalar = match token.kind {
                        TokenKind::String(text) => Some(Kind::String(text)),
                        TokenKind::Number => Some(Kind::Number),
                        TokenKind::Bool(true) => Some(Kind::True),
                        TokenKind::Bool(false) => Some(Kind::False),
                        TokenKind::Newline
                        | TokenKind::Comment
                        | TokenKind::Indent(_)
                        | TokenKind::Colon
                        | TokenKind::Comma
                        | TokenKind::Invalid
                        | TokenKind::Eof => None,
                    };
                    if let Some(kind) = scalar {
                        let node = Node {
                            kind,
                            start: token.start as u32,
                            end: token.end as u32,
                        };
                        let value = self.push_node(node, &token)?;
                        self.add_member(keys, value, member_start, token.end, &token)?;
                        token = self.next()?;
                    } else if colon {
                        if frames.len() >= self.limits.max_depth {
                            return Err(error(
                                YarnErrorKind::DepthLimit {
                                    limit: self.limits.max_depth,
                                },
                                token.start,
                                token.end,
                            ));
                        }
                        let node = Node {
                            kind: Kind::Object {
                                first: 0,
                                len: 0,
                                order: 0,
                                keys: 0,
                            },
                            start: token.start as u32,
                            end: token.start as u32,
                        };
                        let node = self.push_node(node, &token)?;
                        push(
                            &mut frames,
                            Frame {
                                node,
                                indent: indent + 1,
                                pending: self.pending.len(),
                                keys,
                                start: member_start,
                            },
                            &token,
                        )?;
                    } else if token.kind == TokenKind::Invalid {
                        return Err(error(YarnErrorKind::InvalidToken, token.start, token.end));
                    } else {
                        return Err(unexpected(&token));
                    }
                    false
                }
                TokenKind::Invalid => {
                    return Err(error(YarnErrorKind::InvalidToken, token.start, token.end));
                }
                TokenKind::Comment
                | TokenKind::Number
                | TokenKind::Bool(_)
                | TokenKind::Colon
                | TokenKind::Comma => return Err(unexpected(&token)),
            };
            if !ends {
                continue;
            }
            // The object ends. The top-level object must end at the end of
            // the input: Yarn ignores everything after top-level indentation.
            if frames.len() == 1 {
                if token.kind != TokenKind::Eof {
                    return Err(error(YarnErrorKind::Indentation, token.start, token.end));
                }
                self.close(0, 0)?;
                return Ok(());
            }
            // Each parent that is not the top-level object ends as well
            // unless indentation follows, as in Yarn's nested `parse`.
            while frames.len() > 1
                && let Some(frame) = frames.pop()
            {
                let end = self.close(frame.node, frame.pending)?;
                self.add_member(frame.keys, frame.node, frame.start, end, &token)?;
                let parent = frames.last().map_or(0, |frame| frame.indent);
                if parent == 0 || matches!(token.kind, TokenKind::Indent(_)) {
                    break;
                }
            }
        }
    }

    fn add_member(
        &mut self,
        keys: (u32, u32),
        value: u32,
        start: usize,
        end: usize,
        at: &Token,
    ) -> Result<(), YarnError> {
        let member = Member {
            first_key: keys.0,
            keys: keys.1,
            value,
            start: start as u32,
            end: end as u32,
        };
        push(&mut self.pending, member, at)
    }

    /// Moves an object's pending members into the arena, sorts its keys,
    /// marks shadowed keys, and returns the end of its span.
    fn close(&mut self, node: u32, pending: usize) -> Result<usize, YarnError> {
        let at = Token {
            kind: TokenKind::Eof,
            start: 0,
            end: 0,
        };
        let first = self.document.members.len() as u32;
        let len = (self.pending.len() - pending) as u32;
        let order = self.document.order.len() as u32;
        let members: Vec<Member> = self.pending.drain(pending..).collect();
        let mut keys = Vec::new();
        for member in &members {
            keys.try_reserve(member.keys as usize)
                .map_err(|_| error(YarnErrorKind::AllocationFailed, 0, 0))?;
            keys.extend(member.first_key..member.first_key + member.keys);
        }
        let document = &self.document;
        keys.sort_by(|&left, &right| {
            document
                .text(document.keys[left as usize].text)
                .cmp(document.text(document.keys[right as usize].text))
                .then(left.cmp(&right))
        });
        for pair in keys.windows(2) {
            if self
                .document
                .text(self.document.keys[pair[0] as usize].text)
                == self
                    .document
                    .text(self.document.keys[pair[1] as usize].text)
            {
                self.document.keys[pair[0] as usize].shadowed = true;
            }
        }
        let key_count = keys.len() as u32;
        for &key in &keys {
            push(&mut self.document.order, key, &at)?;
        }
        let span = members
            .first()
            .zip(members.last())
            .map(|(first, last)| (first.start, last.end));
        for member in members {
            push(&mut self.document.members, member, &at)?;
        }
        let object = &mut self.document.nodes[node as usize];
        object.kind = Kind::Object {
            first,
            len,
            order,
            keys: key_count,
        };
        if node != 0
            && let Some((start, end)) = span
        {
            object.start = start;
            object.end = end;
        }
        if node == 0 {
            let mut shadowed: Vec<u32> = (0..self.document.keys.len() as u32)
                .filter(|&key| self.document.keys[key as usize].shadowed)
                .collect();
            shadowed.sort_unstable();
            self.document.shadowed = shadowed;
        }
        Ok(object.end as usize)
    }
}

fn unexpected(token: &Token) -> YarnError {
    error(YarnErrorKind::UnexpectedToken, token.start, token.end)
}

fn push<T>(vector: &mut Vec<T>, item: T, at: &Token) -> Result<(), YarnError> {
    vector
        .try_reserve(1)
        .map_err(|_| error(YarnErrorKind::AllocationFailed, at.start, at.end))?;
    vector.push(item);
    Ok(())
}

#[cfg(test)]
mod tests;
