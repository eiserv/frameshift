use frameshift_core::{SourceText, Span};

use crate::lexical::{
    LexicalError, LexicalErrorKind, LexicalLimits, error, is_line_terminator, span, validate,
};

/// Where the parser resumes scanning a template literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplatePosition {
    /// Start at the opening backtick, producing a head or no-substitution token.
    Start,
    /// Start at the `}` that the parser identified as closing a substitution.
    /// The parser must have selected a template-tail lexical goal.
    Continuation,
}

/// The parser-selected template escape policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateMode {
    /// Invalid escapes are early syntax errors, returned without a token.
    Untagged,
    /// Invalid escapes are allowed but make this segment's cooked value undefined.
    Tagged,
}

/// Explicit parser context for one template segment. No context is inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateContext {
    /// Whether scanning starts a template or resumes after a substitution.
    pub position: TemplatePosition,
    /// Whether a tag permits escapes with undefined cooked values.
    pub mode: TemplateMode,
}

/// A template lexical component, independent of the embedded expression grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateKind {
    /// A complete template bounded by backticks, without substitutions.
    NoSubstitution,
    /// The opening backtick, text, and first `${`.
    Head,
    /// A substitution-closing `}`, text, and another `${`.
    Middle,
    /// A substitution-closing `}`, text, and the final backtick.
    Tail,
}

/// One template segment with original-source spans and escape evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateToken {
    /// The recognized template grammar kind.
    pub kind: TemplateKind,
    /// Exact source span, including the leading and trailing delimiters.
    pub span: Span,
    /// Text between delimiters, including escapes and original line endings.
    /// This is source spelling, not ECMAScript's normalized raw or cooked value.
    pub content_span: Span,
    /// First invalid escape's backslash offset, only present in tagged mode.
    /// `Some` means the cooked value is undefined. `None` does not decode a value.
    pub invalid_escape: Option<usize>,
    /// Whether the source segment contains a line terminator, including continuations.
    pub has_line_terminator: bool,
}

/// Scans one ECMAScript template lexical component in explicit parser context.
/// Stops after an unescaped `${` or backtick; it never scans substitutions or
/// decides which right brace closes one. Supports character, hex, Unicode,
/// and line-continuation escapes. Untagged invalid escapes return an error;
/// tagged invalid escapes retain their first offset and still respect delimiters.
/// No values are decoded or normalized, and no source is modified. Offsets are
/// validated first, then the selected opening delimiter, then per-scalar limits.
/// Iterative, allocation-free, and linear in consumed bytes. Aggregate template
/// size, token count, nesting, and expression validity remain parser concerns.
pub fn scan_js_template(
    source: &SourceText,
    offset: usize,
    context: TemplateContext,
    limits: LexicalLimits,
) -> Result<TemplateToken, LexicalError> {
    validate(source, offset)?;
    let text = source.as_str();
    let delimiter = match context.position {
        TemplatePosition::Start => '`',
        TemplatePosition::Continuation => '}',
    };
    if !text[offset..].starts_with(delimiter) {
        return Err(error(offset, LexicalErrorKind::ExpectedTemplateDelimiter));
    }
    let mut scan = TemplateScan {
        text,
        start: offset,
        cursor: offset,
        max_bytes: limits.max_template_bytes,
        has_line_terminator: false,
    };
    scan.advance()?;
    let content_start = scan.cursor;
    let mut invalid_escape = None;
    while let Some(ch) = scan.peek() {
        let content_end = scan.cursor;
        let substitution = scan.rest().starts_with("${");
        if ch == '`' || substitution {
            scan.advance()?;
            if substitution {
                scan.advance()?;
            }
            let kind = match (context.position, substitution) {
                (TemplatePosition::Start, false) => TemplateKind::NoSubstitution,
                (TemplatePosition::Start, true) => TemplateKind::Head,
                (TemplatePosition::Continuation, true) => TemplateKind::Middle,
                (TemplatePosition::Continuation, false) => TemplateKind::Tail,
            };
            return Ok(TemplateToken {
                kind,
                span: span(offset, scan.cursor)?,
                content_span: span(content_start, content_end)?,
                invalid_escape,
                has_line_terminator: scan.has_line_terminator,
            });
        }
        scan.advance()?;
        if ch == '\\' && !scan.escape()? {
            match context.mode {
                TemplateMode::Untagged => {
                    return Err(error(content_end, LexicalErrorKind::InvalidTemplateEscape));
                }
                TemplateMode::Tagged => {
                    invalid_escape.get_or_insert(content_end);
                }
            }
        }
    }
    Err(error(offset, LexicalErrorKind::UnterminatedTemplate))
}

struct TemplateScan<'a> {
    text: &'a str,
    start: usize,
    cursor: usize,
    max_bytes: usize,
    has_line_terminator: bool,
}

impl TemplateScan<'_> {
    fn rest(&self) -> &str {
        &self.text[self.cursor..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn advance(&mut self) -> Result<(), LexicalError> {
        let ch = self
            .peek()
            .ok_or_else(|| error(self.start, LexicalErrorKind::UnterminatedTemplate))?;
        if self.cursor - self.start + ch.len_utf8() > self.max_bytes {
            return Err(error(self.cursor, LexicalErrorKind::TemplateLimit));
        }
        self.has_line_terminator |= is_line_terminator(ch);
        self.cursor += ch.len_utf8();
        Ok(())
    }

    // Invalid escape prefixes leave the next nonmatching character untouched,
    // so a tagged template's delimiter or next escape is still recognized.
    fn escape(&mut self) -> Result<bool, LexicalError> {
        let ch = self
            .peek()
            .ok_or_else(|| error(self.start, LexicalErrorKind::UnterminatedTemplate))?;
        self.advance()?;
        match ch {
            'x' => self.hex_digits(2),
            'u' if self.rest().starts_with('{') => self.code_point(),
            'u' => self.hex_digits(4),
            '0' if self.peek().is_some_and(|next| next.is_ascii_digit()) => {
                self.advance()?;
                Ok(false)
            }
            '1'..='9' => Ok(false),
            '\r' if self.rest().starts_with('\n') => {
                self.advance()?;
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn hex_digits(&mut self, count: usize) -> Result<bool, LexicalError> {
        for _ in 0..count {
            if !self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                return Ok(false);
            }
            self.advance()?;
        }
        Ok(true)
    }

    fn code_point(&mut self) -> Result<bool, LexicalError> {
        self.advance()?;
        let mut value = 0u32;
        let mut has_digit = false;
        while let Some(digit) = self.peek().and_then(|ch| ch.to_digit(16)) {
            // Keep scanning after overflow: later escapes and delimiters must
            // retain their meaning. Capping also handles arbitrarily long zeros.
            value = (value * 16 + digit).min(0x110000);
            has_digit = true;
            self.advance()?;
        }
        if has_digit && value <= 0x10ffff && self.rest().starts_with('}') {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
