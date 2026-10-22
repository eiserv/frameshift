//! `@extend` targets, as Dart Sass 1.105.2 reads and evaluates them (`D-084`).
//!
//! Dart Sass keeps a target as raw text while parsing. Evaluation parses the
//! evaluated text with its selector grammar, without parent selectors, and
//! then requires every complex selector to be a single simple selector. An
//! interpolation-free target that passes both checks is structured. Any
//! other target stays raw text with evidence, because the compiler's parse
//! step accepts it and evaluation may never reach it.

use super::{
    Parser, SassComment, SassError, SassErrorKind as Error, SassNodeId as Id, SassNodeKind as Kind,
    SassSyntax, css_space, newline,
};

impl Parser<'_> {
    /// Reads an `@extend` target and its optional `!optional` flag.
    pub(super) fn extend_header(&mut self, children: &mut Vec<Id>) -> Result<(), SassError> {
        // The target may start on a later line, even in indented syntax.
        self.skip(true)?;
        let nodes = self.nodes.len();
        let comments = self.comments.len();
        let target = self.almost_any_value()?;
        children.push(self.extend_target(target, nodes, comments)?);
        if self.at("!") {
            let start = self.pos;
            self.bump()?;
            // The reference matches each letter, or an escape of it, without
            // case, and then requires the identifier to end.
            if !self.identifier_text("optional")? {
                return Err(self.error(Error::ExpectedIdentifier));
            }
            children.push(self.node(start, Kind::Flag, Vec::new())?);
        }
        Ok(())
    }

    /// Dart Sass's `almostAnyValue`: raw text up to `!`, `;`, `{`, `}`, or an
    /// indented line end outside brackets. The span excludes trailing
    /// whitespace, as evaluation trims it, but keeps an escaped space.
    fn almost_any_value(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        let mut brackets = Vec::new();
        // The URL and interpolated-identifier readers are shared with the
        // parse-time selector grammar, whose rules this reader follows.
        let interpolated = self.interpolated_selector;
        self.interpolated_selector = true;
        let result = self.almost_any_value_parts(&mut parts, &mut brackets);
        self.interpolated_selector = interpolated;
        result?;
        let text = &self.text[start..self.pos];
        let mut end = start + text.trim_end_matches(css_space).len();
        if end > start + 1 && end < self.pos && self.text[..end].ends_with('\\') {
            end += self.text[end..].chars().next().map_or(0, char::len_utf8);
        }
        self.node_between(start, end, Kind::InterpolatedText, parts)
    }

    fn almost_any_value_parts(
        &mut self,
        parts: &mut Vec<Id>,
        brackets: &mut Vec<char>,
    ) -> Result<(), SassError> {
        while let Some(c) = self.peek() {
            match c {
                '\\' => {
                    // The escape is copied raw, as two characters.
                    self.bump()?;
                    if self.bump()?.is_none() {
                        return Err(self.error(Error::Unterminated));
                    }
                }
                '"' | '\'' => parts.push(self.quoted()?),
                '/' if self.at("/*") => self.plain_loud_comment()?,
                '/' if self.at("//") => self.comment(true, false)?,
                '#' if self.at("#{") => {
                    parts.push(self.interpolation()?);
                    self.selector_identifier_body(parts)?;
                }
                c if newline(c) && self.syntax == SassSyntax::Indented && brackets.is_empty() => {
                    break;
                }
                '!' | ';' | '{' | '}' => break,
                'u' | 'U' => {
                    self.selector_url(parts)?;
                }
                '(' | '[' => {
                    brackets.push(if c == '(' { ')' } else { ']' });
                    self.bump()?;
                }
                ')' | ']' => match brackets.pop() {
                    Some(close) if close == c => {
                        self.bump()?;
                    }
                    Some(_) | None => return Err(self.error(Error::ExpectedSyntax)),
                },
                _ if self.identifier_ahead() => self.skip_identifier()?,
                _ => {
                    self.bump()?;
                }
            }
        }
        Ok(())
    }

    /// A loud comment in raw text, which unlike a statement comment has no
    /// interpolation.
    fn plain_loud_comment(&mut self) -> Result<(), SassError> {
        let start = self.pos;
        self.expect("/*")?;
        while !self.at("*/") {
            if self.bump()?.is_none() {
                return Err(self.error(Error::Unterminated));
            }
        }
        self.expect("*/")?;
        self.capacity()?;
        self.comments.push(SassComment {
            span: self.span(start),
            silent: false,
            interpolations: Vec::new(),
        });
        Ok(())
    }

    /// Replaces the raw `target`, whose syntax starts at arena length `nodes`
    /// and comment count `comments`, with an established selector list.
    fn extend_target(
        &mut self,
        target: Id,
        nodes: usize,
        comments: usize,
    ) -> Result<Id, SassError> {
        let span = self.nodes[target.0].span;
        let interpolated = self.nodes[nodes..]
            .iter()
            .any(|node| node.kind == Kind::Interpolation);
        if interpolated || span.start() == span.end() {
            self.selector_evidence.push(target);
            return Ok(target);
        }
        let raw_nodes = self.nodes.split_off(nodes);
        let raw_comments = self.comments.split_off(comments);
        let end = self.pos;
        let full = self.text;
        // A failed attempt can leave grammar state behind.
        let saved = (
            self.depth,
            self.expression_newlines,
            self.single_equals,
            self.stop_words,
            self.stop_depth,
            self.interpolated_selector,
        );
        // The trimmed raw text bounds the attempt, as the evaluated text would.
        self.text = &full[..span.end()];
        self.pos = span.start();
        self.interpolated_selector = false;
        self.parent_selectors = false;
        let result = self.selector_list();
        let complete = self.pos == span.end();
        self.parent_selectors = true;
        self.text = full;
        self.pos = end;
        (
            self.depth,
            self.expression_newlines,
            self.single_equals,
            self.stop_words,
            self.stop_depth,
            self.interpolated_selector,
        ) = saved;
        match result {
            Ok(list) if complete && self.single_simple_selectors(list) => return Ok(list),
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
        self.nodes.truncate(nodes);
        self.comments.truncate(comments);
        self.nodes.extend(raw_nodes);
        self.comments.extend(raw_comments);
        self.selector_evidence.push(target);
        Ok(target)
    }

    /// Whether every complex selector of `list` is one compound selector of
    /// one simple selector, as evaluation requires of extension targets.
    fn single_simple_selectors(&self, list: Id) -> bool {
        self.nodes[list.0].children.iter().all(
            |&complex| match self.nodes[complex.0].children[..] {
                [compound] => {
                    self.nodes[compound.0].kind == Kind::CompoundSelector
                        && self.nodes[compound.0].children.len() == 1
                }
                _ => false,
            },
        )
    }
}
