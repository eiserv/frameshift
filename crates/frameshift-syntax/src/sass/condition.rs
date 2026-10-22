//! Media queries, supports conditions, and raw declaration values, read as
//! Dart Sass 1.105.2 reads them at parse time.

use super::{
    Parser, SassComment, SassConditionOperator as Operator, SassError, SassErrorKind as Error,
    SassMediaRelation as Relation, SassNodeId as Id, SassNodeKind as Kind, SassSyntax, css_space,
    decode_name,
};

/// Options of the reference's `_interpolatedDeclarationValue`.
#[derive(Clone, Copy)]
pub(super) struct ValueOptions {
    pub(super) allow_empty: bool,
    pub(super) allow_semicolon: bool,
    pub(super) allow_colon: bool,
    pub(super) allow_open_brace: bool,
    pub(super) silent_comments: bool,
    pub(super) lines: bool,
}

/// Parser state that a failed speculative parse must restore. Work charges
/// are never restored.
pub(super) struct Checkpoint {
    pos: usize,
    nodes: usize,
    comments: usize,
    depth: usize,
    expression_newlines: bool,
    single_equals: bool,
    until_comparison: bool,
    stop_words: &'static [&'static str],
    stop_depth: usize,
    selector_evidence: usize,
    media_evidence: usize,
    at_root_evidence: usize,
    indentation: Option<char>,
    interpolated_selector: bool,
    parent_selectors: bool,
}

