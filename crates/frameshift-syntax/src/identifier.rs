use frameshift_core::{SourceText, Span};

use crate::lexical::{LexicalError, LexicalErrorKind, LexicalLimits, error, span, validate};
use crate::unicode::{is_js_identifier_part, is_js_identifier_start};

/// The lexical form of a scanned name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierKind {
    /// An `IdentifierName`, which the parser may treat as a keyword.
    Name,
    /// A `PrivateIdentifier`: `#` followed by an `IdentifierName`.
    PrivateName,
}

/// When an ECMAScript reserved word cannot be used as an identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservedWordClass {
    /// Never an identifier, for example `if`, `enum`, or `null`.
    Always,
    /// `await` or `yield`, which the parser permits in some contexts.
    Contextual,
    /// Reserved only in strict mode code, for example `let` or `static`.
    Strict,
}

macro_rules! define_reserved_words {
    ($($kind:ident => $spelling:literal, $class:ident;)+) => {
        /// An ECMAScript `ReservedWord` or strict-mode reserved word.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ReservedWord {
            $(#[doc = concat!("The `", $spelling, "` reserved word.")]
            $kind,)+
        }

        impl ReservedWord {
            /// Returns the exact ASCII spelling of this word.
            pub const fn spelling(self) -> &'static str {
                match self {
                    $(Self::$kind => $spelling,)+
                }
            }

            /// Returns when this word is unavailable as an identifier.
            pub const fn class(self) -> ReservedWordClass {
                match self {
                    $(Self::$kind => ReservedWordClass::$class,)+
                }
            }
        }

        const WORDS: &[ReservedWord] = &[$(ReservedWord::$kind,)+];
    };
}

// ECMAScript section 12.7.2: ReservedWord, then the strict-mode restrictions.
// Contextual keywords such as `async` or `of` are not reserved and stay with the parser.
define_reserved_words! {
    Await => "await", Contextual;
    Break => "break", Always;
    Case => "case", Always;
    Catch => "catch", Always;
    Class => "class", Always;
    Const => "const", Always;
    Continue => "continue", Always;
    Debugger => "debugger", Always;
    Default => "default", Always;
    Delete => "delete", Always;
    Do => "do", Always;
    Else => "else", Always;
    Enum => "enum", Always;
    Export => "export", Always;
    Extends => "extends", Always;
    False => "false", Always;
    Finally => "finally", Always;
    For => "for", Always;
    Function => "function", Always;
    If => "if", Always;
    Import => "import", Always;
    In => "in", Always;
    Instanceof => "instanceof", Always;
    New => "new", Always;
    Null => "null", Always;
    Return => "return", Always;
    Super => "super", Always;
    Switch => "switch", Always;
    This => "this", Always;
    Throw => "throw", Always;
    True => "true", Always;
    Try => "try", Always;
    Typeof => "typeof", Always;
    Var => "var", Always;
    Void => "void", Always;
    While => "while", Always;
    With => "with", Always;
    Yield => "yield", Contextual;
    Implements => "implements", Strict;
    Interface => "interface", Strict;
    Let => "let", Strict;
    Package => "package", Strict;
    Private => "private", Strict;
    Protected => "protected", Strict;
    Public => "public", Strict;
    Static => "static", Strict;
}

const MAX_RESERVED_WORD_BYTES: usize = {
    let mut max = 0;
    let mut index = 0;
    while index < WORDS.len() {
        if WORDS[index].spelling().len() > max {
            max = WORDS[index].spelling().len();
        }
        index += 1;
    }
    max
};

// Every reserved word is lowercase ASCII and at most 16 bytes long, so its
// bytes packed big-endian into a `u128` form a unique key, and a candidate
// name packs the same way while it is scanned. Sorting the keys at compile
// time turns the lookup into a binary search instead of a comparison with
// every spelling (`D-040`).
const fn reserved_word_key(bytes: &[u8]) -> u128 {
    let mut key = 0;
    let mut index = 0;
    while index < bytes.len() {
        key = (key << 8) | bytes[index] as u128;
        index += 1;
    }
    key
}

