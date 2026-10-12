use std::{error::Error, fmt};

use frameshift_core::{SourceText, Span, TextError};

use crate::{
    ast::{
        BinaryOperator, ChildList, NodeId, SyntaxNode, SyntaxNodeKind, SyntaxTree, TemplatePart,
        TemplatePartList, UnaryOperator,
    },
    identifier::{IdentifierKind, IdentifierToken, ReservedWord},
    lexical::{
        Comment, LexicalError, LexicalErrorKind, LexicalLimits, scan_js_string, scan_js_trivia,
    },
    numeric::scan_js_number,
    punctuator::{
        GreaterThanMode, JsLexicalGoal, Punctuator, PunctuatorContext, PunctuatorToken,
        scan_js_punctuator,
    },
    regexp::{RegExpLiteral, validate_js_regexp},
    template::{
        TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken,
        scan_js_template,
    },
    unicode::is_js_identifier_start,
};

/// Explicit resource bounds for one JavaScript expression parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    /// Bounds for individual lexical scans.
    pub lexical: LexicalLimits,
    /// Maximum number of syntax nodes retained, default 100,000.
    pub max_nodes: usize,
    /// Maximum consumed lexical tokens, default 200,000.
    pub max_tokens: usize,
    /// Maximum nested expression calls, default 256.
    pub max_nesting_depth: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            lexical: LexicalLimits::default(),
            max_nodes: 100_000,
            max_tokens: 200_000,
            max_nesting_depth: 256,
        }
    }
}

/// Why the expression parser could not produce a complete tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A lower-level lexical scanner rejected the source.
    Lexical(LexicalErrorKind),
    /// No expression can start at this offset.
    ExpectedExpression,
    /// A required closing parenthesis was not present.
    ExpectedClosingParenthesis,
    /// A template substitution was not followed by its closing right brace.
    ExpectedTemplateContinuation,
    /// The input contains tokens after the parsed expression.
    TrailingInput,
    /// A reserved word cannot be interpreted without statement or strict-mode context.
    ReservedWordRequiresContext,
    /// `await` or `yield` requires function and source-type context.
    ContextualKeywordRequiresContext,
    /// A private identifier requires class or object member context.
    PrivateIdentifierRequiresContext,
    /// An escaped spelling of a reserved word is not accepted in this expression subset.
    EscapedReservedWord,
    /// Exponentiation cannot have an unparenthesized unary expression on its left.
    InvalidExponentiation,
    /// `??` cannot mix with `&&` or `||` without parentheses.
    MixedNullishAndLogical,
    /// Optional chains cannot be used directly as tagged-template tags.
    OptionalChainTag,
    /// The source construct is outside this parser increment's supported grammar.
    UnsupportedSyntax,
    /// The configured node, token, or nesting bound was reached.
    ResourceLimit,
    /// Parser storage could not be reserved.
    AllocationFailed,
    /// Internal span construction failed despite parser invariants.
    InternalInvariant(TextError),
}

/// A parse failure with an original-source byte offset and no source snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Original-source byte offset associated with the failure.
    pub offset: usize,
    /// Typed failure reason.
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match &self.kind {
            ParseErrorKind::Lexical(kind) => {
                return write!(
                    f,
                    "{}",
                    LexicalError {
                        offset: self.offset,
                        kind: kind.clone(),
                    }
                );
            }
            ParseErrorKind::ExpectedExpression => "expected an expression",
            ParseErrorKind::ExpectedClosingParenthesis => "expected a closing parenthesis",
            ParseErrorKind::ExpectedTemplateContinuation => {
                "expected the end of a template substitution"
            }
            ParseErrorKind::TrailingInput => "unexpected tokens after the expression",
            ParseErrorKind::ReservedWordRequiresContext => "reserved word requires parser context",
            ParseErrorKind::ContextualKeywordRequiresContext => {
                "contextual keyword requires parser context"
            }
            ParseErrorKind::PrivateIdentifierRequiresContext => {
                "private identifier requires class or object context"
            }
            ParseErrorKind::EscapedReservedWord => {
                "escaped reserved word is unsupported in this expression context"
            }
            ParseErrorKind::InvalidExponentiation => {
                "unparenthesized unary expression cannot precede exponentiation"
            }
            ParseErrorKind::MixedNullishAndLogical => {
                "nullish coalescing cannot mix with logical operators without parentheses"
            }
            ParseErrorKind::OptionalChainTag => {
                "an optional chain cannot be used as a tagged-template tag"
            }
            ParseErrorKind::UnsupportedSyntax => {
                "syntax is outside the supported expression subset"
            }
            ParseErrorKind::ResourceLimit => "expression parse limit exceeded",
            ParseErrorKind::AllocationFailed => "could not allocate expression tree storage",
            ParseErrorKind::InternalInvariant(error) => {
                return write!(f, "internal parse error at {}: {error}", self.offset);
            }
        };
        write!(f, "{message} at byte offset {}", self.offset)
    }
}

impl Error for ParseError {}

