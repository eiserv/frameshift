//! Validation of regular-expression patterns: ECMAScript section 22.2.1 in
//! Unicode and UnicodeSets mode, and Annex B.1.2 without either flag.
//! The parser is iterative, so nesting depth cannot exhaust the call stack.

use frameshift_core::Span;

use crate::lexical::{LexicalError, LexicalErrorKind, error};
use crate::lookup::LookupMap;
use crate::regexp::{RegExpFlag, RegExpFlags};
use crate::regexp_property::{PropertyKind, classify};
use crate::unicode::{is_js_identifier_part, is_js_identifier_start};

/// Reason a pattern violates the grammar of ECMAScript section 22.2.1, its
/// early errors, or the Annex B.1.2 grammar selected without `u` or `v`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegExpPatternError {
    /// A `)` closes no group.
    UnmatchedParenthesis,
    /// A group is not closed before the end of the pattern; points to its `(`.
    UnterminatedGroup,
    /// `(?` is not followed by a valid group form; points to the `(`.
    InvalidGroup,
    /// A modifier group repeats a flag, adds and removes the same flag, or
    /// has a `-` with no flags; points to the `(`.
    InvalidModifiers,
    /// A group name is empty, unterminated, or contains a code point or escape
    /// that is not permitted at its position.
    InvalidGroupName,
    /// Two groups with the same name can both participate in a match; points
    /// to the later group's `(`.
    DuplicateGroupName,
    /// A `\k<name>` escape names no group in the pattern.
    UnresolvedGroupName,
    /// The pattern has at least 2^32 - 1 capturing groups.
    TooManyCapturingGroups,
    /// A quantifier follows nothing, another quantifier, or an assertion that
    /// cannot be quantified.
    NothingToRepeat,
    /// A `{n,m}` quantifier has `n` greater than `m`.
    InvalidQuantifierRange,
    /// A `{`, `}`, or `]` outside a quantifier or class in Unicode mode.
    LoneSyntaxCharacter,
    /// An escape is malformed or not permitted in this mode and context.
    InvalidEscape,
    /// A decimal escape in Unicode mode exceeds the number of capturing groups.
    InvalidBackreference,
    /// A property escape is malformed, names no property or value, or names a
    /// property of strings outside UnicodeSets mode.
    InvalidPropertyEscape,
    /// A negated class or `\P` escape could match strings.
    NegatedClassStrings,
    /// A class range is out of order, or has a class escape as an endpoint in
    /// Unicode mode; points to the range's first endpoint.
    InvalidClassRange,
    /// A nested UnicodeSets class is not closed; points to its `[`.
    UnterminatedCharacterClass,
    /// A UnicodeSets class contains an unescaped syntax character, a reserved
    /// double punctuator, a missing operand, or mixed set operators.
    InvalidClassSetSyntax,
}

impl RegExpPatternError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::UnmatchedParenthesis => "unmatched parenthesis",
            Self::UnterminatedGroup => "unterminated group",
            Self::InvalidGroup => "invalid group",
            Self::InvalidModifiers => "invalid group modifiers",
            Self::InvalidGroupName => "invalid group name",
            Self::DuplicateGroupName => "duplicate group name",
            Self::UnresolvedGroupName => "reference to an undefined group name",
            Self::TooManyCapturingGroups => "too many capturing groups",
            Self::NothingToRepeat => "nothing to repeat",
            Self::InvalidQuantifierRange => "quantifier range out of order",
            Self::LoneSyntaxCharacter => "lone syntax character",
            Self::InvalidEscape => "invalid escape",
            Self::InvalidBackreference => "backreference to a missing group",
            Self::InvalidPropertyEscape => "invalid property escape",
            Self::NegatedClassStrings => "negated class may contain strings",
            Self::InvalidClassRange => "invalid class range",
            Self::UnterminatedCharacterClass => "unterminated character class",
            Self::InvalidClassSetSyntax => "invalid class set syntax",
        }
    }
}

