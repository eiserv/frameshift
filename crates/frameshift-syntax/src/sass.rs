//! Source-preserving Sass syntax, without evaluation or import resolution.
//!
//! The reference is Dart Sass 1.105.2 (`D-079`). Both surface syntaxes
//! share the same arenas; indentation never rewrites the source text.

mod expression;
mod statement;

use std::{borrow::Cow, fmt};

use frameshift_core::{SourceText, Span};

/// The explicitly selected Sass surface syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassSyntax {
    /// Braces and semicolons, conventionally `.scss`.
    Scss,
    /// Significant indentation, conventionally `.sass`.
    Indented,
}

/// Inclusive resource bounds for one Sass parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SassLimits {
    /// Syntax entries and retained comments together, default 1,000,000.
    pub max_nodes: usize,
    /// Nested grammar calls, default and implementation ceiling 128.
    pub max_depth: usize,
    /// Source scalar visits, including lookahead, default 16,777,216.
    pub max_work: usize,
}

impl Default for SassLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_depth: 128,
            max_work: 16_777_216,
        }
    }
}

/// A document-local syntax arena index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SassNodeId(usize);

/// Sass list separators, without evaluating slash arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassListSeparator {
    /// An empty or single-element list with no explicit separator.
    Undecided,
    /// Whitespace between expressions.
    Space,
    /// Commas, including a single trailing comma.
    Comma,
}

/// A unary SassScript operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassUnaryOperator {
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `not`.
    Not,
}

/// A binary SassScript operator, with no computed value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassBinaryOperator {
    /// `or`.
    Or,
    /// `and`.
    And,
    /// `==`.
    Equal,
    /// `!=`.
    NotEqual,
    /// `<`.
    Less,
    /// `<=`.
    LessEqual,
    /// `>`.
    Greater,
    /// `>=`.
    GreaterEqual,
    /// `+`.
    Add,
    /// `-`.
    Subtract,
    /// `*`.
    Multiply,
    /// `/`, whose evaluation may preserve a slash-separated number.
    Divide,
    /// `%`.
    Modulo,
    /// Legacy CSS function argument `=`.
    SingleEqual,
}

/// A Sass directive; unknown CSS at-rules retain an interpolated prelude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassAtRule {
    /// `@use`.
    Use,
    /// `@forward`.
    Forward,
    /// `@import`.
    Import,
    /// `@mixin`, or `=` in indented syntax.
    Mixin,
    /// `@function`.
    Function,
    /// `@include`, or `+` in indented syntax.
    Include,
    /// `@content`.
    Content,
    /// `@return`.
    Return,
    /// `@if`.
    If,
    /// An `@else` or `@else if` branch following an `@if`.
    Else,
    /// An `@else if` or legacy `@elseif` branch.
    ElseIf,
    /// `@each`.
    Each,
    /// `@for`.
    For,
    /// `@while`.
    While,
    /// `@extend`.
    Extend,
    /// `@at-root`.
    AtRoot,
    /// `@debug`.
    Debug,
    /// `@warn`.
    Warn,
    /// `@error`.
    Error,
    /// A CSS or unknown directive, without grammar-specific validation.
    Css,
}