/// Parses one complete JavaScript expression into a span-precise arena.
///
/// The parser handles identifiers, primitive literals, regular expressions,
/// templates, grouping, calls, member access, unary, binary, and conditional
/// expressions. It does not parse statements, arrays, objects, functions,
/// JSX, or TypeScript syntax. Reserved-word and contextual-name cases without
/// sufficient source context fail closed. Leading and trailing trivia is
/// consumed and comments are retained in source order. Any error returns no
/// tree. Parsing is deterministic and bounded by `limits`.
pub fn parse_js_expression(
    source: &SourceText,
    offset: usize,
    limits: ParseLimits,
) -> Result<SyntaxTree<'_>, ParseError> {
    let mut parser = Parser::new(source, limits);
    parser.skip_trivia_at(offset)?;
    let root = parser.parse_expression(0)?;
    parser.skip_trivia()?;
    if parser.cursor < parser.text.len() {
        return Err(parser.error(parser.cursor, ParseErrorKind::TrailingInput));
    }
    let consumed_span = parser.source_span(offset, parser.cursor)?;
    Ok(SyntaxTree {
        source,
        nodes: parser.nodes,
        children: parser.children,
        template_parts: parser.template_parts,
        comments: parser.comments,
        root,
        consumed_span,
    })
}

struct Parser<'source> {
    source: &'source SourceText,
    text: &'source str,
    limits: ParseLimits,
    cursor: usize,
    depth: usize,
    token_count: usize,
    template_expression_depth: usize,
    nodes: Vec<SyntaxNode>,
    children: Vec<NodeId>,
    template_parts: Vec<TemplatePart>,
    comments: Vec<Comment>,
}

impl<'source> Parser<'source> {
    fn new(source: &'source SourceText, limits: ParseLimits) -> Self {
        Self {
            source,
            text: source.as_str(),
            limits,
            cursor: 0,
            depth: 0,
            token_count: 0,
            template_expression_depth: 0,
            nodes: Vec::new(),
            children: Vec::new(),
            template_parts: Vec::new(),
            comments: Vec::new(),
        }
    }

    fn parse_expression(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        if self.depth >= self.limits.max_nesting_depth {
            return Err(self.error(self.cursor, ParseErrorKind::ResourceLimit));
        }
        self.depth += 1;
        let result = self.parse_expression_inner(minimum);
        self.depth -= 1;
        result
    }

