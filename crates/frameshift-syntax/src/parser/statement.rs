//! Source files, statement lists, statements, and variable declarations.

use super::{
    Label, ParseError, ParseErrorKind, Parser, StrictModeError, Strictness,
    class::ClassPlacement,
    function::FunctionPlacement,
    pattern::TargetMode,
    scope::{Declaration, ScopeKind},
};
use crate::{
    ast::{FunctionKind, NodeId, SyntaxNodeKind, VariableKind},
    identifier::{IdentifierKind, IdentifierToken, ReservedWord},
    lexical::{LegacyOctal, SourceGoal, scan_js_string},
    punctuator::Punctuator,
};

/// The statement list being parsed, which decides where it ends and which
/// declarations it permits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListKind {
    Script,
    Module,
    FunctionBody,
    Block,
    SwitchCase,
    StaticBlock,
}

// Where a single statement appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    // An item of a statement list, which may be a declaration.
    List(ListKind),
    // The body of a loop or `with`, or a labelled item inside one.
    Body,
    // A clause of an `if` statement, where sloppy code permits a plain
    // function declaration (Annex B).
    IfClause,
    // A labelled item whose label chain starts in a statement list, where
    // sloppy code permits a plain function declaration (Annex B).
    LabelledInList,
}

impl<'source> Parser<'source> {
    pub(super) fn parse_program(&mut self, goal: SourceGoal) -> Result<NodeId, ParseError> {
        self.skip_hashbang()?;
        let (scope, list) = match goal {
            SourceGoal::Script => (ScopeKind::Function, ListKind::Script),
            SourceGoal::Module => (ScopeKind::Module, ListKind::Module),
        };
        self.push_scope(scope)?;
        let mut items = self.take_list();
        let result = self.parse_program_items(list, &mut items);
        let module_result = if result.is_ok() && goal == SourceGoal::Module {
            self.check_exported_bindings()
        } else {
            Ok(())
        };
        self.pop_scope();
        result?;
        module_result?;
        let body = self.finish_list(items)?;
        let span = self.source_span(0, self.text.len())?;
        self.cursor = self.text.len();
        self.add_node(
            SyntaxNodeKind::Program {
                goal,
                body,
                strict: self.context.strict == Strictness::Strict,
            },
            span,
        )
    }