/// Syntax kinds. Child indices always follow source order.
///
/// Statements put their header children before body statements. A binary
/// operation has its left and right operands, a map has entry children,
/// and a call has its name followed by its arguments. Interpolated text
/// retains only its interpolation children; the other bytes stay in source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SassNodeKind {
    /// A selector followed by statements. The selector is the first child.
    StyleRule,
    /// A property name, optional value, and optional nested declarations.
    Declaration,
    /// A variable reference and value, followed by source-spelled flags.
    VariableDeclaration,
    /// A directive name and header followed by body statements. Indented
    /// `=` and `+` shorthand omit the directive-name child; their original
    /// marker remains in the node span before the callable name.
    AtRule(SassAtRule),
    /// A source-spelled identifier, including escapes.
    Identifier,
    /// A property or selector fragment with interpolation children.
    InterpolatedText,
    /// `#{...}`, whose only child is its SassScript expression.
    Interpolation,
    /// A variable, optionally preceded by a namespace and `.`.
    Variable,
    /// Exact numeric spelling, optionally followed by a dimension or `%`.
    Number,
    /// A source-spelled hexadecimal color.
    Color,
    /// A source-spelled CSS Unicode range with checked lexical bounds.
    UnicodeRange,
    /// `true` or `false`.
    Boolean,
    /// `null`.
    Null,
    /// An interpolated string, with quotes retained when present.
    String {
        /// Whether the source includes a pair of quotes.
        quoted: bool,
    },
    /// Parent selector `&` used as an expression.
    ParentSelector,
    /// One unary operand.
    Unary(SassUnaryOperator),
    /// Left and right operands, with the operator in the source between them.
    Binary(SassBinaryOperator),
    /// A list of expressions.
    List {
        /// The syntactic separator.
        separator: SassListSeparator,
        /// Whether square brackets enclose this list.
        bracketed: bool,
    },
    /// Parentheses around one expression.
    Parenthesized,
    /// Parenthesized map entries.
    Map,
    /// A key and value.
    MapEntry,
    /// A function name followed by an argument list.
    Call,
    /// CSS special-function contents with interpolation, not SassScript.
    SpecialFunction,
    /// An invocation's arguments, including their enclosing parentheses.
    Arguments,
    /// One argument, optionally named and/or followed by `...`.
    Argument,
    /// A callable's parameter list, including parentheses.
    Parameters,
    /// A variable parameter and optional default expression or `...`.
    Parameter,
    /// A source-spelled `!default`, `!global`, or `!optional` flag.
    Flag,
}

/// One flat syntax entry with exact original UTF-8 byte coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SassNode {
    /// Complete source extent, including delimiters when present.
    pub span: Span,
    /// The syntactic construct.
    pub kind: SassNodeKind,
    /// Immediate children in source order, all contained by `span`.
    pub children: Vec<SassNodeId>,
}

/// A comment retained independently of statement placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SassComment {
    /// Extent including the opening and any closing delimiter.
    pub span: Span,
    /// Whether this is a silent `//` comment.
    pub silent: bool,
    /// Parsed interpolations in a loud comment, in source order.
    pub interpolations: Vec<SassNodeId>,
}

/// A retained grammar not yet fully validated by Sass parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassUncheckedKind {
    /// Selector grammar, including selector-specific early errors.
    Selector,
    /// A CSS or unknown at-rule's context-specific grammar.
    CssDirective,
    /// Import URLs, modifiers, and media/supports conditions.
    ImportPrelude,
    /// Module URL syntax and the namespace derived from its URL.
    ModuleUrl,
    /// The query or inline selector of `@at-root`.
    AtRootPrelude,
    /// Raw contents of a special CSS function.
    SpecialFunction,
}

/// Evidence that a retained grammar still needs validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SassUnchecked {
    /// The grammar that remains unchecked.
    pub kind: SassUncheckedKind,
    /// Its exact original source extent.
    pub span: Span,
}

/// A syntax or resource failure. No partial document is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SassError {
    /// Original UTF-8 byte offset, possibly EOF.
    pub offset: usize,
    /// Failure category, without source-derived prose.
    pub kind: SassErrorKind,
}

/// Sass parser failure categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SassErrorKind {
    /// A required expression was absent.
    ExpectedExpression,
    /// A required identifier was absent.
    ExpectedIdentifier,
    /// A required delimiter, keyword, or statement boundary was absent.
    ExpectedSyntax,
    /// A string, escape, comment, or block was not closed.
    Unterminated,
    /// A directive appeared in an invalid syntactic context.
    InvalidContext,
    /// Duplicate parameters or keyword arguments after underscore folding.
    DuplicateName,
    /// An indentation prefix mixed tabs and spaces or missed an outer level.
    InvalidIndentation,
    /// A recognized grammar extension is not implemented yet.
    UnsupportedSyntax,
    /// The retained node/comment bound was exceeded.
    NodeLimit,
    /// The depth bound or implementation ceiling was exceeded.
    DepthLimit,
    /// The source-visit bound was exceeded.
    WorkLimit,
}