    fn parse_expression_inner(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            if minimum <= 2
                && let Some(question) = self.peek_punctuator(self.goal_after_expression())?
                && question.kind == Punctuator::Question
            {
                self.consume_punctuator(Punctuator::Question, self.goal_after_expression())?;
                let consequent = self.parse_expression(2)?;
                self.expect_punctuator(Punctuator::Colon, self.goal_after_expression())?;
                let alternate = self.parse_expression(2)?;
                let span = self.source_span(
                    self.node_span(left).start(),
                    self.node_span(alternate).end(),
                )?;
                left = self.add_node(
                    SyntaxNodeKind::Conditional {
                        test: left,
                        consequent,
                        alternate,
                    },
                    span,
                )?;
                continue;
            }

            let Some(operator) = self.peek_binary_operator()? else {
                break;
            };
            if operator.precedence < minimum {
                break;
            }
            if operator.operator == BinaryOperator::Exponent
                && matches!(self.node_kind(left), SyntaxNodeKind::Unary { .. })
            {
                return Err(
                    self.error(operator.span.start(), ParseErrorKind::InvalidExponentiation)
                );
            }
            self.consume_binary_operator(operator)?;
            let right_minimum = if operator.right_associative {
                operator.precedence
            } else {
                operator.precedence + 1
            };
            let right = self.parse_expression(right_minimum)?;
            if self.mixes_nullish_and_logical(operator.operator, left, right) {
                return Err(self.error(
                    operator.span.start(),
                    ParseErrorKind::MixedNullishAndLogical,
                ));
            }
            let span =
                self.source_span(self.node_span(left).start(), self.node_span(right).end())?;
            left = self.add_node(
                SyntaxNodeKind::Binary {
                    operator: operator.operator,
                    left,
                    right,
                },
                span,
            )?;
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        if self.identifier_can_start_here()? {
            let token = self.peek_identifier()?;
            if !token.has_escape {
                let operator = match token.reserved_word {
                    Some(ReservedWord::Typeof) => Some(UnaryOperator::Typeof),
                    Some(ReservedWord::Void) => Some(UnaryOperator::Void),
                    Some(ReservedWord::Delete) => Some(UnaryOperator::Delete),
                    _ => None,
                };
                if let Some(operator) = operator {
                    self.consume_identifier(token)?;
                    // Exponentiation binds more tightly than a prefix operator.
                    // Stopping below its precedence lets the outer expression
                    // reject `-x ** y` while still allowing `x ** -y`.
                    let operand = self.parse_expression(15)?;
                    let span =
                        self.source_span(token.span.start(), self.node_span(operand).end())?;
                    return self.add_node(SyntaxNodeKind::Unary { operator, operand }, span);
                }
            }
        }
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())?
            && let Some(operator) = unary_operator(token.kind)
        {
            self.consume_punctuator(token.kind, self.goal_for_expression_start())?;
            let operand = self.parse_expression(15)?;
            let span = self.source_span(token.span.start(), self.node_span(operand).end())?;
            return self.add_node(SyntaxNodeKind::Unary { operator, operand }, span);
        }
        let primary = self.parse_primary()?;
        self.parse_postfix(primary)
    }

    fn parse_primary(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let offset = self.cursor;
        let Some(ch) = self.text[offset..].chars().next() else {
            return Err(self.error(offset, ParseErrorKind::ExpectedExpression));
        };
        if ch.is_ascii_digit()
            || (ch == '.'
                && self
                    .text
                    .as_bytes()
                    .get(offset + 1)
                    .is_some_and(u8::is_ascii_digit))
        {
            let literal = scan_js_number(self.source, offset, self.limits.lexical)
                .map_err(parse_lexical_error)?;
            self.consume_token(literal.span)?;
            return self.add_node(
                SyntaxNodeKind::NumberLiteral {
                    base: literal.base,
                    kind: literal.kind,
                },
                literal.span,
            );
        }
        if matches!(ch, '\'' | '"') {
            let span = scan_js_string(self.source, offset, self.limits.lexical)
                .map_err(parse_lexical_error)?;
            self.consume_token(span)?;
            return self.add_node(SyntaxNodeKind::StringLiteral, span);
        }
        if ch == '/' {
            let literal = validate_js_regexp(self.source, offset, self.limits.lexical)
                .map_err(parse_lexical_error)?;
            self.consume_token(literal.token.span)?;
            return self.add_regexp(literal);
        }
        if ch == '`' {
            return self.parse_template(TemplateMode::Untagged);
        }
        if self.identifier_can_start_here()? {
            let token = self.scan_identifier()?;
            return self.identifier_primary(token);
        }
        if ch == '(' {
            let open =
                self.expect_punctuator(Punctuator::LeftParen, self.goal_for_expression_start())?;
            let expression = self.parse_expression(0)?;
            let close =
                self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
            let span = self.source_span(open.span.start(), close.span.end())?;
            return self.add_node(SyntaxNodeKind::Parenthesized { expression }, span);
        }
        if matches!(ch, '[' | '{') {
            return Err(self.error(offset, ParseErrorKind::UnsupportedSyntax));
        }
        Err(self.error(offset, ParseErrorKind::ExpectedExpression))
    }

    fn identifier_primary(&mut self, token: IdentifierToken) -> Result<NodeId, ParseError> {
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(
                token.span.start(),
                ParseErrorKind::PrivateIdentifierRequiresContext,
            ));
        }
        if token.has_escape && token.reserved_word.is_some() {
            return Err(self.error(token.span.start(), ParseErrorKind::EscapedReservedWord));
        }
        let kind = match token.reserved_word {
            None => SyntaxNodeKind::Identifier {
                kind: token.kind,
                has_escape: token.has_escape,
                reserved_word: token.reserved_word,
            },
            Some(ReservedWord::True) => SyntaxNodeKind::BooleanLiteral(true),
            Some(ReservedWord::False) => SyntaxNodeKind::BooleanLiteral(false),
            Some(ReservedWord::Null) => SyntaxNodeKind::NullLiteral,
            Some(ReservedWord::This) => SyntaxNodeKind::This,
            Some(ReservedWord::Await | ReservedWord::Yield) => {
                return Err(self.error(
                    token.span.start(),
                    ParseErrorKind::ContextualKeywordRequiresContext,
                ));
            }
            Some(
                ReservedWord::Typeof
                | ReservedWord::Void
                | ReservedWord::Delete
                | ReservedWord::If
                | ReservedWord::Else
                | ReservedWord::Break
                | ReservedWord::Case
                | ReservedWord::Catch
                | ReservedWord::Class
                | ReservedWord::Const
                | ReservedWord::Continue
                | ReservedWord::Debugger
                | ReservedWord::Default
                | ReservedWord::Do
                | ReservedWord::Enum
                | ReservedWord::Export
                | ReservedWord::Extends
                | ReservedWord::Finally
                | ReservedWord::For
                | ReservedWord::Function
                | ReservedWord::Import
                | ReservedWord::In
                | ReservedWord::Instanceof
                | ReservedWord::New
                | ReservedWord::Return
                | ReservedWord::Super
                | ReservedWord::Switch
                | ReservedWord::Throw
                | ReservedWord::Try
                | ReservedWord::Var
                | ReservedWord::While
                | ReservedWord::With
                | ReservedWord::Implements
                | ReservedWord::Interface
                | ReservedWord::Let
                | ReservedWord::Package
                | ReservedWord::Private
                | ReservedWord::Protected
                | ReservedWord::Public
                | ReservedWord::Static,
            ) => {
                return Err(self.error(
                    token.span.start(),
                    ParseErrorKind::ReservedWordRequiresContext,
                ));
            }
        };
        self.consume_identifier(token)?;
        self.add_node(kind, token.span)
    }

    fn parse_postfix(&mut self, mut expression: NodeId) -> Result<NodeId, ParseError> {
        loop {
            let goal = self.goal_after_expression();
            let Some(token) = self.peek_punctuator(goal)? else {
                self.skip_trivia()?;
                if self.text[self.cursor..].starts_with('`') {
                    if self.is_optional_chain(expression) {
                        return Err(self.error(self.cursor, ParseErrorKind::OptionalChainTag));
                    }
                    let template = self.parse_template(TemplateMode::Tagged)?;
                    let span = self.source_span(
                        self.node_span(expression).start(),
                        self.node_span(template).end(),
                    )?;
                    expression = self.add_node(
                        SyntaxNodeKind::TaggedTemplate {
                            tag: expression,
                            template,
                        },
                        span,
                    )?;
                    continue;
                }
                break;
            };
            match token.kind {
                Punctuator::Dot => {
                    self.consume_punctuator(Punctuator::Dot, goal)?;
                    let property = self.parse_identifier_name()?;
                    expression = self.add_member(expression, property, false, false)?;
                }
                Punctuator::OptionalChain => {
                    self.consume_punctuator(Punctuator::OptionalChain, goal)?;
                    self.skip_trivia()?;
                    if self.text[self.cursor..].starts_with('[') {
                        expression = self.parse_computed_member(expression, true)?;
                    } else if self.text[self.cursor..].starts_with('(') {
                        expression = self.parse_call(expression, true)?;
                    } else {
                        let property = self.parse_identifier_name()?;
                        expression = self.add_member(expression, property, false, true)?;
                    }
                }
                Punctuator::LeftBracket => {
                    expression = self.parse_computed_member(expression, false)?;
                }
                Punctuator::LeftParen => {
                    expression = self.parse_call(expression, false)?;
                }
                Punctuator::LeftBrace
                | Punctuator::RightParen
                | Punctuator::RightBracket
                | Punctuator::Ellipsis
                | Punctuator::Semicolon
                | Punctuator::Comma
                | Punctuator::LessThan
                | Punctuator::GreaterThan
                | Punctuator::LessThanEqual
                | Punctuator::GreaterThanEqual
                | Punctuator::Equal
                | Punctuator::NotEqual
                | Punctuator::StrictEqual
                | Punctuator::StrictNotEqual
                | Punctuator::Plus
                | Punctuator::Minus
                | Punctuator::Star
                | Punctuator::Percent
                | Punctuator::Exponent
                | Punctuator::Increment
                | Punctuator::Decrement
                | Punctuator::ShiftLeft
                | Punctuator::ShiftRight
                | Punctuator::ShiftRightUnsigned
                | Punctuator::BitAnd
                | Punctuator::BitOr
                | Punctuator::BitXor
                | Punctuator::Bang
                | Punctuator::Tilde
                | Punctuator::LogicalAnd
                | Punctuator::LogicalOr
                | Punctuator::Nullish
                | Punctuator::Question
                | Punctuator::Colon
                | Punctuator::Assign
                | Punctuator::PlusAssign
                | Punctuator::MinusAssign
                | Punctuator::StarAssign
                | Punctuator::PercentAssign
                | Punctuator::ExponentAssign
                | Punctuator::ShiftLeftAssign
                | Punctuator::ShiftRightAssign
                | Punctuator::ShiftRightUnsignedAssign
                | Punctuator::BitAndAssign
                | Punctuator::BitOrAssign
                | Punctuator::BitXorAssign
                | Punctuator::LogicalAndAssign
                | Punctuator::LogicalOrAssign
                | Punctuator::NullishAssign
                | Punctuator::Arrow
                | Punctuator::Slash
                | Punctuator::SlashAssign
                | Punctuator::RightBrace => break,
            }
        }
        Ok(expression)
    }

    fn parse_identifier_name(&mut self) -> Result<NodeId, ParseError> {
        let token = self.scan_identifier()?;
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(
                token.span.start(),
                ParseErrorKind::PrivateIdentifierRequiresContext,
            ));
        }
        self.add_node(
            SyntaxNodeKind::IdentifierName {
                has_escape: token.has_escape,
                reserved_word: token.reserved_word,
            },
            token.span,
        )
    }

    fn parse_computed_member(
        &mut self,
        object: NodeId,
        optional: bool,
    ) -> Result<NodeId, ParseError> {
        self.expect_punctuator(Punctuator::LeftBracket, self.goal_after_expression())?;
        let property = self.parse_expression(0)?;
        let close =
            self.expect_punctuator(Punctuator::RightBracket, self.goal_after_expression())?;
        let span = self.source_span(self.node_span(object).start(), close.span.end())?;
        self.add_node(
            SyntaxNodeKind::Member {
                object,
                property,
                computed: true,
                optional,
            },
            span,
        )
    }

    fn add_member(
        &mut self,
        object: NodeId,
        property: NodeId,
        computed: bool,
        optional: bool,
    ) -> Result<NodeId, ParseError> {
        let span = self.source_span(
            self.node_span(object).start(),
            self.node_span(property).end(),
        )?;
        self.add_node(
            SyntaxNodeKind::Member {
                object,
                property,
                computed,
                optional,
            },
            span,
        )
    }

    fn parse_call(&mut self, callee: NodeId, optional: bool) -> Result<NodeId, ParseError> {
        self.expect_punctuator(Punctuator::LeftParen, self.goal_after_expression())?;
        let mut arguments = Vec::new();
        self.skip_trivia()?;
        if !self.text[self.cursor..].starts_with(')') {
            loop {
                if self
                    .peek_punctuator(self.goal_for_expression_start())?
                    .is_some_and(|token| token.kind == Punctuator::Ellipsis)
                {
                    return Err(self.error(self.cursor, ParseErrorKind::UnsupportedSyntax));
                }
                let argument = self.parse_expression(2)?;
                arguments
                    .try_reserve(1)
                    .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
                arguments.push(argument);
                if self
                    .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                    .is_none()
                {
                    break;
                }
                self.skip_trivia()?;
                if self.text[self.cursor..].starts_with(')') {
                    break;
                }
            }
        }
        let close = self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        let list = self.append_children(&arguments)?;
        let span = self.source_span(self.node_span(callee).start(), close.span.end())?;
        self.add_node(
            SyntaxNodeKind::Call {
                callee,
                arguments: list,
                optional,
            },
            span,
        )
    }

    fn parse_template(&mut self, mode: TemplateMode) -> Result<NodeId, ParseError> {
        let first = self.scan_template(TemplatePosition::Start, mode)?;
        let start = first.span.start();
        let mut end = first.span.end();
        let mut parts = Vec::new();
        self.push_template_part(&mut parts, TemplatePart::Segment(first))?;
        let mut current = first;
        while matches!(current.kind, TemplateKind::Head | TemplateKind::Middle) {
            self.template_expression_depth += 1;
            let expression_result = self.parse_expression(0);
            self.template_expression_depth -= 1;
            let expression = expression_result?;
            self.push_template_part(&mut parts, TemplatePart::Expression(expression))?;
            self.skip_trivia()?;
            if !self.text[self.cursor..].starts_with('}') {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedTemplateContinuation));
            }
            current = self.scan_template(TemplatePosition::Continuation, mode)?;
            end = current.span.end();
            self.push_template_part(&mut parts, TemplatePart::Segment(current))?;
        }
        let parts = self.append_template_parts(&parts)?;
        let span = self.source_span(start, end)?;
        self.add_node(SyntaxNodeKind::TemplateLiteral { parts }, span)
    }

    fn scan_template(
        &mut self,
        position: TemplatePosition,
        mode: TemplateMode,
    ) -> Result<TemplateToken, ParseError> {
        let token = scan_js_template(
            self.source,
            self.cursor,
            TemplateContext { position, mode },
            self.limits.lexical,
        )
        .map_err(parse_lexical_error)?;
        self.consume_token(token.span)?;
        Ok(token)
    }

    fn peek_binary_operator(&mut self) -> Result<Option<BinaryCandidate>, ParseError> {
        let goal = self.goal_after_expression();
        if let Some(token) = self.peek_punctuator(goal)?
            && let Some((operator, precedence, right_associative)) = binary_operator(token.kind)
        {
            return Ok(Some(BinaryCandidate {
                operator,
                precedence,
                right_associative,
                span: token.span,
                punctuator: Some(token.kind),
                identifier: None,
            }));
        }
        if !self.identifier_can_start_here()? {
            return Ok(None);
        }
        let token = self.peek_identifier()?;
        if token.has_escape {
            return Ok(None);
        }
        let (operator, precedence) = match token.reserved_word {
            Some(ReservedWord::In) => (BinaryOperator::In, 10),
            Some(ReservedWord::Instanceof) => (BinaryOperator::Instanceof, 10),
            _ => return Ok(None),
        };
        Ok(Some(BinaryCandidate {
            operator,
            precedence,
            right_associative: false,
            span: token.span,
            punctuator: None,
            identifier: Some(token),
        }))
    }

    fn consume_binary_operator(&mut self, candidate: BinaryCandidate) -> Result<(), ParseError> {
        if let Some(punctuator) = candidate.punctuator {
            self.consume_punctuator(punctuator, self.goal_after_expression())?;
        } else if let Some(identifier) = candidate.identifier {
            self.consume_identifier(identifier)?;
        }
        Ok(())
    }

    fn mixes_nullish_and_logical(
        &self,
        operator: BinaryOperator,
        left: NodeId,
        right: NodeId,
    ) -> bool {
        let is_nullish = |id| {
            matches!(
                self.node_kind(id),
                SyntaxNodeKind::Binary {
                    operator: BinaryOperator::Nullish,
                    ..
                }
            )
        };
        let is_logical = |id| {
            matches!(
                self.node_kind(id),
                SyntaxNodeKind::Binary {
                    operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                    ..
                }
            )
        };
        match operator {
            BinaryOperator::Nullish => is_logical(left) || is_logical(right),
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                is_nullish(left) || is_nullish(right)
            }
            BinaryOperator::Comma
            | BinaryOperator::BitwiseOr
            | BinaryOperator::BitwiseXor
            | BinaryOperator::BitwiseAnd
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::StrictEqual
            | BinaryOperator::StrictNotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::LessThanEqual
            | BinaryOperator::GreaterThanEqual
            | BinaryOperator::In
            | BinaryOperator::Instanceof
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightUnsigned
            | BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Remainder
            | BinaryOperator::Exponent => false,
        }
    }

    fn is_optional_chain(&self, id: NodeId) -> bool {
        let mut current = id;
        loop {
            match self.node_kind(current) {
                SyntaxNodeKind::Member { optional: true, .. }
                | SyntaxNodeKind::Call { optional: true, .. } => return true,
                SyntaxNodeKind::Member { object, .. } => current = object,
                SyntaxNodeKind::Call { callee, .. } => current = callee,
                SyntaxNodeKind::Identifier { .. }
                | SyntaxNodeKind::IdentifierName { .. }
                | SyntaxNodeKind::NumberLiteral { .. }
                | SyntaxNodeKind::StringLiteral
                | SyntaxNodeKind::RegularExpression { .. }
                | SyntaxNodeKind::TemplateLiteral { .. }
                | SyntaxNodeKind::BooleanLiteral(_)
                | SyntaxNodeKind::NullLiteral
                | SyntaxNodeKind::This
                | SyntaxNodeKind::Parenthesized { .. }
                | SyntaxNodeKind::Unary { .. }
                | SyntaxNodeKind::Binary { .. }
                | SyntaxNodeKind::Conditional { .. }
                | SyntaxNodeKind::TaggedTemplate { .. } => return false,
            }
        }
    }

    fn append_children(&mut self, items: &[NodeId]) -> Result<ChildList, ParseError> {
        let start = self.children.len();
        let end = start
            .checked_add(items.len())
            .filter(|&end| end <= self.limits.max_nodes && end <= u32::MAX as usize)
            .ok_or_else(|| self.error(self.cursor, ParseErrorKind::ResourceLimit))?;
        self.children
            .try_reserve(items.len())
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.children.extend_from_slice(items);
        Ok(ChildList::new(start as u32, (end - start) as u32))
    }

    fn append_template_parts(
        &mut self,
        items: &[TemplatePart],
    ) -> Result<TemplatePartList, ParseError> {
        let start = self.template_parts.len();
        let end = start
            .checked_add(items.len())
            .filter(|&end| end <= self.limits.max_tokens && end <= u32::MAX as usize)
            .ok_or_else(|| self.error(self.cursor, ParseErrorKind::ResourceLimit))?;
        self.template_parts
            .try_reserve(items.len())
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.template_parts.extend_from_slice(items);
        Ok(TemplatePartList::new(start as u32, (end - start) as u32))
    }

    fn push_template_part(
        &self,
        parts: &mut Vec<TemplatePart>,
        part: TemplatePart,
    ) -> Result<(), ParseError> {
        parts
            .try_reserve(1)
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        parts.push(part);
        Ok(())
    }

    fn add_regexp(&mut self, literal: RegExpLiteral) -> Result<NodeId, ParseError> {
        let flags_bytes = u32::try_from(literal.token.flags_span.len()).map_err(|_| {
            self.error(
                literal.token.flags_span.start(),
                ParseErrorKind::ResourceLimit,
            )
        })?;
        self.add_node(
            SyntaxNodeKind::RegularExpression {
                flags: literal.flags,
                capturing_groups: literal.capturing_groups,
                flags_bytes,
            },
            literal.token.span,
        )
    }

    fn add_node(&mut self, kind: SyntaxNodeKind, span: Span) -> Result<NodeId, ParseError> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(self.error(span.start(), ParseErrorKind::ResourceLimit));
        }
        let index = u32::try_from(self.nodes.len())
            .map_err(|_| self.error(span.start(), ParseErrorKind::ResourceLimit))?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
        let id = NodeId::new(index);
        self.nodes.push(SyntaxNode { kind, span });
        Ok(id)
    }

    fn consume_token(&mut self, span: Span) -> Result<(), ParseError> {
        self.record_token(span.start())?;
        self.cursor = span.end();
        Ok(())
    }

    fn record_token(&mut self, offset: usize) -> Result<(), ParseError> {
        if self.token_count >= self.limits.max_tokens {
            return Err(self.error(offset, ParseErrorKind::ResourceLimit));
        }
        self.token_count += 1;
        Ok(())
    }

    fn scan_identifier(&mut self) -> Result<IdentifierToken, ParseError> {
        self.skip_trivia()?;
        let token =
            crate::identifier::scan_js_identifier(self.source, self.cursor, self.limits.lexical)
                .map_err(parse_lexical_error)?;
        self.consume_token(token.span)?;
        Ok(token)
    }

    fn peek_identifier(&mut self) -> Result<IdentifierToken, ParseError> {
        self.skip_trivia()?;
        crate::identifier::scan_js_identifier(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)
    }

    fn consume_identifier(&mut self, token: IdentifierToken) -> Result<(), ParseError> {
        self.consume_token(token.span)
    }

    fn peek_punctuator(
        &mut self,
        goal: JsLexicalGoal,
    ) -> Result<Option<PunctuatorToken>, ParseError> {
        self.skip_trivia()?;
        scan_js_punctuator(
            self.source,
            self.cursor,
            PunctuatorContext {
                goal,
                greater_than: GreaterThanMode::LongestMatch,
            },
        )
        .map_err(parse_lexical_error)
    }

    fn consume_punctuator(
        &mut self,
        expected: Punctuator,
        goal: JsLexicalGoal,
    ) -> Result<Option<PunctuatorToken>, ParseError> {
        let Some(token) = self.peek_punctuator(goal)? else {
            return Ok(None);
        };
        if token.kind != expected {
            return Ok(None);
        }
        self.consume_token(token.span)?;
        Ok(Some(token))
    }

    fn expect_punctuator(
        &mut self,
        expected: Punctuator,
        goal: JsLexicalGoal,
    ) -> Result<PunctuatorToken, ParseError> {
        self.consume_punctuator(expected, goal)?
            .ok_or_else(|| self.error(self.cursor, expected_punctuator_error(expected)))
    }

    fn skip_trivia_at(&mut self, offset: usize) -> Result<(), ParseError> {
        self.cursor = offset;
        self.skip_trivia()
    }

    fn skip_trivia(&mut self) -> Result<(), ParseError> {
        let remaining = self
            .limits
            .lexical
            .max_comments
            .saturating_sub(self.comments.len());
        let trivia = scan_js_trivia(
            self.source,
            self.cursor,
            LexicalLimits {
                max_comments: remaining,
                ..self.limits.lexical
            },
        )
        .map_err(parse_lexical_error)?;
        if !trivia.span.is_empty() {
            self.comments
                .try_reserve(trivia.comments.len())
                .map_err(|_| self.error(trivia.span.start(), ParseErrorKind::AllocationFailed))?;
            self.comments.extend(trivia.comments);
            self.cursor = trivia.span.end();
        }
        Ok(())
    }

    fn goal_for_expression_start(&self) -> JsLexicalGoal {
        if self.template_expression_depth == 0 {
            JsLexicalGoal::RegExp
        } else {
            JsLexicalGoal::RegExpOrTemplateTail
        }
    }

    fn goal_after_expression(&self) -> JsLexicalGoal {
        if self.template_expression_depth == 0 {
            JsLexicalGoal::Div
        } else {
            JsLexicalGoal::TemplateTail
        }
    }

    fn identifier_can_start_here(&mut self) -> Result<bool, ParseError> {
        self.skip_trivia()?;
        Ok(self.text[self.cursor..]
            .chars()
            .next()
            .is_some_and(|ch| is_js_identifier_start(ch) || matches!(ch, '#' | '\\')))
    }

    fn node_kind(&self, id: NodeId) -> SyntaxNodeKind {
        self.nodes[id.index()].kind()
    }

    fn node_span(&self, id: NodeId) -> Span {
        self.nodes[id.index()].span()
    }

    fn source_span(&self, start: usize, end: usize) -> Result<Span, ParseError> {
        Span::new(start, end)
            .map_err(|error| self.error(start, ParseErrorKind::InternalInvariant(error)))
    }

    fn error(&self, offset: usize, kind: ParseErrorKind) -> ParseError {
        ParseError { offset, kind }
    }
}