    fn parse_program_items(
        &mut self,
        list: ListKind,
        items: &mut Vec<NodeId>,
    ) -> Result<(), ParseError> {
        self.parse_directive_prologue(items)?;
        self.parse_statement_list(list, items)?;
        if !self.at_end()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedExpression));
        }
        Ok(())
    }

    // Parses the directives at the start of a script, module, or function
    // body, applying `"use strict"`, and returns the offset of the first
    // such directive. A string statement that is not a directive ends the
    // prologue and is kept as an ordinary expression statement.
    pub(super) fn parse_directive_prologue(
        &mut self,
        items: &mut Vec<NodeId>,
    ) -> Result<Option<usize>, ParseError> {
        let mut use_strict = None;
        let mut first_legacy_escape = None;
        while matches!(self.peek_char()?, Some('"' | '\'')) {
            let start = self.cursor;
            let expression = self.parse_expression_with_in()?;
            self.consume_semicolon()?;
            let span = self.span_from(start)?;
            if self.node_kind(expression) != SyntaxNodeKind::StringLiteral {
                let statement =
                    self.add_node(SyntaxNodeKind::ExpressionStatement { expression }, span)?;
                self.push_child(items, statement)?;
                break;
            }
            let literal = self.node_span(expression);
            let spelling = self.slice(literal);
            if spelling == "\"use strict\"" || spelling == "'use strict'" {
                use_strict.get_or_insert(start);
                if self.context.strict != Strictness::Strict {
                    // Earlier directives become strict mode code retroactively.
                    if let Some(offset) = first_legacy_escape {
                        return Err(self.error(
                            offset,
                            ParseErrorKind::StrictMode(StrictModeError::LegacyOctalEscape),
                        ));
                    }
                    self.context.strict = Strictness::Strict;
                }
            } else if first_legacy_escape.is_none() {
                first_legacy_escape = scan_js_string(
                    self.source,
                    literal.start(),
                    LegacyOctal::Permit,
                    self.limits.lexical,
                )
                .ok()
                .and_then(|token| token.legacy_octal_escape);
            }
            let directive = self.add_node(SyntaxNodeKind::Directive { expression }, span)?;
            self.push_child(items, directive)?;
        }
        Ok(use_strict)
    }

    pub(super) fn parse_statement_list(
        &mut self,
        kind: ListKind,
        items: &mut Vec<NodeId>,
    ) -> Result<(), ParseError> {
        loop {
            match kind {
                ListKind::Script | ListKind::Module => {
                    if self.at_end()? {
                        break;
                    }
                }
                ListKind::FunctionBody | ListKind::Block | ListKind::StaticBlock => {
                    if self.at_char('}')? {
                        break;
                    }
                    if self.at_end()? {
                        return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBrace));
                    }
                }
                ListKind::SwitchCase => {
                    if self.at_char('}')? || self.at_word("case")? || self.at_word("default")? {
                        break;
                    }
                    if self.at_end()? {
                        return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBrace));
                    }
                }
            }
            let item = self.parse_statement(Position::List(kind))?;
            self.push_child(items, item)?;
        }
        Ok(())
    }

    // A declaration after `export`; the caller checks the resulting kind.
    pub(super) fn parse_module_item_declaration(&mut self) -> Result<NodeId, ParseError> {
        self.parse_statement(Position::List(ListKind::Module))
    }

    fn parse_statement(&mut self, position: Position) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_statement_inner(position);
        self.leave();
        result
    }

    fn parse_statement_inner(&mut self, position: Position) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let Some(ch) = self.peek_char()? else {
            return Err(self.error(start, ParseErrorKind::ExpectedExpression));
        };
        if ch == '{' {
            return self.parse_block_statement();
        }
        if ch == ';' {
            self.expect_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
            return self.add_node(SyntaxNodeKind::EmptyStatement, self.span_from(start)?);
        }
        if let Some(token) = self.peek_name()?
            && token.kind == IdentifierKind::Name
            && !token.has_escape
            && let Some(statement) = self.parse_keyword_statement(position, start, token)?
        {
            return Ok(statement);
        }
        self.parse_expression_or_labelled_statement(position, start)
    }

    // Dispatches statements and declarations introduced by an unescaped
    // keyword or contextual keyword, or returns `None` for an expression.
    fn parse_keyword_statement(
        &mut self,
        position: Position,
        start: usize,
        token: IdentifierToken,
    ) -> Result<Option<NodeId>, ParseError> {
        let in_list = matches!(position, Position::List(_));
        let statement = match token.reserved_word {
            Some(ReservedWord::Var) => self.parse_variable_statement(start, VariableKind::Var)?,
            Some(ReservedWord::If) => self.parse_if(start)?,
            Some(ReservedWord::For) => self.parse_for(start)?,
            Some(ReservedWord::While) => self.parse_while(start)?,
            Some(ReservedWord::Do) => self.parse_do_while(start)?,
            Some(ReservedWord::Continue) => self.parse_continue(start)?,
            Some(ReservedWord::Break) => self.parse_break(start)?,
            Some(ReservedWord::Return) => self.parse_return(start)?,
            Some(ReservedWord::With) => self.parse_with(start)?,
            Some(ReservedWord::Switch) => self.parse_switch(start)?,
            Some(ReservedWord::Throw) => self.parse_throw(start)?,
            Some(ReservedWord::Try) => self.parse_try(start)?,
            Some(ReservedWord::Debugger) => {
                self.consume_token(token.span)?;
                self.consume_semicolon()?;
                self.add_node(SyntaxNodeKind::DebuggerStatement, self.span_from(start)?)?
            }
            Some(ReservedWord::Function) => {
                self.consume_token(token.span)?;
                let generator = self
                    .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                    .is_some();
                let kind = if generator {
                    FunctionKind::Generator
                } else {
                    FunctionKind::Normal
                };
                self.parse_function_statement(position, start, kind)?
            }
            Some(ReservedWord::Class) => {
                if !in_list {
                    return Err(self.error(start, ParseErrorKind::DeclarationInStatementPosition));
                }
                self.consume_token(token.span)?;
                self.parse_class(start, ClassPlacement::Declaration)?
            }
            Some(ReservedWord::Const) => {
                if !in_list {
                    return Err(self.error(start, ParseErrorKind::DeclarationInStatementPosition));
                }
                self.parse_variable_statement(start, VariableKind::Const)?
            }
            Some(ReservedWord::Import) => {
                let expression = self.lookahead(|parser| {
                    parser.consume_token(token.span)?;
                    Ok(parser
                        .peek_punctuator(parser.goal_after_expression())?
                        .is_some_and(|next| {
                            matches!(next.kind, Punctuator::LeftParen | Punctuator::Dot)
                        }))
                })?;
                if expression {
                    return Ok(None);
                }
                if position != Position::List(ListKind::Module) {
                    return Err(self.error(start, ParseErrorKind::ModuleDeclarationOutsideModule));
                }
                self.parse_import_declaration(start)?
            }
            Some(ReservedWord::Export) => {
                if position != Position::List(ListKind::Module) {
                    return Err(self.error(start, ParseErrorKind::ModuleDeclarationOutsideModule));
                }
                self.parse_export_declaration(start)?
            }
            Some(ReservedWord::Let) => {
                let next = self.lookahead(|parser| {
                    parser.consume_token(token.span)?;
                    parser.binding_start()
                })?;
                match (in_list, next) {
                    (true, BindingStart::Bracket | BindingStart::Brace | BindingStart::Name) => {
                        self.parse_variable_statement(start, VariableKind::Let)?
                    }
                    // ExpressionStatement excludes a leading `let [`.
                    (false, BindingStart::Bracket) => {
                        return Err(
                            self.error(start, ParseErrorKind::DeclarationInStatementPosition)
                        );
                    }
                    (true, BindingStart::Other)
                    | (false, BindingStart::Brace | BindingStart::Name | BindingStart::Other) => {
                        return Ok(None);
                    }
                }
            }
            Some(ReservedWord::Await) => {
                if !(in_list && self.context.await_keyword && self.at_await_using(token)?) {
                    return Ok(None);
                }
                self.check_using_position(start, position)?;
                self.parse_variable_statement(start, VariableKind::AwaitUsing)?
            }
            Some(_) => return Ok(None),
            None => match self.slice(token.span) {
                "async" => {
                    let function = self.lookahead(|parser| {
                        parser.consume_token(token.span)?;
                        Ok(!parser.line_break_before_next()? && parser.at_word("function")?)
                    })?;
                    if !function {
                        return Ok(None);
                    }
                    if !in_list {
                        return Err(
                            self.error(start, ParseErrorKind::DeclarationInStatementPosition)
                        );
                    }
                    self.consume_token(token.span)?;
                    self.scan_identifier()?;
                    let generator = self
                        .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                        .is_some();
                    let kind = if generator {
                        FunctionKind::AsyncGenerator
                    } else {
                        FunctionKind::Async
                    };
                    self.parse_function(start, kind, FunctionPlacement::Declaration)?
                }
                "using" => {
                    if !(in_list && self.at_using(token, false)?) {
                        return Ok(None);
                    }
                    self.check_using_position(start, position)?;
                    self.parse_variable_statement(start, VariableKind::Using)?
                }
                _ => return Ok(None),
            },
        };
        Ok(Some(statement))
    }

    // Reports whether `using` at `token` starts a using declaration: a binding
    // identifier follows on the same line. In a `for` head, `using of` does
    // not, unless `=` follows as in a classic loop declaration.
    fn at_using(&mut self, token: IdentifierToken, for_head: bool) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            parser.consume_token(token.span)?;
            parser.using_binding_follows(for_head)
        })
    }

    fn using_binding_follows(&mut self, for_head: bool) -> Result<bool, ParseError> {
        if self.line_break_before_next()? {
            return Ok(false);
        }
        let Some(next) = self.peek_name()? else {
            return Ok(false);
        };
        if !Self::can_start_binding(next) {
            return Ok(false);
        }
        if for_head && self.is_unescaped(next, "of") {
            self.consume_token(next.span)?;
            return self.at_punctuator(Punctuator::Assign);
        }
        Ok(true)
    }

    // `await using` followed on the same line by a binding identifier.
    fn at_await_using(&mut self, token: IdentifierToken) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            parser.consume_token(token.span)?;
            if parser.line_break_before_next()? {
                return Ok(false);
            }
            let Some(using) = parser.consume_word("using")? else {
                return Ok(false);
            };
            let _ = using;
            parser.using_binding_follows(false)
        })
    }

    // `using` and `await using` cannot appear directly at the top level of
    // a script or in a `switch` clause.
    fn check_using_position(&self, start: usize, position: Position) -> Result<(), ParseError> {
        if matches!(
            position,
            Position::List(ListKind::Script | ListKind::SwitchCase)
        ) {
            return Err(self.error(start, ParseErrorKind::InvalidUsingDeclaration));
        }
        Ok(())
    }

    // Classifies the token after `let`.
    fn binding_start(&mut self) -> Result<BindingStart, ParseError> {
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())? {
            return Ok(if token.kind == Punctuator::LeftBracket {
                BindingStart::Bracket
            } else if token.kind == Punctuator::LeftBrace {
                BindingStart::Brace
            } else {
                BindingStart::Other
            });
        }
        Ok(match self.peek_name()? {
            Some(token) if Self::can_start_binding(token) => BindingStart::Name,
            Some(_) | None => BindingStart::Other,
        })
    }

    // A function declaration after `function` and `*`. Sloppy code permits a
    // plain function as an `if` clause or labelled item (Annex B).
    fn parse_function_statement(
        &mut self,
        position: Position,
        start: usize,
        kind: FunctionKind,
    ) -> Result<NodeId, ParseError> {
        match position {
            Position::List(_) => self.parse_function(start, kind, FunctionPlacement::Declaration),
            Position::IfClause | Position::LabelledInList if kind == FunctionKind::Normal => {
                match self.context.strict {
                    Strictness::Sloppy => {}
                    Strictness::Strict => {
                        return Err(
                            self.error(start, ParseErrorKind::DeclarationInStatementPosition)
                        );
                    }
                    Strictness::Unknown => {
                        return Err(self.error(start, ParseErrorKind::StrictModeRequiresContext));
                    }
                }
                // An `if` clause function behaves as if wrapped in a block.
                let wrapped = position == Position::IfClause;
                if wrapped {
                    self.push_scope(ScopeKind::Block)?;
                }
                let result = self.parse_function(start, kind, FunctionPlacement::Declaration);
                if wrapped {
                    self.pop_scope();
                }
                result
            }
            Position::IfClause | Position::LabelledInList | Position::Body => {
                Err(self.error(start, ParseErrorKind::DeclarationInStatementPosition))
            }
        }
    }

    fn parse_expression_or_labelled_statement(
        &mut self,
        position: Position,
        start: usize,
    ) -> Result<NodeId, ParseError> {
        if let Some(token) = self.peek_name()?
            && token.kind == IdentifierKind::Name
        {
            let is_label = self.lookahead(|parser| {
                parser.consume_token(token.span)?;
                parser.at_punctuator(Punctuator::Colon)
            })?;
            if is_label {
                return self.parse_labelled_statement(position, start, token);
            }
        }
        let expression = self.parse_expression_with_in()?;
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ExpressionStatement { expression }, span)
    }

    fn parse_labelled_statement(
        &mut self,
        position: Position,
        start: usize,
        token: IdentifierToken,
    ) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        self.check_name(token)?;
        self.expect_punctuator(Punctuator::Colon, self.goal_after_expression())?;
        let name = self.name_of(token.span);
        if self.jumps.labels.iter().any(|label| label.name == name) {
            return Err(self.error(start, ParseErrorKind::DuplicateLabel));
        }
        self.skip_trivia()?;
        let item_start = self.cursor;
        let is_loop = self.at_word("for")? || self.at_word("while")? || self.at_word("do")?;
        // Consecutive labels name the same item.
        for label in self.jumps.labels.iter_mut().rev() {
            if label.item_start != start {
                break;
            }
            label.item_start = item_start;
            label.is_loop = is_loop;
        }
        self.jumps
            .labels
            .try_reserve(1)
            .map_err(|_| self.error(start, ParseErrorKind::AllocationFailed))?;
        self.jumps.labels.push(Label {
            name,
            item_start,
            is_loop,
        });
        let label = self.add_node(
            SyntaxNodeKind::LabelIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )?;
        let item_position = match position {
            Position::List(_) | Position::LabelledInList => Position::LabelledInList,
            Position::IfClause | Position::Body => Position::Body,
        };
        let body = self.parse_statement(item_position);
        self.jumps.labels.pop();
        let body = body?;
        let span = self.source_span(start, self.node_span(body).end())?;
        self.add_node(SyntaxNodeKind::LabeledStatement { label, body }, span)
    }

    pub(super) fn parse_block_statement(&mut self) -> Result<NodeId, ParseError> {
        self.push_scope(ScopeKind::Block)?;
        let result = self.parse_block_in_current_scope();
        self.pop_scope();
        result
    }

    // A block whose declarations belong to the innermost scope, as a catch
    // handler shares its scope with the catch parameter.
    fn parse_block_in_current_scope(&mut self) -> Result<NodeId, ParseError> {
        self.within_braces(|parser| {
            let open = parser
                .expect_punctuator(Punctuator::LeftBrace, parser.goal_for_expression_start())?;
            let mut items = parser.take_list();
            parser.parse_statement_list(ListKind::Block, &mut items)?;
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            let body = parser.finish_list(items)?;
            let span = parser.span_from(open.span.start())?;
            parser.add_node(SyntaxNodeKind::BlockStatement { body }, span)
        })
    }

    fn parse_variable_statement(
        &mut self,
        start: usize,
        kind: VariableKind,
    ) -> Result<NodeId, ParseError> {
        self.consume_declaration_keyword(kind)?;
        let declarators = self.parse_declarators(kind, false)?;
        self.consume_semicolon()?;
        let declarators = self.finish_list(declarators)?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::VariableDeclaration { kind, declarators },
            span,
        )
    }

    fn consume_declaration_keyword(&mut self, kind: VariableKind) -> Result<(), ParseError> {
        if kind == VariableKind::AwaitUsing {
            self.scan_identifier()?;
        }
        self.scan_identifier()?;
        Ok(())
    }

    // Parses a declarator list. In a `for` head, initializers are checked
    // later, because `for-in` and `for-of` bindings have none.
    fn parse_declarators(
        &mut self,
        kind: VariableKind,
        for_head: bool,
    ) -> Result<Vec<NodeId>, ParseError> {
        let mut declarators = self.take_list();
        loop {
            self.skip_trivia()?;
            let start = self.cursor;
            let mark = self.bound_names.len();
            let target = self.parse_binding_target(matches!(
                kind,
                VariableKind::Var | VariableKind::Let | VariableKind::Const
            ))?;
            let (declaration, lexical) = match kind {
                VariableKind::Var => (Declaration::Var, false),
                VariableKind::Let
                | VariableKind::Const
                | VariableKind::Using
                | VariableKind::AwaitUsing => (Declaration::Lexical, true),
            };
            let declared = self.declare_bound_names(mark, declaration, lexical);
            self.bound_names.truncate(mark);
            declared?;
            let init = if self
                .consume_punctuator(Punctuator::Assign, self.goal_after_expression())?
                .is_some()
            {
                Some(self.parse_expression(2)?)
            } else {
                if !for_head && self.requires_initializer(kind, target) {
                    return Err(self.error(self.cursor, ParseErrorKind::MissingInitializer));
                }
                None
            };
            let end = init.map_or(self.node_span(target).end(), |id| self.node_span(id).end());
            let span = self.source_span(start, end)?;
            let declarator =
                self.add_node(SyntaxNodeKind::VariableDeclarator { target, init }, span)?;
            self.push_child(&mut declarators, declarator)?;
            if self
                .consume_punctuator(Punctuator::Comma, self.goal_after_expression())?
                .is_none()
            {
                break;
            }
        }
        Ok(declarators)
    }

    fn requires_initializer(&self, kind: VariableKind, target: NodeId) -> bool {
        let pattern = !matches!(
            self.node_kind(target),
            SyntaxNodeKind::BindingIdentifier { .. }
        );
        pattern
            || matches!(
                kind,
                VariableKind::Const | VariableKind::Using | VariableKind::AwaitUsing
            )
    }

    // Parses a binding identifier or, if permitted, a binding pattern, and
    // records its names in `bound_names` without declaring them.
    pub(super) fn parse_binding_target(&mut self, patterns: bool) -> Result<NodeId, ParseError> {
        let Some(ch) = self.peek_char()? else {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        };
        if ch == '[' || ch == '{' {
            if !patterns {
                return Err(self.error(self.cursor, ParseErrorKind::InvalidUsingDeclaration));
            }
            let pattern = self.parse_primary(false)?;
            self.convert_target(pattern, TargetMode::Binding)?;
            return Ok(pattern);
        }
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        }
        let token = self.scan_identifier()?;
        if token.kind == IdentifierKind::PrivateName {
            return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
        }
        self.check_name(token)?;
        let node = self.add_node(
            SyntaxNodeKind::BindingIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )?;
        self.record_bound_name(token.span)?;
        Ok(node)
    }

    fn parse_if(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let test = self.parse_parenthesized_condition()?;
        let consequent = self.parse_statement(Position::IfClause)?;
        let alternate = if self.consume_word("else")?.is_some() {
            Some(self.parse_statement(Position::IfClause)?)
        } else {
            None
        };
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::IfStatement {
                test,
                consequent,
                alternate,
            },
            span,
        )
    }

    fn parse_parenthesized_condition(&mut self) -> Result<NodeId, ParseError> {
        self.expect_punctuator(Punctuator::LeftParen, self.goal_after_expression())?;
        let test = self.parse_expression_with_in()?;
        self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        Ok(test)
    }

    fn parse_loop_body(&mut self) -> Result<NodeId, ParseError> {
        self.jumps.iteration_depth += 1;
        self.jumps.breakable_depth += 1;
        let body = self.parse_statement(Position::Body);
        self.jumps.iteration_depth -= 1;
        self.jumps.breakable_depth -= 1;
        body
    }

    fn parse_while(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let test = self.parse_parenthesized_condition()?;
        let body = self.parse_loop_body()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::WhileStatement { test, body }, span)
    }

    fn parse_do_while(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let body = self.parse_loop_body()?;
        if self.consume_word("while")?.is_none() {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedWhile));
        }
        let test = self.parse_parenthesized_condition()?;
        // A semicolon after `do ... while (...)` is always optional.
        self.consume_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::DoWhileStatement { body, test }, span)
    }

    fn parse_for(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let is_await = self.context.await_keyword && self.consume_word("await")?.is_some();
        self.expect_punctuator(Punctuator::LeftParen, self.goal_after_expression())?;
        self.push_scope(ScopeKind::Block)?;
        let result = self.parse_for_head(start, is_await);
        self.pop_scope();
        result
    }

    fn parse_for_head(&mut self, start: usize, is_await: bool) -> Result<NodeId, ParseError> {
        if self.at_punctuator(Punctuator::Semicolon)? {
            if is_await {
                return Err(self.error(start, ParseErrorKind::InvalidForAwait));
            }
            self.expect_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
            return self.finish_classic_for(start, None);
        }
        self.skip_trivia()?;
        let head_start = self.cursor;
        if let Some(kind) = self.for_declaration_kind()? {
            self.consume_declaration_keyword(kind)?;
            let declarators = self.with_in(true, |parser| parser.parse_declarators(kind, true))?;
            let list = self.append_children(&declarators)?;
            let declaration_span = self.span_from(head_start)?;
            let declaration = self.add_node(
                SyntaxNodeKind::VariableDeclaration {
                    kind,
                    declarators: list,
                },
                declaration_span,
            )?;
            if let Some(of) = self.for_in_of_keyword()? {
                self.check_for_in_of_declaration(kind, &declarators, of)?;
                return self.finish_for_in_of(start, declaration, of, is_await);
            }
            if is_await {
                return Err(self.error(start, ParseErrorKind::InvalidForAwait));
            }
            for &declarator in &declarators {
                if let SyntaxNodeKind::VariableDeclarator { target, init: None } =
                    self.node_kind(declarator)
                    && self.requires_initializer(kind, target)
                {
                    return Err(self.error(
                        self.node_span(declarator).end(),
                        ParseErrorKind::MissingInitializer,
                    ));
                }
            }
            self.expect_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
            return self.finish_classic_for(start, Some(declaration));
        }
        let starts_with_let = self.at_word("let")?;
        let async_token = self
            .peek_name()?
            .filter(|token| self.is_unescaped(*token, "async"));
        let init = self.with_in(true, |parser| parser.parse_expression(0))?;
        if let Some(of) = self.for_in_of_keyword()? {
            if of && starts_with_let {
                return Err(self.error(head_start, ParseErrorKind::InvalidForOfLeftHandSide));
            }
            if of
                && !is_await
                && let Some(token) = async_token
                && self.node_span(init) == token.span
                && matches!(self.node_kind(init), SyntaxNodeKind::Identifier { .. })
            {
                return Err(self.error(head_start, ParseErrorKind::InvalidForOfLeftHandSide));
            }
            if matches!(
                self.node_kind(init),
                SyntaxNodeKind::ArrayExpression { .. } | SyntaxNodeKind::ObjectExpression { .. }
            ) {
                self.convert_target(init, TargetMode::Assignment)?;
            } else {
                self.check_simple_target(init, true)?;
            }
            return self.finish_for_in_of(start, init, of, is_await);
        }
        if is_await {
            return Err(self.error(start, ParseErrorKind::InvalidForAwait));
        }
        self.expect_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
        self.finish_classic_for(start, Some(init))
    }

    // Returns `Some(true)` before `of`, `Some(false)` before `in`.
    fn for_in_of_keyword(&mut self) -> Result<Option<bool>, ParseError> {
        if self.at_word("of")? {
            return Ok(Some(true));
        }
        if self.at_word("in")? {
            return Ok(Some(false));
        }
        Ok(None)
    }

    fn for_declaration_kind(&mut self) -> Result<Option<VariableKind>, ParseError> {
        let Some(token) = self.peek_name()? else {
            return Ok(None);
        };
        if token.has_escape || token.kind != IdentifierKind::Name {
            return Ok(None);
        }
        Ok(match self.slice(token.span) {
            "var" => Some(VariableKind::Var),
            "const" => Some(VariableKind::Const),
            "let" => {
                let next = self.lookahead(|parser| {
                    parser.consume_token(token.span)?;
                    parser.binding_start()
                })?;
                match next {
                    BindingStart::Bracket | BindingStart::Brace | BindingStart::Name => {
                        Some(VariableKind::Let)
                    }
                    BindingStart::Other => None,
                }
            }
            "using" if self.at_using(token, true)? => Some(VariableKind::Using),
            "await" if self.context.await_keyword => {
                let await_using = self.lookahead(|parser| {
                    parser.consume_token(token.span)?;
                    if parser.line_break_before_next()? || parser.consume_word("using")?.is_none() {
                        return Ok(false);
                    }
                    parser.using_binding_follows(true)
                })?;
                await_using.then_some(VariableKind::AwaitUsing)
            }
            _ => None,
        })
    }

    fn check_for_in_of_declaration(
        &self,
        kind: VariableKind,
        declarators: &[NodeId],
        of: bool,
    ) -> Result<(), ParseError> {
        let [declarator] = declarators else {
            let offset = declarators
                .get(1)
                .map_or(self.cursor, |&id| self.node_span(id).start());
            return Err(self.error(offset, ParseErrorKind::InvalidForInOfDeclaration));
        };
        let SyntaxNodeKind::VariableDeclarator { target, init } = self.node_kind(*declarator)
        else {
            return Err(self.error(self.cursor, ParseErrorKind::InvalidForInOfDeclaration));
        };
        if !of && matches!(kind, VariableKind::Using | VariableKind::AwaitUsing) {
            return Err(self.error(
                self.node_span(*declarator).start(),
                ParseErrorKind::InvalidForInOfDeclaration,
            ));
        }
        if let Some(init) = init {
            // Annex B: a sloppy `for (var name = value in object)`.
            let annex_b = !of
                && kind == VariableKind::Var
                && matches!(
                    self.node_kind(target),
                    SyntaxNodeKind::BindingIdentifier { .. }
                );
            let offset = self.node_span(init).start();
            if !annex_b {
                return Err(self.error(offset, ParseErrorKind::InvalidForInOfDeclaration));
            }
            match self.context.strict {
                Strictness::Sloppy => {}
                Strictness::Strict => {
                    return Err(self.error(offset, ParseErrorKind::InvalidForInOfDeclaration));
                }
                Strictness::Unknown => {
                    return Err(self.error(offset, ParseErrorKind::StrictModeRequiresContext));
                }
            }
        }
        Ok(())
    }

    fn finish_for_in_of(
        &mut self,
        start: usize,
        left: NodeId,
        of: bool,
        is_await: bool,
    ) -> Result<NodeId, ParseError> {
        if is_await && !of {
            return Err(self.error(start, ParseErrorKind::InvalidForAwait));
        }
        self.scan_identifier()?;
        let right = if of {
            self.parse_assignment_with_in()?
        } else {
            self.parse_expression_with_in()?
        };
        self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        let body = self.parse_loop_body()?;
        let span = self.span_from(start)?;
        let kind = if of {
            SyntaxNodeKind::ForOfStatement {
                left,
                right,
                body,
                is_await,
            }
        } else {
            SyntaxNodeKind::ForInStatement { left, right, body }
        };
        self.add_node(kind, span)
    }

    // Parses the test, update, and body after the first semicolon.
    fn finish_classic_for(
        &mut self,
        start: usize,
        init: Option<NodeId>,
    ) -> Result<NodeId, ParseError> {
        let test = if self.at_punctuator(Punctuator::Semicolon)? {
            None
        } else {
            Some(self.parse_expression_with_in()?)
        };
        self.expect_punctuator(Punctuator::Semicolon, self.goal_after_expression())?;
        let update = if self.at_punctuator(Punctuator::RightParen)? {
            None
        } else {
            Some(self.parse_expression_with_in()?)
        };
        self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        let body = self.parse_loop_body()?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::ForStatement {
                init,
                test,
                update,
                body,
            },
            span,
        )
    }

    fn parse_continue(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let label = self.parse_jump_label(true)?;
        if label.is_none() && self.jumps.iteration_depth == 0 {
            return Err(self.error(start, ParseErrorKind::InvalidContinue));
        }
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ContinueStatement { label }, span)
    }

    fn parse_break(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let label = self.parse_jump_label(false)?;
        if label.is_none() && self.jumps.breakable_depth == 0 {
            return Err(self.error(start, ParseErrorKind::InvalidBreak));
        }
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::BreakStatement { label }, span)
    }

    // A label after `break` or `continue` on the same line, which must name
    // an enclosing labelled statement, and for `continue` a loop.
    fn parse_jump_label(&mut self, continue_target: bool) -> Result<Option<NodeId>, ParseError> {
        if self.line_break_before_next()? {
            return Ok(None);
        }
        let Some(token) = self.peek_name()? else {
            return Ok(None);
        };
        if token.kind != IdentifierKind::Name {
            return Ok(None);
        }
        self.consume_token(token.span)?;
        self.check_name(token)?;
        let name = self.name_of(token.span);
        let Some(label) = self.jumps.labels.iter().find(|label| label.name == name) else {
            return Err(self.error(token.span.start(), ParseErrorKind::UndefinedLabel));
        };
        if continue_target && !label.is_loop {
            return Err(self.error(token.span.start(), ParseErrorKind::InvalidContinue));
        }
        self.add_node(
            SyntaxNodeKind::LabelIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )
        .map(Some)
    }

    fn parse_return(&mut self, start: usize) -> Result<NodeId, ParseError> {
        if !self.context.in_function {
            return Err(self.error(start, ParseErrorKind::InvalidReturn));
        }
        self.scan_identifier()?;
        let argument = if self.statement_ends_here()? {
            None
        } else {
            Some(self.parse_expression_with_in()?)
        };
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ReturnStatement { argument }, span)
    }

    fn parse_throw(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        if self.line_break_before_next()? {
            return Err(self.error(self.cursor, ParseErrorKind::LineTerminatorAfterThrow));
        }
        let argument = self.parse_expression_with_in()?;
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::ThrowStatement { argument }, span)
    }

    fn parse_with(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.strict_mode_error(start, self.context.strict, StrictModeError::With)?;
        self.scan_identifier()?;
        let object = self.parse_parenthesized_condition()?;
        let body = self.parse_statement(Position::Body)?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::WithStatement { object, body }, span)
    }

    fn parse_switch(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let discriminant = self.parse_parenthesized_condition()?;
        self.push_scope(ScopeKind::Block)?;
        self.jumps.breakable_depth += 1;
        let cases = self.within_braces(Self::parse_case_block);
        self.jumps.breakable_depth -= 1;
        self.pop_scope();
        let cases = cases?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::SwitchStatement {
                discriminant,
                cases,
            },
            span,
        )
    }

    fn parse_case_block(&mut self) -> Result<crate::ast::ChildList, ParseError> {
        self.expect_punctuator(Punctuator::LeftBrace, self.goal_after_expression())?;
        let mut cases = self.take_list();
        let mut default_seen = false;
        loop {
            self.skip_trivia()?;
            let case_start = self.cursor;
            let test = if self.consume_word("case")?.is_some() {
                Some(self.parse_expression_with_in()?)
            } else if self.consume_word("default")?.is_some() {
                if default_seen {
                    return Err(self.error(case_start, ParseErrorKind::DuplicateDefaultClause));
                }
                default_seen = true;
                None
            } else {
                break;
            };
            self.expect_punctuator(Punctuator::Colon, self.goal_after_expression())?;
            let mut consequent = self.take_list();
            self.parse_statement_list(ListKind::SwitchCase, &mut consequent)?;
            let consequent = self.finish_list(consequent)?;
            let span = self.span_from(case_start)?;
            let case = self.add_node(SyntaxNodeKind::SwitchCase { test, consequent }, span)?;
            self.push_child(&mut cases, case)?;
        }
        self.expect_punctuator(Punctuator::RightBrace, self.goal_after_expression())?;
        self.finish_list(cases)
    }

    fn parse_try(&mut self, start: usize) -> Result<NodeId, ParseError> {
        self.scan_identifier()?;
        let block = self.parse_required_block()?;
        let mut handler = None;
        self.skip_trivia()?;
        let catch_start = self.cursor;
        if self.consume_word("catch")?.is_some() {
            handler = Some(self.parse_catch_clause(catch_start)?);
        }
        let finalizer = if self.consume_word("finally")?.is_some() {
            Some(self.parse_required_block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedCatchOrFinally));
        }
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::TryStatement {
                block,
                handler,
                finalizer,
            },
            span,
        )
    }

    fn parse_required_block(&mut self) -> Result<NodeId, ParseError> {
        if !self.at_char('{')? {
            return Err(self.error(
                self.cursor,
                ParseErrorKind::ExpectedPunctuator(Punctuator::LeftBrace),
            ));
        }
        self.parse_block_statement()
    }

    fn parse_catch_clause(&mut self, start: usize) -> Result<NodeId, ParseError> {
        if self
            .consume_punctuator(Punctuator::LeftParen, self.goal_after_expression())?
            .is_none()
        {
            self.push_scope(ScopeKind::Block)?;
            let body = self.parse_catch_body();
            self.pop_scope();
            let body = body?;
            let span = self.span_from(start)?;
            return self.add_node(
                SyntaxNodeKind::CatchClause {
                    parameter: None,
                    body,
                },
                span,
            );
        }
        let mark = self.bound_names.len();
        let parameter = self.parse_binding_target(true);
        let parameter = match parameter {
            Ok(parameter) => parameter,
            Err(error) => {
                self.bound_names.truncate(mark);
                return Err(error);
            }
        };
        let simple_parameter = matches!(
            self.node_kind(parameter),
            SyntaxNodeKind::BindingIdentifier { .. }
        );
        self.push_scope(ScopeKind::Catch { simple_parameter })?;
        let result = self.parse_catch_rest(mark);
        self.pop_scope();
        self.bound_names.truncate(mark);
        let body = result?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::CatchClause {
                parameter: Some(parameter),
                body,
            },
            span,
        )
    }

    fn parse_catch_rest(&mut self, mark: usize) -> Result<NodeId, ParseError> {
        self.declare_bound_names(mark, Declaration::CatchParameter, false)?;
        self.expect_punctuator(Punctuator::RightParen, self.goal_after_expression())?;
        self.parse_catch_body()
    }

    fn parse_catch_body(&mut self) -> Result<NodeId, ParseError> {
        if !self.at_char('{')? {
            return Err(self.error(
                self.cursor,
                ParseErrorKind::ExpectedPunctuator(Punctuator::LeftBrace),
            ));
        }
        self.parse_block_in_current_scope()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingStart {
    Bracket,
    Brace,
    Name,
    Other,
}
