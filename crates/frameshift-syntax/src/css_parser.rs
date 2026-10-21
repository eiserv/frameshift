//! Generic CSS structure from the draft pinned in `D-077` (`D-078`).
//!
//! This layer deliberately has no selector, property, or at-rule catalog.
//! Unknown grammars remain represented, without a claim of CSS validity.

use std::fmt;

use frameshift_core::{SourceText, Span};

use crate::css::{
    CssError, CssLimits, CssParseErrorKind, CssToken, CssTokenContext, CssTokenKind, CssTokens,
    css_token_value, scan_css_token_until, tokenize_css,
};

/// Resource bounds for generic CSS parsing, in addition to tokenization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssParserLimits {
    /// Combined component and syntax arena entries, default 4,194,304.
    pub max_nodes: usize,
    /// Open blocks and functions, default and implementation ceiling 128.
    pub max_depth: usize,
    /// Tokenization and structural diagnostics together, default 256.
    pub max_diagnostics: usize,
    /// Token and component visits, including recovery, default 16,777,216.
    pub max_work: usize,
}

impl Default for CssParserLimits {
    fn default() -> Self {
        Self {
            max_nodes: 4_194_304,
            max_depth: 128,
            max_diagnostics: 256,
            max_work: 16_777_216,
        }
    }
}

/// A failure that returns no tree, including any recovered portion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CssParserError {
    /// Tokenization or token decoding failed.
    Tokenization(CssError),
    /// The combined arena limit was exceeded.
    NodeLimit,
    /// The configured nesting limit or implementation ceiling was exceeded.
    DepthLimit,
    /// The diagnostic limit was exceeded.
    DiagnosticLimit,
    /// The work limit was exceeded.
    WorkLimit,
}

impl fmt::Display for CssParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tokenization(error) => error.fmt(f),
            Self::NodeLimit => f.write_str("the CSS parser node limit was reached"),
            Self::DepthLimit => f.write_str("the CSS parser depth limit was reached"),
            Self::DiagnosticLimit => f.write_str("the CSS parser diagnostic limit was reached"),
            Self::WorkLimit => f.write_str("the CSS parser work limit was reached"),
        }
    }
}

impl std::error::Error for CssParserError {}

impl From<CssError> for CssParserError {
    fn from(error: CssError) -> Self {
        Self::Tokenization(error)
    }
}

/// An index into one document's component arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssComponentId(usize);

/// An index into one document's rule and declaration arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssNodeId(usize);

/// A preserved component value, either a token, simple block, or function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssComponent {
    /// The token itself, or the opening token of a block or function.
    pub token: CssToken,
    /// Exact source extent, including a closer when present.
    pub span: Span,
    /// Immediate contents of a block or function, in source order.
    pub values: Vec<CssComponentId>,
    /// The closing token, absent for ordinary tokens and EOF-closed blocks.
    pub closing: Option<CssToken>,
}

/// A generic at-rule or qualified rule. No context-specific validity is implied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssRule {
    /// The at-keyword token, or `None` for a qualified rule.
    pub name: Option<CssToken>,
    /// Prelude components, including their whitespace.
    pub prelude: Vec<CssComponentId>,
    /// The body block, or `None` for an at-rule ending without a body.
    pub block: Option<CssComponentId>,
    /// Declarations and nested rules in source order. Contiguous declarations
    /// form the draft's declaration lists; no CSSOM materialization is done.
    pub children: Vec<CssNodeId>,
}

/// A generic declaration, with decoded names available through token decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssDeclaration {
    /// The property name's ident token, preserving its case and spelling.
    pub name: CssToken,
    /// Value components with surrounding whitespace and `!important` removed.
    /// A unicode-range value is retokenized with unicode ranges enabled.
    pub value: Vec<CssComponentId>,
    /// Whether the trailing tokens spell `!important`, case-insensitively.
    pub important: bool,
}

/// The kind of a structural syntax node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CssNodeKind {
    /// A qualified rule or an at-rule.
    Rule(CssRule),
    /// A declaration inside a block.
    Declaration(CssDeclaration),
}

/// A syntax node with its original source span, excluding a declaration's
/// separating semicolon, but including an at-rule's terminating semicolon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssNode {
    /// The complete construct's original source extent.
    pub span: Span,
    /// Structural information.
    pub kind: CssNodeKind,
}