/// Validates the pattern in `body` under the grammar selected by `flags` and
/// returns its number of capturing groups.
pub(crate) fn validate_pattern(
    text: &str,
    body: Span,
    flags: RegExpFlags,
) -> Result<u32, LexicalError> {
    let unicode = flags.is_unicode_mode();
    let mode = Mode {
        unicode,
        sets: flags.contains(RegExpFlag::UnicodeSets),
        named_groups: unicode,
    };
    let outcome = Parser::new(text, body, mode).parse()?;
    if !unicode && outcome.has_named_groups {
        // Annex B.1.2.9: a group name requires a reparse with named groups,
        // where `\k` must start a group reference instead of an identity escape.
        let mode = Mode {
            named_groups: true,
            ..mode
        };
        return Ok(Parser::new(text, body, mode).parse()?.capturing_groups);
    }
    Ok(outcome.capturing_groups)
}

/// The grammar parameters `[UnicodeMode]`, `[UnicodeSetsMode]`, and
/// `[NamedCaptureGroups]`.
#[derive(Clone, Copy)]
struct Mode {
    unicode: bool,
    sets: bool,
    named_groups: bool,
}

struct Outcome {
    capturing_groups: u32,
    has_named_groups: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    Capturing,
    NonCapturing,
    Lookahead,
    Lookbehind,
}

struct Group {
    open: usize,
    kind: GroupKind,
    /// Start of the group's current alternative.
    alternative: usize,
}

/// Whether the preceding term accepts a quantifier.
#[derive(Clone, Copy)]
enum Last {
    Nothing,
    Quantifiable,
    Fixed,
}

enum Escaped {
    Character(u32),
    Class,
}