const RESERVED_WORD_KEYS: [(u128, ReservedWord); WORDS.len()] = {
    assert!(MAX_RESERVED_WORD_BYTES <= 16);
    let mut table = [(0, WORDS[0]); WORDS.len()];
    let mut index = 0;
    while index < WORDS.len() {
        let spelling = WORDS[index].spelling().as_bytes();
        let mut byte = 0;
        while byte < spelling.len() {
            assert!(spelling[byte].is_ascii_lowercase());
            byte += 1;
        }
        let key = reserved_word_key(spelling);
        let mut position = index;
        while position > 0 && table[position - 1].0 > key {
            table[position] = table[position - 1];
            position -= 1;
        }
        table[position] = (key, WORDS[index]);
        index += 1;
    }
    let mut position = 1;
    while position < table.len() {
        assert!(table[position - 1].0 < table[position].0);
        position += 1;
    }
    table
};

fn reserved_word_for_key(key: u128) -> Option<ReservedWord> {
    RESERVED_WORD_KEYS
        .binary_search_by(|&(candidate, _)| candidate.cmp(&key))
        .ok()
        .map(|index| RESERVED_WORD_KEYS[index].1)
}

impl ReservedWord {
    pub(crate) fn from_spelling(spelling: &str) -> Option<Self> {
        let bytes = spelling.as_bytes();
        if bytes.len() > MAX_RESERVED_WORD_BYTES || !bytes.iter().all(u8::is_ascii_lowercase) {
            return None;
        }
        reserved_word_for_key(reserved_word_key(bytes))
    }
}

/// One identifier name with its original spelling and lexical facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentifierToken {
    /// Whether the token is a plain or private name.
    pub kind: IdentifierKind,
    /// Exact source span, including `#` and escape spellings.
    pub span: Span,
    /// Span of the `IdentifierName`, excluding the `#` of a private name.
    pub name_span: Span,
    /// Whether any code point was spelled with a `\u` escape. ECMAScript
    /// keywords must not contain escapes, even if their StringValue matches.
    pub has_escape: bool,
    /// The reserved word equal to the StringValue, after decoding escapes.
    /// Always `None` for private names, which are never reserved words.
    pub reserved_word: Option<ReservedWord>,
}

/// Scans one ECMAScript `IdentifierName`, or a `PrivateIdentifier` at `#`.
/// Characters are classified with the pinned Unicode tables; `\u` escapes must
/// denote a code point permitted at their position, and any other backslash
/// inside or directly after the name is an error. Names are never normalized.
/// Reserved words are reported from the StringValue, including escaped spellings,
/// without allocation. Whether a name is a keyword, a permitted identifier, or
/// an early error depends on parser context and is not decided here.
/// The caller must select JavaScript context, never JSX names, JSX text, or a
/// hashbang. Offsets are validated first, then the start character, then the
/// byte limit before each consumed scalar. Iterative and linear in consumed bytes.
pub fn scan_js_identifier(
    source: &SourceText,
    offset: usize,
    limits: LexicalLimits,
) -> Result<IdentifierToken, LexicalError> {
    validate(source, offset)?;
    let mut scan = NameScan {
        text: source.as_str(),
        start: offset,
        cursor: offset,
        max_bytes: limits.max_identifier_bytes,
    };
    let kind = if scan.peek() == Some('#') {
        scan.advance('#')?;
        IdentifierKind::PrivateName
    } else {
        IdentifierKind::Name
    };
    let name_start = scan.cursor;
    let mut has_escape = false;
    let mut word = Word::default();
    let bytes = scan.text.as_bytes();
    while let Some(ch) = scan.peek() {
        let first = scan.cursor == name_start;
        // ASCII names, the common case, are classified without table lookups.
        if let Some(&byte) = bytes.get(scan.cursor)
            && byte.is_ascii()
            && byte != b'\\'
        {
            let permitted = byte.is_ascii_alphabetic()
                || byte == b'$'
                || byte == b'_'
                || (!first && byte.is_ascii_digit());
            if !permitted {
                if first {
                    return Err(error(scan.cursor, LexicalErrorKind::ExpectedIdentifier));
                }
                break;
            }
            scan.advance(ch)?;
            word.push(ch);
            continue;
        }
        let code_point = if ch == '\\' {
            let escape = scan.cursor;
            match scan.unicode_escape()? {
                Some(code_point) if permitted(code_point, first) => {
                    has_escape = true;
                    code_point
                }
                Some(_) | None => {
                    return Err(error(escape, LexicalErrorKind::InvalidIdentifierEscape));
                }
            }
        } else if permitted(ch, first) {
            scan.advance(ch)?;
            ch
        } else if first {
            return Err(error(scan.cursor, LexicalErrorKind::ExpectedIdentifier));
        } else {
            break;
        };
        word.push(code_point);
    }
    if scan.cursor == name_start {
        return Err(error(name_start, LexicalErrorKind::ExpectedIdentifier));
    }
    let reserved_word = match kind {
        IdentifierKind::Name => word.reserved_word(),
        IdentifierKind::PrivateName => None,
    };
    Ok(IdentifierToken {
        kind,
        span: span(offset, scan.cursor)?,
        name_span: span(name_start, scan.cursor)?,
        has_escape,
        reserved_word,
    })
}

