//! Style rule selectors, as Dart Sass 1.105.2 reads them (`D-081`).
//!
//! Dart Sass keeps a selector as raw text while parsing and re-parses the
//! evaluated text: with its selector grammar, or inside `@keyframes` with its
//! keyframe grammar. Interpolation-free text is parsed here with those
//! evaluation-time grammars. Text with interpolation uses the parse-time
//! grammar that Dart Sass offers for interpolated selectors, and keeps
//! unchecked evidence because its evaluated text is parsed again. A selector
//! that neither grammar accepts stays raw text with evidence, as the
//! compiler's own parse step accepts it.

use super::{
    Parser, SassAttributeOperator as Operator, SassCombinator as Combinator, SassError,
    SassErrorKind as Error, SassNamespace as Namespace, SassNodeId as Id, SassNodeKind as Kind,
    SassSyntax, css_space, newline,
};

/// Which selector grammar evaluation applies to a style rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectorContext {
    /// The selector grammar.
    Normal,
    /// The keyframe grammar, directly inside a `@keyframes` rule.
    Keyframes,
    /// Decided by an include site or an interpolated at-rule name.
    Unknown,
}

const SELECTOR_PSEUDO_CLASSES: &[&str] = &[
    "not",
    "is",
    "matches",
    "where",
    "current",
    "any",
    "has",
    "host",
    "host-context",
];