struct ClassAtom {
    start: usize,
    value: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetState {
    Empty,
    Single,
    Union,
    Intersection,
    Subtraction,
}

/// One UnicodeSets class level and its MayContainStrings evidence.
struct SetFrame {
    open: usize,
    negated: bool,
    state: SetState,
    after_operator: bool,
    strings: bool,
}

struct Operand {
    strings: bool,
    range: bool,
}

const SYNTAX_CHARACTERS: &str = "^$\\.*+?()[]{}|";
const CLASS_SET_SYNTAX_CHARACTERS: &str = "()[]{}/-\\|";
const CLASS_SET_RESERVED_PUNCTUATORS: &str = "&-!#%,:;<=>@`~";
const CLASS_SET_DOUBLE_PUNCTUATORS: &str = "&!#$%*+,.:;<=>?@^`~";

struct Parser<'a> {
    text: &'a str,
    end: usize,
    cursor: usize,
    mode: Mode,
    /// Open groups, outermost first, so their `open` offsets ascend.
    groups: Vec<Group>,
    /// Start of the current top-level alternative.
    pattern_alternative: usize,
    capturing_groups: u32,
    /// The latest `(` offset of each group name, decoded from escapes.
    names: LookupMap<String, usize>,
    name_references: Vec<(usize, String)>,
    /// Unicode-mode decimal escapes that exceeded the groups seen so far.
    decimal_references: Vec<(usize, u32)>,
    has_named_groups: bool,
    /// Without Unicode mode, classes see UTF-16 code units: the trail half of
    /// an astral character is the next class atom, with its source offset.
    pending_trail: Option<(u32, usize)>,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, body: Span, mode: Mode) -> Self {
        Self {
            text,
            end: body.end(),
            cursor: body.start(),
            mode,
            groups: Vec::new(),
            pattern_alternative: body.start(),
            capturing_groups: 0,
            names: LookupMap::default(),
            name_references: Vec::new(),
            decimal_references: Vec::new(),
            has_named_groups: false,
            pending_trail: None,
        }
    }

    fn parse(mut self) -> Result<Outcome, LexicalError> {
        let mut last = Last::Nothing;
        while let Some(ch) = self.peek() {
            let at = self.cursor;
            last = match ch {
                '|' => {
                    self.bump(ch);
                    let start = self.cursor;
                    match self.groups.last_mut() {
                        Some(group) => group.alternative = start,
                        None => self.pattern_alternative = start,
                    }
                    Last::Nothing
                }
                '(' => {
                    self.open_group(at)?;
                    Last::Nothing
                }
                ')' => self.close_group(at)?,
                '^' | '$' => {
                    self.bump(ch);
                    Last::Fixed
                }
                '*' | '+' | '?' => {
                    self.bump(ch);
                    self.quantify(at, last)?
                }
                '{' => self.brace(at, last)?,
                '}' | ']' if self.mode.unicode => {
                    return Err(fail(at, RegExpPatternError::LoneSyntaxCharacter));
                }
                '[' if self.mode.sets => {
                    self.class_set(at)?;
                    Last::Quantifiable
                }
                '[' => {
                    self.class(at)?;
                    Last::Quantifiable
                }
                '\\' => self.atom_escape(at)?,
                _ => {
                    self.bump(ch);
                    Last::Quantifiable
                }
            };
        }
        if let Some(group) = self.groups.last() {
            return Err(fail(group.open, RegExpPatternError::UnterminatedGroup));
        }
        let decimal = self
            .decimal_references
            .iter()
            .find(|&&(_, value)| value > self.capturing_groups)
            .map(|&(offset, _)| (offset, RegExpPatternError::InvalidBackreference));
        let named = self
            .name_references
            .iter()
            .find(|(_, name)| !self.names.contains_key(name))
            .map(|&(offset, _)| (offset, RegExpPatternError::UnresolvedGroupName));
        // Report the first unresolved reference in source order.
        let unresolved = match (decimal, named) {
            (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
            (left, right) => left.or(right),
        };
        if let Some((offset, kind)) = unresolved {
            return Err(fail(offset, kind));
        }
        Ok(Outcome {
            capturing_groups: self.capturing_groups,
            has_named_groups: self.has_named_groups,
        })
    }

    fn quantify(&mut self, at: usize, last: Last) -> Result<Last, LexicalError> {
        match last {
            Last::Quantifiable => {}
            Last::Nothing | Last::Fixed => {
                return Err(fail(at, RegExpPatternError::NothingToRepeat));
            }
        }
        self.eat('?');
        Ok(Last::Nothing)
    }

    fn brace(&mut self, at: usize, last: Last) -> Result<Last, LexicalError> {
        let Some((end, minimum, maximum)) = self.braced_quantifier() else {
            if self.mode.unicode {
                return Err(fail(at, RegExpPatternError::LoneSyntaxCharacter));
            }
            // Annex B: a brace that does not form a quantifier is a pattern character.
            self.bump('{');
            return Ok(Last::Quantifiable);
        };
        // Annex B's InvalidBracedQuantifier makes this an error in both modes.
        if let Last::Quantifiable = last
            && maximum.is_some_and(|maximum| greater(minimum, maximum))
        {
            return Err(fail(at, RegExpPatternError::InvalidQuantifierRange));
        }
        self.cursor = end;
        self.quantify(at, last)
    }

    /// Recognizes `{n}`, `{n,}`, or `{n,m}` at the cursor without consuming it.
    fn braced_quantifier(&self) -> Option<(usize, &'a str, Option<&'a str>)> {
        let bytes = &self.text.as_bytes()[..self.end];
        let digits = |from: usize| {
            let count = bytes[from..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            (from + count, &self.text[from..from + count])
        };
        let (cursor, minimum) = digits(self.cursor + 1);
        if minimum.is_empty() {
            return None;
        }
        let (cursor, maximum) = match bytes.get(cursor) {
            Some(b',') => {
                let (cursor, maximum) = digits(cursor + 1);
                (cursor, (!maximum.is_empty()).then_some(maximum))
            }
            Some(_) | None => (cursor, Some(minimum)),
        };
        (bytes.get(cursor) == Some(&b'}')).then_some((cursor + 1, minimum, maximum))
    }

    fn open_group(&mut self, at: usize) -> Result<(), LexicalError> {
        self.bump('(');
        let kind = if self.eat('?') {
            match self.peek() {
                Some(':') => {
                    self.bump(':');
                    GroupKind::NonCapturing
                }
                Some('=' | '!') => {
                    self.cursor += 1;
                    GroupKind::Lookahead
                }
                Some('<') if matches!(self.peek_second(), Some('=' | '!')) => {
                    self.cursor += 2;
                    GroupKind::Lookbehind
                }
                Some('<') => {
                    self.bump('<');
                    let name = self.group_name()?;
                    self.declare(at, name)?;
                    self.count_capture(at)?;
                    GroupKind::Capturing
                }
                Some(_) | None => {
                    self.modifiers(at)?;
                    GroupKind::NonCapturing
                }
            }
        } else {
            self.count_capture(at)?;
            GroupKind::Capturing
        };
        self.groups
            .try_reserve(1)
            .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
        self.groups.push(Group {
            open: at,
            kind,
            alternative: self.cursor,
        });
        Ok(())
    }

    fn close_group(&mut self, at: usize) -> Result<Last, LexicalError> {
        let Some(group) = self.groups.pop() else {
            return Err(fail(at, RegExpPatternError::UnmatchedParenthesis));
        };
        self.bump(')');
        Ok(match group.kind {
            GroupKind::Capturing | GroupKind::NonCapturing => Last::Quantifiable,
            // Annex B.1.2 QuantifiableAssertion, only without Unicode mode.
            GroupKind::Lookahead if !self.mode.unicode => Last::Quantifiable,
            GroupKind::Lookahead | GroupKind::Lookbehind => Last::Fixed,
        })
    }

    fn count_capture(&mut self, at: usize) -> Result<(), LexicalError> {
        if self.capturing_groups >= u32::MAX - 1 {
            return Err(fail(at, RegExpPatternError::TooManyCapturingGroups));
        }
        self.capturing_groups += 1;
        Ok(())
    }

    /// Parses `(?` RegularExpressionModifiers [`-` RegularExpressionModifiers] `:`.
    fn modifiers(&mut self, at: usize) -> Result<(), LexicalError> {
        let (added, added_repeat) = self.modifier_letters();
        let removed = self.eat('-').then(|| self.modifier_letters());
        if !self.eat(':') {
            return Err(fail(at, RegExpPatternError::InvalidGroup));
        }
        let invalid = added_repeat
            || removed.is_some_and(|(letters, repeat)| {
                repeat || letters & added != 0 || letters | added == 0
            });
        if invalid {
            return Err(fail(at, RegExpPatternError::InvalidModifiers));
        }
        Ok(())
    }

    fn modifier_letters(&mut self) -> (u8, bool) {
        let mut letters = 0u8;
        let mut repeat = false;
        while let Some(bit) = self.peek().and_then(|ch| match ch {
            'i' => Some(1),
            'm' => Some(2),
            's' => Some(4),
            _ => None,
        }) {
            repeat |= letters & bit != 0;
            letters |= bit;
            self.cursor += 1;
        }
        (letters, repeat)
    }

    /// Parses a RegExpIdentifierName and its closing `>`, after the `<`.
    /// Escapes always use the Unicode-mode form, as the grammar requires.
    fn group_name(&mut self) -> Result<String, LexicalError> {
        let mut name = String::new();
        loop {
            let at = self.cursor;
            let invalid = || fail(at, RegExpPatternError::InvalidGroupName);
            let ch = self.peek().ok_or_else(invalid)?;
            if ch == '>' && !name.is_empty() {
                self.bump(ch);
                return Ok(name);
            }
            self.bump(ch);
            let code_point = if ch == '\\' {
                if !self.eat('u') {
                    return Err(invalid());
                }
                self.unicode_escape()
                    .and_then(char::from_u32)
                    .ok_or_else(invalid)?
            } else {
                ch
            };
            let permitted = if name.is_empty() {
                is_js_identifier_start(code_point)
            } else {
                is_js_identifier_part(code_point)
            };
            if !permitted {
                return Err(invalid());
            }
            name.try_reserve(code_point.len_utf8())
                .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
            name.push(code_point);
        }
    }

    fn declare(&mut self, at: usize, name: String) -> Result<(), LexicalError> {
        self.has_named_groups = true;
        if let Some(&previous) = self.names.get(&name)
            && self.might_both_participate(previous)
        {
            return Err(fail(at, RegExpPatternError::DuplicateGroupName));
        }
        self.names
            .try_reserve(1)
            .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
        self.names.insert(name, at);
        Ok(())
    }

    /// MightBothParticipate (section 22.2.1.4) for an earlier group at
    /// `previous` and a group at the cursor. They are exclusive exactly when
    /// the deepest disjunction containing both has them in different
    /// alternatives, that is, when `previous` precedes the current
    /// alternative of the deepest open group that contains it. Checking only
    /// the latest same-named group suffices: earlier ones were pairwise
    /// exclusive with it, and exclusivity at an enclosing disjunction also
    /// covers the current group.
    fn might_both_participate(&self, previous: usize) -> bool {
        let depth = self.groups.partition_point(|group| group.open < previous);
        let alternative = match depth.checked_sub(1) {
            Some(index) => self.groups[index].alternative,
            None => self.pattern_alternative,
        };
        previous >= alternative
    }

    fn atom_escape(&mut self, at: usize) -> Result<Last, LexicalError> {
        self.bump('\\');
        match self.peek() {
            Some(ch @ ('b' | 'B')) => {
                self.bump(ch);
                Ok(Last::Fixed)
            }
            Some('1'..='9') => {
                let start = self.cursor;
                while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    self.cursor += 1;
                }
                // Without Unicode mode, Annex B reads excess references as
                // legacy octal or identity escapes, which are always valid.
                if self.mode.unicode {
                    let value = self.text[start..self.cursor]
                        .parse::<u32>()
                        .unwrap_or(u32::MAX);
                    if value > self.capturing_groups {
                        self.decimal_references
                            .try_reserve(1)
                            .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
                        self.decimal_references.push((at, value));
                    }
                }
                Ok(Last::Quantifiable)
            }
            Some('k') if self.mode.named_groups => {
                self.bump('k');
                if !self.eat('<') {
                    return Err(fail(at, RegExpPatternError::InvalidEscape));
                }
                let name = self.group_name()?;
                self.name_references
                    .try_reserve(1)
                    .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
                self.name_references.push((at, name));
                Ok(Last::Quantifiable)
            }
            Some(_) | None => {
                self.escape(at, false)?;
                Ok(Last::Quantifiable)
            }
        }
    }

    /// Parses a character or class escape after its backslash, at `at`.
    /// Atom-only escapes (`\b` assertions, backreferences, `\k`) are handled
    /// by the caller; in a class, `\b` is a backspace.
    fn escape(&mut self, at: usize, in_class: bool) -> Result<Escaped, LexicalError> {
        let unicode = self.mode.unicode;
        let invalid = || fail(at, RegExpPatternError::InvalidEscape);
        let ch = self.peek().ok_or_else(invalid)?;
        let value = match ch {
            'd' | 'D' | 's' | 'S' | 'w' | 'W' => {
                self.bump(ch);
                return Ok(Escaped::Class);
            }
            'p' | 'P' if unicode => {
                self.property(at)?;
                return Ok(Escaped::Class);
            }
            'f' | 'n' | 'r' | 't' | 'v' | 'b' => {
                self.bump(ch);
                match ch {
                    'f' => 0x0c,
                    'n' => 0x0a,
                    'r' => 0x0d,
                    't' => 0x09,
                    'v' => 0x0b,
                    _ => 0x08,
                }
            }
            'c' => {
                let control = self.peek_second().filter(|&next| {
                    next.is_ascii_alphabetic()
                        || (!unicode && in_class && (next.is_ascii_digit() || next == '_'))
                });
                match control {
                    Some(letter) => {
                        self.cursor += 2;
                        u32::from(letter) % 32
                    }
                    None if unicode => return Err(invalid()),
                    // Annex B: the backslash matches itself; `c` is parsed next.
                    None => 0x5c,
                }
            }
            '0' if unicode => {
                self.bump(ch);
                if self.peek().is_some_and(|next| next.is_ascii_digit()) {
                    return Err(invalid());
                }
                0
            }
            '0'..='7' if !unicode => self.legacy_octal(),
            'x' => match self.hex_value(self.cursor + 1, 2) {
                Some(value) => {
                    self.cursor += 3;
                    value
                }
                None if unicode => return Err(invalid()),
                None => {
                    self.bump(ch);
                    u32::from(ch)
                }
            },
            'u' if unicode => {
                self.bump(ch);
                self.unicode_escape().ok_or_else(invalid)?
            }
            'u' => match self.hex_value(self.cursor + 1, 4) {
                Some(value) => {
                    self.cursor += 5;
                    value
                }
                None => {
                    self.bump(ch);
                    u32::from(ch)
                }
            },
            'k' if unicode || self.mode.named_groups => return Err(invalid()),
            '-' if in_class => {
                self.bump(ch);
                0x2d
            }
            _ if unicode => {
                if !SYNTAX_CHARACTERS.contains(ch) && ch != '/' {
                    return Err(invalid());
                }
                self.bump(ch);
                u32::from(ch)
            }
            // Annex B IdentityEscape: any source character but `c`, and `k`
            // with named groups, both handled above.
            _ => {
                let offset = self.cursor;
                self.bump(ch);
                self.unit(ch, offset, in_class)
            }
        };
        Ok(Escaped::Character(value))
    }

    /// LegacyOctalEscapeSequence (section 12.9.4): up to three digits from
    /// `\0` to `\377`, consumed greedily.
    fn legacy_octal(&mut self) -> u32 {
        let mut value = 0;
        let mut remaining = 3;
        while remaining > 0 {
            let Some(digit) = self.peek().and_then(|ch| ch.to_digit(8)) else {
                break;
            };
            if remaining == 3 && digit > 3 {
                remaining = 2;
            }
            value = value * 8 + digit;
            self.cursor += 1;
            remaining -= 1;
        }
        value
    }

    /// Parses the rest of a Unicode-mode `\u` escape after the `u`: braced
    /// code points up to U+10FFFF, or four digits, joining a lead surrogate
    /// with a directly following `\u` trail surrogate. `None` if malformed.
    fn unicode_escape(&mut self) -> Option<u32> {
        if self.eat('{') {
            let mut value = 0u32;
            let mut has_digit = false;
            while let Some(digit) = self.peek().and_then(|ch| ch.to_digit(16)) {
                // Capping keeps long zero runs linear and rejects overflow.
                value = (value * 16 + digit).min(0x11_0000);
                has_digit = true;
                self.cursor += 1;
            }
            return (has_digit && value <= 0x10_ffff && self.eat('}')).then_some(value);
        }
        let lead = self.hex_value(self.cursor, 4)?;
        self.cursor += 4;
        if (0xd800..=0xdbff).contains(&lead)
            && self.text[self.cursor..self.end].starts_with("\\u")
            && let Some(trail) = self.hex_value(self.cursor + 2, 4)
            && (0xdc00..=0xdfff).contains(&trail)
        {
            self.cursor += 6;
            return Some(0x1_0000 + ((lead - 0xd800) << 10) + (trail - 0xdc00));
        }
        Some(lead)
    }

    /// Parses `{Name=Value}` or `{NameOrValue}` after `\p` or `\P`.
    fn property(&mut self, at: usize) -> Result<PropertyKind, LexicalError> {
        let invalid = || fail(at, RegExpPatternError::InvalidPropertyEscape);
        let negated = self.peek() == Some('P');
        self.cursor += 1;
        if !self.eat('{') {
            return Err(invalid());
        }
        let name = self.property_characters();
        let value = self.eat('=').then(|| self.property_characters());
        if !self.eat('}') {
            return Err(invalid());
        }
        let kind = classify(name, value).ok_or_else(invalid)?;
        match kind {
            PropertyKind::CodePoints => {}
            PropertyKind::Strings if !self.mode.sets => return Err(invalid()),
            PropertyKind::Strings if negated => {
                return Err(fail(at, RegExpPatternError::NegatedClassStrings));
            }
            PropertyKind::Strings => {}
        }
        Ok(kind)
    }

    fn property_characters(&mut self) -> &'a str {
        let start = self.cursor;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            self.cursor += 1;
        }
        &self.text[start..self.cursor]
    }

    /// Parses a class without UnicodeSets mode (NonemptyClassRanges).
    fn class(&mut self, at: usize) -> Result<(), LexicalError> {
        self.bump('[');
        self.eat('^');
        loop {
            if self.pending_trail.is_none() && self.eat(']') {
                return Ok(());
            }
            let Some(first) = self.class_atom()? else {
                return Err(fail(at, RegExpPatternError::UnterminatedCharacterClass));
            };
            if self.pending_trail.is_none()
                && self.peek() == Some('-')
                && !matches!(self.peek_second(), Some(']') | None)
            {
                self.bump('-');
                let Some(second) = self.class_atom()? else {
                    return Err(fail(at, RegExpPatternError::UnterminatedCharacterClass));
                };
                let ordered = match (first.value, second.value) {
                    (Some(low), Some(high)) => low <= high,
                    // Annex B.1.2.1: without Unicode mode, a class escape
                    // endpoint makes `-` a literal in the union.
                    (None, _) | (_, None) => !self.mode.unicode,
                };
                if !ordered {
                    return Err(fail(first.start, RegExpPatternError::InvalidClassRange));
                }
            }
        }
    }

    fn class_atom(&mut self) -> Result<Option<ClassAtom>, LexicalError> {
        if let Some((value, start)) = self.pending_trail.take() {
            return Ok(Some(ClassAtom {
                start,
                value: Some(value),
            }));
        }
        let start = self.cursor;
        let Some(ch) = self.peek() else {
            return Ok(None);
        };
        self.bump(ch);
        let value = if ch == '\\' {
            match self.escape(start, true)? {
                Escaped::Character(value) => Some(value),
                Escaped::Class => None,
            }
        } else {
            Some(self.unit(ch, start, true))
        };
        Ok(Some(ClassAtom { start, value }))
    }

    /// The CharacterValue of a source character. Without Unicode mode, a
    /// class sees an astral character as a lead and a pending trail unit.
    fn unit(&mut self, ch: char, offset: usize, in_class: bool) -> u32 {
        let value = u32::from(ch);
        if self.mode.unicode || !in_class || value <= 0xffff {
            return value;
        }
        let offset_value = value - 0x1_0000;
        self.pending_trail = Some((0xdc00 + (offset_value & 0x3ff), offset));
        0xd800 + (offset_value >> 10)
    }

    /// Parses a UnicodeSets class (ClassSetExpression), nested classes included.
    fn class_set(&mut self, at: usize) -> Result<(), LexicalError> {
        let mut stack = Vec::new();
        self.open_set(&mut stack, at)?;
        loop {
            let position = self.cursor;
            let Some(frame) = stack.last_mut() else {
                return Ok(());
            };
            let Some(ch) = self.peek() else {
                return Err(fail(
                    frame.open,
                    RegExpPatternError::UnterminatedCharacterClass,
                ));
            };
            let rest = &self.text[position..self.end];
            if ch == ']' {
                if frame.after_operator {
                    return Err(fail(position, RegExpPatternError::InvalidClassSetSyntax));
                }
                self.bump(ch);
                if frame.negated && frame.strings {
                    return Err(fail(frame.open, RegExpPatternError::NegatedClassStrings));
                }
                // A negated class never contains strings.
                let strings = frame.strings && !frame.negated;
                stack.pop();
                if let Some(parent) = stack.last_mut() {
                    parent.add(
                        Operand {
                            strings,
                            range: false,
                        },
                        position,
                    )?;
                }
                continue;
            }
            if rest.starts_with("&&") || rest.starts_with("--") {
                let state = if ch == '&' {
                    SetState::Intersection
                } else {
                    SetState::Subtraction
                };
                frame.operator(state, position)?;
                self.cursor += 2;
                if state == SetState::Intersection && self.peek() == Some('&') {
                    return Err(fail(self.cursor, RegExpPatternError::InvalidClassSetSyntax));
                }
                continue;
            }
            if matches!(frame.state, SetState::Intersection | SetState::Subtraction)
                && !frame.after_operator
            {
                return Err(fail(position, RegExpPatternError::InvalidClassSetSyntax));
            }
            if ch == '[' {
                self.open_set(&mut stack, position)?;
                continue;
            }
            let operand = self.set_operand(position)?;
            if let Some(frame) = stack.last_mut() {
                frame.add(operand, position)?;
            }
        }
    }

    fn open_set(&mut self, stack: &mut Vec<SetFrame>, at: usize) -> Result<(), LexicalError> {
        self.bump('[');
        let negated = self.eat('^');
        stack
            .try_reserve(1)
            .map_err(|_| error(at, LexicalErrorKind::AllocationFailed))?;
        stack.push(SetFrame {
            open: at,
            negated,
            state: SetState::Empty,
            after_operator: false,
            strings: false,
        });
        Ok(())
    }

    /// Parses a ClassSetOperand other than a nested bracket class, or a
    /// ClassSetRange.
    fn set_operand(&mut self, at: usize) -> Result<Operand, LexicalError> {
        let rest = &self.text[at..self.end];
        if rest.starts_with("\\q{") {
            self.cursor += 3;
            let strings = self.class_strings(at)?;
            return Ok(Operand {
                strings,
                range: false,
            });
        }
        if let Some(ch @ ('d' | 'D' | 's' | 'S' | 'w' | 'W' | 'p' | 'P')) =
            rest.strip_prefix('\\').and_then(|rest| rest.chars().next())
        {
            self.bump('\\');
            let strings = if matches!(ch, 'p' | 'P') {
                self.property(at)? == PropertyKind::Strings
            } else {
                self.bump(ch);
                false
            };
            return Ok(Operand {
                strings,
                range: false,
            });
        }
        let low = self.set_character()?;
        let rest = &self.text[self.cursor..self.end];
        if !rest.starts_with('-') || rest.starts_with("--") {
            return Ok(Operand {
                strings: false,
                range: false,
            });
        }
        self.bump('-');
        if low > self.set_character()? {
            return Err(fail(at, RegExpPatternError::InvalidClassRange));
        }
        Ok(Operand {
            strings: false,
            range: true,
        })
    }

    /// Parses `\q{...}` contents after the brace; reports MayContainStrings,
    /// which holds when any alternative is empty or longer than one character.
    fn class_strings(&mut self, at: usize) -> Result<bool, LexicalError> {
        let mut strings = false;
        let mut length = 0usize;
        loop {
            match self.peek() {
                Some(ch @ ('}' | '|')) => {
                    self.bump(ch);
                    strings |= length != 1;
                    length = 0;
                    if ch == '}' {
                        return Ok(strings);
                    }
                }
                Some(_) => {
                    self.set_character()?;
                    length += 1;
                }
                None => return Err(fail(at, RegExpPatternError::InvalidClassSetSyntax)),
            }
        }
    }

    /// Parses one ClassSetCharacter and returns its CharacterValue.
    fn set_character(&mut self) -> Result<u32, LexicalError> {
        let at = self.cursor;
        let syntax = || fail(at, RegExpPatternError::InvalidClassSetSyntax);
        let ch = self.peek().ok_or_else(syntax)?;
        if ch == '\\' {
            self.bump(ch);
            if let Some(next) = self
                .peek()
                .filter(|&next| next == 'b' || CLASS_SET_RESERVED_PUNCTUATORS.contains(next))
            {
                self.bump(next);
                return Ok(if next == 'b' { 0x08 } else { u32::from(next) });
            }
            return match self.escape(at, true)? {
                Escaped::Character(value) => Ok(value),
                Escaped::Class => Err(syntax()),
            };
        }
        let doubled = self.peek_second() == Some(ch) && CLASS_SET_DOUBLE_PUNCTUATORS.contains(ch);
        if doubled || CLASS_SET_SYNTAX_CHARACTERS.contains(ch) {
            return Err(syntax());
        }
        self.bump(ch);
        Ok(u32::from(ch))
    }

    fn peek(&self) -> Option<char> {
        self.text[self.cursor..self.end].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut chars = self.text[self.cursor..self.end].chars();
        chars.next();
        chars.next()
    }

    /// Consumes `ch`, which the caller has just peeked.
    fn bump(&mut self, ch: char) {
        self.cursor += ch.len_utf8();
    }

    fn eat(&mut self, ch: char) -> bool {
        let matched = self.peek() == Some(ch);
        if matched {
            self.bump(ch);
        }
        matched
    }

    /// Reads `count` hexadecimal digits at `from` without consuming them.
    fn hex_value(&self, from: usize, count: usize) -> Option<u32> {
        let digits = self.text.as_bytes()[..self.end].get(from..from + count)?;
        digits.iter().try_fold(0, |value, &byte| {
            Some(value * 16 + char::from(byte).to_digit(16)?)
        })
    }
}

