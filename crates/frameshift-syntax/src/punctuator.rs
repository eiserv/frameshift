use frameshift_core::{SourceText, Span};

use crate::lexical::{LexicalError, span, validate};

/// The parser-selected ECMAScript input-element goal.
/// Choosing a goal does not scan regular expressions or template tails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsLexicalGoal {
    /// `InputElementDiv`: division and ordinary right braces are permitted.
    Div,
    /// `InputElementRegExp`: regular expressions and ordinary right braces.
    RegExp,
    /// `InputElementRegExpOrTemplateTail`: regular expressions and template tails.
    RegExpOrTemplateTail,
    /// `InputElementTemplateTail`: division and template tails.
    TemplateTail,
}

/// How a parser consumes `>` in JavaScript expressions or TypeScript types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GreaterThanMode {
    /// Longest matching operator, including `>=`, shifts, and shift assignments.
    LongestMatch,
    /// Consume exactly one `>` as a type argument/parameter closing delimiter.
    Single,
}

/// Explicit lexical context. There is no default that guesses parser state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PunctuatorContext {
    /// Whether slash and right brace belong to punctuation in this context.
    pub goal: JsLexicalGoal,
    /// Whether `>` closes a type construct or starts a maximal operator.
    pub greater_than: GreaterThanMode,
}

macro_rules! define_punctuators {
    ($($kind:ident => $spelling:literal),+ $(,)?) => {
        /// An ECMAScript punctuator, independent of its expression semantics.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Punctuator {
            $(#[doc = concat!("The `", $spelling, "` punctuator.")]
            $kind,)+
        }

        impl Punctuator {
            /// Returns the exact ASCII spelling of this punctuator.
            pub const fn spelling(self) -> &'static str {
                match self {
                    $(Self::$kind => $spelling,)+
                }
            }
        }

        const PUNCTUATORS: &[Punctuator] = &[$(Punctuator::$kind,)+];
    };
}

// One grammar table generates the closed kinds, spellings, and candidate list.
define_punctuators! {
    LeftBrace => "{",
    LeftParen => "(",
    RightParen => ")",
    LeftBracket => "[",
    RightBracket => "]",
    Dot => ".",
    Ellipsis => "...",
    Semicolon => ";",
    Comma => ",",
    LessThan => "<",
    GreaterThan => ">",
    LessThanEqual => "<=",
    GreaterThanEqual => ">=",
    Equal => "==",
    NotEqual => "!=",
    StrictEqual => "===",
    StrictNotEqual => "!==",
    Plus => "+",
    Minus => "-",
    Star => "*",
    Percent => "%",
    Exponent => "**",
    Increment => "++",
    Decrement => "--",
    ShiftLeft => "<<",
    ShiftRight => ">>",
    ShiftRightUnsigned => ">>>",
    BitAnd => "&",
    BitOr => "|",
    BitXor => "^",
    Bang => "!",
    Tilde => "~",
    LogicalAnd => "&&",
    LogicalOr => "||",
    Nullish => "??",
    Question => "?",
    Colon => ":",
    Assign => "=",
    PlusAssign => "+=",
    MinusAssign => "-=",
    StarAssign => "*=",
    PercentAssign => "%=",
    ExponentAssign => "**=",
    ShiftLeftAssign => "<<=",
    ShiftRightAssign => ">>=",
    ShiftRightUnsignedAssign => ">>>=",
    BitAndAssign => "&=",
    BitOrAssign => "|=",
    BitXorAssign => "^=",
    LogicalAndAssign => "&&=",
    LogicalOrAssign => "||=",
    NullishAssign => "??=",
    Arrow => "=>",
    OptionalChain => "?.",
    Slash => "/",
    SlashAssign => "/=",
    RightBrace => "}",
}

// The candidate list grouped by first byte, derived from the same table at
// compile time: bit `i` of entry `b` is set when `PUNCTUATORS[i]` starts with
// byte `b`. A scan compares only these few candidates instead of every
// spelling, which dominated parser time before (`D-040`).
const _: () = assert!(PUNCTUATORS.len() <= u64::BITS as usize);
const CANDIDATES_BY_FIRST_BYTE: [u64; 128] = {
    let mut table = [0u64; 128];
    let mut index = 0;
    while index < PUNCTUATORS.len() {
        let first = PUNCTUATORS[index].spelling().as_bytes()[0];
        table[first as usize] |= 1 << index;
        index += 1;
    }
    table
};

/// A classified punctuation token with its exact original-source span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PunctuatorToken {
    /// The recognized grammar kind.
    pub kind: Punctuator,
    /// The consumed half-open range, between one and four ASCII bytes.
    pub span: Span,
}

/// Scans one ECMAScript punctuator in an explicit parser-selected context.
/// Returns `None` without consuming input for non-punctuation, comment openings,
/// slash in a regular-expression goal, and right brace in a template-tail goal.
/// A dot before a decimal digit is left for numeric scanning. Optional chaining
/// obeys the `?.` decimal-digit lookahead restriction. Type closing mode consumes
/// only one `>`; the caller must choose the context again for the next scan.
/// JSX, TypeScript decorators, private names, regular-expression bodies, and
/// template tails require their own scanners. This never infers those contexts
/// or validates a file. Time, lookahead, and storage are constant and bounded by
/// the fixed punctuation grammar; no allocation or source mutation occurs.
pub fn scan_js_punctuator(
    source: &SourceText,
    offset: usize,
    context: PunctuatorContext,
) -> Result<Option<PunctuatorToken>, LexicalError> {
    validate(source, offset)?;
    let rest = &source.as_str()[offset..];
    if rest.starts_with("//") || rest.starts_with("/*") {
        return Ok(None);
    }
    let (division, right_brace) = match context.goal {
        JsLexicalGoal::Div => (true, true),
        JsLexicalGoal::RegExp => (false, true),
        JsLexicalGoal::RegExpOrTemplateTail => (false, false),
        JsLexicalGoal::TemplateTail => (true, false),
    };
    if (!division && rest.starts_with('/')) || (!right_brace && rest.starts_with('}')) {
        return Ok(None);
    }
    if rest.starts_with('.') && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit) {
        return Ok(None);
    }
    match context.greater_than {
        GreaterThanMode::Single if rest.starts_with('>') => {
            return token(offset, Punctuator::GreaterThan);
        }
        GreaterThanMode::Single | GreaterThanMode::LongestMatch => {}
    }
    let mut candidates = rest
        .as_bytes()
        .first()
        .and_then(|&first| CANDIDATES_BY_FIRST_BYTE.get(usize::from(first)))
        .copied()
        .unwrap_or(0);
    let mut best = None;
    let mut best_length = 0;
    while candidates != 0 {
        let kind = PUNCTUATORS[candidates.trailing_zeros() as usize];
        candidates &= candidates - 1;
        if kind == Punctuator::OptionalChain
            && rest.as_bytes().get(2).is_some_and(u8::is_ascii_digit)
        {
            continue;
        }
        let spelling = kind.spelling();
        if spelling.len() > best_length && rest.starts_with(spelling) {
            best = Some(kind);
            best_length = spelling.len();
        }
    }
    match best {
        Some(kind) => token(offset, kind),
        None => Ok(None),
    }
}

fn token(offset: usize, kind: Punctuator) -> Result<Option<PunctuatorToken>, LexicalError> {
    Ok(Some(PunctuatorToken {
        kind,
        span: span(offset, offset + kind.spelling().len())?,
    }))
}