impl fmt::Display for SassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            SassErrorKind::ExpectedExpression => "expected a Sass expression",
            SassErrorKind::ExpectedIdentifier => "expected a Sass identifier",
            SassErrorKind::ExpectedSyntax => "expected Sass syntax",
            SassErrorKind::Unterminated => "unterminated Sass syntax",
            SassErrorKind::InvalidContext => "Sass syntax is not allowed in this context",
            SassErrorKind::DuplicateName => "duplicate Sass parameter or argument",
            SassErrorKind::InvalidIndentation => "inconsistent Sass indentation",
            SassErrorKind::UnsupportedSyntax => "this Sass grammar extension is not implemented",
            SassErrorKind::NodeLimit => "the Sass node limit was reached",
            SassErrorKind::DepthLimit => "the Sass depth limit was reached",
            SassErrorKind::WorkLimit => "the Sass work limit was reached",
        };
        write!(f, "{message} at byte {}", self.offset)
    }
}

impl std::error::Error for SassError {}

/// A parsed Sass stylesheet. Parsing never loads or evaluates its imports.
#[derive(Debug)]
pub struct SassDocument<'source> {
    source: &'source SourceText,
    syntax: SassSyntax,
    nodes: Vec<SassNode>,
    statements: Vec<SassNodeId>,
    comments: Vec<SassComment>,
}

impl PartialEq for SassDocument<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.source.as_str() == other.source.as_str()
            && self.syntax == other.syntax
            && self.nodes == other.nodes
            && self.statements == other.statements
            && self.comments == other.comments
    }
}
impl Eq for SassDocument<'_> {}

impl<'source> SassDocument<'source> {
    /// The immutable original source text.
    pub fn source(&self) -> &'source SourceText {
        self.source
    }
    /// The selected surface syntax.
    pub fn syntax(&self) -> SassSyntax {
        self.syntax
    }
    /// The flat syntax arena in deterministic construction order.
    pub fn nodes(&self) -> &[SassNode] {
        &self.nodes
    }
    /// Top-level statements in source order.
    pub fn statements(&self) -> &[SassNodeId] {
        &self.statements
    }
    /// Comments in original source order.
    pub fn comments(&self) -> &[SassComment] {
        &self.comments
    }
    /// Looks up a document-local index, returning `None` if out of range.
    pub fn node(&self, id: SassNodeId) -> Option<&SassNode> {
        self.nodes.get(id.0)
    }

    /// Lists retained, unchecked grammar in arena construction order.
    ///
    /// Every entry prevents treating the whole file as syntax-validated.
    /// An empty iterator establishes neither evaluation success nor migration
    /// support. Full conformance remains M3 work; no result of this in-progress
    /// parser authorizes transformation.
    pub fn unchecked_syntax(&self) -> impl Iterator<Item = SassUnchecked> + '_ {
        self.nodes.iter().filter_map(|node| {
            let kind = if node.kind == SassNodeKind::StyleRule {
                SassUncheckedKind::Selector
            } else if node.kind == SassNodeKind::AtRule(SassAtRule::Css) {
                SassUncheckedKind::CssDirective
            } else if matches!(
                node.kind,
                SassNodeKind::AtRule(SassAtRule::Use | SassAtRule::Forward)
            ) {
                SassUncheckedKind::ModuleUrl
            } else if node.kind == SassNodeKind::AtRule(SassAtRule::Import) {
                SassUncheckedKind::ImportPrelude
            } else if node.kind == SassNodeKind::AtRule(SassAtRule::Extend) {
                SassUncheckedKind::Selector
            } else if node.kind == SassNodeKind::AtRule(SassAtRule::AtRoot) {
                SassUncheckedKind::AtRootPrelude
            } else if node.kind == SassNodeKind::SpecialFunction {
                SassUncheckedKind::SpecialFunction
            } else {
                return None;
            };
            Some(SassUnchecked {
                kind,
                span: node.span,
            })
        })
    }
}

