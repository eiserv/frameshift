//! JSON and JSONC parsing with exact values (`D-068`).
//!
//! The JSON dialect follows RFC 8259 exactly; the JSONC dialect adds
//! comments and trailing commas and is otherwise identical. Parsing is
//! iterative, so input nesting never consumes stack, and values live in flat
//! arenas with exact source spans. Numbers keep their source text, strings
//! are decoded during parsing, and objects keep every member, so duplicate
//! keys stay visible.

use std::cmp::Ordering;
use std::fmt;

use frameshift_core::{SourceText, Span};

/// Marks a string node whose value is its source content, without escapes.
const NOT_DECODED: u32 = u32::MAX;

/// The JSON variant that a document is parsed as. The caller selects it
/// from the file's role; the parser never infers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonDialect {
    /// RFC 8259, for `package.json` and `package-lock.json`.
    Json,
    /// JSON with `//` and `/* */` comments and one trailing comma after the
    /// last element or member, for `tsconfig.json`, `jsconfig.json`, and
    /// `bun.lock`.
    Jsonc,
}

/// Explicit resource limits for one JSON parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonLimits {
    /// Maximum nesting of arrays and objects, default 256. The root
    /// container has depth 1.
    pub max_depth: usize,
    /// Maximum number of values and object keys, default 4,194,304.
    pub max_nodes: usize,
}

impl Default for JsonLimits {
    fn default() -> Self {
        Self {
            max_depth: 256,
            max_nodes: 4_194_304,
        }
    }
}

/// What the parser expected where it found something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonExpected {
    /// A value.
    Value,
    /// A string key, or `}` directly after `{`.
    Key,
    /// The `:` after a key.
    Colon,
    /// `,` or `]` after an array element.
    ArrayContinuation,
    /// `,` or `}` after an object member.
    ObjectContinuation,
    /// The end of the input after the top-level value.
    End,
}

impl JsonExpected {
    const fn describe(self) -> &'static str {
        match self {
            Self::Value => "a value",
            Self::Key => "a string key",
            Self::Colon => "':'",
            Self::ArrayContinuation => "',' or ']'",
            Self::ObjectContinuation => "',' or '}'",
            Self::End => "the end of the input",
        }
    }
}

/// Why a document was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonErrorKind {
    /// A character that the grammar does not allow here.
    UnexpectedCharacter {
        /// What the grammar allows here.
        expected: JsonExpected,
    },
    /// The input ended early.
    UnexpectedEnd {
        /// What the grammar requires here.
        expected: JsonExpected,
    },
    /// A number that does not follow the RFC 8259 grammar, such as `01`,
    /// `1.`, `.5`, or `-`.
    InvalidNumber,
    /// A backslash escape other than the eight short escapes and `\u` with
    /// four hexadecimal digits.
    InvalidEscape,
    /// A `\u` escape of a surrogate that is not part of a high and low pair.
    LoneSurrogate,
    /// A raw control character (U+0000 to U+001F) in a string.
    ControlCharacter,
    /// A string without its closing quote.
    UnterminatedString,
    /// A `/*` comment without its closing `*/`.
    UnterminatedComment,
    /// A comment in the JSON dialect.
    CommentNotAllowed,
    /// A comma before `]` or `}` in the JSON dialect.
    TrailingCommaNotAllowed,
    /// Arrays and objects nest deeper than the limit.
    DepthLimit {
        /// The configured maximum depth.
        limit: usize,
    },
    /// The document has more values and keys than the limit.
    NodeLimit {
        /// The configured maximum node count.
        limit: usize,
    },
    /// Storage could not be reserved.
    AllocationFailed,
}

/// A rejected document. It names a kind and a span, never source content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonError {
    kind: JsonErrorKind,
    span: Span,
}

impl JsonError {
    /// Returns why the document was rejected.
    pub const fn kind(self) -> JsonErrorKind {
        self.kind
    }