impl SetFrame {
    fn operator(&mut self, state: SetState, at: usize) -> Result<(), LexicalError> {
        let allowed = !self.after_operator
            && match self.state {
                SetState::Single => true,
                SetState::Intersection | SetState::Subtraction => self.state == state,
                SetState::Empty | SetState::Union => false,
            };
        if !allowed {
            return Err(fail(at, RegExpPatternError::InvalidClassSetSyntax));
        }
        self.state = state;
        self.after_operator = true;
        Ok(())
    }

    /// Adds an operand and updates MayContainStrings: any operand for a
    /// union, all operands for an intersection, the first for a subtraction.
    fn add(&mut self, operand: Operand, at: usize) -> Result<(), LexicalError> {
        match self.state {
            SetState::Empty => {
                self.state = if operand.range {
                    SetState::Union
                } else {
                    SetState::Single
                };
                self.strings = operand.strings;
            }
            SetState::Single | SetState::Union => {
                self.state = SetState::Union;
                self.strings |= operand.strings;
            }
            SetState::Intersection | SetState::Subtraction if operand.range => {
                return Err(fail(at, RegExpPatternError::InvalidClassSetSyntax));
            }
            SetState::Intersection => self.strings &= operand.strings,
            SetState::Subtraction => {}
        }
        self.after_operator = false;
        Ok(())
    }
}

/// Compares two ASCII decimal digit strings by value, without overflow.
fn greater(left: &str, right: &str) -> bool {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    (left.len(), left) > (right.len(), right)
}

fn fail(offset: usize, kind: RegExpPatternError) -> LexicalError {
    error(offset, LexicalErrorKind::InvalidRegExpPattern(kind))
}
