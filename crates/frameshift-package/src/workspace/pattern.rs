//! Workspace patterns in the grammar that every modeled package manager
//! reads alike (`D-073`).
//!
//! The grammar is a core that all modeled releases accept: literal
//! characters, `*`, `?`, `**` as a whole segment, character classes of
//! ASCII letters, digits, and `_` with ranges and `!` negation, brace
//! alternatives of two or more non-empty alternatives within one segment,
//! one leading `./`, a trailing `/`, and a leading `!`. Everything else
//! is rejected rather than guessed: extglobs, POSIX classes, brace
//! sequences, escapes, absolute and parent-relative paths, and the
//! characters that at least one matcher reads specially. How a parsed
//! pattern selects directories differs between package managers and is
//! decided in the semantics module.

use std::fmt;

/// One matching element of a segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    /// One literal character.
    Literal(char),
    /// `*`: any run of characters within the segment.
    Star,
    /// `?`: exactly one character unit.
    Any,
    /// `[...]`: one character unit in (or, negated, not in) the ranges.
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
}

impl Token {
    pub(crate) const fn is_wildcard(&self) -> bool {
        !matches!(self, Self::Literal(_))
    }
}

/// One path segment of a pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Segment {
    /// `**`: zero or more segments.
    Globstar,
    /// A segment that matches when one of its brace alternatives matches;
    /// a segment without braces has exactly one alternative.
    Alternatives(Vec<Vec<Token>>),
}

impl Segment {
    /// Returns the literal text of a segment without wildcards or braces.
    pub(crate) fn literal(&self) -> Option<String> {
        match self {
            Self::Globstar => None,
            Self::Alternatives(alternatives) => match alternatives.as_slice() {
                [tokens] => tokens
                    .iter()
                    .map(|token| match token {
                        Token::Literal(character) => Some(*character),
                        Token::Star | Token::Any | Token::Class { .. } => None,
                    })
                    .collect(),
                _ => None,
            },
        }
    }
}

/// A parsed workspace pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pattern {
    /// Whether the pattern began with `!`.
    pub negated: bool,
    /// The segments; never empty.
    pub segments: Vec<Segment>,
    /// Whether the text ended with `/`.
    pub trailing_slash: bool,
    /// Whether the text began with `./` after any `!`.
    pub dot_slash: bool,
    /// The text after a leading `!` and `./` and without a trailing `/`,
    /// as npm compares patterns with each other.
    pub body: Box<str>,
    /// The text of each segment.
    pub texts: Vec<Box<str>>,
}

/// One brace expansion of a pattern, as fast-glob expands patterns before
/// grouping them by base directory.
#[derive(Clone, Debug)]
pub(crate) struct Expansion {
    /// Segments with exactly one alternative each.
    pub segments: Vec<Segment>,
    /// The text of each segment.
    pub texts: Vec<String>,
}

impl Pattern {
    /// Returns whether no segment has wildcards or braces.
    pub(crate) fn is_literal(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.literal().is_some())
    }

    /// Every brace expansion, or `None` beyond `limit` expansions.
    pub(crate) fn expansions(&self, limit: usize) -> Option<Vec<Expansion>> {
        let mut expansions = vec![Expansion {
            segments: Vec::new(),
            texts: Vec::new(),
        }];
        for segment in &self.segments {
            let mut next = Vec::new();
            for expansion in &expansions {
                match segment {
                    Segment::Globstar => {
                        let mut extended = expansion.clone();
                        extended.segments.push(Segment::Globstar);
                        extended.texts.push("**".to_owned());
                        next.push(extended);
                    }
                    Segment::Alternatives(alternatives) => {
                        for tokens in alternatives {
                            if next.len() == limit {
                                return None;
                            }
                            let mut extended = expansion.clone();
                            extended
                                .segments
                                .push(Segment::Alternatives(vec![tokens.clone()]));
                            extended.texts.push(render(tokens));
                            next.push(extended);
                        }
                    }
                }
            }
            expansions = next;
        }
        Some(expansions)
    }
}