/// Parses SCSS or indented Sass with exact spans and explicit resource bounds.
///
/// No import is resolved and no expression is evaluated. Recognized syntax
/// errors return no tree. This is an in-progress M3 parser, not a full Sass
/// validity check. [`SassDocument::unchecked_syntax`] identifies grammars
/// retained as interpolated text. No result authorizes transformation.
pub fn parse_sass(
    source: &SourceText,
    syntax: SassSyntax,
    limits: SassLimits,
) -> Result<SassDocument<'_>, SassError> {
    let mut parser = Parser {
        text: source.as_str(),
        syntax,
        limits,
        pos: 0,
        depth: 0,
        work: 0,
        nodes: Vec::new(),
        comments: Vec::new(),
        indentation: None,
        expression_newlines: syntax == SassSyntax::Scss,
        single_equals: false,
        stop_words: &[],
        stop_depth: 0,
    };
    if parser.text.starts_with('\u{feff}') {
        parser.bump()?;
    }
    let statements = parser.statements(0, statement::Context::default())?;
    parser.skip_statements()?;
    if parser.pos != parser.text.len() {
        return Err(parser.error(SassErrorKind::ExpectedSyntax));
    }
    parser.comments.sort_by_key(|comment| comment.span.start());
    Ok(SassDocument {
        source,
        syntax,
        nodes: parser.nodes,
        statements,
        comments: parser.comments,
    })
}

struct Parser<'source> {
    text: &'source str,
    syntax: SassSyntax,
    limits: SassLimits,
    pos: usize,
    depth: usize,
    work: usize,
    nodes: Vec<SassNode>,
    comments: Vec<SassComment>,
    indentation: Option<char>,
    expression_newlines: bool,
    single_equals: bool,
    stop_words: &'static [&'static str],
    stop_depth: usize,
}