    /// Returns the source bytes at fault, at character boundaries. An empty
    /// span at the end marks input that ended early.
    pub const fn span(self) -> Span {
        self.span
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            JsonErrorKind::UnexpectedCharacter { expected } => {
                write!(f, "unexpected character, expected {}", expected.describe())?;
            }
            JsonErrorKind::UnexpectedEnd { expected } => {
                write!(
                    f,
                    "unexpected end of input, expected {}",
                    expected.describe()
                )?;
            }
            JsonErrorKind::InvalidNumber => f.write_str("invalid number")?,
            JsonErrorKind::InvalidEscape => f.write_str("invalid escape sequence")?,
            JsonErrorKind::LoneSurrogate => f.write_str("unpaired surrogate escape")?,
            JsonErrorKind::ControlCharacter => f.write_str("unescaped control character")?,
            JsonErrorKind::UnterminatedString => f.write_str("unterminated string")?,
            JsonErrorKind::UnterminatedComment => f.write_str("unterminated comment")?,
            JsonErrorKind::CommentNotAllowed => f.write_str("comments are not allowed in JSON")?,
            JsonErrorKind::TrailingCommaNotAllowed => {
                f.write_str("trailing commas are not allowed in JSON")?;
            }
            JsonErrorKind::DepthLimit { limit } => {
                write!(f, "nesting exceeds the limit of {limit} (json.max-depth)")?;
            }
            JsonErrorKind::NodeLimit { limit } => {
                write!(
                    f,
                    "the document exceeds {limit} values and keys (json.max-nodes)"
                )?;
            }
            JsonErrorKind::AllocationFailed => f.write_str("JSON storage allocation failed")?,
        }
        write!(f, " at bytes {}..{}", self.span.start(), self.span.end())
    }
}

impl std::error::Error for JsonError {}

/// Identifies a value or key in one [`JsonDocument`]. Identities ascend in
/// source order of the values' starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsonId(u32);

impl JsonId {
    /// Returns the zero-based position in the document's node arena.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Null,
    True,
    False,
    Number,
    String { decoded: u32 },
    Array { first: u32, len: u32 },
    Object { first: u32, len: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node {
    kind: Kind,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Member {
    key: u32,
    value: u32,
    shadowed: bool,
}

/// A parsed document that borrows its source text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonDocument<'s> {
    source: &'s str,
    nodes: Vec<Node>,
    elements: Vec<u32>,
    members: Vec<Member>,
    /// Per object, member offsets sorted by decoded key, then by position.
    order: Vec<u32>,
    decoded: String,
    ranges: Vec<(u32, u32)>,
    /// Shadowed members, in source order.
    shadowed: Vec<u32>,
    root: u32,
    bom: bool,
}

impl<'s> JsonDocument<'s> {
    /// Returns the top-level value.
    pub const fn root(&self) -> JsonId {
        JsonId(self.root)
    }

    /// Returns whether the source starts with a UTF-8 byte order mark,
    /// which the parser skipped.
    pub const fn has_bom(&self) -> bool {
        self.bom
    }

    /// Returns the number of values and keys.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterates over every value and key identity, in source order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = JsonId> {
        (0..self.nodes.len() as u32).map(JsonId)
    }

    /// Returns the source span of a value or key, including quotes and brackets.
    pub fn span(&self, id: JsonId) -> Option<Span> {
        let node = self.nodes.get(id.index())?;
        Span::new(node.start as usize, node.end as usize).ok()
    }

    /// Returns a value or key.
    pub fn value(&self, id: JsonId) -> Option<JsonValue<'_>> {
        let node = self.nodes.get(id.index())?;
        Some(match node.kind {
            Kind::Null => JsonValue::Null,
            Kind::True => JsonValue::Bool(true),
            Kind::False => JsonValue::Bool(false),
            Kind::Number => JsonValue::Number(JsonNumber {
                text: &self.source[node.start as usize..node.end as usize],
            }),
            Kind::String { .. } => JsonValue::String(self.string(id.0)),
            Kind::Array { first, len } => JsonValue::Array(JsonArray {
                elements: &self.elements[first as usize..(first + len) as usize],
            }),
            Kind::Object { first, len } => JsonValue::Object(JsonObject {
                document: self,
                first: first as usize,
                len: len as usize,
            }),
        })
    }