/// The text of tokens, as fast-glob sees an expanded pattern.
pub(crate) fn render(tokens: &[Token]) -> String {
    let mut text = String::new();
    for token in tokens {
        match token {
            Token::Literal(character) => text.push(*character),
            Token::Star => text.push('*'),
            Token::Any => text.push('?'),
            Token::Class { negated, ranges } => {
                text.push('[');
                if *negated {
                    text.push('!');
                }
                for &(low, high) in ranges {
                    text.push(low);
                    if low != high {
                        text.push('-');
                        text.push(high);
                    }
                }
                text.push(']');
            }
        }
    }
    text
}

/// The number of leading segments of fast-glob's base directory, as
/// glob-parent computes it with is-glob's strict check: a segment is part
/// of the base unless it contains `*`, `[`, `{`, or a `?` after `]`, `.`,
/// `+`, or `)`. The last segment is part of it only after a trailing `/`.
pub(crate) fn glob_parent<S: AsRef<str>>(texts: &[S], trailing_slash: bool) -> usize {
    let glob = |text: &str| {
        let characters: Vec<char> = text.chars().collect();
        characters
            .iter()
            .enumerate()
            .any(|(index, character)| match character {
                '*' | '[' | '{' => true,
                '?' => index > 0 && matches!(characters[index - 1], ']' | '.' | '+' | ')'),
                _ => false,
            })
    };
    let candidates = if trailing_slash {
        texts
    } else {
        &texts[..texts.len() - 1]
    };
    candidates
        .iter()
        .take_while(|text| !glob(text.as_ref()))
        .count()
}

/// What a pattern text stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Parsed {
    /// `.` or `./`, the workspace root itself, which every modeled manager
    /// skips.
    Root,
    /// A pattern.
    Pattern(Pattern),
}

/// Why a pattern is not evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternRejection {
    /// The pattern is empty, which several managers reject.
    Empty,
    /// The pattern exceeds `workspace.max-pattern-bytes`.
    TooLong,
    /// The brace alternatives of one segment exceed `workspace.max-alternatives`.
    TooManyAlternatives,
    /// The pattern starts with `/` or names a parent or current directory
    /// segment other than one leading `./`.
    Path,
    /// An empty segment, as in `a//b`.
    EmptySegment,
    /// A negation other than one leading `!`, or a negation in a manager
    /// without negation.
    Negation,
    /// A character that at least one manager reads specially: `\`, `#`,
    /// `,` outside braces, `(`, `)`, `|`, `+`, `<`, `>`, `$`, `^`, `:`,
    /// quotes, a backquote, or a control character.
    Character,
    /// `**` that is not a whole segment.
    Globstar,
    /// A segment that starts with `.` and is not literal.
    DotWildcard,
    /// A brace form other than two or more non-empty alternatives within a
    /// segment, each starting with a literal character at a segment start.
    Braces,
    /// A character class other than ASCII letters, digits, `_`, and
    /// ranges of digits, lowercase, or uppercase letters.
    Class,
    /// A form that this release matches differently from the documented
    /// semantics, or that its releases match differently from each other.
    Quirk,
}

impl fmt::Display for PatternRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "the pattern is empty",
            Self::TooLong => "the pattern exceeds workspace.max-pattern-bytes",
            Self::TooManyAlternatives => {
                "the brace alternatives of a segment exceed workspace.max-alternatives"
            }
            Self::Path => "the pattern is absolute or names a `.` or `..` segment",
            Self::EmptySegment => "the pattern has an empty segment",
            Self::Negation => "the pattern uses a negation that is not one leading `!`",
            Self::Character => {
                "the pattern contains a character that a package manager reads \
                                specially"
            }
            Self::Globstar => "`**` is not a whole segment",
            Self::DotWildcard => "a segment starting with `.` contains wildcards",
            Self::Braces => "the pattern uses a brace form outside the evaluated grammar",
            Self::Class => "the pattern uses a character class outside the evaluated grammar",
            Self::Quirk => {
                "the package manager release matches this form in a way that \
                            Frameshift does not model"
            }
        })
    }
}

/// Bounds of pattern parsing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ParseBounds {
    pub max_bytes: usize,
    pub max_alternatives: usize,
}