impl Parser<'_> {
    /// Replaces the raw selector `header`, whose syntax starts at arena
    /// length `nodes` and comment count `comments`, with selector structure.
    pub(super) fn style_selector(
        &mut self,
        header: Id,
        nodes: usize,
        comments: usize,
        context: SelectorContext,
    ) -> Result<Id, SassError> {
        let span = self.nodes[header.0].span;
        let interpolated = self.nodes[nodes..]
            .iter()
            .any(|node| node.kind == Kind::Interpolation);
        if interpolated && context == SelectorContext::Keyframes {
            self.selector_evidence.push(header);
            return Ok(header);
        }
        let saved = (
            self.depth,
            self.expression_newlines,
            self.single_equals,
            self.stop_words,
            self.stop_depth,
        );
        self.nodes.truncate(nodes);
        self.comments.truncate(comments);
        // The raw header bounds the attempt, as the evaluated text would.
        let full = self.text;
        self.text = &full[..span.end()];
        self.pos = span.start();
        self.interpolated_selector = interpolated;
        let result = if context == SelectorContext::Keyframes {
            self.keyframe_selectors()
        } else {
            self.selector_list()
        };
        let complete = self.pos == span.end();
        self.text = full;
        self.interpolated_selector = false;
        match result {
            Ok(id) if complete => {
                if interpolated || context == SelectorContext::Unknown {
                    self.selector_evidence.push(id);
                }
                return Ok(id);
            }
            Err(error)
                if matches!(
                    error.kind,
                    Error::NodeLimit | Error::DepthLimit | Error::WorkLimit
                ) =>
            {
                return Err(error);
            }
            Ok(_) | Err(_) => {}
        }
        (
            self.depth,
            self.expression_newlines,
            self.single_equals,
            self.stop_words,
            self.stop_depth,
        ) = saved;
        self.nodes.truncate(nodes);
        self.comments.truncate(comments);
        self.pos = span.start();
        let (header, _) = self.raw_header(false)?;
        debug_assert_eq!(self.nodes[header.0].span, span);
        self.selector_evidence.push(header);
        Ok(header)
    }

    pub(super) fn selector_list(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        let first = self.complex_selector()?;
        let mut end = self.end(first);
        let mut children = vec![first];
        self.selector_space(false)?;
        while self.eat(",")? {
            end = self.pos;
            self.selector_space(true)?;
            if self.at(",") {
                continue;
            }
            if self.peek().is_none() {
                break;
            }
            let complex = self.complex_selector()?;
            end = self.end(complex);
            children.push(complex);
        }
        let id = self.node_between(start, end, Kind::SelectorList, children);
        self.depth -= 1;
        id
    }

    fn complex_selector(&mut self) -> Result<Id, SassError> {
        let mut children = Vec::new();
        let mut combinator = false;
        loop {
            self.selector_space(false)?;
            let Some(c) = self.peek() else { break };
            let explicit = match c {
                '+' => Some(Combinator::NextSibling),
                '>' => Some(Combinator::Child),
                '~' => Some(Combinator::FollowingSibling),
                _ => None,
            };
            if let Some(kind) = explicit {
                // Only evaluation tolerates consecutive combinators.
                if self.interpolated_selector && combinator {
                    break;
                }
                let start = self.pos;
                self.bump()?;
                children.push(self.node(start, Kind::Combinator(kind), Vec::new())?);
                combinator = true;
            } else if matches!(c, '[' | '.' | '#' | '%' | ':' | '&' | '*' | '|')
                || self.selector_identifier_ahead()
            {
                children.push(self.compound_selector()?);
                combinator = false;
                if self.at("&") {
                    return Err(self.error(Error::ExpectedSyntax));
                }
            } else {
                break;
            }
        }
        let (Some(&first), Some(&last)) = (children.first(), children.last()) else {
            return Err(self.error(Error::ExpectedSyntax));
        };
        let (start, end) = (self.start(first), self.end(last));
        self.node_between(start, end, Kind::ComplexSelector, children)
    }

    fn compound_selector(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut children = vec![self.simple_selector()?];
        while self
            .peek()
            .is_some_and(|c| matches!(c, '*' | '[' | '.' | '#' | '%' | ':'))
        {
            children.push(self.simple_selector()?);
        }
        self.node(start, Kind::CompoundSelector, children)
    }

    fn simple_selector(&mut self) -> Result<Id, SassError> {
        match self.peek() {
            Some('[') => self.attribute_selector(),
            Some('.') => self.named_selector(Kind::ClassSelector),
            Some('#') if !(self.interpolated_selector && self.at("#{")) => {
                self.named_selector(Kind::IdSelector)
            }
            Some('%') => self.named_selector(Kind::PlaceholderSelector),
            Some(':') => self.pseudo_selector(),
            Some('&') => self.parent_selector(),
            _ => self.type_or_universal_selector(),
        }
    }

    fn named_selector(&mut self, kind: Kind) -> Result<Id, SassError> {
        let start = self.pos;
        self.bump()?;
        let name = self.selector_identifier()?;
        self.node(start, kind, vec![name])
    }

    fn parent_selector(&mut self) -> Result<Id, SassError> {
        if !self.parent_selectors {
            return Err(self.error(Error::InvalidContext));
        }
        let start = self.pos;
        self.expect("&")?;
        let mut children = Vec::new();
        if self.peek().is_some_and(|c| name_char(c) || c == '\\')
            || (self.interpolated_selector && self.at("#{"))
        {
            let suffix = self.pos;
            let mut parts = Vec::new();
            self.selector_identifier_body(&mut parts)?;
            children.push(self.node(suffix, Kind::InterpolatedText, parts)?);
        }
        self.node(start, Kind::NestingSelector, children)
    }

    fn type_or_universal_selector(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let (namespace, mut children) = if self.eat("*")? {
            if !self.eat("|")? {
                return self.node(
                    start,
                    Kind::UniversalSelector {
                        namespace: Namespace::Unspecified,
                    },
                    Vec::new(),
                );
            }
            (Namespace::Any, Vec::new())
        } else if self.eat("|")? {
            (Namespace::Empty, Vec::new())
        } else {
            let first = self.selector_identifier()?;
            if !self.eat("|")? {
                return self.node(
                    start,
                    Kind::TypeSelector {
                        namespace: Namespace::Unspecified,
                    },
                    vec![first],
                );
            }
            (Namespace::Named, vec![first])
        };
        if self.eat("*")? {
            return self.node(start, Kind::UniversalSelector { namespace }, children);
        }
        children.push(self.selector_identifier()?);
        self.node(start, Kind::TypeSelector { namespace }, children)
    }

    fn attribute_selector(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect("[")?;
        self.selector_space(true)?;
        let (namespace, mut children) = if self.eat("*")? {
            self.expect("|")?;
            (Namespace::Any, vec![self.selector_identifier()?])
        } else if self.eat("|")? {
            (Namespace::Empty, vec![self.selector_identifier()?])
        } else {
            let first = self.selector_identifier()?;
            if self.at("|") && !self.at("|=") {
                self.bump()?;
                (Namespace::Named, vec![first, self.selector_identifier()?])
            } else {
                (Namespace::Unspecified, vec![first])
            }
        };
        self.selector_space(true)?;
        if self.eat("]")? {
            return self.node(
                start,
                Kind::AttributeSelector {
                    namespace,
                    operator: None,
                },
                children,
            );
        }
        let operator = match self.bump()? {
            Some('=') => Operator::Equal,
            Some('~') => Operator::Include,
            Some('|') => Operator::Dash,
            Some('^') => Operator::Prefix,
            Some('$') => Operator::Suffix,
            Some('*') => Operator::Substring,
            Some(_) | None => return Err(self.error(Error::ExpectedSyntax)),
        };
        if operator != Operator::Equal {
            self.expect("=")?;
        }
        self.selector_space(true)?;
        children.push(if self.peek().is_some_and(|c| matches!(c, '"' | '\'')) {
            if self.interpolated_selector {
                self.quoted()?
            } else {
                self.selector_string()?
            }
        } else {
            self.selector_identifier()?
        });
        self.selector_space(true)?;
        if self.interpolated_selector {
            if self.selector_identifier_ahead() {
                children.push(self.selector_identifier()?);
                self.selector_space(true)?;
            }
        } else if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            // Evaluation reads one letter and then requires the bracket.
            let modifier = self.pos;
            self.bump()?;
            children.push(self.node(modifier, Kind::Identifier, Vec::new())?);
        }
        self.expect("]")?;
        self.node(
            start,
            Kind::AttributeSelector {
                namespace,
                operator: Some(operator),
            },
            children,
        )
    }

    fn pseudo_selector(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        self.expect(":")?;
        let element = self.eat(":")?;
        let name = self.selector_identifier()?;
        let mut children = vec![name];
        if self.eat("(")? {
            self.selector_space(true)?;
            let plain = (self.nodes[name.0].kind == Kind::Identifier)
                .then(|| plain_identifier(self.spelling(name)));
            let unvendored = plain.as_deref().map(unvendor);
            if (element && unvendored == Some("slotted"))
                || (!element && unvendored.is_some_and(|n| SELECTOR_PSEUDO_CLASSES.contains(&n)))
            {
                children.push(self.selector_list()?);
            } else if !element && matches!(unvendored, Some("nth-child" | "nth-last-child")) {
                self.nth_argument(&mut children)?;
            } else {
                children.push(self.selector_value(true, false)?);
            }
            self.expect(")")?;
        }
        self.node(start, Kind::PseudoSelector { element }, children)
    }

    fn nth_argument(&mut self, children: &mut Vec<Id>) -> Result<(), SassError> {
        if self.interpolated_selector {
            children.push(self.selector_value(false, true)?);
            if !self.at(")") {
                children.push(self.selector_list()?);
            }
            return Ok(());
        }
        let start = self.pos;
        self.an_plus_b()?;
        let mut end = self.pos;
        self.selector_space(true)?;
        let of = !self.at(")")
            && self.text[..self.pos]
                .chars()
                .next_back()
                .is_some_and(css_space);
        if of {
            self.expect_identifier_text("of")?;
            end = self.pos;
        }
        children.push(self.node_between(start, end, Kind::InterpolatedText, Vec::new())?);
        if of {
            self.selector_space(true)?;
            children.push(self.selector_list()?);
        }
        Ok(())
    }

    fn an_plus_b(&mut self) -> Result<(), SassError> {
        match self.peek() {
            Some('e' | 'E') => return self.expect_identifier_text("even"),
            Some('o' | 'O') => return self.expect_identifier_text("odd"),
            Some('+' | '-') => {
                self.bump()?;
            }
            Some(_) | None => {}
        }
        if self.digits()? {
            self.selector_space(true)?;
            if !self.scan_identifier_char('n')? {
                return Ok(());
            }
        } else if !self.scan_identifier_char('n')? {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.selector_space(true)?;
        if !self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
            return Ok(());
        }
        self.bump()?;
        self.selector_space(true)?;
        if !self.digits()? {
            return Err(self.error(Error::ExpectedSyntax));
        }
        Ok(())
    }

    fn keyframe_selectors(&mut self) -> Result<Id, SassError> {
        let mut children = Vec::new();
        loop {
            self.selector_space(true)?;
            let start = self.pos;
            if self.identifier_ahead() {
                if !self.scan_identifier_text("from")? {
                    self.expect_identifier_text("to")?;
                }
            } else {
                self.percentage()?;
            }
            children.push(self.node(start, Kind::KeyframeSelector, Vec::new())?);
            self.selector_space(true)?;
            if !self.eat(",")? {
                break;
            }
        }
        let start = self.start(children[0]);
        let end = self.end(children[children.len() - 1]);
        self.node_between(start, end, Kind::KeyframeSelectorList, children)
    }

    fn percentage(&mut self) -> Result<(), SassError> {
        self.eat("+")?;
        if !self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.digits()?;
        if self.eat(".")? {
            self.digits()?;
        }
        if self.scan_identifier_char('e')? {
            if self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
                self.bump()?;
            }
            if !self.digits()? {
                return Err(self.error(Error::ExpectedSyntax));
            }
        }
        self.expect("%")
    }

    fn digits(&mut self) -> Result<bool, SassError> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump()?;
        }
        Ok(self.pos != start)
    }

    /// A pseudo-selector argument or `nth-*` prefix, kept as raw text.
    fn selector_value(&mut self, allow_empty: bool, end_after_of: bool) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        let interpolated = self.interpolated_selector;
        let mut parts = Vec::new();
        let mut brackets = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                '\\' => self.selector_escape()?,
                '"' | '\'' => parts.push(if interpolated {
                    self.quoted()?
                } else {
                    self.selector_string()?
                }),
                '/' if self.at("/*") => self.comment(false, false)?,
                // The compiler's raw selector text ends at these characters,
                // even inside brackets, and a silent comment runs to the line
                // end. Such selectors stay unchecked.
                '/' if self.at("//") => return Err(self.error(Error::ExpectedSyntax)),
                '!' | ';' | '{' | '}' => return Err(self.error(Error::ExpectedSyntax)),
                '#' if interpolated && self.at("#{") => {
                    parts.push(self.interpolation()?);
                    self.selector_identifier_body(&mut parts)?;
                }
                c if newline(c)
                    && interpolated
                    && !end_after_of
                    && self.syntax == SassSyntax::Indented
                    && brackets.is_empty() =>
                {
                    break;
                }
                '(' | '[' => {
                    brackets.push(if c == '(' { ')' } else { ']' });
                    self.bump()?;
                }
                ')' | ']' => match brackets.pop() {
                    None => break,
                    Some(close) if close == c => {
                        self.bump()?;
                    }
                    Some(_) => return Err(self.error(Error::ExpectedSyntax)),
                },
                'o' | 'O' if end_after_of && brackets.is_empty() => {
                    if self.scan_identifier_text("of")? {
                        break;
                    }
                    self.bump()?;
                }
                'u' | 'U' if self.selector_url(&mut parts)? => {}
                _ if self.identifier_ahead() => self.skip_identifier()?,
                _ => {
                    self.bump()?;
                }
            }
        }
        if !brackets.is_empty() || (!allow_empty && self.pos == start) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        let end = start + self.text[start..self.pos].trim_end_matches(css_space).len();
        let id = self.node_between(start, end, Kind::InterpolatedText, parts);
        self.depth -= 1;
        id
    }

    /// Consumes a raw `url(...)`, returning `false` without consuming
    /// anything when the contents are not a raw URL.
    pub(super) fn selector_url(&mut self, parts: &mut Vec<Id>) -> Result<bool, SassError> {
        let start = self.pos;
        let nodes = self.nodes.len();
        let interpolated = self.interpolated_selector;
        // Evaluation matches `url` without case; the parse-time grammar
        // compares the decoded name and also accepts `url-prefix`.
        let named = if interpolated {
            let name_start = self.pos;
            self.skip_identifier()?;
            let name = plain_identifier(&self.text[name_start..self.pos]);
            if name != "url" && name != "url-prefix" {
                return Ok(true);
            }
            true
        } else {
            self.scan_identifier_text("url")?
        };
        if named && self.eat("(")? {
            while self.peek().is_some_and(css_space) {
                self.bump()?;
            }
            let mut contents = Vec::new();
            while let Some(c) = self.peek() {
                match c {
                    '\\' => self.selector_escape()?,
                    '#' if interpolated && self.at("#{") => contents.push(self.interpolation()?),
                    ')' => {
                        self.bump()?;
                        parts.extend(contents);
                        return Ok(true);
                    }
                    c if css_space(c) => {
                        while self.peek().is_some_and(css_space) {
                            self.bump()?;
                        }
                        if !self.at(")") {
                            break;
                        }
                    }
                    '!' if interpolated => {
                        self.bump()?;
                    }
                    '%' | '&' | '#' | '*'..='~' | '\u{80}'.. => {
                        self.bump()?;
                    }
                    _ => break,
                }
            }
        }
        self.nodes.truncate(nodes);
        self.pos = start;
        self.bump()?;
        Ok(true)
    }

    /// A quoted string without interpolation, as evaluation reads it.
    fn selector_string(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let quote = self.bump()?;
        loop {
            match self.peek() {
                None => return Err(self.error(Error::Unterminated)),
                Some(c) if Some(c) == quote => {
                    self.bump()?;
                    break;
                }
                Some(c) if newline(c) => return Err(self.error(Error::Unterminated)),
                Some('\\') => {
                    self.bump()?;
                    match self.peek() {
                        // A backslash before a newline skips exactly one code
                        // point, so a CR LF pair leaves its LF in the string.
                        Some(c) if newline(c) => {
                            self.bump()?;
                        }
                        Some(c) if c.is_ascii_hexdigit() => {
                            for _ in 0..6 {
                                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                                    break;
                                }
                                self.bump()?;
                            }
                            if self.peek().is_some_and(css_space) {
                                self.bump()?;
                            }
                        }
                        Some(_) => {
                            self.bump()?;
                        }
                        None => {}
                    }
                }
                Some(_) => {
                    self.bump()?;
                }
            }
        }
        self.node(start, Kind::String { quoted: true }, Vec::new())
    }

    fn selector_identifier(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        let dashed = self.eat("-")?;
        if !(dashed && self.eat("-")?) {
            match self.peek() {
                Some(c) if name_start_char(c) => {
                    self.bump()?;
                }
                Some('\\') => self.selector_escape()?,
                Some('#') if self.interpolated_selector && self.at("#{") => {
                    parts.push(self.interpolation()?);
                }
                Some(_) | None => return Err(self.error(Error::ExpectedIdentifier)),
            }
        }
        self.selector_identifier_body(&mut parts)?;
        let kind = if parts.is_empty() {
            Kind::Identifier
        } else {
            Kind::InterpolatedText
        };
        self.node(start, kind, parts)
    }

    pub(super) fn selector_identifier_body(
        &mut self,
        parts: &mut Vec<Id>,
    ) -> Result<(), SassError> {
        loop {
            match self.peek() {
                Some(c) if name_char(c) => {
                    self.bump()?;
                }
                Some('\\') => self.selector_escape()?,
                Some('#') if self.interpolated_selector && self.at("#{") => {
                    parts.push(self.interpolation()?);
                }
                Some(_) | None => return Ok(()),
            }
        }
    }

    /// Consumes a plain identifier without building a node.
    pub(super) fn skip_identifier(&mut self) -> Result<(), SassError> {
        if self.eat("-")? && self.eat("-")? {
        } else if self.at("\\") {
            self.selector_escape()?;
        } else {
            self.bump()?;
        }
        loop {
            match self.peek() {
                Some(c) if name_char(c) => {
                    self.bump()?;
                }
                Some('\\') => self.selector_escape()?,
                Some(_) | None => return Ok(()),
            }
        }
    }

    /// An identifier escape, which unlike a string escape needs a code point.
    fn selector_escape(&mut self) -> Result<(), SassError> {
        self.expect("\\")?;
        match self.peek() {
            None => Err(self.error(Error::Unterminated)),
            Some(c) if newline(c) => Err(self.error(Error::Unterminated)),
            Some(c) if c.is_ascii_hexdigit() => {
                let mut value = 0;
                for _ in 0..6 {
                    let Some(digit) = self.peek().and_then(|c| c.to_digit(16)) else {
                        break;
                    };
                    self.bump()?;
                    value = value * 16 + digit;
                }
                // Exactly one whitespace code point ends the escape.
                if self.peek().is_some_and(css_space) {
                    self.bump()?;
                }
                if value > 0x10ffff {
                    return Err(self.error(Error::ExpectedSyntax));
                }
                Ok(())
            }
            Some(_) => {
                self.bump()?;
                Ok(())
            }
        }
    }

    /// Whether a plain CSS identifier starts here.
    pub(super) fn identifier_ahead(&self) -> bool {
        let mut chars = self.text[self.pos..].chars();
        match chars.next() {
            Some('-') => chars
                .next()
                .is_some_and(|c| name_start_char(c) || c == '\\' || c == '-'),
            Some(c) => name_start_char(c) || c == '\\',
            None => false,
        }
    }

    fn selector_identifier_ahead(&self) -> bool {
        if !self.interpolated_selector {
            return self.identifier_ahead();
        }
        let rest = &self.text[self.pos..];
        self.identifier_ahead() || rest.starts_with("#{") || rest.starts_with("-#{")
    }

    /// Matches one character or escape against an ASCII letter, ignoring case.
    fn scan_identifier_char(&mut self, letter: char) -> Result<bool, SassError> {
        let start = self.pos;
        let actual = match self.peek() {
            Some('\\') => {
                self.bump()?;
                match self.peek() {
                    None => '\u{fffd}',
                    Some(c) if newline(c) => return Err(self.error(Error::Unterminated)),
                    Some(c) if c.is_ascii_hexdigit() => {
                        let mut value = 0;
                        for _ in 0..6 {
                            let Some(digit) = self.peek().and_then(|c| c.to_digit(16)) else {
                                break;
                            };
                            self.bump()?;
                            value = value * 16 + digit;
                        }
                        if self.peek().is_some_and(css_space) {
                            self.bump()?;
                        }
                        char::from_u32(value).unwrap_or('\u{fffd}')
                    }
                    Some(_) => self.bump()?.unwrap_or('\u{fffd}'),
                }
            }
            Some(c) => {
                self.bump()?;
                c
            }
            None => return Ok(false),
        };
        if actual.eq_ignore_ascii_case(&letter) {
            Ok(true)
        } else {
            self.pos = start;
            Ok(false)
        }
    }

    pub(super) fn identifier_text(&mut self, text: &str) -> Result<bool, SassError> {
        for letter in text.chars() {
            if !self.scan_identifier_char(letter)? {
                return Ok(false);
            }
        }
        Ok(!self.peek().is_some_and(|c| name_char(c) || c == '\\'))
    }

    fn scan_identifier_text(&mut self, text: &str) -> Result<bool, SassError> {
        if !self.identifier_ahead() {
            return Ok(false);
        }
        let start = self.pos;
        if self.identifier_text(text)? {
            Ok(true)
        } else {
            self.pos = start;
            Ok(false)
        }
    }

    fn expect_identifier_text(&mut self, text: &str) -> Result<(), SassError> {
        if self.identifier_text(text)? {
            Ok(())
        } else {
            Err(self.error(Error::ExpectedIdentifier))
        }
    }

    /// Whitespace and comments. Evaluation-time grammars read text whose
    /// lines were already joined, so only the parse-time grammar stops at
    /// indented line ends unless `lines` is set.
    fn selector_space(&mut self, lines: bool) -> Result<(), SassError> {
        let lines = lines || !self.interpolated_selector || self.syntax == SassSyntax::Scss;
        loop {
            match self.peek() {
                Some(c) if css_space(c) && (lines || !newline(c)) => {
                    self.bump()?;
                }
                Some('/') if self.at("//") => self.comment(true, false)?,
                Some('/') if self.at("/*") => {
                    let glued = self.text[..self.pos]
                        .chars()
                        .next_back()
                        .is_some_and(|c| !css_space(c));
                    self.comment(false, false)?;
                    // Indented Sass removes comments before evaluation, which
                    // joins the text around a comment without whitespace.
                    if glued
                        && !self.interpolated_selector
                        && self.syntax == SassSyntax::Indented
                        && self.peek().is_some_and(|c| !css_space(c))
                    {
                        return Err(self.error(Error::ExpectedSyntax));
                    }
                }
                Some(_) | None => return Ok(()),
            }
        }
    }
}