#[derive(Clone, Copy)]
struct BinaryCandidate {
    operator: BinaryOperator,
    precedence: u8,
    right_associative: bool,
    span: Span,
    punctuator: Option<Punctuator>,
    identifier: Option<IdentifierToken>,
}

fn parse_lexical_error(error: LexicalError) -> ParseError {
    ParseError {
        offset: error.offset,
        kind: ParseErrorKind::Lexical(error.kind),
    }
}

fn unary_operator(kind: Punctuator) -> Option<UnaryOperator> {
    match kind {
        Punctuator::Bang => Some(UnaryOperator::LogicalNot),
        Punctuator::Tilde => Some(UnaryOperator::BitwiseNot),
        Punctuator::Plus => Some(UnaryOperator::Plus),
        Punctuator::Minus => Some(UnaryOperator::Minus),
        Punctuator::LeftBrace
        | Punctuator::LeftParen
        | Punctuator::RightParen
        | Punctuator::LeftBracket
        | Punctuator::RightBracket
        | Punctuator::Dot
        | Punctuator::Ellipsis
        | Punctuator::Semicolon
        | Punctuator::Comma
        | Punctuator::LessThan
        | Punctuator::GreaterThan
        | Punctuator::LessThanEqual
        | Punctuator::GreaterThanEqual
        | Punctuator::Equal
        | Punctuator::NotEqual
        | Punctuator::StrictEqual
        | Punctuator::StrictNotEqual
        | Punctuator::Star
        | Punctuator::Percent
        | Punctuator::Exponent
        | Punctuator::Increment
        | Punctuator::Decrement
        | Punctuator::ShiftLeft
        | Punctuator::ShiftRight
        | Punctuator::ShiftRightUnsigned
        | Punctuator::BitAnd
        | Punctuator::BitOr
        | Punctuator::BitXor
        | Punctuator::LogicalAnd
        | Punctuator::LogicalOr
        | Punctuator::Nullish
        | Punctuator::Question
        | Punctuator::Colon
        | Punctuator::Assign
        | Punctuator::PlusAssign
        | Punctuator::MinusAssign
        | Punctuator::StarAssign
        | Punctuator::PercentAssign
        | Punctuator::ExponentAssign
        | Punctuator::ShiftLeftAssign
        | Punctuator::ShiftRightAssign
        | Punctuator::ShiftRightUnsignedAssign
        | Punctuator::BitAndAssign
        | Punctuator::BitOrAssign
        | Punctuator::BitXorAssign
        | Punctuator::LogicalAndAssign
        | Punctuator::LogicalOrAssign
        | Punctuator::NullishAssign
        | Punctuator::Arrow
        | Punctuator::OptionalChain
        | Punctuator::Slash
        | Punctuator::SlashAssign
        | Punctuator::RightBrace => None,
    }
}