/// Parses a pattern. `negation` tells whether the manager reads a leading
/// `!` as a negation; Yarn classic reads it as a literal character.
pub(crate) fn parse(
    text: &str,
    negation: bool,
    bounds: ParseBounds,
) -> Result<Parsed, PatternRejection> {
    if text.is_empty() {
        return Err(PatternRejection::Empty);
    }
    if text.len() > bounds.max_bytes {
        return Err(PatternRejection::TooLong);
    }
    let (negated, rest) = match text.strip_prefix('!') {
        Some(rest) if negation => (true, rest),
        _ => (false, text),
    };
    // Yarn classic reads a leading `!` as a literal character.
    let literal_bang = !negation && rest.starts_with('!');
    let tail = if literal_bang { &rest[1..] } else { rest };
    // A `!` other than the one that negates a class.
    let mut previous = None;
    for character in tail.chars() {
        if character == '!' && previous != Some('[') {
            return Err(PatternRejection::Negation);
        }
        previous = Some(character);
    }
    if rest == "." || rest == "./" {
        return if negated {
            Err(PatternRejection::Path)
        } else {
            Ok(Parsed::Root)
        };
    }
    let dot_slash = rest.starts_with("./");
    let rest = rest.strip_prefix("./").unwrap_or(rest);
    let (rest, trailing_slash) = match rest.strip_suffix('/') {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    if rest.is_empty() || rest.starts_with('/') {
        return Err(if rest.is_empty() {
            PatternRejection::Empty
        } else {
            PatternRejection::Path
        });
    }
    let mut segments = Vec::new();
    let mut texts = Vec::new();
    for (index, text) in rest.split('/').enumerate() {
        segments.push(segment(text, literal_bang && index == 0, bounds)?);
        texts.push(text.into());
    }
    Ok(Parsed::Pattern(Pattern {
        negated,
        segments,
        trailing_slash,
        dot_slash,
        body: rest.into(),
        texts,
    }))
}

fn segment(
    text: &str,
    literal_bang: bool,
    bounds: ParseBounds,
) -> Result<Segment, PatternRejection> {
    if text.is_empty() {
        return Err(PatternRejection::EmptySegment);
    }
    if text == "." || text == ".." {
        return Err(PatternRejection::Path);
    }
    if text == "**" {
        return Ok(Segment::Globstar);
    }
    if text.contains("**") {
        return Err(PatternRejection::Globstar);
    }
    let characters: Vec<char> = text.chars().collect();
    let mut parser = SegmentParser {
        characters: &characters,
        position: 0,
        literal_bang,
        bounds,
    };
    let alternatives = parser.sequence(0)?;
    if parser.position != characters.len() {
        // A stray `}` or `,` outside braces.
        return Err(match characters[parser.position] {
            '}' => PatternRejection::Braces,
            _ => PatternRejection::Character,
        });
    }
    if text.starts_with('.')
        && alternatives
            .iter()
            .any(|tokens| tokens.iter().any(Token::is_wildcard))
    {
        return Err(PatternRejection::DotWildcard);
    }
    for tokens in &alternatives {
        // An alternative that starts a segment with `.` meets each
        // matcher's dot rule differently.
        if tokens.is_empty() || (!text.starts_with('.') && tokens[0] == Token::Literal('.')) {
            return Err(PatternRejection::Braces);
        }
    }
    Ok(Segment::Alternatives(alternatives))
}

struct SegmentParser<'a> {
    characters: &'a [char],
    position: usize,
    literal_bang: bool,
    bounds: ParseBounds,
}