    /// Lists every member whose key occurs again later in the same object,
    /// in source order. Lookups return the last occurrence.
    pub fn duplicate_keys(&self) -> impl ExactSizeIterator<Item = JsonMember<'_>> {
        self.shadowed
            .iter()
            .map(|&member| self.member(member as usize))
    }

    fn string(&self, node: u32) -> &str {
        let node = self.nodes[node as usize];
        match node.kind {
            Kind::String { decoded } if decoded == NOT_DECODED => {
                &self.source[node.start as usize + 1..node.end as usize - 1]
            }
            Kind::String { decoded } => {
                let (start, end) = self.ranges[decoded as usize];
                &self.decoded[start as usize..end as usize]
            }
            Kind::Null
            | Kind::True
            | Kind::False
            | Kind::Number
            | Kind::Array { .. }
            | Kind::Object { .. } => "",
        }
    }

    fn member(&self, index: usize) -> JsonMember<'_> {
        let member = self.members[index];
        let key = self.nodes[member.key as usize];
        JsonMember {
            key: self.string(member.key),
            key_span: Span::new(key.start as usize, key.end as usize).unwrap_or_default(),
            key_id: JsonId(member.key),
            value: JsonId(member.value),
            shadowed: member.shadowed,
        }
    }
}

/// A value of a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonValue<'a> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number with its exact source text.
    Number(JsonNumber<'a>),
    /// A decoded string, always valid UTF-8.
    String(&'a str),
    /// An array.
    Array(JsonArray<'a>),
    /// An object.
    Object(JsonObject<'a>),
}

impl<'a> JsonValue<'a> {
    /// Returns the string, if this is one.
    pub const fn as_str(self) -> Option<&'a str> {
        match self {
            Self::String(text) => Some(text),
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::Array(_) | Self::Object(_) => None,
        }
    }

    /// Returns the boolean, if this is one.
    pub const fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(value),
            Self::Null | Self::Number(_) | Self::String(_) | Self::Array(_) | Self::Object(_) => {
                None
            }
        }
    }

    /// Returns the number, if this is one.
    pub const fn as_number(self) -> Option<JsonNumber<'a>> {
        match self {
            Self::Number(number) => Some(number),
            Self::Null | Self::Bool(_) | Self::String(_) | Self::Array(_) | Self::Object(_) => None,
        }
    }

    /// Returns the array, if this is one.
    pub const fn as_array(self) -> Option<JsonArray<'a>> {
        match self {
            Self::Array(array) => Some(array),
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::String(_) | Self::Object(_) => {
                None
            }
        }
    }

    /// Returns the object, if this is one.
    pub const fn as_object(self) -> Option<JsonObject<'a>> {
        match self {
            Self::Object(object) => Some(object),
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::String(_) | Self::Array(_) => None,
        }
    }
}

/// A number with its exact source text, which follows the RFC 8259 grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonNumber<'a> {
    text: &'a str,
}

impl<'a> JsonNumber<'a> {
    /// Borrows the exact source text, such as `-0`, `1.50`, or `2E+3`.
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// Returns whether the text has neither a fraction nor an exponent.
    pub fn is_integer(self) -> bool {
        !self.text.contains(['.', 'e', 'E'])
    }

    /// Converts an integer without fraction, exponent, or sign to `u64`, or
    /// returns `None`. `-0` is not converted.
    pub fn as_u64(self) -> Option<u64> {
        if !self.is_integer() || self.text.starts_with('-') {
            return None;
        }
        self.text.parse().ok()
    }

    /// Converts an integer without fraction or exponent to `i64`, or returns
    /// `None`. `-0` converts to 0.
    pub fn as_i64(self) -> Option<i64> {
        if !self.is_integer() {
            return None;
        }
        self.text.parse().ok()
    }
}

/// The elements of an array, in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonArray<'a> {
    elements: &'a [u32],
}

impl<'a> JsonArray<'a> {
    /// Returns the number of elements.
    pub const fn len(self) -> usize {
        self.elements.len()
    }

    /// Returns whether the array is empty.
    pub const fn is_empty(self) -> bool {
        self.elements.is_empty()
    }