fn binary_operator(kind: Punctuator) -> Option<(BinaryOperator, u8, bool)> {
    Some(match kind {
        Punctuator::Comma => (BinaryOperator::Comma, 1, false),
        Punctuator::Nullish => (BinaryOperator::Nullish, 3, false),
        Punctuator::LogicalOr => (BinaryOperator::LogicalOr, 4, false),
        Punctuator::LogicalAnd => (BinaryOperator::LogicalAnd, 5, false),
        Punctuator::BitOr => (BinaryOperator::BitwiseOr, 6, false),
        Punctuator::BitXor => (BinaryOperator::BitwiseXor, 7, false),
        Punctuator::BitAnd => (BinaryOperator::BitwiseAnd, 8, false),
        Punctuator::Equal => (BinaryOperator::Equal, 9, false),
        Punctuator::NotEqual => (BinaryOperator::NotEqual, 9, false),
        Punctuator::StrictEqual => (BinaryOperator::StrictEqual, 9, false),
        Punctuator::StrictNotEqual => (BinaryOperator::StrictNotEqual, 9, false),
        Punctuator::LessThan => (BinaryOperator::LessThan, 10, false),
        Punctuator::GreaterThan => (BinaryOperator::GreaterThan, 10, false),
        Punctuator::LessThanEqual => (BinaryOperator::LessThanEqual, 10, false),
        Punctuator::GreaterThanEqual => (BinaryOperator::GreaterThanEqual, 10, false),
        Punctuator::ShiftLeft => (BinaryOperator::ShiftLeft, 11, false),
        Punctuator::ShiftRight => (BinaryOperator::ShiftRight, 11, false),
        Punctuator::ShiftRightUnsigned => (BinaryOperator::ShiftRightUnsigned, 11, false),
        Punctuator::Plus => (BinaryOperator::Add, 12, false),
        Punctuator::Minus => (BinaryOperator::Subtract, 12, false),
        Punctuator::Star => (BinaryOperator::Multiply, 13, false),
        Punctuator::Slash => (BinaryOperator::Divide, 13, false),
        Punctuator::Percent => (BinaryOperator::Remainder, 13, false),
        Punctuator::Exponent => (BinaryOperator::Exponent, 14, true),
        Punctuator::LeftBrace
        | Punctuator::LeftParen
        | Punctuator::RightParen
        | Punctuator::LeftBracket
        | Punctuator::RightBracket
        | Punctuator::Dot
        | Punctuator::Ellipsis
        | Punctuator::Semicolon
        | Punctuator::Increment
        | Punctuator::Decrement
        | Punctuator::Bang
        | Punctuator::Tilde
        | Punctuator::Question
        | Punctuator::Colon
        | Punctuator::Assign
        | Punctuator::PlusAssign
        | Punctuator::MinusAssign
        | Punctuator::StarAssign
        | Punctuator::PercentAssign
        | Punctuator::ExponentAssign
        | Punctuator::ShiftLeftAssign
        | Punctuator::ShiftRightAssign
        | Punctuator::ShiftRightUnsignedAssign
        | Punctuator::BitAndAssign
        | Punctuator::BitOrAssign
        | Punctuator::BitXorAssign
        | Punctuator::LogicalAndAssign
        | Punctuator::LogicalOrAssign
        | Punctuator::NullishAssign
        | Punctuator::Arrow
        | Punctuator::OptionalChain
        | Punctuator::SlashAssign
        | Punctuator::RightBrace => return None,
    })
}