/// A diagnostic that makes the whole file ineligible for transformation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssDiagnosticKind {
    /// A tokenizer parse error, preserved without normalization.
    Tokenization(CssParseErrorKind),
    /// A qualified rule ended without a body or began like a custom property.
    InvalidRule,
    /// An unmatched closing delimiter is preserved as a component token.
    UnexpectedClosing,
    /// A declaration contains a top-level `!` outside its final priority.
    InvalidDeclarationValue,
}

/// A syntax diagnostic in original UTF-8 byte coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssDiagnostic {
    /// The diagnostic category.
    pub kind: CssDiagnosticKind,
    /// Original byte offset, possibly at EOF.
    pub offset: usize,
}

/// Flat component and syntax arenas for one complete stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssDocument {
    tokens: CssTokens,
    components: Vec<CssComponent>,
    values: Vec<CssComponentId>,
    nodes: Vec<CssNode>,
    rules: Vec<CssNodeId>,
    diagnostics: Vec<CssDiagnostic>,
}

impl CssDocument {
    /// Original default-context tokens, comments, BOM, and lexical errors.
    pub fn tokens(&self) -> &CssTokens {
        &self.tokens
    }

    /// Top-level component values, including trivia and discarded syntax.
    pub fn values(&self) -> &[CssComponentId] {
        &self.values
    }

    /// All components, including alternate unicode-range value components.
    pub fn components(&self) -> &[CssComponent] {
        &self.components
    }

    /// Looks up a component index. Indices belong to the producing document.
    pub fn component(&self, id: CssComponentId) -> Option<&CssComponent> {
        self.components.get(id.0)
    }

    /// All syntax nodes. Children precede parents in this arena.
    pub fn nodes(&self) -> &[CssNode] {
        &self.nodes
    }

    /// Looks up a syntax index. Indices belong to the producing document.
    pub fn node(&self, id: CssNodeId) -> Option<&CssNode> {
        self.nodes.get(id.0)
    }

    /// Top-level rules in source order.
    pub fn rules(&self) -> &[CssNodeId] {
        &self.rules
    }

    /// Tokenizer and structural diagnostics, stably sorted by byte offset.
    pub fn diagnostics(&self) -> &[CssDiagnostic] {
        &self.diagnostics
    }

    /// Whether syntax diagnostics prohibit every transformation of this file.
    /// `false` does not establish semantic validity or migration support.
    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

/// Parses generic CSS structure with the draft's component boundaries and
/// declaration/rule disambiguation. Grammar-specific validation is deferred.
/// Syntax diagnostics retain a recovered tree, but prohibit transformation of
/// the whole file. Resource exhaustion returns no document.
pub fn parse_css(
    source: &SourceText,
    tokens: CssLimits,
    limits: CssParserLimits,
) -> Result<CssDocument, CssParserError> {
    let tokens = tokenize_css(source, tokens)?;
    if tokens.errors().len() > limits.max_diagnostics {
        return Err(CssParserError::DiagnosticLimit);
    }
    let diagnostics = tokens
        .errors()
        .iter()
        .map(|error| CssDiagnostic {
            kind: CssDiagnosticKind::Tokenization(error.kind),
            offset: error.offset,
        })
        .collect();
    let mut parser = Parser {
        source,
        limits,
        work: 0,
        components: Vec::new(),
        nodes: Vec::new(),
        diagnostics,
    };
    let values = parser.components(tokens.tokens(), source.len())?;
    let rules = parser.contents(&values, false)?;
    parser.diagnostics.sort_by_key(|error| error.offset);
    Ok(CssDocument {
        tokens,
        components: parser.components,
        values,
        nodes: parser.nodes,
        rules,
        diagnostics: parser.diagnostics,
    })
}

struct Parser<'a> {
    source: &'a SourceText,
    limits: CssParserLimits,
    work: usize,
    components: Vec<CssComponent>,
    nodes: Vec<CssNode>,
    diagnostics: Vec<CssDiagnostic>,
}

