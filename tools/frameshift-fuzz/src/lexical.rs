//! The lexical scanner target (`D-044`).
//!
//! Every scanner runs at every character boundary of the input, in each of
//! its modes and twice, and its token or error is checked against the
//! contracts in section 4 of `docs/IMPLEMENTATION.md`: exact spans that
//! start at the offset, spellings that match the token kind, mode
//! relationships, and error offsets at character boundaries at or after the
//! offset. Regular-expression literals have their own target.

use std::fmt::Debug;

use frameshift_engine::{
    CommentKind, GreaterThanMode, IdentifierKind, JsLexicalGoal, LegacyOctal, LexicalError,
    LexicalLimits, PunctuatorContext, SourceGoal, SourceText, Span, TemplateContext, TemplateKind,
    TemplateMode, TemplatePosition, scan_js_identifier, scan_js_number, scan_js_punctuator,
    scan_js_string, scan_js_template, scan_js_trivia, scan_jsx_identifier, scan_jsx_string,
    scan_jsx_text,
};

use crate::targets::{self, GOALS};

const LINE_TERMINATORS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

/// Runs every scanner at every character boundary of the input.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let source = targets::source(text)?;
    for offset in (0..=text.len()).filter(|&offset| text.is_char_boundary(offset)) {
        let scan = Scan {
            text,
            source: &source,
            offset,
            limits: LexicalLimits::default(),
        };
        for (name, result) in [
            ("trivia", scan.trivia()),
            ("string", scan.string()),
            ("number", scan.number()),
            ("punctuator", scan.punctuator()),
            ("template", scan.template()),
            ("identifier", scan.identifier()),
            ("JSX identifier", scan.jsx_identifier()),
            ("JSX string", scan.jsx_string()),
            ("JSX text", scan.jsx_text()),
        ] {
            result.map_err(|error| format!("{name} at {offset}: {error}"))?;
        }
    }
    Ok(())
}

struct Scan<'a> {
    text: &'a str,
    source: &'a SourceText,
    offset: usize,
    limits: LexicalLimits,
}

impl<'a> Scan<'a> {
    // Scans twice and checks that the result repeats and that an error points
    // to a character boundary at or after the offset.
    fn twice<T: PartialEq + Debug>(
        &self,
        scan: impl Fn() -> Result<T, LexicalError>,
    ) -> Result<Result<T, LexicalError>, String> {
        let first = scan();
        let second = scan();
        if first != second {
            return Err(format!("repeated scan returned {second:?} after {first:?}"));
        }
        if let Err(error) = &first
            && (error.offset < self.offset || !self.text.is_char_boundary(error.offset))
        {
            return Err(format!("error offset outside the scanned text: {error:?}"));
        }
        Ok(first)
    }