pub(super) fn name_start_char(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c >= '\u{80}'
}

fn name_char(c: char) -> bool {
    name_start_char(c) || c.is_ascii_digit() || c == '-'
}

/// Decodes escapes the way Dart Sass builds an identifier's plain text:
/// escapes of name characters become the character; any other escape keeps a
/// backslash, so it can never match a keyword.
pub(super) fn plain_identifier(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut at_start = true;
    if chars.next_if_eq(&'-').is_some() {
        result.push('-');
        if chars.next_if_eq(&'-').is_some() {
            result.push('-');
            at_start = false;
        }
    }
    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut value = 0;
            let mut digits = 0;
            while digits < 6 {
                let Some(digit) = chars.peek().and_then(|c| c.to_digit(16)) else {
                    break;
                };
                chars.next();
                value = value * 16 + digit;
                digits += 1;
            }
            let decoded = if digits == 0 {
                chars.next()
            } else {
                chars.next_if(|&c| css_space(c));
                char::from_u32(value)
            };
            match decoded {
                Some(c) if (at_start && name_start_char(c)) || (!at_start && name_char(c)) => {
                    result.push(c);
                }
                _ => result.push('\\'),
            }
        } else {
            result.push(c);
        }
        at_start = false;
    }
    result
}

/// Removes a vendor prefix such as `-webkit-`, as Dart Sass does.
pub(super) fn unvendor(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'-' || bytes[1] == b'-' {
        return name;
    }
    match bytes[2..].iter().position(|&b| b == b'-') {
        Some(index) => &name[index + 3..],
        None => name,
    }
}