    /// Returns the element at a position.
    pub fn get(self, index: usize) -> Option<JsonId> {
        self.elements.get(index).copied().map(JsonId)
    }

    /// Iterates over the elements in source order.
    pub fn iter(self) -> impl ExactSizeIterator<Item = JsonId> + 'a {
        self.elements.iter().copied().map(JsonId)
    }
}

/// The members of an object, in source order, including shadowed ones.
#[derive(Clone, Copy, Debug)]
pub struct JsonObject<'a> {
    document: &'a JsonDocument<'a>,
    first: usize,
    len: usize,
}

impl PartialEq for JsonObject<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.document, other.document)
            && self.first == other.first
            && self.len == other.len
    }
}

impl Eq for JsonObject<'_> {}

impl<'a> JsonObject<'a> {
    /// Returns the number of members, including shadowed ones.
    pub const fn len(self) -> usize {
        self.len
    }

    /// Returns whether the object has no members.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Returns the value of the last member with this decoded key, as
    /// `JSON.parse` does.
    pub fn get(self, key: &str) -> Option<JsonId> {
        let order = &self.document.order[self.first..self.first + self.len];
        let key_of = |offset: u32| {
            let member = self.document.members[self.first + offset as usize];
            self.document.string(member.key)
        };
        let after = order.partition_point(|&offset| key_of(offset) <= key);
        let last = *order.get(after.checked_sub(1)?)?;
        (key_of(last) == key)
            .then(|| JsonId(self.document.members[self.first + last as usize].value))
    }

    /// Returns the member at a source position.
    pub fn member(self, index: usize) -> Option<JsonMember<'a>> {
        (index < self.len).then(|| self.document.member(self.first + index))
    }

    /// Iterates over the members in source order, including shadowed ones.
    pub fn members(self) -> impl ExactSizeIterator<Item = JsonMember<'a>> + 'a {
        let document = self.document;
        (self.first..self.first + self.len).map(move |index| document.member(index))
    }
}

/// One member of an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonMember<'a> {
    key: &'a str,
    key_span: Span,
    key_id: JsonId,
    value: JsonId,
    shadowed: bool,
}

impl<'a> JsonMember<'a> {
    /// Borrows the decoded key.
    pub const fn key(self) -> &'a str {
        self.key
    }

    /// Returns the source span of the key, including its quotes.
    pub const fn key_span(self) -> Span {
        self.key_span
    }

    /// Returns the identity of the key's string node.
    pub const fn key_id(self) -> JsonId {
        self.key_id
    }

    /// Returns the member's value.
    pub const fn value(self) -> JsonId {
        self.value
    }

    /// Returns whether a later member of the same object has the same key.
    pub const fn is_shadowed(self) -> bool {
        self.shadowed
    }
}

/// Parses one complete JSON or JSONC document.
///
/// A leading UTF-8 byte order mark is skipped. Spans refer to the original
/// source, including the mark. Content after the top-level value is an error.
pub fn parse_json(
    source: &SourceText,
    dialect: JsonDialect,
    limits: JsonLimits,
) -> Result<JsonDocument<'_>, JsonError> {
    let text = source.as_str();
    let bom = text.starts_with('\u{FEFF}');
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: if bom { '\u{FEFF}'.len_utf8() } else { 0 },
        dialect,
        limits,
        document: JsonDocument {
            source: text,
            nodes: Vec::new(),
            elements: Vec::new(),
            members: Vec::new(),
            order: Vec::new(),
            decoded: String::new(),
            ranges: Vec::new(),
            shadowed: Vec::new(),
            root: 0,
            bom,
        },
        frames: Vec::new(),
        pending_elements: Vec::new(),
        pending_members: Vec::new(),
        comma: 0,
    };
    parser.run()?;
    let mut document = parser.document;
    let members = &document.members;
    document
        .shadowed
        .sort_unstable_by_key(|&member| members[member as usize].key);
    Ok(document)
}

#[derive(Clone, Copy)]
enum State {
    Value,
    ArrayFirst,
    ArrayAfterComma,
    ArrayNext,
    ObjectFirst,
    ObjectAfterComma,
    ObjectColon,
    ObjectNext,
    End,
}