    // Returns the spelling of a token span that must start at the offset.
    fn spelling(&self, span: Span) -> Result<&'a str, String> {
        if span.start() != self.offset {
            return Err(format!("{span:?} does not start at the offset"));
        }
        self.source
            .slice(span)
            .map_err(|error| format!("{span:?}: {error:?}"))
    }

    fn trivia(&self) -> Result<(), String> {
        let mut ends = [None; 2];
        for (goal, end) in GOALS.into_iter().zip(&mut ends) {
            let Ok(trivia) =
                self.twice(|| scan_js_trivia(self.source, self.offset, goal, self.limits))?
            else {
                continue;
            };
            let consumed = self.spelling(trivia.span)?;
            if trivia.has_line_terminator != consumed.contains(LINE_TERMINATORS) {
                return Err(format!("line terminator flag is wrong for {trivia:?}"));
            }
            let mut previous = self.offset;
            for comment in &trivia.comments {
                if comment.span.start() < previous || comment.span.end() > trivia.span.end() {
                    return Err(format!("{comment:?} is not ordered inside {trivia:?}"));
                }
                previous = comment.span.end();
                let spelling = self
                    .source
                    .slice(comment.span)
                    .map_err(|error| format!("{comment:?}: {error:?}"))?;
                let single_line = !spelling.contains(LINE_TERMINATORS);
                let script = goal == SourceGoal::Script;
                let valid = match comment.kind {
                    CommentKind::Line => spelling.starts_with("//") && single_line,
                    CommentKind::Block => {
                        spelling.len() >= 4
                            && spelling.starts_with("/*")
                            && spelling.ends_with("*/")
                    }
                    CommentKind::HtmlOpen => script && spelling.starts_with("<!--") && single_line,
                    CommentKind::HtmlClose => script && spelling.starts_with("-->") && single_line,
                    CommentKind::Hashbang => false,
                };
                if !valid {
                    return Err(format!(
                        "{comment:?} does not match its spelling in {goal:?}"
                    ));
                }
            }
            *end = Some(trivia.span.end());
        }
        // Script trivia also consumes HTML-like comments, so it extends module
        // trivia.
        if let [Some(script), Some(module)] = ends
            && script < module
        {
            return Err(format!(
                "script trivia ends at {script}, before module trivia at {module}"
            ));
        }
        Ok(())
    }

    fn string(&self) -> Result<(), String> {
        let permitted = self
            .twice(|| scan_js_string(self.source, self.offset, LegacyOctal::Permit, self.limits))?;
        let rejected = self
            .twice(|| scan_js_string(self.source, self.offset, LegacyOctal::Reject, self.limits))?;
        if let Ok(token) = &permitted {
            let spelling = self.spelling(token.span)?;
            let quote = spelling
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'));
            if spelling.len() < 2 || !quote.is_some_and(|quote| spelling.ends_with(quote)) {
                return Err(format!("{token:?} is not a quoted string"));
            }
            if let Some(escape) = token.legacy_octal_escape
                && !(token.span.start() < escape
                    && escape < token.span.end()
                    && self.text[escape..].starts_with('\\'))
            {
                return Err(format!(
                    "{token:?} reports a legacy escape outside its body"
                ));
            }
        }
        if let Ok(token) = rejected
            && (token.legacy_octal_escape.is_some() || permitted != Ok(token))
        {
            return Err(format!(
                "strict {token:?} differs from sloppy {permitted:?}"
            ));
        }
        Ok(())
    }

    fn number(&self) -> Result<(), String> {
        let permitted = self
            .twice(|| scan_js_number(self.source, self.offset, LegacyOctal::Permit, self.limits))?;
        let rejected = self
            .twice(|| scan_js_number(self.source, self.offset, LegacyOctal::Reject, self.limits))?;
        if let Ok(literal) = &permitted {
            let spelling = self.spelling(literal.span)?;
            let digit = spelling
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit());
            let dot = spelling.starts_with('.')
                && spelling.as_bytes().get(1).is_some_and(u8::is_ascii_digit);
            let legacy = spelling.len() > 1
                && spelling.starts_with('0')
                && spelling.as_bytes()[1].is_ascii_digit();
            if !(digit || dot) || literal.legacy_octal != legacy {
                return Err(format!(
                    "{literal:?} does not match its spelling {spelling:?}"
                ));
            }
        }
        if let Ok(literal) = rejected
            && (literal.legacy_octal || permitted != Ok(literal))
        {
            return Err(format!(
                "strict {literal:?} differs from sloppy {permitted:?}"
            ));
        }
        Ok(())
    }

    fn punctuator(&self) -> Result<(), String> {
        for goal in [
            JsLexicalGoal::Div,
            JsLexicalGoal::RegExp,
            JsLexicalGoal::RegExpOrTemplateTail,
            JsLexicalGoal::TemplateTail,
        ] {
            for greater_than in [GreaterThanMode::LongestMatch, GreaterThanMode::Single] {
                let context = PunctuatorContext { goal, greater_than };
                let token = self
                    .twice(|| scan_js_punctuator(self.source, self.offset, context))?
                    .map_err(|error| format!("{error:?} at a character boundary"))?;
                let Some(token) = token else {
                    continue;
                };
                let spelling = self.spelling(token.span)?;
                if spelling != token.kind.spelling()
                    || (greater_than == GreaterThanMode::Single
                        && spelling.starts_with('>')
                        && spelling != ">")
                {
                    return Err(format!(
                        "{token:?} does not match {spelling:?} in {context:?}"
                    ));
                }
            }
        }
        Ok(())
    }

    fn template(&self) -> Result<(), String> {
        for position in [TemplatePosition::Start, TemplatePosition::Continuation] {
            let scan = |mode| {
                let context = TemplateContext { position, mode };
                self.twice(|| scan_js_template(self.source, self.offset, context, self.limits))
            };
            let tagged = scan(TemplateMode::Tagged)?;
            let untagged = scan(TemplateMode::Untagged)?;
            if let Ok(token) = &tagged {
                let spelling = self.spelling(token.span)?;
                let content = token.content_span;
                let opening = match position {
                    TemplatePosition::Start => '`',
                    TemplatePosition::Continuation => '}',
                };
                let (kinds_match, closing) = match token.kind {
                    TemplateKind::NoSubstitution => (position == TemplatePosition::Start, "`"),
                    TemplateKind::Head => (position == TemplatePosition::Start, "${"),
                    TemplateKind::Middle => (position == TemplatePosition::Continuation, "${"),
                    TemplateKind::Tail => (position == TemplatePosition::Continuation, "`"),
                };
                let body = self
                    .source
                    .slice(content)
                    .map_err(|error| format!("{token:?}: {error:?}"))?;
                if !kinds_match
                    || !spelling.starts_with(opening)
                    || !spelling.ends_with(closing)
                    || content.start() != self.offset + 1
                    || content.end() + closing.len() != token.span.end()
                    || token.has_line_terminator != body.contains(LINE_TERMINATORS)
                {
                    return Err(format!("{token:?} does not match its spelling"));
                }
                if let Some(escape) = token.invalid_escape
                    && !(content.start() <= escape
                        && escape < content.end()
                        && self.text[escape..].starts_with('\\'))
                {
                    return Err(format!("{token:?} reports an escape outside its content"));
                }
            }
            if let Ok(token) = untagged
                && (token.invalid_escape.is_some() || tagged != Ok(token))
            {
                return Err(format!("untagged {token:?} differs from tagged {tagged:?}"));
            }
        }
        Ok(())
    }

    fn identifier(&self) -> Result<(), String> {
        let Ok(token) = self.twice(|| scan_js_identifier(self.source, self.offset, self.limits))?
        else {
            return Ok(());
        };
        self.spelling(token.span)?;
        let private = self.text[self.offset..].starts_with('#');
        let name = token.name_span;
        let spelling = self
            .source
            .slice(name)
            .map_err(|error| format!("{token:?}: {error:?}"))?;
        if (token.kind == IdentifierKind::PrivateName) != private
            || name.start() != self.offset + usize::from(private)
            || name.end() != token.span.end()
            || name.is_empty()
            || token.has_escape != spelling.contains('\\')
            || (private && token.reserved_word.is_some())
            || token
                .reserved_word
                .is_some_and(|word| !token.has_escape && word.spelling() != spelling)
        {
            return Err(format!(
                "{token:?} does not match its spelling {spelling:?}"
            ));
        }
        Ok(())
    }

    fn jsx_identifier(&self) -> Result<(), String> {
        if let Ok(span) =
            self.twice(|| scan_jsx_identifier(self.source, self.offset, self.limits))?
        {
            let spelling = self.spelling(span)?;
            if spelling.is_empty() || spelling.contains('\\') {
                return Err(format!("{span:?} is not a JSX name: {spelling:?}"));
            }
        }
        Ok(())
    }

    fn jsx_string(&self) -> Result<(), String> {
        if let Ok(token) = self.twice(|| scan_jsx_string(self.source, self.offset, self.limits))? {
            let spelling = self.spelling(token.span)?;
            let quote = spelling
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'));
            if spelling.len() < 2
                || !quote.is_some_and(|quote| spelling[1..].find(quote) == Some(spelling.len() - 2))
                || token.content_span.start() != token.span.start() + 1
                || token.content_span.end() + 1 != token.span.end()
            {
                return Err(format!("{token:?} is not a JSX string: {spelling:?}"));
            }
        }
        Ok(())
    }

    fn jsx_text(&self) -> Result<(), String> {
        if let Ok(span) = self.twice(|| scan_jsx_text(self.source, self.offset, self.limits))? {
            let spelling = self.spelling(span)?;
            let next = self.text[span.end()..].chars().next();
            if spelling.contains(['{', '<', '>', '}']) || !matches!(next, None | Some('{' | '<')) {
                return Err(format!("{span:?} is not JSX text: {spelling:?}"));
            }
        }
        Ok(())
    }
}