impl Parser<'_> {
    fn error(&self, kind: SassErrorKind) -> SassError {
        SassError {
            offset: self.pos,
            kind,
        }
    }
    fn at(&self, text: &str) -> bool {
        self.text[self.pos..].starts_with(text)
    }
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }
    fn visit(&mut self) -> Result<(), SassError> {
        if self.work >= self.limits.max_work {
            return Err(self.error(SassErrorKind::WorkLimit));
        }
        self.work += 1;
        Ok(())
    }
    fn bump(&mut self) -> Result<Option<char>, SassError> {
        self.visit()?;
        let next = self.peek();
        if let Some(c) = next {
            self.pos += c.len_utf8();
        }
        Ok(next)
    }
    fn eat(&mut self, text: &str) -> Result<bool, SassError> {
        if !self.at(text) {
            return Ok(false);
        }
        for _ in text.chars() {
            self.bump()?;
        }
        Ok(true)
    }
    fn expect(&mut self, text: &str) -> Result<(), SassError> {
        if self.eat(text)? {
            Ok(())
        } else {
            Err(self.error(SassErrorKind::ExpectedSyntax))
        }
    }
    fn span(&self, start: usize) -> Span {
        Span::new(start, self.pos).unwrap()
    }
    fn enter(&mut self) -> Result<(), SassError> {
        self.visit()?;
        if self.depth >= self.limits.max_depth.min(128) {
            return Err(self.error(SassErrorKind::DepthLimit));
        }
        self.depth += 1;
        Ok(())
    }
    fn capacity(&self) -> Result<(), SassError> {
        if self.nodes.len() + self.comments.len() >= self.limits.max_nodes {
            Err(self.error(SassErrorKind::NodeLimit))
        } else {
            Ok(())
        }
    }
    fn node(
        &mut self,
        start: usize,
        kind: SassNodeKind,
        children: Vec<SassNodeId>,
    ) -> Result<SassNodeId, SassError> {
        self.capacity()?;
        let id = SassNodeId(self.nodes.len());
        self.nodes.push(SassNode {
            span: self.span(start),
            kind,
            children,
        });
        Ok(id)
    }
    fn start(&self, id: SassNodeId) -> usize {
        self.nodes[id.0].span.start()
    }
    fn end(&self, id: SassNodeId) -> usize {
        self.nodes[id.0].span.end()
    }
    fn spelling(&self, id: SassNodeId) -> &str {
        let span = self.nodes[id.0].span;
        &self.text[span.start()..span.end()]
    }
    fn name_start(&self) -> bool {
        self.name_start_at(self.pos)
    }
    fn name_start_at(&self, offset: usize) -> bool {
        let text = &self.text[offset..];
        text.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '\\' || c >= '\u{80}')
            || (text.starts_with('-')
                && text[1..].chars().next().is_some_and(|c| {
                    c == '-' || c == '_' || c == '\\' || c.is_ascii_alphabetic() || c >= '\u{80}'
                }))
    }
    fn escape(&mut self, identifier: bool) -> Result<(), SassError> {
        self.expect("\\")?;
        if self.peek().is_none() {
            return Ok(());
        }
        if self.peek().is_some_and(newline) {
            return Err(self.error(SassErrorKind::Unterminated));
        }
        if self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            let mut value = 0;
            for _ in 0..6 {
                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    break;
                }
                value = value * 16 + self.bump()?.unwrap().to_digit(16).unwrap();
            }
            if identifier && value > 0x10ffff {
                return Err(self.error(SassErrorKind::ExpectedSyntax));
            }
            if self.peek().is_some_and(css_space) {
                let cr = self.bump()? == Some('\r');
                if cr && self.at("\n") {
                    self.bump()?;
                }
            }
        } else {
            self.bump()?;
        }
        Ok(())
    }
    fn identifier(&mut self) -> Result<SassNodeId, SassError> {
        let start = self.pos;
        if !self.name_start() {
            return Err(self.error(SassErrorKind::ExpectedIdentifier));
        }
        self.name_body()?;
        self.node(start, SassNodeKind::Identifier, Vec::new())
    }
    fn name_body(&mut self) -> Result<(), SassError> {
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.escape(true)?;
            } else if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c >= '\u{80}' {
                self.bump()?;
            } else {
                break;
            }
        }
        Ok(())
    }
    fn keyword(&mut self, word: &str) -> Result<bool, SassError> {
        if !self.at(word) {
            return Ok(false);
        }
        if self.text[self.pos + word.len()..]
            .chars()
            .next()
            .is_some_and(|c| {
                c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '\\' || c >= '\u{80}'
            })
        {
            return Ok(false);
        }
        self.eat(word)
    }
    fn skip(&mut self, lines: bool) -> Result<bool, SassError> {
        self.skip_comments(lines, false)
    }
    fn skip_statements(&mut self) -> Result<bool, SassError> {
        self.skip_comments(true, true)
    }
    fn skip_comments(&mut self, lines: bool, statement: bool) -> Result<bool, SassError> {
        let start = self.pos;
        loop {
            if self
                .peek()
                .is_some_and(|c| css_space(c) && (lines || !newline(c)))
            {
                self.bump()?;
            } else if self.at("//") {
                self.comment(true, statement)?;
            } else if self.at("/*") {
                self.comment(false, statement)?;
            } else {
                break;
            }
        }
        Ok(start != self.pos)
    }
    fn comment(&mut self, silent: bool, statement: bool) -> Result<(), SassError> {
        let start = self.pos;
        let mut interpolations = Vec::new();
        let line = self.text[..start]
            .rfind(['\r', '\n', '\u{c}'])
            .map_or(0, |i| i + 1);
        let indent = self.text[line..start]
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .count();
        // Only statement comments use indentation as their closing boundary.
        let indented = statement
            && self.syntax == SassSyntax::Indented
            && self.text[line..start]
                .chars()
                .all(|c| matches!(c, ' ' | '\t' | '\u{feff}'));
        self.expect(if silent { "//" } else { "/*" })?;
        if silent {
            loop {
                while self.peek().is_some_and(|c| !newline(c)) {
                    self.bump()?;
                }
                if !indented || !self.comment_continuation(indent)? {
                    break;
                }
            }
        } else {
            while !self.at("*/") {
                if self.peek().is_none() {
                    if indented {
                        break;
                    }
                    return Err(self.error(SassErrorKind::Unterminated));
                }
                if indented && self.peek().is_some_and(newline) {
                    if !self.comment_continuation(indent)? {
                        break;
                    }
                    continue;
                }
                // Interpolation in a loud comment is grammar, not comment text.
                if self.at("#{") {
                    interpolations.push(self.interpolation()?);
                } else {
                    self.bump()?;
                }
            }
            self.eat("*/")?;
        }
        self.capacity()?;
        self.comments.push(SassComment {
            span: self.span(start),
            silent,
            interpolations,
        });
        Ok(())
    }

    fn comment_continuation(&mut self, indent: usize) -> Result<bool, SassError> {
        let start = self.pos;
        while self.peek().is_some_and(newline) {
            let cr = self.bump()? == Some('\r');
            if cr && self.at("\n") {
                self.bump()?;
            }
            let mut next_indent = 0;
            while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                self.bump()?;
                next_indent += 1;
            }
            if self.peek().is_some_and(newline) {
                continue;
            }
            if self.peek().is_some() && next_indent > indent {
                return Ok(true);
            }
            break;
        }
        self.pos = start;
        Ok(false)
    }
    fn interpolation(&mut self) -> Result<SassNodeId, SassError> {
        self.enter()?;
        let start = self.pos;
        self.expect("#{")?;
        self.skip(true)?;
        let before = self.expression_newlines;
        self.expression_newlines = true;
        let equals = self.single_equals;
        self.single_equals = false;
        let value = self.expression(true)?;
        self.single_equals = equals;
        self.expression_newlines = before;
        self.skip(true)?;
        self.expect("}")?;
        let id = self.node(start, SassNodeKind::Interpolation, vec![value]);
        self.depth -= 1;
        id
    }
    fn quoted(&mut self) -> Result<SassNodeId, SassError> {
        let start = self.pos;
        let quote = self
            .bump()?
            .ok_or_else(|| self.error(SassErrorKind::Unterminated))?;
        let mut children = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.error(SassErrorKind::Unterminated));
            };
            if c == quote {
                self.bump()?;
                break;
            }
            if newline(c) {
                return Err(self.error(SassErrorKind::Unterminated));
            }
            if self.at("#{") {
                children.push(self.interpolation()?);
            } else if c == '\\' {
                if self.text[self.pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(newline)
                {
                    self.bump()?;
                    let cr = self.bump()? == Some('\r');
                    if cr && self.at("\n") {
                        self.bump()?;
                    }
                } else {
                    self.escape(false)?;
                }
            } else {
                self.bump()?;
            }
        }
        self.node(start, SassNodeKind::String { quoted: true }, children)
    }
}

fn newline(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{c}')
}
fn css_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n' | '\u{c}')
}

// Sass compares names after CSS escape decoding and underscore folding.
fn canonical_name(text: &str) -> Cow<'_, str> {
    decode_name(text, true)
}

fn decode_name(text: &str, fold_underscore: bool) -> Cow<'_, str> {
    if !text.contains('\\') && (!fold_underscore || !text.contains('_')) {
        return Cow::Borrowed(text);
    }
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = if c == '\\' {
            let mut value = 0;
            let mut digits = 0;
            while digits < 6 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                value = value * 16 + chars.next().unwrap().to_digit(16).unwrap();
                digits += 1;
            }
            if digits == 0 {
                chars.next().unwrap_or('\u{fffd}')
            } else {
                if chars.peek().is_some_and(|&c| css_space(c)) {
                    let cr = chars.next() == Some('\r');
                    if cr && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                char::from_u32(value)
                    .filter(|&c| c != '\0')
                    .unwrap_or('\u{fffd}')
            }
        } else {
            c
        };
        result.push(if fold_underscore && c == '_' { '-' } else { c });
    }
    Cow::Owned(result)
}
