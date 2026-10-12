use std::{error::Error, fmt};

use frameshift_core::{SourceText, Span, TextError};

use crate::{
    ast::{
        AssignmentOperator, BinaryOperator, ChildList, NodeId, SyntaxNode, SyntaxNodeKind,
        SyntaxTree, TemplatePart, TemplatePartList, UnaryOperator, UpdateOperator,
    },
    identifier::{IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass},
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
    value::{identifier_chars, identifier_equals, string_literal_equals},
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
    /// A required closing bracket was not present.
    ExpectedClosingBracket,
    /// A required closing brace was not present.
    ExpectedClosingBrace,
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
    /// An optional chain cannot be the constructor of a `new` expression.
    OptionalChainInNew,
    /// An object literal member is not a property, shorthand property, or spread element.
    InvalidPropertyDefinition,
    /// The left side of an assignment, an update operand, or a destructuring
    /// element is not a valid assignment target.
    InvalidAssignmentTarget,
    /// A rest element is not last, is followed by a comma, has a default value,
    /// or has a form its pattern does not permit.
    InvalidRestElement,
    /// A name or parenthesized list before `=>` is not a valid parameter list.
    InvalidArrowParameters,
    /// An arrow function declares the same parameter name more than once.
    DuplicateParameter,
    /// An empty list, rest element, or trailing comma in parentheses is not
    /// followed by `=>`.
    ExpectedArrow,
    /// `=>` appears where an arrow function is not permitted.
    UnexpectedArrow,
    /// A line terminator separates arrow parameters from `=>`.
    LineTerminatorBeforeArrow,
    /// A shorthand property with an initializer is used outside a destructuring pattern.
    ShorthandInitializerOutsidePattern,
    /// An object literal defines `__proto__` more than once with `:`.
    DuplicateProto,
    /// The construct is valid in sloppy mode and invalid in strict mode code,
    /// so its validity requires source context.
    StrictModeRequiresContext,
    /// `new.target` requires function context.
    MetaPropertyRequiresContext,
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
            ParseErrorKind::ExpectedClosingBracket => "expected a closing bracket",
            ParseErrorKind::ExpectedClosingBrace => "expected a closing brace",
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
            ParseErrorKind::OptionalChainInNew => {
                "an optional chain cannot be used as a constructor in a new expression"
            }
            ParseErrorKind::InvalidPropertyDefinition => "invalid object literal member",
            ParseErrorKind::InvalidAssignmentTarget => "invalid assignment target",
            ParseErrorKind::InvalidRestElement => "invalid rest element",
            ParseErrorKind::InvalidArrowParameters => "invalid arrow function parameters",
            ParseErrorKind::DuplicateParameter => "duplicate arrow function parameter name",
            ParseErrorKind::ExpectedArrow => "expected an arrow after the parameter list",
            ParseErrorKind::UnexpectedArrow => "an arrow function is not permitted here",
            ParseErrorKind::LineTerminatorBeforeArrow => {
                "a line terminator cannot precede an arrow"
            }
            ParseErrorKind::ShorthandInitializerOutsidePattern => {
                "a shorthand property initializer is only valid in a destructuring pattern"
            }
            ParseErrorKind::DuplicateProto => "duplicate __proto__ property in an object literal",
            ParseErrorKind::StrictModeRequiresContext => {
                "construct depends on strict mode and requires source context"
            }
            ParseErrorKind::MetaPropertyRequiresContext => "new.target requires function context",
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
/// templates, array and object literals, grouping, calls, `new`, member
/// access, spread, unary, update, binary, conditional, and assignment
/// expressions, destructuring assignment, and arrow functions with expression
/// bodies. Destructuring targets and arrow parameters are reinterpreted from
/// their cover expressions in place. It does not parse statements, function
/// bodies, methods, classes, JSX, or TypeScript syntax. Reserved-word,
/// contextual-name, and strict-mode-dependent cases without sufficient source
/// context fail closed. Leading and trailing trivia is consumed and comments
/// are retained in source order. Any error returns no tree. Parsing is
/// deterministic and bounded by `limits`.
pub fn parse_js_expression(
    source: &SourceText,
    offset: usize,
    limits: ParseLimits,
) -> Result<SyntaxTree<'_>, ParseError> {
    let mut parser = Parser::new(source, limits);
    parser.skip_trivia_at(offset)?;
    let root = parser.parse_expression(0)?;
    parser.check_expression_only_errors()?;
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
    // Whether the trivia before the next token contains a line terminator.
    line_break_before: bool,
    nodes: Vec<SyntaxNode>,
    children: Vec<NodeId>,
    template_parts: Vec<TemplatePart>,
    comments: Vec<Comment>,
    // Names declared by the arrow parameter list being converted.
    bound_names: Vec<Span>,
    // Errors that apply only if the object literal stays an expression.
    expression_only_errors: Vec<(NodeId, ParseError)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PostfixMode {
    Full,
    // A `new` constructor: members and tagged templates, but no calls.
    NewCallee,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetMode {
    // Destructuring assignment: targets are references, including members.
    Assignment,
    // Arrow parameters: targets declare names.
    Binding,
}

impl TargetMode {
    fn invalid(self) -> ParseErrorKind {
        match self {
            Self::Assignment => ParseErrorKind::InvalidAssignmentTarget,
            Self::Binding => ParseErrorKind::InvalidArrowParameters,
        }
    }
}

#[derive(Default)]
struct ObjectState {
    proto_seen: bool,
    first_error: Option<ParseError>,
}

impl ObjectState {
    // Properties are parsed in source order, so the first error is the earliest.
    fn record(&mut self, error: ParseError) {
        if self.first_error.is_none() {
            self.first_error = Some(error);
        }
    }
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
            line_break_before: false,
            nodes: Vec::new(),
            children: Vec::new(),
            template_parts: Vec::new(),
            comments: Vec::new(),
            bound_names: Vec::new(),
            expression_only_errors: Vec::new(),
        }
    }

    fn parse_expression(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_expression_inner(minimum);
        self.depth -= 1;
        result
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        if self.depth >= self.limits.max_nesting_depth {
            return Err(self.error(self.cursor, ParseErrorKind::ResourceLimit));
        }
        self.depth += 1;
        Ok(())
    }

    fn parse_expression_inner(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        // Conditional, assignment, and arrow function expressions are
        // AssignmentExpressions, which binary and unary operands exclude.
        let assignment_level = minimum <= 2;
        let mut left = self.parse_unary(assignment_level)?;
        loop {
            if assignment_level
                && let Some(token) = self.peek_punctuator(self.goal_after_expression())?
            {
                if token.kind == Punctuator::Question {
                    self.consume_token(token.span)?;
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
                if let Some(operator) = assignment_operator(token.kind) {
                    self.prepare_assignment_target(left, operator)?;
                    self.consume_token(token.span)?;
                    let value = self.parse_expression(2)?;
                    let span = self
                        .source_span(self.node_span(left).start(), self.node_span(value).end())?;
                    left = self.add_node(
                        SyntaxNodeKind::Assignment {
                            operator,
                            target: left,
                            value,
                        },
                        span,
                    )?;
                    continue;
                }
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

    fn parse_unary(&mut self, arrow_allowed: bool) -> Result<NodeId, ParseError> {
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
                    self.consume_token(token.span)?;
                    // Exponentiation binds more tightly than a prefix operator.
                    // Stopping below its precedence lets the outer expression
                    // reject `-x ** y` while still allowing `x ** -y`.
                    let operand = self.parse_expression(15)?;
                    if operator == UnaryOperator::Delete && self.is_identifier_reference(operand) {
                        // Strict mode code forbids deleting an unqualified name.
                        return Err(self.error(
                            token.span.start(),
                            ParseErrorKind::StrictModeRequiresContext,
                        ));
                    }
                    let span =
                        self.source_span(token.span.start(), self.node_span(operand).end())?;
                    return self.add_node(SyntaxNodeKind::Unary { operator, operand }, span);
                }
            }
        }
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())? {
            if let Some(operator) = unary_operator(token.kind) {
                self.consume_token(token.span)?;
                let operand = self.parse_expression(15)?;
                let span = self.source_span(token.span.start(), self.node_span(operand).end())?;
                return self.add_node(SyntaxNodeKind::Unary { operator, operand }, span);
            }
            if let Some(operator) = update_operator(token.kind) {
                self.consume_token(token.span)?;
                let operand = self.parse_expression(15)?;
                self.check_simple_target(operand, true)?;
                let span = self.source_span(token.span.start(), self.node_span(operand).end())?;
                return self.add_node(
                    SyntaxNodeKind::Update {
                        operator,
                        prefix: true,
                        operand,
                    },
                    span,
                );
            }
        }
        let primary = self.parse_primary(arrow_allowed)?;
        if matches!(
            self.node_kind(primary),
            SyntaxNodeKind::ArrowFunction { .. }
        ) {
            return Ok(primary);
        }
        let expression = self.parse_postfix(primary, PostfixMode::Full)?;
        if let Some(token) = self.peek_punctuator(self.goal_after_expression())?
            && !self.line_break_before
            && let Some(operator) = update_operator(token.kind)
        {
            self.check_simple_target(expression, true)?;
            self.consume_token(token.span)?;
            let span = self.source_span(self.node_span(expression).start(), token.span.end())?;
            return self.add_node(
                SyntaxNodeKind::Update {
                    operator,
                    prefix: false,
                    operand: expression,
                },
                span,
            );
        }
        Ok(expression)
    }

    fn parse_primary(&mut self, arrow_allowed: bool) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let offset = self.cursor;
        let Some(ch) = self.text[offset..].chars().next() else {
            return Err(self.error(offset, ParseErrorKind::ExpectedExpression));
        };
        if self.number_starts_here(ch) {
            return self.parse_number();
        }
        if matches!(ch, '\'' | '"') {
            return self.parse_string().map(|(node, _)| node);
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
            return self.parse_identifier_start(token, arrow_allowed);
        }
        match ch {
            '(' => self.parse_parenthesized(arrow_allowed),
            '[' => self.parse_array(),
            '{' => self.parse_object(),
            _ => Err(self.error(offset, ParseErrorKind::ExpectedExpression)),
        }
    }

    fn number_starts_here(&self, ch: char) -> bool {
        ch.is_ascii_digit()
            || (ch == '.'
                && self
                    .text
                    .as_bytes()
                    .get(self.cursor + 1)
                    .is_some_and(u8::is_ascii_digit))
    }

    fn parse_number(&mut self) -> Result<NodeId, ParseError> {
        let literal = scan_js_number(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)?;
        self.consume_token(literal.span)?;
        self.add_node(
            SyntaxNodeKind::NumberLiteral {
                base: literal.base,
                kind: literal.kind,
            },
            literal.span,
        )
    }

    fn parse_string(&mut self) -> Result<(NodeId, Span), ParseError> {
        let span = scan_js_string(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)?;
        self.consume_token(span)?;
        Ok((self.add_node(SyntaxNodeKind::StringLiteral, span)?, span))
    }

    // Handles a consumed identifier name at the start of a primary expression:
    // `new`, a single-parameter arrow function, or an identifier reference.
    fn parse_identifier_start(
        &mut self,
        token: IdentifierToken,
        arrow_allowed: bool,
    ) -> Result<NodeId, ParseError> {
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(
                token.span.start(),
                ParseErrorKind::PrivateIdentifierRequiresContext,
            ));
        }
        if !token.has_escape && token.reserved_word == Some(ReservedWord::New) {
            return self.parse_new(token);
        }
        if let Some(arrow) = self.peek_arrow()? {
            if !arrow_allowed {
                return Err(self.error(arrow.span.start(), ParseErrorKind::UnexpectedArrow));
            }
            if self.line_break_before {
                return Err(self.error(
                    arrow.span.start(),
                    ParseErrorKind::LineTerminatorBeforeArrow,
                ));
            }
            if let Some(kind) = reference_name_error(token, ParseErrorKind::InvalidArrowParameters)
            {
                return Err(self.error(token.span.start(), kind));
            }
            self.check_restricted_name(token.span)?;
            let parameter = self.add_node(
                SyntaxNodeKind::BindingIdentifier {
                    has_escape: token.has_escape,
                },
                token.span,
            )?;
            self.consume_token(arrow.span)?;
            return self.parse_arrow_body(token.span.start(), &[parameter]);
        }
        if self.is_unescaped(token, "async")
            && !self.line_break_before
            && self.async_function_or_arrow_follows()?
        {
            return Err(self.error(token.span.start(), ParseErrorKind::UnsupportedSyntax));
        }
        self.identifier_primary(token)
    }

    // After `async` without a line break, a name other than `in` or
    // `instanceof` can only start an async arrow function or function.
    fn async_function_or_arrow_follows(&mut self) -> Result<bool, ParseError> {
        if !self.identifier_can_start_here()? {
            return Ok(false);
        }
        let next = self.peek_identifier()?;
        Ok(next.has_escape
            || !matches!(
                next.reserved_word,
                Some(ReservedWord::In | ReservedWord::Instanceof)
            ))
    }

    fn identifier_primary(&mut self, token: IdentifierToken) -> Result<NodeId, ParseError> {
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
            // Valid expression forms that need function or class bodies.
            Some(ReservedWord::Function | ReservedWord::Class | ReservedWord::Import) => {
                return Err(self.error(token.span.start(), ParseErrorKind::UnsupportedSyntax));
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
        self.add_node(kind, token.span)
    }

    fn parse_new(&mut self, keyword: IdentifierToken) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_new_inner(keyword);
        self.depth -= 1;
        result
    }

    fn parse_new_inner(&mut self, keyword: IdentifierToken) -> Result<NodeId, ParseError> {
        if self
            .peek_punctuator(self.goal_for_expression_start())?
            .is_some_and(|token| token.kind == Punctuator::Dot)
        {
            return Err(self.error(
                keyword.span.start(),
                ParseErrorKind::MetaPropertyRequiresContext,
            ));
        }
        // A nested `new` consumes the first argument list, as the grammar's
        // `new MemberExpression Arguments` production requires.
        let callee = self.parse_primary(false)?;
        let callee = self.parse_postfix(callee, PostfixMode::NewCallee)?;
        if self
            .peek_punctuator(self.goal_after_expression())?
            .is_some_and(|token| token.kind == Punctuator::LeftParen)
        {
            let (arguments, close) = self.parse_arguments()?;
            let span = self.source_span(keyword.span.start(), close.end())?;
            return self.add_node(
                SyntaxNodeKind::New {
                    callee,
                    arguments: Some(arguments),
                },
                span,
            );
        }
        let span = self.source_span(keyword.span.start(), self.node_span(callee).end())?;
        self.add_node(
            SyntaxNodeKind::New {
                callee,
                arguments: None,
            },
            span,
        )
    }

    fn parse_postfix(
        &mut self,
        mut expression: NodeId,
        mode: PostfixMode,
    ) -> Result<NodeId, ParseError> {
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
                    self.consume_token(token.span)?;
                    let property = self.parse_identifier_name()?;
                    expression = self.add_member(expression, property, false, false)?;
                }
                Punctuator::OptionalChain => {
                    if mode == PostfixMode::NewCallee {
                        return Err(
                            self.error(token.span.start(), ParseErrorKind::OptionalChainInNew)
                        );
                    }
                    self.consume_token(token.span)?;
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
                    if mode == PostfixMode::NewCallee {
                        break;
                    }
                    expression = self.parse_call(expression, false)?;
                }
                Punctuator::Arrow => return Err(self.arrow_error(expression, token)),
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
                | Punctuator::Slash
                | Punctuator::SlashAssign
                | Punctuator::RightBrace => break,
            }
        }
        Ok(expression)
    }

    // Valid arrow functions are built before postfix parsing, so an arrow
    // here follows an expression that cannot be a parameter list.
    fn arrow_error(&self, expression: NodeId, arrow: PunctuatorToken) -> ParseError {
        if let SyntaxNodeKind::Call {
            callee,
            optional: false,
            ..
        } = self.node_kind(expression)
            && matches!(
                self.node_kind(callee),
                SyntaxNodeKind::Identifier {
                    has_escape: false,
                    ..
                }
            )
            && self.slice(self.node_span(callee)) == "async"
        {
            return self.error(
                self.node_span(expression).start(),
                ParseErrorKind::UnsupportedSyntax,
            );
        }
        self.error(arrow.span.start(), ParseErrorKind::UnexpectedArrow)
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
        let (arguments, close) = self.parse_arguments()?;
        let span = self.source_span(self.node_span(callee).start(), close.end())?;
        self.add_node(
            SyntaxNodeKind::Call {
                callee,
                arguments,
                optional,
            },
            span,
        )
    }

    // Parses `( arguments )` and returns the list and the closing parenthesis span.
    fn parse_arguments(&mut self) -> Result<(ChildList, Span), ParseError> {
        self.expect_punctuator(Punctuator::LeftParen, self.goal_after_expression())?;
        let mut arguments = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.text[self.cursor..].starts_with(')') {
                break;
            }
            let argument = self.parse_element()?;
            self.push_child(&mut arguments, argument)?;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
        }
        let close = self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        let list = self.append_children(&arguments)?;
        Ok((list, close.span))
    }

    // Parses an AssignmentExpression or a spread element `...expression`.
    fn parse_element(&mut self) -> Result<NodeId, ParseError> {
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())?
            && token.kind == Punctuator::Ellipsis
        {
            self.consume_token(token.span)?;
            let argument = self.parse_expression(2)?;
            let span = self.source_span(token.span.start(), self.node_span(argument).end())?;
            return self.add_node(SyntaxNodeKind::Spread { argument }, span);
        }
        self.parse_expression(2)
    }

    // Parses CoverParenthesizedExpressionAndArrowParameterList, then decides
    // between a parenthesized expression and arrow parameters.
    fn parse_parenthesized(&mut self, arrow_allowed: bool) -> Result<NodeId, ParseError> {
        let open =
            self.expect_punctuator(Punctuator::LeftParen, self.goal_for_expression_start())?;
        let mut elements = Vec::new();
        let mut trailing_comma = false;
        let mut has_rest = false;
        loop {
            self.skip_trivia()?;
            if self.text[self.cursor..].starts_with(')') {
                break;
            }
            let element = self.parse_element()?;
            has_rest |= matches!(self.node_kind(element), SyntaxNodeKind::Spread { .. });
            self.push_child(&mut elements, element)?;
            trailing_comma = false;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
            trailing_comma = true;
        }
        let close = self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        if let Some(arrow) = self.peek_arrow()? {
            if !arrow_allowed {
                return Err(self.error(arrow.span.start(), ParseErrorKind::UnexpectedArrow));
            }
            if self.line_break_before {
                return Err(self.error(
                    arrow.span.start(),
                    ParseErrorKind::LineTerminatorBeforeArrow,
                ));
            }
            self.convert_parameters(&elements, trailing_comma)?;
            self.consume_token(arrow.span)?;
            return self.parse_arrow_body(open.span.start(), &elements);
        }
        let Some((&first, rest)) = elements.split_first() else {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedArrow));
        };
        if has_rest || trailing_comma {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedArrow));
        }
        let mut expression = first;
        for &right in rest {
            let span = self.source_span(
                self.node_span(expression).start(),
                self.node_span(right).end(),
            )?;
            expression = self.add_node(
                SyntaxNodeKind::Binary {
                    operator: BinaryOperator::Comma,
                    left: expression,
                    right,
                },
                span,
            )?;
        }
        let span = self.source_span(open.span.start(), close.span.end())?;
        self.add_node(SyntaxNodeKind::Parenthesized { expression }, span)
    }

    // Parses a concise body after a consumed `=>`.
    fn parse_arrow_body(
        &mut self,
        start: usize,
        parameters: &[NodeId],
    ) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        if self.text[self.cursor..].starts_with('{') {
            // Function bodies need statement parsing.
            return Err(self.error(self.cursor, ParseErrorKind::UnsupportedSyntax));
        }
        let body = self.parse_expression(2)?;
        let parameters = self.append_children(parameters)?;
        let span = self.source_span(start, self.node_span(body).end())?;
        self.add_node(SyntaxNodeKind::ArrowFunction { parameters, body }, span)
    }

    fn parse_array(&mut self) -> Result<NodeId, ParseError> {
        let open =
            self.expect_punctuator(Punctuator::LeftBracket, self.goal_for_expression_start())?;
        let mut elements = Vec::new();
        let mut trailing_comma = false;
        loop {
            let token = self.peek_punctuator(self.goal_for_expression_start())?;
            match token.map(|token| token.kind) {
                Some(Punctuator::RightBracket) => break,
                Some(Punctuator::Comma) => {
                    if let Some(comma) = token {
                        let span = self.source_span(comma.span.start(), comma.span.start())?;
                        let hole = self.add_node(SyntaxNodeKind::Elision, span)?;
                        self.push_child(&mut elements, hole)?;
                        self.consume_token(comma.span)?;
                    }
                    trailing_comma = false;
                    continue;
                }
                _ => {}
            }
            let element = self.parse_element()?;
            self.push_child(&mut elements, element)?;
            trailing_comma = false;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
            trailing_comma = true;
        }
        let close =
            self.expect_punctuator(Punctuator::RightBracket, self.goal_after_expression())?;
        let elements = self.append_children(&elements)?;
        let span = self.source_span(open.span.start(), close.span.end())?;
        self.add_node(
            SyntaxNodeKind::ArrayExpression {
                elements,
                trailing_comma,
            },
            span,
        )
    }

    fn parse_object(&mut self) -> Result<NodeId, ParseError> {
        // Inside braces, a right brace closes the object, not a template substitution.
        let template_expression_depth = self.template_expression_depth;
        self.template_expression_depth = 0;
        let result = self.parse_object_literal();
        self.template_expression_depth = template_expression_depth;
        result
    }

    fn parse_object_literal(&mut self) -> Result<NodeId, ParseError> {
        let open =
            self.expect_punctuator(Punctuator::LeftBrace, self.goal_for_expression_start())?;
        let mut properties = Vec::new();
        let mut trailing_comma = false;
        let mut state = ObjectState::default();
        loop {
            if self
                .peek_punctuator(self.goal_for_expression_start())?
                .is_some_and(|token| token.kind == Punctuator::RightBrace)
            {
                break;
            }
            let property = self.parse_property(&mut state)?;
            self.push_child(&mut properties, property)?;
            trailing_comma = false;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
            trailing_comma = true;
        }
        let close = self.expect_punctuator(Punctuator::RightBrace, self.goal_after_expression())?;
        let properties = self.append_children(&properties)?;
        let span = self.source_span(open.span.start(), close.span.end())?;
        let object = self.add_node(
            SyntaxNodeKind::ObjectExpression {
                properties,
                trailing_comma,
            },
            span,
        )?;
        if let Some(error) = state.first_error {
            self.expression_only_errors
                .try_reserve(1)
                .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
            self.expression_only_errors.push((object, error));
        }
        Ok(object)
    }

    fn parse_property(&mut self, state: &mut ObjectState) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let Some(ch) = self.text[start..].chars().next() else {
            return Err(self.error(start, ParseErrorKind::ExpectedClosingBrace));
        };
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())? {
            return match token.kind {
                Punctuator::Ellipsis => self.parse_element(),
                Punctuator::LeftBracket => {
                    self.consume_token(token.span)?;
                    let key = self.parse_expression(2)?;
                    self.expect_punctuator(Punctuator::RightBracket, self.goal_after_expression())?;
                    self.finish_keyed_property(start, key, true, false, state)
                }
                // Generator methods need function bodies.
                Punctuator::Star => Err(self.error(start, ParseErrorKind::UnsupportedSyntax)),
                Punctuator::LeftBrace
                | Punctuator::LeftParen
                | Punctuator::RightParen
                | Punctuator::RightBracket
                | Punctuator::Dot
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
                | Punctuator::RightBrace => {
                    Err(self.error(start, ParseErrorKind::InvalidPropertyDefinition))
                }
            };
        }
        if matches!(ch, '\'' | '"') {
            let (key, span) = self.parse_string()?;
            let is_proto = string_literal_equals(self.slice(span), "__proto__");
            return self.finish_keyed_property(start, key, false, is_proto, state);
        }
        if self.number_starts_here(ch) {
            let key = self.parse_number()?;
            return self.finish_keyed_property(start, key, false, false, state);
        }
        if self.identifier_can_start_here()? {
            let token = self.scan_identifier()?;
            if token.kind == IdentifierKind::PrivateName {
                return Err(self.error(
                    token.span.start(),
                    ParseErrorKind::PrivateIdentifierRequiresContext,
                ));
            }
            return self.finish_identifier_property(start, token, state);
        }
        Err(self.error(start, ParseErrorKind::InvalidPropertyDefinition))
    }

    fn finish_keyed_property(
        &mut self,
        start: usize,
        key: NodeId,
        computed: bool,
        is_proto: bool,
        state: &mut ObjectState,
    ) -> Result<NodeId, ParseError> {
        let Some(token) = self.peek_punctuator(self.goal_after_expression())? else {
            return Err(self.error(self.cursor, ParseErrorKind::InvalidPropertyDefinition));
        };
        if token.kind == Punctuator::LeftParen {
            // Methods need function bodies.
            return Err(self.error(token.span.start(), ParseErrorKind::UnsupportedSyntax));
        }
        if token.kind != Punctuator::Colon {
            return Err(self.error(
                token.span.start(),
                ParseErrorKind::InvalidPropertyDefinition,
            ));
        }
        self.consume_token(token.span)?;
        let value = self.parse_expression(2)?;
        if is_proto {
            if state.proto_seen {
                state.record(
                    self.error(self.node_span(key).start(), ParseErrorKind::DuplicateProto),
                );
            }
            state.proto_seen = true;
        }
        let span = self.source_span(start, self.node_span(value).end())?;
        self.add_node(
            SyntaxNodeKind::Property {
                key,
                value,
                computed,
            },
            span,
        )
    }

    fn finish_identifier_property(
        &mut self,
        start: usize,
        token: IdentifierToken,
        state: &mut ObjectState,
    ) -> Result<NodeId, ParseError> {
        let next = self.peek_punctuator(self.goal_after_expression())?;
        let next_kind = next.map(|next| next.kind);
        if matches!(next_kind, Some(Punctuator::Colon | Punctuator::LeftParen)) {
            let key = self.add_node(
                SyntaxNodeKind::IdentifierName {
                    has_escape: token.has_escape,
                    reserved_word: token.reserved_word,
                },
                token.span,
            )?;
            let is_proto = identifier_equals(self.slice(token.span), "__proto__");
            return self.finish_keyed_property(start, key, false, is_proto, state);
        }
        if let Some(next) = next
            && matches!(
                next.kind,
                Punctuator::Comma | Punctuator::RightBrace | Punctuator::Assign
            )
        {
            if let Some(kind) =
                reference_name_error(token, ParseErrorKind::InvalidPropertyDefinition)
            {
                return Err(self.error(token.span.start(), kind));
            }
            let identifier = self.add_node(
                SyntaxNodeKind::Identifier {
                    kind: IdentifierKind::Name,
                    has_escape: token.has_escape,
                    reserved_word: token.reserved_word,
                },
                token.span,
            )?;
            if next.kind != Punctuator::Assign {
                return self.add_node(SyntaxNodeKind::ShorthandProperty { identifier }, token.span);
            }
            // CoverInitializedName is valid only if the object becomes a pattern.
            self.consume_token(next.span)?;
            let default = self.parse_expression(2)?;
            state.record(self.error(
                token.span.start(),
                ParseErrorKind::ShorthandInitializerOutsidePattern,
            ));
            let span = self.source_span(start, self.node_span(default).end())?;
            return self.add_node(
                SyntaxNodeKind::ShorthandPatternProperty {
                    target: identifier,
                    default: Some(default),
                },
                span,
            );
        }
        if !token.has_escape && matches!(self.slice(token.span), "get" | "set" | "async") {
            // Accessors and async methods need function bodies.
            return Err(self.error(token.span.start(), ParseErrorKind::UnsupportedSyntax));
        }
        Err(self.error(self.cursor, ParseErrorKind::InvalidPropertyDefinition))
    }

    fn prepare_assignment_target(
        &mut self,
        target: NodeId,
        operator: AssignmentOperator,
    ) -> Result<(), ParseError> {
        if operator == AssignmentOperator::Assign
            && matches!(
                self.node_kind(target),
                SyntaxNodeKind::ArrayExpression { .. } | SyntaxNodeKind::ObjectExpression { .. }
            )
        {
            return self.convert_target(target, TargetMode::Assignment);
        }
        // Logical assignment requires a simple target; `=` and compound
        // assignment also accept web-compat call targets in sloppy mode.
        let web_compat = !matches!(
            operator,
            AssignmentOperator::LogicalAndAssign
                | AssignmentOperator::LogicalOrAssign
                | AssignmentOperator::NullishAssign
        );
        self.check_simple_target(target, web_compat)
    }

    // Accepts an identifier reference or a member expression without optional
    // chaining, possibly parenthesized. Where `web_compat` permits it, a call
    // is a valid target in sloppy mode only (a runtime ReferenceError), so its
    // validity requires strict-mode context.
    fn check_simple_target(&self, id: NodeId, web_compat: bool) -> Result<(), ParseError> {
        let mut current = id;
        while let SyntaxNodeKind::Parenthesized { expression } = self.node_kind(current) {
            current = expression;
        }
        match self.node_kind(current) {
            SyntaxNodeKind::Identifier {
                kind: IdentifierKind::Name,
                ..
            } => return self.check_restricted_name(self.node_span(current)),
            SyntaxNodeKind::Member { .. } if !self.is_optional_chain(current) => return Ok(()),
            SyntaxNodeKind::Call { .. } if web_compat && !self.is_optional_chain(current) => {
                return Err(self.error(
                    self.node_span(id).start(),
                    ParseErrorKind::StrictModeRequiresContext,
                ));
            }
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
            | SyntaxNodeKind::Member { .. }
            | SyntaxNodeKind::Call { .. }
            | SyntaxNodeKind::TaggedTemplate { .. }
            | SyntaxNodeKind::ArrayExpression { .. }
            | SyntaxNodeKind::ObjectExpression { .. }
            | SyntaxNodeKind::Elision
            | SyntaxNodeKind::Spread { .. }
            | SyntaxNodeKind::Property { .. }
            | SyntaxNodeKind::ShorthandProperty { .. }
            | SyntaxNodeKind::Assignment { .. }
            | SyntaxNodeKind::Update { .. }
            | SyntaxNodeKind::New { .. }
            | SyntaxNodeKind::ArrowFunction { .. }
            | SyntaxNodeKind::BindingIdentifier { .. }
            | SyntaxNodeKind::ArrayPattern { .. }
            | SyntaxNodeKind::ObjectPattern { .. }
            | SyntaxNodeKind::PatternProperty { .. }
            | SyntaxNodeKind::ShorthandPatternProperty { .. }
            | SyntaxNodeKind::RestElement { .. }
            | SyntaxNodeKind::AssignmentPattern { .. } => {}
        }
        Err(self.error(
            self.node_span(id).start(),
            ParseErrorKind::InvalidAssignmentTarget,
        ))
    }

    // Reinterprets a parenthesized cover list as arrow parameters.
    fn convert_parameters(
        &mut self,
        parameters: &[NodeId],
        trailing_comma: bool,
    ) -> Result<(), ParseError> {
        self.bound_names.clear();
        for (index, &parameter) in parameters.iter().enumerate() {
            let last = index + 1 == parameters.len();
            self.convert_list_element(parameter, last, trailing_comma, TargetMode::Binding)?;
        }
        self.check_duplicate_bound_names()
    }

    // Converts an element of an array pattern or parameter list, including
    // holes, rest elements, and defaults.
    fn convert_list_element(
        &mut self,
        element: NodeId,
        last: bool,
        trailing_comma: bool,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        match self.node_kind(element) {
            SyntaxNodeKind::Elision => Ok(()),
            SyntaxNodeKind::Spread { argument } | SyntaxNodeKind::RestElement { argument } => {
                if !last || trailing_comma {
                    return Err(self.error(
                        self.node_span(element).start(),
                        ParseErrorKind::InvalidRestElement,
                    ));
                }
                self.convert_rest_argument(argument, false, mode)?;
                self.set_kind(element, SyntaxNodeKind::RestElement { argument });
                Ok(())
            }
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
            | SyntaxNodeKind::Member { .. }
            | SyntaxNodeKind::Call { .. }
            | SyntaxNodeKind::TaggedTemplate { .. }
            | SyntaxNodeKind::ArrayExpression { .. }
            | SyntaxNodeKind::ObjectExpression { .. }
            | SyntaxNodeKind::Property { .. }
            | SyntaxNodeKind::ShorthandProperty { .. }
            | SyntaxNodeKind::Assignment { .. }
            | SyntaxNodeKind::Update { .. }
            | SyntaxNodeKind::New { .. }
            | SyntaxNodeKind::ArrowFunction { .. }
            | SyntaxNodeKind::BindingIdentifier { .. }
            | SyntaxNodeKind::ArrayPattern { .. }
            | SyntaxNodeKind::ObjectPattern { .. }
            | SyntaxNodeKind::PatternProperty { .. }
            | SyntaxNodeKind::ShorthandPatternProperty { .. }
            | SyntaxNodeKind::AssignmentPattern { .. } => self.convert_element(element, mode),
        }
    }

    // A rest target cannot have a default; an object rest target cannot be a
    // nested pattern.
    fn convert_rest_argument(
        &mut self,
        argument: NodeId,
        object_rest: bool,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        let kind = self.node_kind(argument);
        let nested_pattern = matches!(
            kind,
            SyntaxNodeKind::ArrayExpression { .. }
                | SyntaxNodeKind::ObjectExpression { .. }
                | SyntaxNodeKind::ArrayPattern { .. }
                | SyntaxNodeKind::ObjectPattern { .. }
        );
        if matches!(
            kind,
            SyntaxNodeKind::Assignment { .. } | SyntaxNodeKind::AssignmentPattern { .. }
        ) || (object_rest && nested_pattern)
        {
            return Err(self.error(
                self.node_span(argument).start(),
                ParseErrorKind::InvalidRestElement,
            ));
        }
        self.convert_target(argument, mode)
    }

    // Converts a target that may carry a default value (`target = default`).
    fn convert_element(&mut self, element: NodeId, mode: TargetMode) -> Result<(), ParseError> {
        if let SyntaxNodeKind::Assignment {
            operator: AssignmentOperator::Assign,
            target,
            value,
        } = self.node_kind(element)
        {
            // An assignment target was already converted when it was parsed.
            if mode == TargetMode::Binding {
                self.convert_target(target, mode)?;
            }
            self.set_kind(
                element,
                SyntaxNodeKind::AssignmentPattern {
                    target,
                    default: value,
                },
            );
            return Ok(());
        }
        if let SyntaxNodeKind::AssignmentPattern { target, .. } = self.node_kind(element) {
            if mode == TargetMode::Binding {
                self.convert_target(target, mode)?;
            }
            return Ok(());
        }
        self.convert_target(element, mode)
    }

    // Converts a target without a default. Patterns produced by an earlier
    // assignment conversion are walked again when they become parameters.
    fn convert_target(&mut self, id: NodeId, mode: TargetMode) -> Result<(), ParseError> {
        match self.node_kind(id) {
            SyntaxNodeKind::Identifier {
                kind: IdentifierKind::Name,
                has_escape,
                ..
            } => {
                let span = self.node_span(id);
                self.check_restricted_name(span)?;
                if mode == TargetMode::Binding {
                    self.set_kind(id, SyntaxNodeKind::BindingIdentifier { has_escape });
                    self.bound_names
                        .try_reserve(1)
                        .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
                    self.bound_names.push(span);
                }
                Ok(())
            }
            SyntaxNodeKind::Member { .. }
                if mode == TargetMode::Assignment && !self.is_optional_chain(id) =>
            {
                Ok(())
            }
            SyntaxNodeKind::Parenthesized { .. } if mode == TargetMode::Assignment => {
                self.check_simple_target(id, false)
            }
            SyntaxNodeKind::ArrayExpression {
                elements,
                trailing_comma,
            } => {
                self.convert_array_elements(elements, trailing_comma, mode)?;
                self.set_kind(
                    id,
                    SyntaxNodeKind::ArrayPattern {
                        elements,
                        trailing_comma,
                    },
                );
                Ok(())
            }
            SyntaxNodeKind::ArrayPattern {
                elements,
                trailing_comma,
            } => match mode {
                TargetMode::Assignment => Ok(()),
                TargetMode::Binding => self.convert_array_elements(elements, trailing_comma, mode),
            },
            SyntaxNodeKind::ObjectExpression {
                properties,
                trailing_comma,
            } => {
                self.convert_object_properties(properties, trailing_comma, mode)?;
                self.set_kind(
                    id,
                    SyntaxNodeKind::ObjectPattern {
                        properties,
                        trailing_comma,
                    },
                );
                Ok(())
            }
            SyntaxNodeKind::ObjectPattern {
                properties,
                trailing_comma,
            } => match mode {
                TargetMode::Assignment => Ok(()),
                TargetMode::Binding => {
                    self.convert_object_properties(properties, trailing_comma, mode)
                }
            },
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
            | SyntaxNodeKind::Member { .. }
            | SyntaxNodeKind::Call { .. }
            | SyntaxNodeKind::TaggedTemplate { .. }
            | SyntaxNodeKind::Elision
            | SyntaxNodeKind::Spread { .. }
            | SyntaxNodeKind::Property { .. }
            | SyntaxNodeKind::ShorthandProperty { .. }
            | SyntaxNodeKind::Assignment { .. }
            | SyntaxNodeKind::Update { .. }
            | SyntaxNodeKind::New { .. }
            | SyntaxNodeKind::ArrowFunction { .. }
            | SyntaxNodeKind::BindingIdentifier { .. }
            | SyntaxNodeKind::PatternProperty { .. }
            | SyntaxNodeKind::ShorthandPatternProperty { .. }
            | SyntaxNodeKind::RestElement { .. }
            | SyntaxNodeKind::AssignmentPattern { .. } => {
                Err(self.error(self.node_span(id).start(), mode.invalid()))
            }
        }
    }

    fn convert_array_elements(
        &mut self,
        elements: ChildList,
        trailing_comma: bool,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        let range = elements.range();
        let last = range.end;
        for index in range {
            let element = self.children[index];
            self.convert_list_element(element, index + 1 == last, trailing_comma, mode)?;
        }
        Ok(())
    }

    fn convert_object_properties(
        &mut self,
        properties: ChildList,
        trailing_comma: bool,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        let range = properties.range();
        let last = range.end;
        for index in range {
            let property = self.children[index];
            match self.node_kind(property) {
                SyntaxNodeKind::Property {
                    key,
                    value,
                    computed,
                } => {
                    self.convert_element(value, mode)?;
                    self.set_kind(
                        property,
                        SyntaxNodeKind::PatternProperty {
                            key,
                            value,
                            computed,
                        },
                    );
                }
                SyntaxNodeKind::PatternProperty { value, .. } => {
                    self.convert_element(value, mode)?;
                }
                SyntaxNodeKind::ShorthandProperty { identifier } => {
                    self.convert_target(identifier, mode)?;
                    self.set_kind(
                        property,
                        SyntaxNodeKind::ShorthandPatternProperty {
                            target: identifier,
                            default: None,
                        },
                    );
                }
                SyntaxNodeKind::ShorthandPatternProperty { target, .. } => {
                    self.convert_target(target, mode)?;
                }
                SyntaxNodeKind::Spread { argument } | SyntaxNodeKind::RestElement { argument } => {
                    if index + 1 != last || trailing_comma {
                        return Err(self.error(
                            self.node_span(property).start(),
                            ParseErrorKind::InvalidRestElement,
                        ));
                    }
                    self.convert_rest_argument(argument, true, mode)?;
                    self.set_kind(property, SyntaxNodeKind::RestElement { argument });
                }
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
                | SyntaxNodeKind::Member { .. }
                | SyntaxNodeKind::Call { .. }
                | SyntaxNodeKind::TaggedTemplate { .. }
                | SyntaxNodeKind::ArrayExpression { .. }
                | SyntaxNodeKind::ObjectExpression { .. }
                | SyntaxNodeKind::Elision
                | SyntaxNodeKind::Assignment { .. }
                | SyntaxNodeKind::Update { .. }
                | SyntaxNodeKind::New { .. }
                | SyntaxNodeKind::ArrowFunction { .. }
                | SyntaxNodeKind::BindingIdentifier { .. }
                | SyntaxNodeKind::ArrayPattern { .. }
                | SyntaxNodeKind::ObjectPattern { .. }
                | SyntaxNodeKind::AssignmentPattern { .. } => {
                    return Err(self.error(self.node_span(property).start(), mode.invalid()));
                }
            }
        }
        Ok(())
    }

    // Reports the earliest second declaration of a parameter name, comparing
    // StringValues so that escaped and plain spellings collide.
    fn check_duplicate_bound_names(&self) -> Result<(), ParseError> {
        let names = &self.bound_names;
        if names.len() < 2 {
            return Ok(());
        }
        let mut order = Vec::new();
        order
            .try_reserve_exact(names.len())
            .map_err(|_| self.error(names[0].start(), ParseErrorKind::AllocationFailed))?;
        order.extend(0..names.len());
        let value = |index: usize| identifier_chars(self.slice(names[index]));
        // Sorting keeps the check O(n log n) for hostile parameter counts.
        order.sort_unstable_by(|&a, &b| value(a).cmp(value(b)).then(a.cmp(&b)));
        let duplicate = order
            .windows(2)
            .filter(|pair| value(pair[0]).eq(value(pair[1])))
            .map(|pair| names[pair[1]].start())
            .min();
        match duplicate {
            Some(offset) => Err(self.error(offset, ParseErrorKind::DuplicateParameter)),
            None => Ok(()),
        }
    }

    // Shorthand initializers and duplicate `__proto__` properties are errors
    // only in object literals that did not become patterns.
    fn check_expression_only_errors(&self) -> Result<(), ParseError> {
        let earliest = self
            .expression_only_errors
            .iter()
            .filter(|(object, _)| {
                matches!(
                    self.node_kind(*object),
                    SyntaxNodeKind::ObjectExpression { .. }
                )
            })
            .map(|(_, error)| error)
            .min_by_key(|error| error.offset);
        match earliest {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    // `eval` and `arguments` cannot be assigned or declared in strict mode code.
    fn check_restricted_name(&self, span: Span) -> Result<(), ParseError> {
        let name = self.slice(span);
        if identifier_equals(name, "eval") || identifier_equals(name, "arguments") {
            return Err(self.error(span.start(), ParseErrorKind::StrictModeRequiresContext));
        }
        Ok(())
    }

    fn is_identifier_reference(&self, id: NodeId) -> bool {
        let mut current = id;
        while let SyntaxNodeKind::Parenthesized { expression } = self.node_kind(current) {
            current = expression;
        }
        matches!(self.node_kind(current), SyntaxNodeKind::Identifier { .. })
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
        }))
    }

    fn consume_binary_operator(&mut self, candidate: BinaryCandidate) -> Result<(), ParseError> {
        self.consume_token(candidate.span)
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
                | SyntaxNodeKind::TaggedTemplate { .. }
                | SyntaxNodeKind::ArrayExpression { .. }
                | SyntaxNodeKind::ObjectExpression { .. }
                | SyntaxNodeKind::Elision
                | SyntaxNodeKind::Spread { .. }
                | SyntaxNodeKind::Property { .. }
                | SyntaxNodeKind::ShorthandProperty { .. }
                | SyntaxNodeKind::Assignment { .. }
                | SyntaxNodeKind::Update { .. }
                | SyntaxNodeKind::New { .. }
                | SyntaxNodeKind::ArrowFunction { .. }
                | SyntaxNodeKind::BindingIdentifier { .. }
                | SyntaxNodeKind::ArrayPattern { .. }
                | SyntaxNodeKind::ObjectPattern { .. }
                | SyntaxNodeKind::PatternProperty { .. }
                | SyntaxNodeKind::ShorthandPatternProperty { .. }
                | SyntaxNodeKind::RestElement { .. }
                | SyntaxNodeKind::AssignmentPattern { .. } => return false,
            }
        }
    }

    fn push_child(&self, items: &mut Vec<NodeId>, item: NodeId) -> Result<(), ParseError> {
        items
            .try_reserve(1)
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        items.push(item);
        Ok(())
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

    // Reinterprets a cover node in place, keeping its ID and span.
    fn set_kind(&mut self, id: NodeId, kind: SyntaxNodeKind) {
        self.nodes[id.index()].kind = kind;
    }

    fn consume_token(&mut self, span: Span) -> Result<(), ParseError> {
        self.record_token(span.start())?;
        self.cursor = span.end();
        self.line_break_before = false;
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
        let token = self.peek_identifier()?;
        self.consume_token(token.span)?;
        Ok(token)
    }

    fn peek_identifier(&mut self) -> Result<IdentifierToken, ParseError> {
        self.skip_trivia()?;
        crate::identifier::scan_js_identifier(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)
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

    fn peek_arrow(&mut self) -> Result<Option<PunctuatorToken>, ParseError> {
        Ok(self
            .peek_punctuator(self.goal_after_expression())?
            .filter(|token| token.kind == Punctuator::Arrow))
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
            self.line_break_before = trivia.has_line_terminator;
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

    fn is_unescaped(&self, token: IdentifierToken, word: &str) -> bool {
        !token.has_escape && self.slice(token.span) == word
    }

    fn slice(&self, span: Span) -> &'source str {
        &self.text[span.start()..span.end()]
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
}

fn parse_lexical_error(error: LexicalError) -> ParseError {
    ParseError {
        offset: error.offset,
        kind: ParseErrorKind::Lexical(error.kind),
    }
}

// Classifies a reserved word used as a binding or shorthand reference.
// `invalid` is reported for words that no context permits.
fn reference_name_error(token: IdentifierToken, invalid: ParseErrorKind) -> Option<ParseErrorKind> {
    let word = token.reserved_word?;
    Some(match word.class() {
        ReservedWordClass::Always => invalid,
        ReservedWordClass::Contextual => ParseErrorKind::ContextualKeywordRequiresContext,
        ReservedWordClass::Strict => ParseErrorKind::ReservedWordRequiresContext,
    })
}

fn update_operator(kind: Punctuator) -> Option<UpdateOperator> {
    if kind == Punctuator::Increment {
        Some(UpdateOperator::Increment)
    } else if kind == Punctuator::Decrement {
        Some(UpdateOperator::Decrement)
    } else {
        None
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

fn assignment_operator(kind: Punctuator) -> Option<AssignmentOperator> {
    Some(match kind {
        Punctuator::Assign => AssignmentOperator::Assign,
        Punctuator::PlusAssign => AssignmentOperator::AddAssign,
        Punctuator::MinusAssign => AssignmentOperator::SubtractAssign,
        Punctuator::StarAssign => AssignmentOperator::MultiplyAssign,
        Punctuator::SlashAssign => AssignmentOperator::DivideAssign,
        Punctuator::PercentAssign => AssignmentOperator::RemainderAssign,
        Punctuator::ExponentAssign => AssignmentOperator::ExponentAssign,
        Punctuator::ShiftLeftAssign => AssignmentOperator::ShiftLeftAssign,
        Punctuator::ShiftRightAssign => AssignmentOperator::ShiftRightAssign,
        Punctuator::ShiftRightUnsignedAssign => AssignmentOperator::ShiftRightUnsignedAssign,
        Punctuator::BitAndAssign => AssignmentOperator::BitwiseAndAssign,
        Punctuator::BitOrAssign => AssignmentOperator::BitwiseOrAssign,
        Punctuator::BitXorAssign => AssignmentOperator::BitwiseXorAssign,
        Punctuator::LogicalAndAssign => AssignmentOperator::LogicalAndAssign,
        Punctuator::LogicalOrAssign => AssignmentOperator::LogicalOrAssign,
        Punctuator::NullishAssign => AssignmentOperator::NullishAssign,
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
        | Punctuator::Arrow
        | Punctuator::OptionalChain
        | Punctuator::Slash
        | Punctuator::RightBrace => return None,
    })
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
        Punctuator::RightBracket => ParseErrorKind::ExpectedClosingBracket,
        Punctuator::RightBrace => ParseErrorKind::ExpectedClosingBrace,
        Punctuator::LeftBrace
        | Punctuator::LeftParen
        | Punctuator::LeftBracket
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
        | Punctuator::SlashAssign => ParseErrorKind::ExpectedExpression,
    }
}