impl SegmentParser<'_> {
    /// Parses tokens and brace groups until the end, a `,`, or a `}` at
    /// brace depth `depth`, returning every expansion.
    fn sequence(&mut self, depth: usize) -> Result<Vec<Vec<Token>>, PatternRejection> {
        let mut expansions: Vec<Vec<Token>> = vec![Vec::new()];
        while let Some(&character) = self.characters.get(self.position) {
            match character {
                ',' | '}' if depth > 0 => break,
                ',' | '}' => return Ok(expansions),
                '{' => {
                    let start_of_segment = expansions.iter().all(Vec::is_empty);
                    self.position += 1;
                    let group = self.group(depth + 1)?;
                    if start_of_segment
                        && group
                            .iter()
                            .any(|tokens| tokens.first().is_some_and(Token::is_wildcard))
                    {
                        return Err(PatternRejection::Braces);
                    }
                    let mut combined = Vec::new();
                    for prefix in &expansions {
                        for suffix in &group {
                            if combined.len() == self.bounds.max_alternatives {
                                return Err(PatternRejection::TooManyAlternatives);
                            }
                            let mut tokens = prefix.clone();
                            tokens.extend(suffix.iter().cloned());
                            combined.push(tokens);
                        }
                    }
                    expansions = combined;
                }
                _ => {
                    let token = self.token()?;
                    for tokens in &mut expansions {
                        tokens.push(token.clone());
                    }
                }
            }
        }
        Ok(expansions)
    }

    /// Parses the alternatives of a brace group after its `{`.
    fn group(&mut self, depth: usize) -> Result<Vec<Vec<Token>>, PatternRejection> {
        let mut alternatives = Vec::new();
        let mut count = 0usize;
        loop {
            let expansions = self.sequence(depth)?;
            count += 1;
            for tokens in expansions {
                if tokens.is_empty() {
                    return Err(PatternRejection::Braces);
                }
                if alternatives.len() == self.bounds.max_alternatives {
                    return Err(PatternRejection::TooManyAlternatives);
                }
                alternatives.push(tokens);
            }
            match self.characters.get(self.position) {
                Some(',') => self.position += 1,
                Some('}') => {
                    self.position += 1;
                    // `{a}` is literal for minimatch and picomatch but an
                    // alternative for wax and Bun.
                    if count < 2 {
                        return Err(PatternRejection::Braces);
                    }
                    return Ok(alternatives);
                }
                _ => return Err(PatternRejection::Braces),
            }
        }
    }

    fn token(&mut self) -> Result<Token, PatternRejection> {
        let character = self.characters[self.position];
        self.position += 1;
        match character {
            '*' => {
                if self.characters.get(self.position) == Some(&'*') {
                    return Err(PatternRejection::Globstar);
                }
                Ok(Token::Star)
            }
            '?' => Ok(Token::Any),
            '[' => self.class(),
            ']' | '{' | '}' => Err(PatternRejection::Braces),
            '!' if self.literal_bang && self.position == 1 => Ok(Token::Literal('!')),
            _ if is_rejected(character) => Err(PatternRejection::Character),
            _ => Ok(Token::Literal(character)),
        }
    }

    /// Parses a class after its `[`.
    fn class(&mut self) -> Result<Token, PatternRejection> {
        let mut negated = false;
        if self.characters.get(self.position) == Some(&'!') {
            negated = true;
            self.position += 1;
        }
        let mut ranges = Vec::new();
        loop {
            let Some(&low) = self.characters.get(self.position) else {
                return Err(PatternRejection::Class);
            };
            self.position += 1;
            if low == ']' {
                if ranges.is_empty() {
                    return Err(PatternRejection::Class);
                }
                return Ok(Token::Class { negated, ranges });
            }
            if !is_class_character(low) {
                return Err(PatternRejection::Class);
            }
            if self.characters.get(self.position) == Some(&'-') {
                self.position += 1;
                let Some(&high) = self.characters.get(self.position) else {
                    return Err(PatternRejection::Class);
                };
                self.position += 1;
                if !is_class_character(high) || !same_kind(low, high) || low > high {
                    return Err(PatternRejection::Class);
                }
                ranges.push((low, high));
            } else {
                ranges.push((low, low));
            }
        }
    }
}

fn is_class_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn same_kind(low: char, high: char) -> bool {
    (low.is_ascii_digit() && high.is_ascii_digit())
        || (low.is_ascii_lowercase() && high.is_ascii_lowercase())
        || (low.is_ascii_uppercase() && high.is_ascii_uppercase())
        || (low == high)
}

/// Characters that some modeled matcher reads specially or that cannot be
/// compared portably.
fn is_rejected(character: char) -> bool {
    matches!(
        character,
        '\\' | '#'
            | ','
            | '('
            | ')'
            | '|'
            | '+'
            | '<'
            | '>'
            | '$'
            | '^'
            | ':'
            | '"'
            | '\''
            | '`'
            | '!'
    ) || character.is_control()
}
