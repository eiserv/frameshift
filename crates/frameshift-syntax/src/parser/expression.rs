use frameshift_core::Span;

use super::{
    CoverMarks, Goal, ParseError, ParseErrorKind, Parser, Permission, StrictModeError,
    parse_lexical_error,
};
use crate::{
    ast::{
        AssignmentOperator, BinaryOperator, FunctionKind, MetaPropertyKind, MethodKind, NodeId,
        SyntaxNodeKind, TemplatePart, UnaryOperator, UpdateOperator,
    },
    identifier::{IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass},
    lexical::{LegacyOctal, StringToken, scan_js_string},
    numeric::scan_js_number,
    punctuator::{Punctuator, PunctuatorToken},
    regexp::{RegExpLiteral, validate_js_regexp},
    template::{
        TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken,
        scan_js_template,
    },
    value::{identifier_equals, string_literal_equals},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PostfixMode {
    Full,
    // A `new` constructor: members and tagged templates, but no calls.
    NewCallee,
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

#[derive(Clone, Copy)]
struct BinaryCandidate {
    operator: BinaryOperator,
    precedence: u8,
    right_associative: bool,
    span: Span,
}

// A parsed parenthesized list that may become arrow or async arrow parameters.
pub(super) struct CoverList {
    pub(super) elements: Vec<NodeId>,
    pub(super) trailing_comma: bool,
    pub(super) open: Span,
    pub(super) close: Span,
}

impl<'source> Parser<'source> {
    pub(super) fn parse_expression(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_expression_inner(minimum);
        self.leave();
        result
    }

    // AssignmentExpression with the `[In]` parameter set, as inside brackets.
    pub(super) fn parse_assignment_with_in(&mut self) -> Result<NodeId, ParseError> {
        self.with_in(false, |parser| parser.parse_expression(2))
    }

    // Expression with the `[In]` parameter set.
    pub(super) fn parse_expression_with_in(&mut self) -> Result<NodeId, ParseError> {
        self.with_in(false, |parser| parser.parse_expression(0))
    }

    fn parse_expression_inner(&mut self, minimum: u8) -> Result<NodeId, ParseError> {
        // Conditional, assignment, yield, and arrow function expressions are
        // AssignmentExpressions, which binary and unary operands exclude.
        let assignment_level = minimum <= 2;
        let mut left = if assignment_level && self.context.yield_keyword && self.at_word("yield")? {
            self.parse_yield()?
        } else {
            self.parse_unary(assignment_level)?
        };
        loop {
            // Only a comma can continue an AssignmentExpression. Operators are
            // usually absorbed by its last operand, but not by an arrow
            // function with a block body, as in `a = () => {}\n/b/`.
            let complete = matches!(
                self.node_kind(left),
                SyntaxNodeKind::ArrowFunction { .. }
                    | SyntaxNodeKind::Yield { .. }
                    | SyntaxNodeKind::Conditional { .. }
                    | SyntaxNodeKind::Assignment { .. }
            );
            if assignment_level
                && !complete
                && let Some(token) = self.peek_punctuator(self.goal_after_expression())?
            {
                if token.kind == Punctuator::Question {
                    self.check_private_operand(left)?;
                    self.consume_token(token.span)?;
                    let consequent = self.parse_assignment_with_in()?;
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
            if operator.precedence < minimum
                || (complete && operator.operator != BinaryOperator::Comma)
            {
                break;
            }
            if matches!(
                self.node_kind(left),
                SyntaxNodeKind::PrivateIdentifier { .. }
            ) && operator.operator != BinaryOperator::In
            {
                break;
            }
            if operator.operator == BinaryOperator::Exponent
                && matches!(
                    self.node_kind(left),
                    SyntaxNodeKind::Unary { .. } | SyntaxNodeKind::Await { .. }
                )
            {
                return Err(
                    self.error(operator.span.start(), ParseErrorKind::InvalidExponentiation)
                );
            }
            self.consume_token(operator.span)?;
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
        self.check_private_operand(left)?;
        Ok(left)
    }

    // `#name` is only valid as the direct left operand of `in`.
    fn check_private_operand(&self, id: NodeId) -> Result<(), ParseError> {
        if matches!(self.node_kind(id), SyntaxNodeKind::PrivateIdentifier { .. }) {
            return Err(self.error(
                self.node_span(id).start(),
                ParseErrorKind::UnexpectedPrivateName,
            ));
        }
        Ok(())
    }

    fn parse_yield(&mut self) -> Result<NodeId, ParseError> {
        let token = self.scan_identifier()?;
        let start = token.span.start();
        if self.context.in_parameters {
            return Err(self.error(start, ParseErrorKind::YieldInParameters));
        }
        self.cover.yield_await.get_or_insert(start);
        let mut delegate = false;
        let mut argument = None;
        if !self.line_break_before_next()? {
            if self
                .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                .is_some()
            {
                delegate = true;
                argument = Some(self.parse_expression(2)?);
            } else if self.can_start_expression()? {
                argument = Some(self.parse_expression(2)?);
            }
        }
        let end = argument.map_or(token.span.end(), |id| self.node_span(id).end());
        let span = self.source_span(start, end)?;
        self.add_node(SyntaxNodeKind::Yield { argument, delegate }, span)
    }

    // Whether a line terminator separates the last token from the next one.
    pub(super) fn line_break_before_next(&mut self) -> Result<bool, ParseError> {
        self.skip_trivia()?;
        Ok(self.line_break_before)
    }

    // Reports whether the next token can begin an expression, which decides
    // whether `yield` has an operand.
    fn can_start_expression(&mut self) -> Result<bool, ParseError> {
        let goal = self.goal_for_expression_start();
        if let Some(token) = self.peek_punctuator(goal)? {
            return Ok(matches!(
                token.kind,
                Punctuator::LeftParen
                    | Punctuator::LeftBracket
                    | Punctuator::LeftBrace
                    | Punctuator::Plus
                    | Punctuator::Minus
                    | Punctuator::Bang
                    | Punctuator::Tilde
                    | Punctuator::Increment
                    | Punctuator::Decrement
            ));
        }
        let Some(ch) = self.peek_char()? else {
            return Ok(false);
        };
        if self.identifier_can_start_here()? {
            let token = self.peek_identifier()?;
            return Ok(match token.reserved_word {
                Some(word) if !token.has_escape && word.class() == ReservedWordClass::Always => {
                    starts_expression(word)
                }
                Some(_) | None => true,
            });
        }
        Ok(ch.is_ascii_digit() || matches!(ch, '.' | '\'' | '"' | '`' | '/'))
    }

    fn parse_unary(&mut self, arrow_allowed: bool) -> Result<NodeId, ParseError> {
        if let Some(token) = self.peek_name()?
            && !token.has_escape
        {
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
                if operator == UnaryOperator::Delete {
                    self.check_delete_operand(token.span.start(), operand)?;
                }
                let span = self.source_span(token.span.start(), self.node_span(operand).end())?;
                return self.add_node(SyntaxNodeKind::Unary { operator, operand }, span);
            }
            if token.reserved_word == Some(ReservedWord::Await) && self.context.await_keyword {
                return self.parse_await();
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
            SyntaxNodeKind::ArrowFunction { .. } | SyntaxNodeKind::PrivateIdentifier { .. }
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

    fn check_delete_operand(&self, offset: usize, operand: NodeId) -> Result<(), ParseError> {
        let mut current = operand;
        while let SyntaxNodeKind::Parenthesized { expression } = self.node_kind(current) {
            current = expression;
        }
        match self.node_kind(current) {
            // Strict mode code forbids deleting an unqualified name.
            SyntaxNodeKind::Identifier { .. } => self.strict_mode_error(
                offset,
                self.context.strict,
                StrictModeError::DeleteIdentifier,
            ),
            SyntaxNodeKind::Member { property, .. }
                if matches!(
                    self.node_kind(property),
                    SyntaxNodeKind::PrivateIdentifier { .. }
                ) =>
            {
                Err(self.error(offset, ParseErrorKind::DeletePrivateName))
            }
            SyntaxNodeKind::Program { .. }
            | SyntaxNodeKind::Directive { .. }
            | SyntaxNodeKind::ExpressionStatement { .. }
            | SyntaxNodeKind::BlockStatement { .. }
            | SyntaxNodeKind::EmptyStatement
            | SyntaxNodeKind::DebuggerStatement
            | SyntaxNodeKind::IfStatement { .. }
            | SyntaxNodeKind::ForStatement { .. }
            | SyntaxNodeKind::ForInStatement { .. }
            | SyntaxNodeKind::ForOfStatement { .. }
            | SyntaxNodeKind::WhileStatement { .. }
            | SyntaxNodeKind::DoWhileStatement { .. }
            | SyntaxNodeKind::ContinueStatement { .. }
            | SyntaxNodeKind::BreakStatement { .. }
            | SyntaxNodeKind::ReturnStatement { .. }
            | SyntaxNodeKind::WithStatement { .. }
            | SyntaxNodeKind::SwitchStatement { .. }
            | SyntaxNodeKind::SwitchCase { .. }
            | SyntaxNodeKind::LabeledStatement { .. }
            | SyntaxNodeKind::LabelIdentifier { .. }
            | SyntaxNodeKind::ThrowStatement { .. }
            | SyntaxNodeKind::TryStatement { .. }
            | SyntaxNodeKind::CatchClause { .. }
            | SyntaxNodeKind::VariableDeclaration { .. }
            | SyntaxNodeKind::VariableDeclarator { .. }
            | SyntaxNodeKind::FunctionDeclaration { .. }
            | SyntaxNodeKind::FunctionExpression { .. }
            | SyntaxNodeKind::FormalParameters { .. }
            | SyntaxNodeKind::FunctionBody { .. }
            | SyntaxNodeKind::ArrowFunction { .. }
            | SyntaxNodeKind::ClassDeclaration { .. }
            | SyntaxNodeKind::ClassExpression { .. }
            | SyntaxNodeKind::ClassBody { .. }
            | SyntaxNodeKind::MethodDefinition { .. }
            | SyntaxNodeKind::FieldDefinition { .. }
            | SyntaxNodeKind::StaticBlock { .. }
            | SyntaxNodeKind::IdentifierName { .. }
            | SyntaxNodeKind::PrivateIdentifier { .. }
            | SyntaxNodeKind::NumberLiteral { .. }
            | SyntaxNodeKind::StringLiteral
            | SyntaxNodeKind::RegularExpression { .. }
            | SyntaxNodeKind::TemplateLiteral { .. }
            | SyntaxNodeKind::BooleanLiteral(_)
            | SyntaxNodeKind::NullLiteral
            | SyntaxNodeKind::This
            | SyntaxNodeKind::Super
            | SyntaxNodeKind::MetaProperty { .. }
            | SyntaxNodeKind::ImportCall { .. }
            | SyntaxNodeKind::Parenthesized { .. }
            | SyntaxNodeKind::Unary { .. }
            | SyntaxNodeKind::Await { .. }
            | SyntaxNodeKind::Yield { .. }
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
            | SyntaxNodeKind::BindingIdentifier { .. }
            | SyntaxNodeKind::ArrayPattern { .. }
            | SyntaxNodeKind::ObjectPattern { .. }
            | SyntaxNodeKind::PatternProperty { .. }
            | SyntaxNodeKind::ShorthandPatternProperty { .. }
            | SyntaxNodeKind::RestElement { .. }
            | SyntaxNodeKind::AssignmentPattern { .. }
            | SyntaxNodeKind::ImportDeclaration { .. }
            | SyntaxNodeKind::ImportDefaultSpecifier { .. }
            | SyntaxNodeKind::ImportNamespaceSpecifier { .. }
            | SyntaxNodeKind::ImportSpecifier { .. }
            | SyntaxNodeKind::ImportAttributes { .. }
            | SyntaxNodeKind::ImportAttribute { .. }
            | SyntaxNodeKind::ExportDeclaration { .. }
            | SyntaxNodeKind::ExportNamed { .. }
            | SyntaxNodeKind::ExportSpecifier { .. }
            | SyntaxNodeKind::ExportAll { .. }
            | SyntaxNodeKind::ExportDefault { .. } => Ok(()),
        }
    }

    fn parse_await(&mut self) -> Result<NodeId, ParseError> {
        let token = self.scan_identifier()?;
        let start = token.span.start();
        if self.context.in_parameters {
            return Err(self.error(start, ParseErrorKind::AwaitInParameters));
        }
        self.cover.yield_await.get_or_insert(start);
        let argument = self.parse_expression(15)?;
        let span = self.source_span(start, self.node_span(argument).end())?;
        self.add_node(SyntaxNodeKind::Await { argument }, span)
    }

    pub(super) fn parse_primary(&mut self, arrow_allowed: bool) -> Result<NodeId, ParseError> {
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

    pub(super) fn parse_number(&mut self) -> Result<NodeId, ParseError> {
        let literal = scan_js_number(
            self.source,
            self.cursor,
            LegacyOctal::Permit,
            self.limits.lexical,
        )
        .map_err(parse_lexical_error)?;
        if literal.legacy_octal {
            self.strict_mode_error(
                literal.span.start(),
                self.context.strict,
                StrictModeError::LegacyOctalLiteral,
            )?;
        }
        self.consume_token(literal.span)?;
        self.add_node(
            SyntaxNodeKind::NumberLiteral {
                base: literal.base,
                kind: literal.kind,
                legacy_octal: literal.legacy_octal,
            },
            literal.span,
        )
    }

    pub(super) fn parse_string(&mut self) -> Result<(NodeId, StringToken), ParseError> {
        let token = scan_js_string(
            self.source,
            self.cursor,
            LegacyOctal::Permit,
            self.limits.lexical,
        )
        .map_err(parse_lexical_error)?;
        if let Some(offset) = token.legacy_octal_escape {
            self.strict_mode_error(
                offset,
                self.context.strict,
                StrictModeError::LegacyOctalEscape,
            )?;
        }
        self.consume_token(token.span)?;
        Ok((
            self.add_node(SyntaxNodeKind::StringLiteral, token.span)?,
            token,
        ))
    }

    // Handles a consumed identifier name at the start of a primary expression.
    fn parse_identifier_start(
        &mut self,
        token: IdentifierToken,
        arrow_allowed: bool,
    ) -> Result<NodeId, ParseError> {
        let start = token.span.start();
        if token.kind == IdentifierKind::PrivateName {
            return self.parse_private_in(token);
        }
        if !token.has_escape {
            if matches!(
                token.reserved_word,
                Some(
                    ReservedWord::This
                        | ReservedWord::Null
                        | ReservedWord::True
                        | ReservedWord::False
                )
            ) && self.peek_arrow()?.is_some()
            {
                return Err(self.error(start, ParseErrorKind::InvalidArrowParameters));
            }
            match token.reserved_word {
                Some(ReservedWord::New) => return self.parse_new(token),
                Some(ReservedWord::Function) => {
                    return self.parse_function_expression(start, false);
                }
                Some(ReservedWord::Class) => {
                    return self.parse_class(start, super::class::ClassPlacement::Expression);
                }
                Some(ReservedWord::Super) => return self.parse_super(token),
                Some(ReservedWord::Import) => return self.parse_import_expression(token),
                Some(ReservedWord::This) => return self.add_node(SyntaxNodeKind::This, token.span),
                Some(ReservedWord::Null) => {
                    return self.add_node(SyntaxNodeKind::NullLiteral, token.span);
                }
                Some(ReservedWord::True) => {
                    return self.add_node(SyntaxNodeKind::BooleanLiteral(true), token.span);
                }
                Some(ReservedWord::False) => {
                    return self.add_node(SyntaxNodeKind::BooleanLiteral(false), token.span);
                }
                Some(_) | None => {}
            }
            if token.reserved_word.is_none()
                && self.slice(token.span) == "async"
                && let Some(expression) = self.parse_async_start(token, arrow_allowed)?
            {
                return Ok(expression);
            }
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
            self.check_arrow_parameter_name(token)?;
            return self.parse_arrow_from_identifier(start, token, false);
        }
        self.identifier_reference(token)
    }

    fn check_arrow_parameter_name(&mut self, token: IdentifierToken) -> Result<(), ParseError> {
        if let Some(word) = token.reserved_word
            && word.class() == ReservedWordClass::Always
        {
            return Err(self.error(token.span.start(), ParseErrorKind::InvalidArrowParameters));
        }
        self.check_name(token)
    }

    pub(super) fn identifier_reference(
        &mut self,
        token: IdentifierToken,
    ) -> Result<NodeId, ParseError> {
        self.check_reference(token)?;
        self.add_node(
            SyntaxNodeKind::Identifier {
                has_escape: token.has_escape,
                reserved_word: token.reserved_word,
            },
            token.span,
        )
    }

    // After an unescaped `async`: an async function expression, an async
    // arrow function, a call of a function named `async`, or `None` when
    // `async` is an ordinary identifier reference.
    fn parse_async_start(
        &mut self,
        token: IdentifierToken,
        arrow_allowed: bool,
    ) -> Result<Option<NodeId>, ParseError> {
        let start = token.span.start();
        let line_break = self.line_break_before_next()?;
        if !line_break && self.at_word("function")? {
            self.scan_identifier()?;
            return self.parse_function_expression(start, true).map(Some);
        }
        if !line_break
            && let Some(parameter) = self.peek_name()?
            && parameter.kind == IdentifierKind::Name
        {
            // `async x => body`; otherwise `async` is a name and `x` must
            // begin something else, which the caller reports.
            let arrow_follows = self.lookahead(|parser| {
                parser.consume_token(parameter.span)?;
                let arrow = parser.peek_arrow()?;
                Ok(arrow.is_some() && !parser.line_break_before)
            })?;
            if arrow_follows {
                if !arrow_allowed {
                    return Err(self.error(start, ParseErrorKind::UnexpectedArrow));
                }
                self.consume_token(parameter.span)?;
                if self.reserved_word_of(parameter.span) == Some(ReservedWord::Await) {
                    return Err(
                        self.error(parameter.span.start(), ParseErrorKind::AwaitInParameters)
                    );
                }
                self.check_arrow_parameter_name(parameter)?;
                return self
                    .parse_arrow_from_identifier(start, parameter, true)
                    .map(Some);
            }
            return Ok(None);
        }
        if !self.at_punctuator(Punctuator::LeftParen)? {
            return Ok(None);
        }
        // `async (...)` is a call unless `=>` follows, which makes it the
        // head of an async arrow function.
        let saved = std::mem::take(&mut self.cover);
        let list = self.parse_cover_list(Punctuator::LeftParen)?;
        let marks = std::mem::replace(&mut self.cover, saved);
        if let Some(arrow) = self.peek_arrow()? {
            if !arrow_allowed {
                return Err(self.error(arrow.span.start(), ParseErrorKind::UnexpectedArrow));
            }
            if line_break || self.line_break_before {
                return Err(self.error(
                    arrow.span.start(),
                    ParseErrorKind::LineTerminatorBeforeArrow,
                ));
            }
            self.check_async_arrow_marks(marks)?;
            return self.parse_arrow_from_cover(start, list, true).map(Some);
        }
        self.cover = self.cover.merge(marks);
        // The callee follows its arguments in the arena; only spans are ordered.
        let callee = self.identifier_reference(token)?;
        let arguments = self.finish_list(list.elements)?;
        let span = self.source_span(start, list.close.end())?;
        let call = self.add_node(
            SyntaxNodeKind::Call {
                callee,
                arguments,
                optional: false,
            },
            span,
        )?;
        Ok(Some(call))
    }

    fn check_async_arrow_marks(&self, marks: CoverMarks) -> Result<(), ParseError> {
        if let Some(offset) = marks.yield_await {
            let kind = if self.text[offset..].starts_with("yield") {
                ParseErrorKind::YieldInParameters
            } else {
                ParseErrorKind::AwaitInParameters
            };
            return Err(self.error(offset, kind));
        }
        if let Some(offset) = marks.await_identifier {
            return Err(self.error(offset, ParseErrorKind::AwaitInParameters));
        }
        Ok(())
    }

    // `#name in object`. The private name is recorded for class validation.
    fn parse_private_in(&mut self, token: IdentifierToken) -> Result<NodeId, ParseError> {
        if self.no_in || !self.at_word("in")? {
            return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
        }
        self.use_private_name(token)?;
        self.add_node(
            SyntaxNodeKind::PrivateIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )
    }

    fn parse_new(&mut self, keyword: IdentifierToken) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_new_inner(keyword);
        self.leave();
        result
    }

    fn parse_new_inner(&mut self, keyword: IdentifierToken) -> Result<NodeId, ParseError> {
        let start = keyword.span.start();
        if self
            .consume_punctuator(Punctuator::Dot, self.goal_for_expression_start())?
            .is_some()
        {
            let property = self.scan_meta_property_name()?;
            if !self.is_unescaped(property, "target") {
                return Err(self.error(property.span.start(), ParseErrorKind::InvalidMetaProperty));
            }
            match self.context.new_target {
                Permission::Allowed => {}
                Permission::Denied => {
                    return Err(self.error(start, ParseErrorKind::InvalidNewTarget));
                }
                Permission::Unknown => {
                    return Err(self.error(start, ParseErrorKind::MetaPropertyRequiresContext));
                }
            }
            let span = self.source_span(start, property.span.end())?;
            return self.add_node(
                SyntaxNodeKind::MetaProperty {
                    kind: MetaPropertyKind::NewTarget,
                },
                span,
            );
        }
        // A nested `new` consumes the first argument list, as the grammar's
        // `new MemberExpression Arguments` production requires.
        let callee = self.parse_primary(false)?;
        let callee_kind = self.node_kind(callee);
        if matches!(callee_kind, SyntaxNodeKind::ImportCall { .. }) {
            return Err(self.error(
                self.node_span(callee).start(),
                ParseErrorKind::ExpectedExpression,
            ));
        }
        if callee_kind == SyntaxNodeKind::Super && self.at_punctuator(Punctuator::LeftParen)? {
            return Err(self.error(self.node_span(callee).start(), ParseErrorKind::InvalidSuper));
        }
        let callee = self.parse_postfix(callee, PostfixMode::NewCallee)?;
        if self.at_punctuator(Punctuator::LeftParen)? {
            let list = self.parse_cover_list(Punctuator::LeftParen)?;
            let arguments = self.finish_list(list.elements)?;
            let span = self.source_span(start, list.close.end())?;
            return self.add_node(
                SyntaxNodeKind::New {
                    callee,
                    arguments: Some(arguments),
                },
                span,
            );
        }
        let span = self.source_span(start, self.node_span(callee).end())?;
        self.add_node(
            SyntaxNodeKind::New {
                callee,
                arguments: None,
            },
            span,
        )
    }

    fn scan_meta_property_name(&mut self) -> Result<IdentifierToken, ParseError> {
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::InvalidMetaProperty));
        }
        self.scan_identifier()
    }

    // `super` must be followed by `.`, `[`, or call arguments.
    fn parse_super(&mut self, token: IdentifierToken) -> Result<NodeId, ParseError> {
        let start = token.span.start();
        let next = self
            .peek_punctuator(self.goal_after_expression())?
            .map(|token| token.kind);
        let permission = match next {
            Some(Punctuator::LeftParen) => self.context.super_call,
            Some(Punctuator::Dot | Punctuator::LeftBracket) => self.context.super_property,
            _ => Permission::Denied,
        };
        match permission {
            Permission::Allowed => {}
            Permission::Denied => return Err(self.error(start, ParseErrorKind::InvalidSuper)),
            Permission::Unknown => {
                return Err(self.error(start, ParseErrorKind::SuperRequiresContext));
            }
        }
        if next == Some(Punctuator::Dot) {
            let after_dot = self.lookahead(|parser| {
                parser.consume_punctuator(Punctuator::Dot, parser.goal_after_expression())?;
                parser.at_char('#')
            })?;
            if after_dot {
                return Err(self.error(start, ParseErrorKind::InvalidSuper));
            }
        }
        self.add_node(SyntaxNodeKind::Super, token.span)
    }

    // `import(...)` or `import.meta` in expression position.
    fn parse_import_expression(&mut self, token: IdentifierToken) -> Result<NodeId, ParseError> {
        let start = token.span.start();
        if self
            .consume_punctuator(Punctuator::Dot, self.goal_after_expression())?
            .is_some()
        {
            let property = self.scan_meta_property_name()?;
            if !self.is_unescaped(property, "meta") {
                return Err(self.error(property.span.start(), ParseErrorKind::InvalidMetaProperty));
            }
            match self.goal {
                Goal::Module => {}
                Goal::Script => {
                    return Err(self.error(start, ParseErrorKind::ImportMetaOutsideModule));
                }
                Goal::Unknown => {
                    return Err(self.error(start, ParseErrorKind::SourceGoalRequiresContext));
                }
            }
            let span = self.source_span(start, property.span.end())?;
            return self.add_node(
                SyntaxNodeKind::MetaProperty {
                    kind: MetaPropertyKind::ImportMeta,
                },
                span,
            );
        }
        if !self.at_punctuator(Punctuator::LeftParen)? {
            return Err(self.error(start, ParseErrorKind::ExpectedExpression));
        }
        let (source, options, close) = self.with_in(false, |parser| {
            parser.expect_punctuator(Punctuator::LeftParen, parser.goal_after_expression())?;
            let source = parser.parse_expression(2)?;
            let mut options = None;
            if parser
                .consume_punctuator(Punctuator::Comma, parser.goal_after_expression())?
                .is_some()
                && !parser.at_punctuator(Punctuator::RightParen)?
            {
                options = Some(parser.parse_expression(2)?);
                parser.consume_punctuator(Punctuator::Comma, parser.goal_after_expression())?;
            }
            let close =
                parser.expect_punctuator(Punctuator::RightParen, parser.goal_after_expression())?;
            Ok((source, options, close))
        })?;
        let span = self.source_span(start, close.span.end())?;
        self.add_node(SyntaxNodeKind::ImportCall { source, options }, span)
    }

    pub(super) fn parse_postfix(
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
                    let property = self.parse_member_name()?;
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
                        let property = self.parse_member_name()?;
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
                // Valid arrow functions are built before postfix parsing, so
                // an arrow here follows an expression that cannot be parameters.
                Punctuator::Arrow => {
                    return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedArrow));
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
                | Punctuator::Slash
                | Punctuator::SlashAssign
                | Punctuator::RightBrace => break,
            }
        }
        Ok(expression)
    }

    // A member property after `.` or `?.`: an identifier name or a private name.
    fn parse_member_name(&mut self) -> Result<NodeId, ParseError> {
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        }
        let token = self.scan_identifier()?;
        if token.kind == IdentifierKind::PrivateName {
            self.use_private_name(token)?;
            return self.add_node(
                SyntaxNodeKind::PrivateIdentifier {
                    has_escape: token.has_escape,
                },
                token.span,
            );
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
        let property = self.parse_expression_with_in()?;
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
        let list = self.parse_cover_list(Punctuator::LeftParen)?;
        let arguments = self.finish_list(list.elements)?;
        let span = self.source_span(self.node_span(callee).start(), list.close.end())?;
        self.add_node(
            SyntaxNodeKind::Call {
                callee,
                arguments,
                optional,
            },
            span,
        )
    }

    // Parses a parenthesized list of AssignmentExpressions and spread
    // elements, allowing a trailing comma. It covers call arguments,
    // parenthesized expressions, and arrow parameters.
    pub(super) fn parse_cover_list(&mut self, open: Punctuator) -> Result<CoverList, ParseError> {
        self.with_in(false, |parser| {
            let open_token = parser.expect_punctuator(open, parser.goal_for_expression_start())?;
            let mut elements = parser.take_list();
            let mut trailing_comma = false;
            loop {
                if parser.at_char(')')? {
                    break;
                }
                let element = parser.parse_element()?;
                parser.push_child(&mut elements, element)?;
                trailing_comma = false;
                if parser
                    .consume_punctuator(Punctuator::Comma, parser.goal_after_expression())?
                    .is_none()
                {
                    break;
                }
                trailing_comma = true;
            }
            let close =
                parser.expect_punctuator(Punctuator::RightParen, parser.goal_after_expression())?;
            Ok(CoverList {
                elements,
                trailing_comma,
                open: open_token.span,
                close: close.span,
            })
        })
    }

    // Parses an AssignmentExpression or a spread element `...expression`.
    pub(super) fn parse_element(&mut self) -> Result<NodeId, ParseError> {
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
        let saved = std::mem::take(&mut self.cover);
        let list = self.parse_cover_list(Punctuator::LeftParen)?;
        let marks = std::mem::replace(&mut self.cover, saved);
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
            if let Some(offset) = marks.yield_await {
                let kind = if self.text[offset..].starts_with("yield") {
                    ParseErrorKind::YieldInParameters
                } else {
                    ParseErrorKind::AwaitInParameters
                };
                return Err(self.error(offset, kind));
            }
            // An enclosing async arrow head also rejects `await` names in
            // these parameters.
            self.cover = self.cover.merge(CoverMarks {
                yield_await: None,
                await_identifier: marks.await_identifier,
            });
            return self.parse_arrow_from_cover(list.open.start(), list, false);
        }
        self.cover = self.cover.merge(marks);
        let Some((&first, rest)) = list.elements.split_first() else {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedArrow));
        };
        let has_rest = list
            .elements
            .iter()
            .any(|&element| matches!(self.node_kind(element), SyntaxNodeKind::Spread { .. }));
        if has_rest || list.trailing_comma {
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
        let span = self.source_span(list.open.start(), list.close.end())?;
        self.recycle_list(list.elements);
        self.add_node(SyntaxNodeKind::Parenthesized { expression }, span)
    }

    pub(super) fn peek_arrow(&mut self) -> Result<Option<PunctuatorToken>, ParseError> {
        Ok(self
            .peek_punctuator(self.goal_after_expression())?
            .filter(|token| token.kind == Punctuator::Arrow))
    }

    fn parse_array(&mut self) -> Result<NodeId, ParseError> {
        self.with_in(false, Self::parse_array_literal)
    }

    fn parse_array_literal(&mut self) -> Result<NodeId, ParseError> {
        let open =
            self.expect_punctuator(Punctuator::LeftBracket, self.goal_for_expression_start())?;
        let mut elements = self.take_list();
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
        let elements = self.finish_list(elements)?;
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
        self.within_braces(Self::parse_object_literal)
    }

    fn parse_object_literal(&mut self) -> Result<NodeId, ParseError> {
        let open =
            self.expect_punctuator(Punctuator::LeftBrace, self.goal_for_expression_start())?;
        let mut properties = self.take_list();
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
        let properties = self.finish_list(properties)?;
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
        if self
            .peek_punctuator(self.goal_for_expression_start())?
            .is_some_and(|token| token.kind == Punctuator::Ellipsis)
        {
            return self.parse_element();
        }
        if self
            .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
            .is_some()
        {
            let key = self.parse_property_key(false)?;
            return self.parse_method(start, key, MethodKind::Generator, false, None);
        }
        if let Some(token) = self.peek_name()?
            && token.kind == IdentifierKind::Name
            && !token.has_escape
            && let Some(modifier) = self.method_modifier(token)?
        {
            self.consume_token(token.span)?;
            let mut kind = modifier;
            if modifier == MethodKind::Async
                && self
                    .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                    .is_some()
            {
                kind = MethodKind::AsyncGenerator;
            }
            let key = self.parse_property_key(false)?;
            return self.parse_method(start, key, kind, false, None);
        }
        let Some(ch) = self.peek_char()? else {
            return Err(self.error(start, ParseErrorKind::ExpectedClosingBrace));
        };
        if ch == '[' || ch == '\'' || ch == '"' || self.number_starts_here(ch) {
            let key = self.parse_property_key(false)?;
            let is_proto = matches!(self.node_kind(key.node), SyntaxNodeKind::StringLiteral)
                && string_literal_equals(self.slice(self.node_span(key.node)), "__proto__");
            return self.finish_keyed_property(start, key, is_proto, state);
        }
        if self.identifier_can_start_here()? {
            let token = self.scan_identifier()?;
            if token.kind == IdentifierKind::PrivateName {
                return Err(self.error(token.span.start(), self.private_key_error()));
            }
            return self.finish_identifier_property(start, token, state);
        }
        Err(self.error(start, ParseErrorKind::InvalidPropertyDefinition))
    }

    pub(super) fn private_key_error(&self) -> ParseErrorKind {
        if self.goal == Goal::Unknown {
            ParseErrorKind::PrivateIdentifierRequiresContext
        } else {
            ParseErrorKind::UnexpectedPrivateName
        }
    }

    // `get`, `set`, and `async` are method modifiers when a property name
    // follows; `async` additionally requires the name on the same line.
    pub(super) fn method_modifier(
        &mut self,
        token: IdentifierToken,
    ) -> Result<Option<MethodKind>, ParseError> {
        let kind = match self.slice(token.span) {
            "get" => MethodKind::Getter,
            "set" => MethodKind::Setter,
            "async" => MethodKind::Async,
            _ => return Ok(None),
        };
        let starts_key = self.lookahead(|parser| {
            parser.consume_token(token.span)?;
            let line_break = parser.line_break_before_next()?;
            if kind == MethodKind::Async && line_break {
                return Ok(false);
            }
            if let Some(next) = parser.peek_punctuator(parser.goal_for_expression_start())? {
                return Ok(next.kind == Punctuator::LeftBracket
                    || (next.kind == Punctuator::Star && kind == MethodKind::Async));
            }
            let Some(ch) = parser.peek_char()? else {
                return Ok(false);
            };
            Ok(ch.is_ascii_digit()
                || matches!(ch, '.' | '\'' | '"')
                || parser.identifier_can_start_here()?)
        })?;
        Ok(starts_key.then_some(kind))
    }

    // Parses a property name: an identifier name, string, number, computed
    // key, or with `private_allowed` a private name.
    pub(super) fn parse_property_key(
        &mut self,
        private_allowed: bool,
    ) -> Result<PropertyKey, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if self
            .consume_punctuator(Punctuator::LeftBracket, self.goal_for_expression_start())?
            .is_some()
        {
            let node = self.parse_assignment_with_in()?;
            self.expect_punctuator(Punctuator::RightBracket, self.goal_after_expression())?;
            return Ok(PropertyKey {
                node,
                computed: true,
            });
        }
        let Some(ch) = self.peek_char()? else {
            return Err(self.error(start, ParseErrorKind::InvalidPropertyDefinition));
        };
        let node = if matches!(ch, '\'' | '"') {
            self.parse_string()?.0
        } else if self.number_starts_here(ch) {
            self.parse_number()?
        } else if self.identifier_can_start_here()? {
            let token = self.scan_identifier()?;
            if token.kind == IdentifierKind::PrivateName {
                if !private_allowed {
                    return Err(self.error(token.span.start(), self.private_key_error()));
                }
                self.add_node(
                    SyntaxNodeKind::PrivateIdentifier {
                        has_escape: token.has_escape,
                    },
                    token.span,
                )?
            } else {
                self.add_node(
                    SyntaxNodeKind::IdentifierName {
                        has_escape: token.has_escape,
                        reserved_word: token.reserved_word,
                    },
                    token.span,
                )?
            }
        } else {
            return Err(self.error(start, ParseErrorKind::InvalidPropertyDefinition));
        };
        Ok(PropertyKey {
            node,
            computed: false,
        })
    }

    fn finish_keyed_property(
        &mut self,
        start: usize,
        key: PropertyKey,
        is_proto: bool,
        state: &mut ObjectState,
    ) -> Result<NodeId, ParseError> {
        let Some(token) = self.peek_punctuator(self.goal_after_expression())? else {
            return Err(self.error(self.cursor, ParseErrorKind::InvalidPropertyDefinition));
        };
        if token.kind == Punctuator::LeftParen {
            return self.parse_method(start, key, MethodKind::Method, false, None);
        }
        if token.kind != Punctuator::Colon {
            return Err(self.error(
                token.span.start(),
                ParseErrorKind::InvalidPropertyDefinition,
            ));
        }
        self.consume_token(token.span)?;
        let value = self.parse_expression(2)?;
        if is_proto && !key.computed {
            if state.proto_seen {
                state.record(self.error(
                    self.node_span(key.node).start(),
                    ParseErrorKind::DuplicateProto,
                ));
            }
            state.proto_seen = true;
        }
        let span = self.source_span(start, self.node_span(value).end())?;
        self.add_node(
            SyntaxNodeKind::Property {
                key: key.node,
                value,
                computed: key.computed,
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
            let node = self.add_node(
                SyntaxNodeKind::IdentifierName {
                    has_escape: token.has_escape,
                    reserved_word: token.reserved_word,
                },
                token.span,
            )?;
            let is_proto = identifier_equals(self.slice(token.span), "__proto__");
            let key = PropertyKey {
                node,
                computed: false,
            };
            return self.finish_keyed_property(start, key, is_proto, state);
        }
        if let Some(next) = next
            && matches!(
                next.kind,
                Punctuator::Comma | Punctuator::RightBrace | Punctuator::Assign
            )
        {
            if token
                .reserved_word
                .is_some_and(|word| word.class() == ReservedWordClass::Always)
            {
                return Err(self.error(
                    token.span.start(),
                    ParseErrorKind::InvalidPropertyDefinition,
                ));
            }
            let identifier = self.identifier_reference(token)?;
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
        Err(self.error(self.cursor, ParseErrorKind::InvalidPropertyDefinition))
    }

    pub(super) fn parse_template(&mut self, mode: TemplateMode) -> Result<NodeId, ParseError> {
        let first = self.scan_template(TemplatePosition::Start, mode)?;
        let start = first.span.start();
        let mut end = first.span.end();
        let mut parts = Vec::new();
        self.push_template_part(&mut parts, TemplatePart::Segment(first))?;
        let mut current = first;
        while matches!(current.kind, TemplateKind::Head | TemplateKind::Middle) {
            self.template_expression_depth += 1;
            let saved_in = std::mem::replace(&mut self.no_in, false);
            let expression_result = self.parse_expression(0);
            self.no_in = saved_in;
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
        let Some(token) = self.peek_name()? else {
            return Ok(None);
        };
        if token.has_escape {
            return Ok(None);
        }
        let (operator, precedence) = match token.reserved_word {
            Some(ReservedWord::In) if !self.no_in => (BinaryOperator::In, 10),
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

    // Function expressions after a consumed `function`, or `async function`.
    fn parse_function_expression(
        &mut self,
        start: usize,
        is_async: bool,
    ) -> Result<NodeId, ParseError> {
        let generator = self
            .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
            .is_some();
        let kind = match (is_async, generator) {
            (false, false) => FunctionKind::Normal,
            (false, true) => FunctionKind::Generator,
            (true, false) => FunctionKind::Async,
            (true, true) => FunctionKind::AsyncGenerator,
        };
        self.parse_function(start, kind, super::function::FunctionPlacement::Expression)
    }
}

/// A parsed property key.
#[derive(Clone, Copy)]
pub(super) struct PropertyKey {
    pub(super) node: NodeId,
    pub(super) computed: bool,
}

// Words reserved in every context that can nevertheless begin an expression.
fn starts_expression(word: ReservedWord) -> bool {
    matches!(
        word,
        ReservedWord::This
            | ReservedWord::Null
            | ReservedWord::True
            | ReservedWord::False
            | ReservedWord::Function
            | ReservedWord::Class
            | ReservedWord::New
            | ReservedWord::Typeof
            | ReservedWord::Void
            | ReservedWord::Delete
            | ReservedWord::Super
            | ReservedWord::Import
    )
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
    if kind == Punctuator::Bang {
        Some(UnaryOperator::LogicalNot)
    } else if kind == Punctuator::Tilde {
        Some(UnaryOperator::BitwiseNot)
    } else if kind == Punctuator::Plus {
        Some(UnaryOperator::Plus)
    } else if kind == Punctuator::Minus {
        Some(UnaryOperator::Minus)
    } else {
        None
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