impl Parser<'_> {
    /// The legacy Gecko rule has a fixed parse-time function grammar.
    pub(super) fn moz_document_value(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        loop {
            if self.at("#{") {
                parts.push(self.interpolation()?);
            } else {
                let name = self.identifier()?;
                let decoded = decode_name(self.spelling(name), false);
                let raw_url = matches!(decoded.as_ref(), "url" | "url-prefix" | "domain");
                if !raw_url && decoded != "regexp" {
                    return Err(self.error(Error::ExpectedIdentifier));
                }
                parts.push(name);
                if raw_url && let Some(contents) = self.raw_url_contents()? {
                    parts.extend(contents);
                } else {
                    self.enter()?;
                    self.expect("(")?;
                    if raw_url {
                        self.space()?;
                    }
                    if !self.peek().is_some_and(|c| matches!(c, '\'' | '"')) {
                        return Err(self.error(Error::ExpectedSyntax));
                    }
                    parts.push(self.quoted()?);
                    self.expect(")")?;
                    self.depth -= 1;
                }
            }
            self.space()?;
            if !self.eat(",")? {
                break;
            }
            self.space()?;
        }
        self.node(start, Kind::InterpolatedText, parts)
    }

    pub(super) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            pos: self.pos,
            nodes: self.nodes.len(),
            comments: self.comments.len(),
            depth: self.depth,
            expression_newlines: self.expression_newlines,
            single_equals: self.single_equals,
            until_comparison: self.until_comparison,
            stop_words: self.stop_words,
            stop_depth: self.stop_depth,
            selector_evidence: self.selector_evidence.len(),
            media_evidence: self.media_evidence.len(),
            at_root_evidence: self.at_root_evidence.len(),
            indentation: self.indentation,
            interpolated_selector: self.interpolated_selector,
            parent_selectors: self.parent_selectors,
        }
    }

    pub(super) fn restore(&mut self, checkpoint: &Checkpoint) {
        self.pos = checkpoint.pos;
        self.nodes.truncate(checkpoint.nodes);
        self.comments.truncate(checkpoint.comments);
        self.depth = checkpoint.depth;
        self.expression_newlines = checkpoint.expression_newlines;
        self.single_equals = checkpoint.single_equals;
        self.until_comparison = checkpoint.until_comparison;
        self.stop_words = checkpoint.stop_words;
        self.stop_depth = checkpoint.stop_depth;
        self.selector_evidence
            .truncate(checkpoint.selector_evidence);
        self.media_evidence.truncate(checkpoint.media_evidence);
        self.at_root_evidence.truncate(checkpoint.at_root_evidence);
        self.indentation = checkpoint.indentation;
        self.interpolated_selector = checkpoint.interpolated_selector;
        self.parent_selectors = checkpoint.parent_selectors;
    }

    /// Whitespace where a statement may end (`consumeNewlines: false`).
    fn space(&mut self) -> Result<bool, SassError> {
        self.skip(self.syntax == SassSyntax::Scss)
    }

    fn space_lines(&mut self, lines: bool) -> Result<bool, SassError> {
        self.skip(lines || self.syntax == SassSyntax::Scss)
    }

    /// The reference's `expectWhitespace`: a whitespace character, including
    /// a newline it may not consume, or a comment.
    fn expect_space(&mut self, lines: bool) -> Result<(), SassError> {
        if !(self.peek().is_some_and(css_space) || self.at("//") || self.at("/*")) {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.space_lines(lines)?;
        Ok(())
    }

    fn interpolated_identifier_ahead(&self) -> bool {
        self.name_start() || self.at("#{") || self.at("-#{")
    }

    /// An interpolated identifier. One consisting of a single interpolation
    /// is that `Interpolation` node itself.
    pub(super) fn condition_identifier(&mut self) -> Result<Id, SassError> {
        if self.at("#{") {
            let first = self.interpolation()?;
            if !self.name_body_ahead() {
                return Ok(first);
            }
            let start = self.start(first);
            let mut parts = vec![first];
            loop {
                self.name_body()?;
                if !self.at("#{") {
                    break;
                }
                parts.push(self.interpolation()?);
            }
            return self.node(start, Kind::InterpolatedText, parts);
        }
        let start = self.pos;
        if self.at("-#{") {
            self.bump()?;
        }
        let name = self.interpolated_name()?;
        self.nodes[name.0].span = super::Span::new(start, self.end(name)).unwrap();
        Ok(name)
    }

    fn name_body_ahead(&self) -> bool {
        self.at("#{")
            || self.peek().is_some_and(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\\') || c >= '\u{80}'
            })
    }

    /// The decoded spelling of an interpolation-free name.
    pub(super) fn plain_name(&self, id: Id) -> Option<String> {
        (self.nodes[id.0].kind == Kind::Identifier)
            .then(|| decode_name(self.spelling(id), false).into_owned())
    }

    /// The reference's `scanIdentifier`, as a keyword node.
    fn condition_keyword(
        &mut self,
        word: &str,
        operator: Operator,
    ) -> Result<Option<Id>, SassError> {
        let start = self.pos;
        if !self.name_start() || !self.module_keyword(word, false)? {
            return Ok(None);
        }
        self.node(start, Kind::ConditionOperator(operator), Vec::new())
            .map(Some)
    }

    fn and_or(&mut self) -> Result<Option<(Id, Operator)>, SassError> {
        for (word, operator) in [("and", Operator::And), ("or", Operator::Or)] {
            if let Some(id) = self.condition_keyword(word, operator)? {
                return Ok(Some((id, operator)));
            }
        }
        Ok(None)
    }

    fn word(operator: Operator) -> &'static str {
        match operator {
            Operator::And => "and",
            Operator::Or => "or",
            Operator::Not => "not",
        }
    }

    /// Comma-separated media queries. `@media` and import modifiers share
    /// this parse-time grammar.
    pub(super) fn media_query_list(&mut self) -> Result<Id, SassError> {
        let mut queries = Vec::new();
        loop {
            self.space()?;
            queries.push(self.media_query()?);
            self.space()?;
            if !self.eat(",")? {
                break;
            }
        }
        let start = self.start(queries[0]);
        let end = self.end(*queries.last().unwrap());
        self.node_between(start, end, Kind::MediaQueryList, queries)
    }

    fn media_query(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        if self.at("(") {
            parts.push(self.media_in_parens()?);
            self.space()?;
            if let Some((keyword, operator)) = self.and_or()? {
                parts.push(keyword);
                self.expect_space(false)?;
                self.media_sequence(operator, &mut parts)?;
            }
            return self.query_node(start, parts);
        }
        let first = self.condition_identifier()?;
        if self
            .plain_name(first)
            .is_some_and(|name| name.eq_ignore_ascii_case("not"))
        {
            self.expect_space(false)?;
            if !self.interpolated_identifier_ahead() {
                // `not` followed by a condition is a keyword, not a modifier.
                self.nodes[first.0].kind = Kind::ConditionOperator(Operator::Not);
                parts.push(first);
                parts.push(self.media_or_interpolation()?);
                return self.query_node(start, parts);
            }
        }
        self.space()?;
        parts.push(first);
        if !self.interpolated_identifier_ahead() {
            return self.query_node(start, parts);
        }
        let second = self.condition_identifier()?;
        if self
            .plain_name(second)
            .is_some_and(|name| name.eq_ignore_ascii_case("and"))
        {
            self.expect_space(false)?;
            self.nodes[second.0].kind = Kind::ConditionOperator(Operator::And);
            parts.push(second);
        } else {
            self.space()?;
            parts.push(second);
            let Some(and) = self.condition_keyword("and", Operator::And)? else {
                return self.query_node(start, parts);
            };
            self.expect_space(false)?;
            parts.push(and);
        }
        if let Some(not) = self.condition_keyword("not", Operator::Not)? {
            self.expect_space(false)?;
            parts.push(not);
            parts.push(self.media_or_interpolation()?);
            return self.query_node(start, parts);
        }
        self.media_sequence(Operator::And, &mut parts)?;
        self.query_node(start, parts)
    }

    fn query_node(&mut self, start: usize, parts: Vec<Id>) -> Result<Id, SassError> {
        let end = self.end(*parts.last().unwrap());
        self.node_between(start, end, Kind::MediaQuery, parts)
    }

    /// Conditions separated by one operator. The reference never consumes
    /// newlines here, even inside parentheses.
    fn media_sequence(&mut self, operator: Operator, parts: &mut Vec<Id>) -> Result<(), SassError> {
        loop {
            parts.push(self.media_or_interpolation()?);
            self.space()?;
            let Some(keyword) = self.condition_keyword(Self::word(operator), operator)? else {
                return Ok(());
            };
            self.expect_space(false)?;
            parts.push(keyword);
        }
    }

    fn media_or_interpolation(&mut self) -> Result<Id, SassError> {
        if self.at("#") {
            self.interpolation()
        } else {
            self.media_in_parens()
        }
    }

    fn media_in_parens(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        self.expect("(")?;
        self.skip(true)?;
        let mut parts = Vec::new();
        if self.at("(") {
            parts.push(self.media_in_parens()?);
            self.skip(true)?;
            if let Some((keyword, operator)) = self.and_or()? {
                parts.push(keyword);
                self.expect_space(true)?;
                self.media_sequence(operator, &mut parts)?;
            }
        } else if let Some(not) = self.condition_keyword("not", Operator::Not)? {
            parts.push(not);
            self.expect_space(true)?;
            parts.push(self.media_or_interpolation()?);
        } else {
            parts.push(self.comparison_operand()?);
            let relation_start = self.pos;
            if self.eat(":")? {
                parts.push(self.node(
                    relation_start,
                    Kind::MediaRelation(Relation::Colon),
                    Vec::new(),
                )?);
                self.skip(true)?;
                let before = self.expression_newlines;
                self.expression_newlines = true;
                parts.push(self.expression(true)?);
                self.expression_newlines = before;
            } else if let Some(first) = self.peek().filter(|c| matches!(c, '<' | '>' | '=')) {
                parts.push(self.relation()?);
                parts.push(self.comparison_operand()?);
                // A range may repeat the direction of its first comparison.
                if first != '=' && self.at(if first == '<' { "<" } else { ">" }) {
                    parts.push(self.relation()?);
                    parts.push(self.comparison_operand()?);
                }
            }
        }
        self.expect(")")?;
        let id = self.node(start, Kind::MediaInParens, parts)?;
        self.space()?;
        self.depth -= 1;
        Ok(id)
    }

    fn relation(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        let relation = match self.bump()? {
            Some('<') if self.eat("=")? => Relation::LessEqual,
            Some('<') => Relation::Less,
            Some('>') if self.eat("=")? => Relation::GreaterEqual,
            Some('>') => Relation::Greater,
            Some(_) | None => Relation::Equal,
        };
        let id = self.node(start, Kind::MediaRelation(relation), Vec::new())?;
        self.skip(true)?;
        Ok(id)
    }

    /// An expression ending before a top-level `<`, `>`, or single `=`.
    fn comparison_operand(&mut self) -> Result<Id, SassError> {
        let newlines = self.expression_newlines;
        let until = self.until_comparison;
        self.expression_newlines = true;
        self.until_comparison = true;
        let result = self.expression(true);
        self.expression_newlines = newlines;
        self.until_comparison = until;
        result
    }

    /// Whether evaluation produces media query text that depends on values,
    /// which Dart Sass then parses again with its stricter query grammar.
    pub(super) fn media_depends_on_values(&mut self, list: Id) -> Result<bool, SassError> {
        let mut stack = vec![list];
        while let Some(id) = stack.pop() {
            self.visit()?;
            let node = &self.nodes[id.0];
            if matches!(
                node.kind,
                Kind::Interpolation
                    | Kind::Variable
                    | Kind::Call
                    | Kind::SpecialFunction
                    | Kind::IfExpression
                    | Kind::String { quoted: true }
                    | Kind::ParentSelector
                    | Kind::Null
                    | Kind::Map
            ) {
                return Ok(true);
            }
            stack.extend_from_slice(&node.children);
        }
        Ok(false)
    }

    /// A `@supports` condition; `parens` consumes newlines in indented Sass.
    pub(super) fn supports_condition(&mut self, parens: bool) -> Result<Id, SassError> {
        self.enter()?;
        let start = self.pos;
        let result = if let Some(not) = self.condition_keyword("not", Operator::Not)? {
            self.space_lines(parens)?;
            let operand = self.supports_in_parens()?;
            let end = self.end(operand);
            self.node_between(start, end, Kind::SupportsNegation, vec![not, operand])
        } else {
            let first = self.supports_in_parens()?;
            self.space_lines(parens)?;
            self.supports_operation(start, first, parens)
        };
        self.depth -= 1;
        result
    }

    /// Continues `first` with one repeated operator while an identifier
    /// follows, as the reference does.
    fn supports_operation(
        &mut self,
        start: usize,
        first: Id,
        lines: bool,
    ) -> Result<Id, SassError> {
        let mut parts = vec![first];
        let mut operator = None;
        while self.name_start() {
            let keyword = match operator {
                Some(operator) => self.condition_keyword(Self::word(operator), operator)?,
                None => match self.and_or()? {
                    Some((keyword, found)) => {
                        operator = Some(found);
                        Some(keyword)
                    }
                    None => None,
                },
            };
            let Some(keyword) = keyword else {
                return Err(self.error(Error::ExpectedSyntax));
            };
            parts.push(keyword);
            self.space_lines(lines)?;
            parts.push(self.supports_in_parens()?);
            self.space_lines(lines)?;
        }
        if parts.len() == 1 {
            return Ok(first);
        }
        let end = self.end(*parts.last().unwrap());
        self.node_between(start, end, Kind::SupportsOperation, parts)
    }

    pub(super) fn supports_in_parens(&mut self) -> Result<Id, SassError> {
        self.enter()?;
        let result = self.supports_in_parens_inner();
        self.depth -= 1;
        result
    }

    fn supports_in_parens_inner(&mut self) -> Result<Id, SassError> {
        let start = self.pos;
        if self.interpolated_identifier_ahead() {
            let name = self.condition_identifier()?;
            if self
                .plain_name(name)
                .is_some_and(|name| name.to_lowercase() == "not")
            {
                return Err(self.error(Error::ExpectedSyntax));
            }
            if self.eat("(")? {
                let arguments = self.declaration_value(ValueOptions {
                    allow_empty: true,
                    allow_semicolon: true,
                    allow_colon: true,
                    allow_open_brace: true,
                    silent_comments: true,
                    lines: true,
                })?;
                self.expect(")")?;
                return self.node(start, Kind::SupportsFunction, vec![name, arguments]);
            }
            if self.nodes[name.0].kind == Kind::Interpolation {
                return Ok(name);
            }
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.expect("(")?;
        self.skip(true)?;
        if let Some(not) = self.condition_keyword("not", Operator::Not)? {
            self.skip(true)?;
            let operand = self.supports_in_parens()?;
            self.expect(")")?;
            return self.node(start, Kind::SupportsNegation, vec![not, operand]);
        }
        if self.at("(") {
            let condition = self.supports_condition(true)?;
            self.expect(")")?;
            self.nodes[condition.0].span = self.span(start);
            return Ok(condition);
        }
        // The reference parses `Expression ":" Expression` first and, after
        // any grammar failure, reparses `InterpolatedIdentifier AnyValue?`.
        let checkpoint = self.checkpoint();
        let declaration = self.supports_declaration_name();
        let name = match declaration {
            Ok(name) => name,
            Err(error) if is_limit(error) => return Err(error),
            Err(error) => {
                self.restore(&checkpoint);
                let name_start = self.pos;
                let identifier = self.condition_identifier()?;
                if let Some(operation) = self.interpolation_operation(identifier, name_start)? {
                    self.expect(")")?;
                    self.nodes[operation.0].span = self.span(start);
                    return Ok(operation);
                }
                let rest = self.declaration_value(ValueOptions {
                    allow_empty: true,
                    allow_semicolon: true,
                    allow_colon: false,
                    allow_open_brace: true,
                    silent_comments: true,
                    lines: true,
                })?;
                if self.at(":") {
                    return Err(error);
                }
                self.expect(")")?;
                return self.node(start, Kind::SupportsAnything, vec![identifier, rest]);
            }
        };
        let value = self.supports_declaration_value(name)?;
        self.expect(")")?;
        self.node(start, Kind::SupportsDeclaration, vec![name, value])
    }

    fn supports_declaration_name(&mut self) -> Result<Id, SassError> {
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let name = self.expression(true)?;
        self.expression_newlines = before;
        self.expect(":")?;
        Ok(name)
    }

    /// A custom property's value is raw text; any other value is SassScript.
    pub(super) fn supports_declaration_value(&mut self, name: Id) -> Result<Id, SassError> {
        if self.nodes[name.0].kind == (Kind::String { quoted: false })
            && self.spelling(name).starts_with("--")
        {
            return self.declaration_value(ValueOptions {
                allow_empty: false,
                allow_semicolon: false,
                allow_colon: true,
                allow_open_brace: true,
                silent_comments: true,
                lines: false,
            });
        }
        self.skip(true)?;
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let value = self.expression(true)?;
        self.expression_newlines = before;
        Ok(value)
    }

    /// `#{...} and (...)`: only a sole interpolation can start an operation
    /// after the declaration grammar failed.
    fn interpolation_operation(
        &mut self,
        identifier: Id,
        start: usize,
    ) -> Result<Option<Id>, SassError> {
        if self.nodes[identifier.0].kind != Kind::Interpolation {
            return Ok(None);
        }
        let before = self.pos;
        let comments = self.comments.len();
        self.skip(true)?;
        if self.name_start() && !self.at_keyword("and")? && !self.at_keyword("or")? {
            self.pos = before;
            self.comments.truncate(comments);
            return Ok(None);
        }
        let operation = self.supports_operation(start, identifier, true)?;
        Ok((operation != identifier).then_some(operation))
    }

    fn at_keyword(&mut self, word: &str) -> Result<bool, SassError> {
        let start = self.pos;
        let found = self.module_keyword(word, false)?;
        self.pos = start;
        Ok(found)
    }

    /// Raw text with interpolation, matching the reference's
    /// `_interpolatedDeclarationValue` with the given options.
    pub(super) fn declaration_value(&mut self, options: ValueOptions) -> Result<Id, SassError> {
        let start = self.pos;
        let mut parts = Vec::new();
        let mut brackets = Vec::new();
        let mut empty = true;
        while let Some(c) = self.peek() {
            match c {
                '\\' => self.escape(true)?,
                '"' | '\'' => parts.push(self.quoted()?),
                '/' if self.at("/*") => self.raw_loud_comment()?,
                '/' if options.silent_comments && self.at("//") => {
                    self.comment(true, false)?;
                    continue;
                }
                '#' if self.at("#{") => loop {
                    parts.push(self.interpolation()?);
                    self.name_body()?;
                    if !self.at("#{") {
                        break;
                    }
                },
                '\r' | '\n' | '\u{c}'
                    if self.syntax == SassSyntax::Indented
                        && !options.lines
                        && brackets.is_empty() =>
                {
                    break;
                }
                '{' if !options.allow_open_brace => break,
                '(' | '{' | '[' => {
                    self.enter()?;
                    self.bump()?;
                    brackets.push(match c {
                        '(' => ')',
                        '{' => '}',
                        _ => ']',
                    });
                }
                ')' | '}' | ']' => {
                    let Some(close) = brackets.pop() else {
                        break;
                    };
                    if c != close {
                        return Err(self.error(Error::ExpectedSyntax));
                    }
                    self.bump()?;
                    self.depth -= 1;
                }
                ';' if !options.allow_semicolon && brackets.is_empty() => break,
                ':' if !options.allow_colon && brackets.is_empty() => break,
                'u' | 'U' => {
                    let before = self.pos;
                    self.name_body()?;
                    let name = decode_name(&self.text[before..self.pos], false);
                    if matches!(name.as_ref(), "url" | "url-prefix") {
                        if let Some(url) = self.raw_url_contents()? {
                            parts.extend(url);
                        } else {
                            self.pos = before;
                            self.bump()?;
                        }
                    }
                }
                _ if self.name_start() => self.name_body()?,
                _ => {
                    self.bump()?;
                }
            }
            empty = false;
        }
        if !brackets.is_empty() {
            return Err(self.error(Error::Unterminated));
        }
        if empty && !options.allow_empty {
            return Err(self.error(Error::ExpectedSyntax));
        }
        self.node(start, Kind::InterpolatedText, parts)
    }

    /// A loud comment as raw text: interpolation is not parsed inside it.
    pub(super) fn raw_loud_comment(&mut self) -> Result<(), SassError> {
        let start = self.pos;
        self.expect("/*")?;
        while !self.eat("*/")? {
            if self.bump()?.is_none() {
                return Err(self.error(Error::Unterminated));
            }
        }
        self.capacity()?;
        self.comments.push(SassComment {
            span: self.span(start),
            silent: false,
            interpolations: Vec::new(),
        });
        Ok(())
    }

    /// Raw `url(...)` contents, or `None` after restoring speculative state.
    /// Failed lookahead retains work charges and propagates grammar/limit errors.
    pub(super) fn raw_url_contents(&mut self) -> Result<Option<Vec<Id>>, SassError> {
        let checkpoint = self.checkpoint();
        if !self.eat("(")? {
            return Ok(None);
        }
        while self.peek().is_some_and(css_space) {
            self.bump()?;
        }
        let mut parts = Vec::new();
        loop {
            match self.peek() {
                Some('\\') => self.escape(true)?,
                Some('#') if self.at("#{") => parts.push(self.interpolation()?),
                Some(')') => {
                    self.bump()?;
                    return Ok(Some(parts));
                }
                Some(c) if css_space(c) => {
                    while self.peek().is_some_and(css_space) {
                        self.bump()?;
                    }
                    if !self.at(")") {
                        break;
                    }
                }
                Some(c)
                    if matches!(c, '!' | '%' | '&' | '#')
                        || ('*'..='~').contains(&c)
                        || c >= '\u{80}' =>
                {
                    self.bump()?;
                }
                Some(_) | None => break,
            }
        }
        self.restore(&checkpoint);
        Ok(None)
    }
}

fn is_limit(error: SassError) -> bool {
    matches!(
        error.kind,
        Error::NodeLimit | Error::DepthLimit | Error::WorkLimit
    )
}