struct Frame {
    node: u32,
    array: bool,
    /// Where this container's pending elements or members start.
    pending: usize,
    /// The key of the member being parsed.
    key: u32,
}

struct Parser<'s> {
    text: &'s str,
    bytes: &'s [u8],
    pos: usize,
    dialect: JsonDialect,
    limits: JsonLimits,
    document: JsonDocument<'s>,
    frames: Vec<Frame>,
    pending_elements: Vec<u32>,
    pending_members: Vec<(u32, u32)>,
    /// The offset of the last comma, for trailing-comma errors.
    comma: usize,
}

fn push<T>(vector: &mut Vec<T>, item: T, at: Span) -> Result<(), JsonError> {
    vector.try_reserve(1).map_err(|_| JsonError {
        kind: JsonErrorKind::AllocationFailed,
        span: at,
    })?;
    vector.push(item);
    Ok(())
}

/// Reads four hexadecimal digits at `at`.
fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)?
        .iter()
        .try_fold(0, |value, &byte| Some(value << 4 | hex(byte)?))
}

const fn hex(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'f' => Some((byte - b'a' + 10) as u32),
        b'A'..=b'F' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}

impl Parser<'_> {
    fn run(&mut self) -> Result<(), JsonError> {
        let mut state = State::Value;
        loop {
            self.trivia()?;
            let byte = self.bytes.get(self.pos).copied();
            state = match state {
                State::End => match byte {
                    None => return Ok(()),
                    Some(_) => return Err(self.unexpected(JsonExpected::End)),
                },
                State::Value => self.value()?,
                State::ArrayFirst if byte == Some(b']') => self.close()?,
                State::ArrayAfterComma if byte == Some(b']') => {
                    self.trailing_comma()?;
                    self.close()?
                }
                State::ArrayFirst | State::ArrayAfterComma => self.value()?,
                State::ArrayNext => match byte {
                    Some(b',') => {
                        self.comma = self.pos;
                        self.pos += 1;
                        State::ArrayAfterComma
                    }
                    Some(b']') => self.close()?,
                    _ => return Err(self.unexpected(JsonExpected::ArrayContinuation)),
                },
                State::ObjectFirst if byte == Some(b'}') => self.close()?,
                State::ObjectAfterComma if byte == Some(b'}') => {
                    self.trailing_comma()?;
                    self.close()?
                }
                State::ObjectFirst | State::ObjectAfterComma => self.key()?,
                State::ObjectColon => match byte {
                    Some(b':') => {
                        self.pos += 1;
                        State::Value
                    }
                    _ => return Err(self.unexpected(JsonExpected::Colon)),
                },
                State::ObjectNext => match byte {
                    Some(b',') => {
                        self.comma = self.pos;
                        self.pos += 1;
                        State::ObjectAfterComma
                    }
                    Some(b'}') => self.close()?,
                    _ => return Err(self.unexpected(JsonExpected::ObjectContinuation)),
                },
            };
        }
    }

    fn error(&self, kind: JsonErrorKind, start: usize, end: usize) -> JsonError {
        JsonError {
            kind,
            span: Span::new(start, end).unwrap_or_default(),
        }
    }

    /// The span of the character at `at`, or an empty span at the end.
    fn character(&self, at: usize) -> (usize, usize) {
        let width = self.text[at..].chars().next().map_or(0, char::len_utf8);
        (at, at + width)
    }

    fn unexpected(&self, expected: JsonExpected) -> JsonError {
        self.unexpected_at(self.pos, expected)
    }

    fn unexpected_at(&self, at: usize, expected: JsonExpected) -> JsonError {
        if at >= self.bytes.len() {
            let end = self.bytes.len();
            return self.error(JsonErrorKind::UnexpectedEnd { expected }, end, end);
        }
        let (start, end) = self.character(at);
        self.error(JsonErrorKind::UnexpectedCharacter { expected }, start, end)
    }

    fn trailing_comma(&self) -> Result<(), JsonError> {
        match self.dialect {
            JsonDialect::Jsonc => Ok(()),
            JsonDialect::Json => Err(self.error(
                JsonErrorKind::TrailingCommaNotAllowed,
                self.comma,
                self.comma + 1,
            )),
        }
    }

    /// Skips whitespace and, in JSONC, comments.
    fn trivia(&mut self) -> Result<(), JsonError> {
        loop {
            match self.bytes.get(self.pos) {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') => {
                    let start = self.pos;
                    let block = match self.bytes.get(start + 1) {
                        Some(b'/') => false,
                        Some(b'*') => true,
                        _ => return Ok(()),
                    };
                    if self.dialect == JsonDialect::Json {
                        return Err(self.error(JsonErrorKind::CommentNotAllowed, start, start + 2));
                    }
                    let body = start + 2;
                    if block {
                        let Some(close) = self.text[body..].find("*/") else {
                            let end = self.bytes.len();
                            return Err(self.error(JsonErrorKind::UnterminatedComment, start, end));
                        };
                        self.pos = body + close + 2;
                    } else {
                        self.pos = self.bytes[body..]
                            .iter()
                            .position(|&byte| byte == b'\n' || byte == b'\r')
                            .map_or(self.bytes.len(), |offset| body + offset);
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn alloc(&mut self, kind: Kind, start: usize, end: usize) -> Result<u32, JsonError> {
        if self.document.nodes.len() >= self.limits.max_nodes {
            let limit = self.limits.max_nodes;
            return Err(self.error(JsonErrorKind::NodeLimit { limit }, start, end));
        }
        let id = self.document.nodes.len() as u32;
        let node = Node {
            kind,
            start: start as u32,
            end: end as u32,
        };
        let at = Span::new(start, end).unwrap_or_default();
        push(&mut self.document.nodes, node, at)?;
        Ok(id)
    }

    /// Records a finished value in its container and returns the next state.
    fn complete(&mut self, id: u32) -> Result<State, JsonError> {
        let at = Span::new(self.pos, self.pos).unwrap_or_default();
        match self.frames.last() {
            None => {
                self.document.root = id;
                Ok(State::End)
            }
            Some(frame) if frame.array => {
                push(&mut self.pending_elements, id, at)?;
                Ok(State::ArrayNext)
            }
            Some(frame) => {
                let key = frame.key;
                push(&mut self.pending_members, (key, id), at)?;
                Ok(State::ObjectNext)
            }
        }
    }

    fn value(&mut self) -> Result<State, JsonError> {
        let start = self.pos;
        match self.bytes.get(start) {
            Some(b'{') => self.open(false),
            Some(b'[') => self.open(true),
            Some(b'"') => {
                let id = self.string()?;
                self.complete(id)
            }
            Some(b't') => self.literal("true", Kind::True),
            Some(b'f') => self.literal("false", Kind::False),
            Some(b'n') => self.literal("null", Kind::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.unexpected(JsonExpected::Value)),
        }
    }

    fn key(&mut self) -> Result<State, JsonError> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(self.unexpected(JsonExpected::Key));
        }
        let id = self.string()?;
        if let Some(frame) = self.frames.last_mut() {
            frame.key = id;
        }
        Ok(State::ObjectColon)
    }

    fn open(&mut self, array: bool) -> Result<State, JsonError> {
        let start = self.pos;
        if self.frames.len() >= self.limits.max_depth {
            let limit = self.limits.max_depth;
            return Err(self.error(JsonErrorKind::DepthLimit { limit }, start, start + 1));
        }
        let placeholder = if array {
            Kind::Array { first: 0, len: 0 }
        } else {
            Kind::Object { first: 0, len: 0 }
        };
        let node = self.alloc(placeholder, start, start + 1)?;
        let pending = if array {
            self.pending_elements.len()
        } else {
            self.pending_members.len()
        };
        let frame = Frame {
            node,
            array,
            pending,
            key: 0,
        };
        push(
            &mut self.frames,
            frame,
            Span::new(start, start + 1).unwrap_or_default(),
        )?;
        self.pos += 1;
        Ok(if array {
            State::ArrayFirst
        } else {
            State::ObjectFirst
        })
    }

    fn close(&mut self) -> Result<State, JsonError> {
        self.pos += 1;
        let at = Span::new(self.pos - 1, self.pos).unwrap_or_default();
        let Some(frame) = self.frames.pop() else {
            return Err(self.error(JsonErrorKind::AllocationFailed, at.start(), at.end()));
        };
        let document = &mut self.document;
        let kind = if frame.array {
            let first = document.elements.len();
            let len = self.pending_elements.len() - frame.pending;
            document.elements.try_reserve(len).map_err(|_| JsonError {
                kind: JsonErrorKind::AllocationFailed,
                span: at,
            })?;
            document
                .elements
                .extend(self.pending_elements.drain(frame.pending..));
            Kind::Array {
                first: first as u32,
                len: len as u32,
            }
        } else {
            let first = document.members.len();
            let len = self.pending_members.len() - frame.pending;
            let reserved = document.members.try_reserve(len).is_ok()
                && document.order.try_reserve(len).is_ok();
            let mut order: Vec<u32> = Vec::new();
            if !reserved || order.try_reserve_exact(len).is_err() {
                return Err(JsonError {
                    kind: JsonErrorKind::AllocationFailed,
                    span: at,
                });
            }
            document
                .members
                .extend(
                    self.pending_members
                        .drain(frame.pending..)
                        .map(|(key, value)| Member {
                            key,
                            value,
                            shadowed: false,
                        }),
                );
            order.extend(0..len as u32);
            let members = &document.members[first..];
            let key = |offset: u32| document.string(members[offset as usize].key);
            order.sort_unstable_by(|&left, &right| match key(left).cmp(key(right)) {
                Ordering::Equal => left.cmp(&right),
                unequal @ (Ordering::Less | Ordering::Greater) => unequal,
            });
            let mut shadowed = Vec::new();
            for pair in order.windows(2) {
                if key(pair[0]) == key(pair[1]) {
                    push(&mut shadowed, first as u32 + pair[0], at)?;
                }
            }
            for &member in &shadowed {
                document.members[member as usize].shadowed = true;
            }
            document
                .shadowed
                .try_reserve(shadowed.len())
                .map_err(|_| JsonError {
                    kind: JsonErrorKind::AllocationFailed,
                    span: at,
                })?;
            document.shadowed.extend(shadowed);
            document.order.extend(order);
            Kind::Object {
                first: first as u32,
                len: len as u32,
            }
        };
        let node = &mut document.nodes[frame.node as usize];
        node.kind = kind;
        node.end = self.pos as u32;
        self.complete(frame.node)
    }

    fn literal(&mut self, word: &str, kind: Kind) -> Result<State, JsonError> {
        let start = self.pos;
        let rest = &self.bytes[start..];
        if rest.starts_with(word.as_bytes()) {
            let end = start + word.len();
            let id = self.alloc(kind, start, end)?;
            self.pos = end;
            return self.complete(id);
        }
        let matched = rest
            .iter()
            .zip(word.as_bytes())
            .take_while(|(left, right)| left == right)
            .count();
        Err(self.unexpected_at(start + matched, JsonExpected::Value))
    }

    fn number(&mut self) -> Result<State, JsonError> {
        let start = self.pos;
        let digit = |at: usize| self.bytes.get(at).is_some_and(u8::is_ascii_digit);
        let invalid = |end: usize| {
            let end = if end < self.bytes.len() {
                self.character(end).1
            } else {
                end
            };
            self.error(JsonErrorKind::InvalidNumber, start, end)
        };
        let mut at = start;
        if self.bytes[at] == b'-' {
            at += 1;
        }
        match self.bytes.get(at) {
            Some(b'0') => {
                at += 1;
                if digit(at) {
                    return Err(invalid(at));
                }
            }
            Some(b'1'..=b'9') => {
                while digit(at) {
                    at += 1;
                }
            }
            _ => return Err(invalid(at)),
        }
        if self.bytes.get(at) == Some(&b'.') {
            at += 1;
            if !digit(at) {
                return Err(invalid(at));
            }
            while digit(at) {
                at += 1;
            }
        }
        if matches!(self.bytes.get(at), Some(b'e' | b'E')) {
            at += 1;
            if matches!(self.bytes.get(at), Some(b'+' | b'-')) {
                at += 1;
            }
            if !digit(at) {
                return Err(invalid(at));
            }
            while digit(at) {
                at += 1;
            }
        }
        let id = self.alloc(Kind::Number, start, at)?;
        self.pos = at;
        self.complete(id)
    }

    /// Validates the escape at `at`, a backslash, and returns the offset after it.
    fn escape(&self, start: usize, at: usize) -> Result<usize, JsonError> {
        match self.bytes.get(at + 1) {
            Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => Ok(at + 2),
            Some(b'u') => {
                let Some(unit) = hex4(self.bytes, at + 2) else {
                    return Err(self.error(JsonErrorKind::InvalidEscape, at, at + 2));
                };
                let lone = || self.error(JsonErrorKind::LoneSurrogate, at, at + 6);
                match unit {
                    0xD800..=0xDBFF => {
                        let paired = self.bytes.get(at + 6..at + 8) == Some(b"\\u")
                            && hex4(self.bytes, at + 8)
                                .is_some_and(|low| (0xDC00..=0xDFFF).contains(&low));
                        if paired { Ok(at + 12) } else { Err(lone()) }
                    }
                    0xDC00..=0xDFFF => Err(lone()),
                    _ => Ok(at + 6),
                }
            }
            Some(_) => {
                let (_, end) = self.character(at + 1);
                Err(self.error(JsonErrorKind::InvalidEscape, at, end))
            }
            None => {
                let end = self.bytes.len();
                Err(self.error(JsonErrorKind::UnterminatedString, start, end))
            }
        }
    }

    fn string(&mut self) -> Result<u32, JsonError> {
        let start = self.pos;
        let mut at = start + 1;
        let mut escaped = false;
        loop {
            match self.bytes.get(at) {
                None => {
                    let end = self.bytes.len();
                    return Err(self.error(JsonErrorKind::UnterminatedString, start, end));
                }
                Some(b'"') => break,
                Some(b'\\') => {
                    escaped = true;
                    at = self.escape(start, at)?;
                }
                Some(&byte) if byte < 0x20 => {
                    return Err(self.error(JsonErrorKind::ControlCharacter, at, at + 1));
                }
                Some(_) => at += 1,
            }
        }
        let end = at + 1;
        let decoded = if escaped {
            self.decode(start + 1, at)?
        } else {
            NOT_DECODED
        };
        let id = self.alloc(Kind::String { decoded }, start, end)?;
        self.pos = end;
        Ok(id)
    }

    /// Decodes validated string content between `start` and `end`.
    fn decode(&mut self, start: usize, end: usize) -> Result<u32, JsonError> {
        let failed = JsonError {
            kind: JsonErrorKind::AllocationFailed,
            span: Span::new(start, end).unwrap_or_default(),
        };
        let all = self.bytes;
        let raw = &self.text[start..end];
        let output = &mut self.document.decoded;
        output.try_reserve(raw.len()).map_err(|_| failed)?;
        let first = output.len();
        let bytes = raw.as_bytes();
        let mut at = 0;
        while let Some(offset) = raw[at..].find('\\') {
            output.push_str(&raw[at..at + offset]);
            at += offset;
            let decoded = match bytes[at + 1] {
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let unit = hex4(all, start + at + 2).ok_or(failed)?;
                    let scalar = if (0xD800..=0xDBFF).contains(&unit) {
                        let low = hex4(all, start + at + 8).ok_or(failed)?;
                        at += 6;
                        0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                    } else {
                        unit
                    };
                    at += 4;
                    char::from_u32(scalar).ok_or(failed)?
                }
                other => char::from(other),
            };
            output.push(decoded);
            at += 2;
        }
        output.push_str(&raw[at..]);
        let range = (first as u32, output.len() as u32);
        let index = self.document.ranges.len() as u32;
        push(&mut self.document.ranges, range, failed.span)?;
        Ok(index)
    }
}

#[cfg(test)]
mod tests;