fn permitted(ch: char, first: bool) -> bool {
    if first {
        is_js_identifier_start(ch)
    } else {
        is_js_identifier_part(ch)
    }
}

// The key of the StringValue scanned so far, which can match a reserved word
// even when spelled with escapes, or `other` once it cannot match any.
#[derive(Default)]
struct Word {
    key: u128,
    len: usize,
    other: bool,
}

impl Word {
    fn push(&mut self, ch: char) {
        match u8::try_from(ch) {
            Ok(byte) if byte.is_ascii_lowercase() && self.len < MAX_RESERVED_WORD_BYTES => {
                self.key = (self.key << 8) | u128::from(byte);
                self.len += 1;
            }
            Ok(_) | Err(_) => self.other = true,
        }
    }

    fn reserved_word(&self) -> Option<ReservedWord> {
        if self.other {
            return None;
        }
        reserved_word_for_key(self.key)
    }
}

struct NameScan<'a> {
    text: &'a str,
    start: usize,
    cursor: usize,
    max_bytes: usize,
}

impl NameScan<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.cursor..].chars().next()
    }

    /// Consumes `ch`, which the caller has just peeked.
    fn advance(&mut self, ch: char) -> Result<(), LexicalError> {
        if self.cursor - self.start + ch.len_utf8() > self.max_bytes {
            return Err(error(self.cursor, LexicalErrorKind::IdentifierLimit));
        }
        self.cursor += ch.len_utf8();
        Ok(())
    }

    fn hex_digit(&mut self) -> Result<Option<u32>, LexicalError> {
        let Some(ch) = self.peek().filter(char::is_ascii_hexdigit) else {
            return Ok(None);
        };
        self.advance(ch)?;
        Ok(ch.to_digit(16))
    }

    // Consumes `\u` with four hex digits or a braced code point. `None` means a
    // malformed escape or a surrogate; the caller reports the backslash.
    fn unicode_escape(&mut self) -> Result<Option<char>, LexicalError> {
        self.advance('\\')?;
        if self.peek() != Some('u') {
            return Ok(None);
        }
        self.advance('u')?;
        let mut value = 0u32;
        if self.peek() == Some('{') {
            self.advance('{')?;
            let mut has_digit = false;
            while let Some(digit) = self.hex_digit()? {
                // Capping keeps long zero runs linear and rejects overflow.
                value = (value * 16 + digit).min(0x11_0000);
                has_digit = true;
            }
            if !has_digit || self.peek() != Some('}') {
                return Ok(None);
            }
            self.advance('}')?;
        } else {
            for _ in 0..4 {
                let Some(digit) = self.hex_digit()? else {
                    return Ok(None);
                };
                value = value * 16 + digit;
            }
        }
        Ok(char::from_u32(value))
    }
}