impl Parser<'_> {
    fn visit(&mut self) -> Result<(), CssParserError> {
        if self.work == self.limits.max_work {
            return Err(CssParserError::WorkLimit);
        }
        self.work += 1;
        Ok(())
    }

    fn room(&self) -> Result<(), CssParserError> {
        if self.components.len() + self.nodes.len() >= self.limits.max_nodes {
            return Err(CssParserError::NodeLimit);
        }
        Ok(())
    }

    fn diagnostic(&mut self, offset: usize) -> Result<(), CssParserError> {
        self.report(CssDiagnosticKind::InvalidRule, offset)
    }

    fn report(&mut self, kind: CssDiagnosticKind, offset: usize) -> Result<(), CssParserError> {
        if self.diagnostics.len() == self.limits.max_diagnostics {
            return Err(CssParserError::DiagnosticLimit);
        }
        self.diagnostics.push(CssDiagnostic { kind, offset });
        Ok(())
    }

    fn node(&mut self, span: Span, kind: CssNodeKind) -> Result<CssNodeId, CssParserError> {
        self.room()?;
        let id = CssNodeId(self.nodes.len());
        self.nodes.push(CssNode { span, kind });
        Ok(id)
    }

    // Component nesting is iterative, and every arena entry owns only indices.
    fn components(
        &mut self,
        tokens: &[CssToken],
        end: usize,
    ) -> Result<Vec<CssComponentId>, CssParserError> {
        let mut roots = Vec::new();
        let mut stack: Vec<CssComponentId> = Vec::new();
        for &token in tokens {
            self.visit()?;
            if let Some(&id) = stack.last()
                && closing(self.components[id.0].token.kind) == Some(token.kind)
            {
                let component = &mut self.components[id.0];
                component.closing = Some(token);
                component.span = Span::new(component.span.start(), token.span.end()).unwrap();
                stack.pop();
                continue;
            }
            let opens = closing(token.kind).is_some();
            if matches!(
                token.kind,
                CssTokenKind::CloseParen | CssTokenKind::CloseSquare | CssTokenKind::CloseCurly
            ) {
                self.report(CssDiagnosticKind::UnexpectedClosing, token.span.start())?;
            }
            if opens && stack.len() >= self.limits.max_depth.min(128) {
                return Err(CssParserError::DepthLimit);
            }
            self.room()?;
            let id = CssComponentId(self.components.len());
            self.components.push(CssComponent {
                token,
                span: token.span,
                values: Vec::new(),
                closing: None,
            });
            if let Some(parent) = stack.last() {
                self.components[parent.0].values.push(id);
            } else {
                roots.push(id);
            }
            if opens {
                stack.push(id);
            }
        }
        for id in stack {
            let component = &mut self.components[id.0];
            component.span = Span::new(component.span.start(), end).unwrap();
        }
        Ok(roots)
    }

    fn kind(&self, id: CssComponentId) -> CssTokenKind {
        self.components[id.0].token.kind
    }

    fn name(&self, id: CssComponentId) -> Result<String, CssParserError> {
        Ok(css_token_value(self.source, &self.components[id.0].token)?
            .unwrap_or_default()
            .into_owned())
    }

    fn trim<'a>(&self, mut values: &'a [CssComponentId]) -> &'a [CssComponentId] {
        while values
            .first()
            .is_some_and(|&id| self.kind(id) == CssTokenKind::Whitespace)
        {
            values = &values[1..];
        }
        while values
            .last()
            .is_some_and(|&id| self.kind(id) == CssTokenKind::Whitespace)
        {
            values = &values[..values.len() - 1];
        }
        values
    }

    fn contents(
        &mut self,
        values: &[CssComponentId],
        nested: bool,
    ) -> Result<Vec<CssNodeId>, CssParserError> {
        let mut nodes = Vec::new();
        let mut pos = 0;
        while pos < values.len() {
            self.visit()?;
            let kind = self.kind(values[pos]);
            if kind == CssTokenKind::Whitespace
                || (nested && kind == CssTokenKind::Semicolon)
                || (!nested && matches!(kind, CssTokenKind::Cdo | CssTokenKind::Cdc))
            {
                pos += 1;
                continue;
            }
            if kind == CssTokenKind::AtKeyword {
                let (node, end) = self.rule(values, pos, true, nested)?;
                nodes.extend(node);
                pos = end;
                continue;
            }
            if nested && let Some((node, end)) = self.declaration(values, pos)? {
                nodes.push(node);
                pos = end;
                continue;
            }
            let (node, end) = self.rule(values, pos, false, nested)?;
            nodes.extend(node);
            pos = end;
        }
        Ok(nodes)
    }

    fn rule(
        &mut self,
        values: &[CssComponentId],
        start: usize,
        at: bool,
        nested: bool,
    ) -> Result<(Option<CssNodeId>, usize), CssParserError> {
        let first = &self.components[values[start].0];
        let offset = first.span.start();
        let name = at.then_some(first.token);
        let prelude_start = start + usize::from(at);
        let mut pos = prelude_start;
        while pos < values.len() {
            self.visit()?;
            let id = values[pos];
            let kind = self.kind(id);
            if kind == CssTokenKind::CloseCurly {
                if !at {
                    self.diagnostic(self.components[id.0].span.start())?;
                }
                if nested {
                    break;
                }
            }
            if kind == CssTokenKind::Semicolon && (at || nested) {
                if !at {
                    self.diagnostic(self.components[id.0].span.start())?;
                    return Ok((None, pos + 1));
                }
                let span = Span::new(offset, self.components[id.0].span.end()).unwrap();
                let node = self.node(
                    span,
                    CssNodeKind::Rule(CssRule {
                        name,
                        prelude: values[prelude_start..pos].to_vec(),
                        block: None,
                        children: Vec::new(),
                    }),
                )?;
                return Ok((Some(node), pos + 1));
            }
            if kind == CssTokenKind::OpenCurly {
                let prelude = &values[prelude_start..pos];
                if !at && self.custom_prefix(prelude)? {
                    self.diagnostic(offset)?;
                    pos += 1;
                    if nested {
                        while pos < values.len() {
                            self.visit()?;
                            if matches!(
                                self.kind(values[pos]),
                                CssTokenKind::Semicolon | CssTokenKind::CloseCurly
                            ) {
                                break;
                            }
                            pos += 1;
                        }
                    }
                    return Ok((None, pos));
                }
                let children = self.components[id.0].values.clone();
                let children = self.contents(&children, true)?;
                let span = Span::new(offset, self.components[id.0].span.end()).unwrap();
                let node = self.node(
                    span,
                    CssNodeKind::Rule(CssRule {
                        name,
                        prelude: prelude.to_vec(),
                        block: Some(id),
                        children,
                    }),
                )?;
                return Ok((Some(node), pos + 1));
            }
            pos += 1;
        }
        if at {
            let end = if pos > start {
                self.components[values[pos - 1].0].span.end()
            } else {
                offset
            };
            let node = self.node(
                Span::new(offset, end).unwrap(),
                CssNodeKind::Rule(CssRule {
                    name,
                    prelude: values[prelude_start..pos].to_vec(),
                    block: None,
                    children: Vec::new(),
                }),
            )?;
            Ok((Some(node), pos))
        } else {
            self.diagnostic(if pos < values.len() {
                self.components[values[pos].0].span.start()
            } else {
                self.source.len()
            })?;
            // Unmatched close-curly tokens within component blocks are data;
            // consume them here so recovery always advances.
            Ok((None, if pos == start { pos + 1 } else { pos }))
        }
    }

    fn custom_prefix(&self, values: &[CssComponentId]) -> Result<bool, CssParserError> {
        let values = self.trim(values);
        if let Some(&first) = values.first()
            && self.kind(first) == CssTokenKind::Ident
            && self.name(first)?.starts_with("--")
        {
            let rest = self.trim(&values[1..]);
            return Ok(rest
                .first()
                .is_some_and(|&id| self.kind(id) == CssTokenKind::Colon));
        }
        Ok(false)
    }

    fn declaration(
        &mut self,
        values: &[CssComponentId],
        start: usize,
    ) -> Result<Option<(CssNodeId, usize)>, CssParserError> {
        let id = values[start];
        if self.kind(id) != CssTokenKind::Ident {
            return Ok(None);
        }
        let mut pos = start + 1;
        while pos < values.len() && self.kind(values[pos]) == CssTokenKind::Whitespace {
            self.visit()?;
            pos += 1;
        }
        if pos == values.len() || self.kind(values[pos]) != CssTokenKind::Colon {
            return Ok(None);
        }
        let name = self.components[id.0].token;
        let decoded = self.name(id)?;
        let value_start = pos + 1;
        pos = value_start;
        let mut significant = 0;
        let mut curly = false;
        while pos < values.len() {
            self.visit()?;
            let kind = self.kind(values[pos]);
            if matches!(kind, CssTokenKind::Semicolon | CssTokenKind::CloseCurly) {
                break;
            }
            if !decoded.starts_with("--") && kind != CssTokenKind::Whitespace {
                // The draft permits a generic declaration's {} block only
                // as its entire value. Exit at the first proof of a rule,
                // rather than repeatedly scanning the remaining stylesheet.
                if kind == CssTokenKind::OpenCurly && significant > 0 {
                    return Ok(None);
                }
                if curly {
                    if kind != CssTokenKind::Delim('!') {
                        return Ok(None);
                    }
                    let mut end = pos + 1;
                    while end < values.len() && self.kind(values[end]) == CssTokenKind::Whitespace {
                        self.visit()?;
                        end += 1;
                    }
                    let Some(&priority) = values.get(end) else {
                        return Ok(None);
                    };
                    if self.kind(priority) != CssTokenKind::Ident
                        || !self.name(priority)?.eq_ignore_ascii_case("important")
                    {
                        return Ok(None);
                    }
                    end += 1;
                    while end < values.len() && self.kind(values[end]) == CssTokenKind::Whitespace {
                        self.visit()?;
                        end += 1;
                    }
                    if end < values.len()
                        && !matches!(
                            self.kind(values[end]),
                            CssTokenKind::Semicolon | CssTokenKind::CloseCurly
                        )
                    {
                        return Ok(None);
                    }
                    pos = end;
                    break;
                }
                significant += 1;
                curly = kind == CssTokenKind::OpenCurly;
            }
            pos += 1;
        }
        let mut value = self.trim(&values[value_start..pos]);
        let mut important = false;
        if let Some(&last) = value.last()
            && self.kind(last) == CssTokenKind::Ident
            && self.name(last)?.eq_ignore_ascii_case("important")
        {
            let prefix = self.trim(&value[..value.len() - 1]);
            if prefix
                .last()
                .is_some_and(|&id| self.kind(id) == CssTokenKind::Delim('!'))
            {
                value = self.trim(&prefix[..prefix.len() - 1]);
                important = true;
            }
        }
        if !decoded.starts_with("--") {
            let mut non_space = 0;
            let mut curly = false;
            for &id in value {
                self.visit()?;
                if self.kind(id) != CssTokenKind::Whitespace {
                    non_space += 1;
                    curly |= self.kind(id) == CssTokenKind::OpenCurly;
                }
            }
            if curly && non_space > 1 {
                return Ok(None);
            }
        }
        let span = Span::new(
            name.span.start(),
            self.components[values[pos - 1].0].span.end(),
        )
        .unwrap();
        let value = value.to_vec();
        for &id in &value {
            if self.kind(id) == CssTokenKind::Delim('!') {
                self.report(
                    CssDiagnosticKind::InvalidDeclarationValue,
                    self.components[id.0].span.start(),
                )?;
            }
        }
        let value = if decoded.eq_ignore_ascii_case("unicode-range") && !value.is_empty() {
            self.unicode_value(&value)?
        } else {
            value
        };
        let node = self.node(
            span,
            CssNodeKind::Declaration(CssDeclaration {
                name,
                value,
                important,
            }),
        )?;
        Ok(Some((node, pos)))
    }

    fn unicode_value(
        &mut self,
        values: &[CssComponentId],
    ) -> Result<Vec<CssComponentId>, CssParserError> {
        let mut offset = self.components[values[0].0].span.start();
        let end = self.components[values[values.len() - 1].0].span.end();
        let mut tokens = Vec::new();
        while offset < end {
            self.visit()?;
            let scan = scan_css_token_until(
                self.source,
                offset,
                end,
                CssTokenContext {
                    unicode_ranges_allowed: true,
                },
            )?;
            for error in scan.errors {
                self.report(CssDiagnosticKind::Tokenization(error.kind), error.offset)?;
            }
            let Some(token) = scan.token else { break };
            if token.span.start() >= end {
                break;
            }
            offset = token.span.end();
            tokens.push(token);
        }
        self.components(&tokens, end)
    }
}

fn closing(kind: CssTokenKind) -> Option<CssTokenKind> {
    if matches!(kind, CssTokenKind::Function | CssTokenKind::OpenParen) {
        Some(CssTokenKind::CloseParen)
    } else if kind == CssTokenKind::OpenSquare {
        Some(CssTokenKind::CloseSquare)
    } else if kind == CssTokenKind::OpenCurly {
        Some(CssTokenKind::CloseCurly)
    } else {
        None
    }
}