fn expected_punctuator_error(kind: Punctuator) -> ParseErrorKind {
    match kind {
        Punctuator::RightParen => ParseErrorKind::ExpectedClosingParenthesis,
        Punctuator::LeftBrace
        | Punctuator::LeftParen
        | Punctuator::LeftBracket
        | Punctuator::RightBracket
        | Punctuator::Dot
        | Punctuator::Ellipsis
        | Punctuator::Semicolon
        | Punctuator::Comma
        | Punctuator::LessThan
        | Punctuator::GreaterThan
        | Punctuator::LessThanEqual
        | Punctuator::GreaterThanEqual
        | Punctuator::Equal
        | Punctuator::NotEqual
        | Punctuator::StrictEqual
        | Punctuator::StrictNotEqual
        | Punctuator::Plus
        | Punctuator::Minus
        | Punctuator::Star
        | Punctuator::Percent
        | Punctuator::Exponent
        | Punctuator::Increment
        | Punctuator::Decrement
        | Punctuator::ShiftLeft
        | Punctuator::ShiftRight
        | Punctuator::ShiftRightUnsigned
        | Punctuator::BitAnd
        | Punctuator::BitOr
        | Punctuator::BitXor
        | Punctuator::Bang
        | Punctuator::Tilde
        | Punctuator::LogicalAnd
        | Punctuator::LogicalOr
        | Punctuator::Nullish
        | Punctuator::Question
        | Punctuator::Colon
        | Punctuator::Assign
        | Punctuator::PlusAssign
        | Punctuator::MinusAssign
        | Punctuator::StarAssign
        | Punctuator::PercentAssign
        | Punctuator::ExponentAssign
        | Punctuator::ShiftLeftAssign
        | Punctuator::ShiftRightAssign
        | Punctuator::ShiftRightUnsignedAssign
        | Punctuator::BitAndAssign
        | Punctuator::BitOrAssign
        | Punctuator::BitXorAssign
        | Punctuator::LogicalAndAssign
        | Punctuator::LogicalOrAssign
        | Punctuator::NullishAssign
        | Punctuator::Arrow
        | Punctuator::OptionalChain
        | Punctuator::Slash
        | Punctuator::SlashAssign
        | Punctuator::RightBrace => ParseErrorKind::ExpectedExpression,
    }
}
