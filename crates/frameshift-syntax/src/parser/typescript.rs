//! TypeScript grammar integrated with the JavaScript parser and arena.

use super::{Context, ParseError, ParseErrorKind, Parser};
use crate::{
    TypeScriptKind as Ts,
    ast::{NodeId, SyntaxNodeKind},
    identifier::{IdentifierKind, ReservedWordClass},
    punctuator::Punctuator,
    template::{TemplateContext, TemplateKind, TemplateMode, TemplatePosition, scan_js_template},
};

// A speculative reading that the TypeScript grammar tries before another
// one: an arrow function head, the lookahead for a function type, type
// arguments in an expression, or a constraint after `infer`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Speculation {
    ArrowHead { start: usize, is_async: bool },
    FunctionTypeStart,
    Instantiation,
    InferConstraint,
}

// A speculation and every piece of parser state that its outcome depends on.
// The outcome repeats in the same state, so the parser speculates once per
// attempt and records failures and lookahead answers. Without the record,
// nested constructs that each start a speculation, such as `([([a])])` or
// `f<([[f<([[a]])>]])>`, are parsed once inside every failed or discarded
// reading and again in the next one, which at least doubles the work per
// level (the compiler keeps a set of positions that are not arrow functions
// for the same reason).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Attempt {
    speculation: Speculation,
    cursor: usize,
    context: Context,
    no_in: bool,
    template_expression_depth: usize,
    disallow_conditional: bool,
}

impl Parser<'_> {
    fn ts_attempt(&self, speculation: Speculation) -> Attempt {
        Attempt {
            speculation,
            cursor: self.cursor,
            context: self.context,
            no_in: self.no_in,
            template_expression_depth: self.template_expression_depth,
            disallow_conditional: self.ts_disallow_conditional,
        }
    }

    fn ts_record(&mut self, attempt: Attempt, outcome: bool) -> Result<(), ParseError> {
        self.speculations
            .try_reserve(1)
            .map_err(|_| self.error(attempt.cursor, ParseErrorKind::AllocationFailed))?;
        self.speculations.insert(attempt, outcome);
        Ok(())
    }

    // Speculates unless the same attempt failed before, and records a
    // failure.
    fn ts_try_once<T>(
        &mut self,
        speculation: Speculation,
        parse: impl FnOnce(&mut Self) -> Result<Option<T>, ParseError>,
    ) -> Result<Option<T>, ParseError> {
        let attempt = self.ts_attempt(speculation);
        if self.speculations.get(&attempt) == Some(&false) {
            return Ok(None);
        }
        let result = self.ts_try(parse)?;
        if result.is_none() {
            self.ts_record(attempt, false)?;
        }
        Ok(result)
    }

    fn ts_can_start_type(&mut self) -> Result<bool, ParseError> {
        self.skip_trivia()?;
        if self.text[self.cursor..].starts_with("??") {
            return Ok(false);
        }
        if let Some(token) = self.peek_name()? {
            return Ok(token.kind == IdentifierKind::Name
                && token.reserved_word.is_none_or(|word| {
                    word.class() != ReservedWordClass::Always
                        || matches!(
                            word,
                            crate::ReservedWord::New
                                | crate::ReservedWord::This
                                | crate::ReservedWord::Typeof
                                | crate::ReservedWord::Void
                                | crate::ReservedWord::Null
                                | crate::ReservedWord::True
                                | crate::ReservedWord::False
                                | crate::ReservedWord::Import
                                | crate::ReservedWord::Function
                        )
                }));
        }
        Ok(self.identifier_can_start_here()?
            || self.peek_char()?.is_some_and(|ch| {
                ch.is_ascii_digit()
                    || matches!(
                        ch,
                        '(' | '{'
                            | '['
                            | '<'
                            | '|'
                            | '&'
                            | '*'
                            | '?'
                            | '!'
                            | '-'
                            | '`'
                            | '\''
                            | '"'
                    )
            }))
    }

    fn ts_function_type_start(&mut self) -> Result<bool, ParseError> {
        if self.at_char('<')? || self.at_word("new")? {
            return Ok(true);
        }
        if self.at_word("abstract")?
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.at_word("new")
            })?
        {
            return Ok(true);
        }
        if !self.at_char('(')? {
            return Ok(false);
        }
        let attempt = self.ts_attempt(Speculation::FunctionTypeStart);
        if let Some(&starts) = self.speculations.get(&attempt) {
            return Ok(starts);
        }
        let mut starts = false;
        self.ts_try(|parser| {
            parser.ts_expect('(')?;
            if parser.at_char(')')? || parser.at_punctuator(Punctuator::Ellipsis)? {
                starts = true;
                return Ok(None::<()>);
            }
            let mut modifiers = parser.take_list();
            parser.ts_member_modifiers(&mut modifiers)?;
            parser.recycle_list(modifiers);
            if parser.identifier_can_start_here()? {
                parser.scan_identifier()?;
            } else if parser.at_char('[')? || parser.at_char('{')? {
                parser.parse_binding_target(true)?;
            } else {
                return Ok(None);
            }
            starts = matches!(parser.peek_char()?, Some(':' | ',' | '?' | '='));
            if parser.ts_symbol(')')? {
                starts = parser.peek_arrow()?.is_some();
            }
            Ok(None)
        })?;
        self.ts_record(attempt, starts)?;
        Ok(starts)
    }
    pub(super) fn ts_node(
        &mut self,
        kind: Ts,
        start: usize,
        items: &[NodeId],
    ) -> Result<NodeId, ParseError> {
        let children = self.append_children(items)?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::TypeScript { kind, children }, span)
    }

    pub(super) fn ts_symbol(&mut self, ch: char) -> Result<bool, ParseError> {
        if !self.at_char(ch)? {
            return Ok(false);
        }
        let span = self.source_span(self.cursor, self.cursor + ch.len_utf8())?;
        self.consume_token(span)?;
        Ok(true)
    }

    fn ts_expect(&mut self, ch: char) -> Result<(), ParseError> {
        if self.ts_symbol(ch)? {
            Ok(())
        } else {
            Err(self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax))
        }
    }

    fn ts_word(&mut self, word: &str) -> Result<(), ParseError> {
        if self.consume_word(word)?.is_some() {
            Ok(())
        } else {
            Err(self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax))
        }
    }

    // Speculation is restricted to type syntax and function heads. Bodies are
    // parsed only after a head commits. Failed work still consumes `ts_work`.
    pub(super) fn ts_try<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<Option<T>, ParseError>,
    ) -> Result<Option<T>, ParseError> {
        let checkpoint = self.checkpoint();
        let mut references = Vec::new();
        references
            .try_reserve_exact(self.classes.len())
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        references.extend(self.classes.iter().map(|class| class.references.len()));
        let lengths = (
            self.nodes.len(),
            self.children.len(),
            self.template_parts.len(),
            self.bound_names.len(),
            self.expression_only_errors.len(),
        );
        let state = (
            self.context,
            self.cover,
            self.no_in,
            self.template_expression_depth,
        );
        let result = parse(self);
        if matches!(&result, Ok(Some(_))) {
            return result;
        }
        self.restore(checkpoint);
        for (class, len) in self.classes.iter_mut().zip(references) {
            class.references.truncate(len);
        }
        self.nodes.truncate(lengths.0);
        self.children.truncate(lengths.1);
        self.template_parts.truncate(lengths.2);
        self.bound_names.truncate(lengths.3);
        self.expression_only_errors.truncate(lengths.4);
        (
            self.context,
            self.cover,
            self.no_in,
            self.template_expression_depth,
        ) = state;
        match result {
            Err(error)
                if matches!(
                    error.kind,
                    ParseErrorKind::ResourceLimit
                        | ParseErrorKind::AllocationFailed
                        | ParseErrorKind::InternalInvariant(_)
                ) =>
            {
                Err(error)
            }
            Ok(None) | Err(_) => Ok(None),
            Ok(Some(_)) => unreachable!(),
        }
    }

    pub(super) fn ts_name(&mut self, binding: bool) -> Result<NodeId, ParseError> {
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
        }
        let token = self.scan_identifier()?;
        if token.kind != IdentifierKind::Name
            || (binding
                && token
                    .reserved_word
                    .is_some_and(|word| word.class() == ReservedWordClass::Always))
        {
            return Err(self.error(token.span.start(), ParseErrorKind::ExpectedIdentifier));
        }
        let kind = if binding {
            SyntaxNodeKind::BindingIdentifier {
                has_escape: token.has_escape,
            }
        } else {
            SyntaxNodeKind::IdentifierName {
                has_escape: token.has_escape,
                reserved_word: token.reserved_word,
            }
        };
        self.add_node(kind, token.span)
    }

    fn ts_entity_name(&mut self) -> Result<NodeId, ParseError> {
        let mut name = self.ts_name(false)?;
        while self.ts_symbol('.')? {
            let right = self.ts_name(false)?;
            name = self.ts_node(
                Ts::QualifiedName,
                self.node_span(name).start(),
                &[name, right],
            )?;
        }
        Ok(name)
    }

    pub(super) fn ts_annotation(&mut self) -> Result<Option<NodeId>, ParseError> {
        if !self.at_char(':')? {
            return Ok(None);
        }
        let start = self.cursor;
        self.ts_expect(':')?;
        let ty = self.ts_return_type()?;
        self.ts_node(Ts::Annotation, start, &[ty]).map(Some)
    }

    fn ts_return_type(&mut self) -> Result<NodeId, ParseError> {
        let predicate = self.lookahead(|parser| {
            if parser.at_word("asserts")? {
                parser.scan_identifier()?;
                return Ok(!parser.line_break_before_next()? && parser.identifier_can_start_here()?);
            }
            if parser.identifier_can_start_here()? {
                parser.scan_identifier()?;
                return Ok(!parser.line_break_before_next()? && parser.at_word("is")?);
            }
            Ok(false)
        })?;
        if !predicate {
            return self.ts_type(!self.ts_disallow_conditional);
        }
        let start = self.cursor;
        let asserts = self.consume_word("asserts")?.is_some();
        let name = self.ts_name(false)?;
        let mut items = self.take_list();
        self.push_child(&mut items, name)?;
        if self.consume_word("is")?.is_some() {
            let ty = self.ts_type(true)?;
            self.push_child(&mut items, ty)?;
        } else if !asserts {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedType));
        }
        let node = self.ts_node(Ts::Predicate { asserts }, start, &items);
        self.recycle_list(items);
        node
    }

    pub(super) fn ts_type_parameters(&mut self) -> Result<Option<NodeId>, ParseError> {
        if !self.at_char('<')? {
            return Ok(None);
        }
        let start = self.cursor;
        self.ts_expect('<')?;
        let mut items = self.take_list();
        if self.at_char('>')? {
            self.ts_expect('>')?;
            self.recycle_list(items);
            return self
                .ts_node(
                    Ts::TypeParameters {
                        trailing_comma: false,
                    },
                    start,
                    &[],
                )
                .map(Some);
        }
        let mut trailing_comma = false;
        loop {
            let parameter = self.ts_type_parameter()?;
            self.push_child(&mut items, parameter)?;
            if !self.ts_symbol(',')? {
                break;
            }
            if self.at_char('>')? {
                trailing_comma = true;
                break;
            }
        }
        self.ts_expect('>')?;
        let result = self.ts_node(Ts::TypeParameters { trailing_comma }, start, &items);
        self.recycle_list(items);
        result.map(Some)
    }

    fn ts_type_parameter(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let mut items = self.take_list();
        while (self.at_word("in")? || self.at_word("out")? || self.at_word("const")?)
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.identifier_can_start_here()
            })?
        {
            let modifier = self.ts_modifier()?;
            self.push_child(&mut items, modifier)?;
        }
        let name = self.ts_name(true)?;
        self.push_child(&mut items, name)?;
        if self.at_word("extends")? {
            let start = self.cursor;
            self.scan_identifier()?;
            let ty = self.ts_type(true)?;
            let constraint = self.ts_node(Ts::Constraint, start, &[ty])?;
            self.push_child(&mut items, constraint)?;
        }
        if self.at_char('=')? {
            let start = self.cursor;
            self.ts_expect('=')?;
            let ty = self.ts_type(true)?;
            let default = self.ts_node(Ts::DefaultType, start, &[ty])?;
            self.push_child(&mut items, default)?;
        }
        let result = self.ts_node(Ts::TypeParameter, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_type_arguments(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        self.ts_expect('<')?;
        let mut items = self.take_list();
        if self.at_char('>')? {
            self.ts_expect('>')?;
            self.recycle_list(items);
            return self.ts_node(Ts::TypeArguments, start, &[]);
        }
        loop {
            if !self.ts_can_start_type()? {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedType));
            }
            let ty = self.ts_type(true)?;
            self.push_child(&mut items, ty)?;
            if !self.ts_symbol(',')? {
                break;
            }
            if self.at_char('>')? {
                break;
            }
        }
        self.ts_expect('>')?;
        let result = self.ts_node(Ts::TypeArguments, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_type(&mut self, conditional: bool) -> Result<NodeId, ParseError> {
        self.enter()?;
        let saved = std::mem::replace(&mut self.ts_disallow_conditional, !conditional);
        let result = self.ts_type_inner(conditional);
        self.ts_disallow_conditional = saved;
        self.leave();
        result
    }

    fn ts_type_inner(&mut self, conditional: bool) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if self.ts_function_type_start()? {
            return self
                .ts_function_type()?
                .ok_or_else(|| self.error(self.cursor, ParseErrorKind::ExpectedType));
        }
        let ty = self.ts_union()?;
        if conditional && !self.line_break_before_next()? && self.consume_word("extends")?.is_some()
        {
            let constraint = self.ts_type(false)?;
            self.expect_punctuator(Punctuator::Question, self.goal_after_expression())?;
            let yes = self.ts_type(true)?;
            self.ts_expect(':')?;
            let no = self.ts_type(true)?;
            return self.ts_node(Ts::Conditional, start, &[ty, constraint, yes, no]);
        }
        Ok(ty)
    }

    fn ts_union(&mut self) -> Result<NodeId, ParseError> {
        self.ts_type_chain('|', Ts::Union, Self::ts_intersection)
    }

    fn ts_intersection(&mut self) -> Result<NodeId, ParseError> {
        self.ts_type_chain('&', Ts::Intersection, Self::ts_type_operator)
    }

    fn ts_type_chain(
        &mut self,
        ch: char,
        kind: Ts,
        parse: fn(&mut Self) -> Result<NodeId, ParseError>,
    ) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let leading = self.ts_symbol(ch)?;
        if leading && self.ts_function_type_start()? {
            return Err(self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax));
        }
        let first = parse(self)?;
        if !self.at_char(ch)? && !leading {
            return Ok(first);
        }
        if leading
            && matches!(
                self.node_kind(first),
                SyntaxNodeKind::TypeScript {
                    kind: Ts::FunctionType | Ts::ConstructorType,
                    ..
                }
            )
        {
            return Err(self.error(
                self.node_span(first).start(),
                ParseErrorKind::InvalidTypeScriptSyntax,
            ));
        }
        let mut items = self.take_list();
        self.push_child(&mut items, first)?;
        while self.ts_symbol(ch)? {
            if self.ts_function_type_start()? {
                return Err(self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax));
            }
            let ty = parse(self)?;
            if matches!(
                self.node_kind(ty),
                SyntaxNodeKind::TypeScript {
                    kind: Ts::FunctionType | Ts::ConstructorType,
                    ..
                }
            ) {
                return Err(self.error(
                    self.node_span(ty).start(),
                    ParseErrorKind::InvalidTypeScriptSyntax,
                ));
            }
            self.push_child(&mut items, ty)?;
        }
        let result = self.ts_node(kind, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_type_operator(&mut self) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.ts_type_operator_inner();
        self.leave();
        result
    }

    fn ts_type_operator_inner(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if self.at_word("keyof")? || self.at_word("unique")? || self.at_word("readonly")? {
            self.scan_identifier()?;
            let ty = self.ts_type_operator()?;
            return self.ts_node(Ts::Operator, start, &[ty]);
        }
        if self.consume_word("infer")?.is_some() {
            let name = self.ts_name(true)?;
            let constraint = self.ts_try_once(Speculation::InferConstraint, |parser| {
                if parser.consume_word("extends")?.is_none() {
                    return Ok(None);
                }
                let start = parser.last_end - "extends".len();
                let ty = parser.ts_type(false)?;
                if !parser.ts_disallow_conditional && parser.at_char('?')? {
                    return Ok(None);
                }
                parser.ts_node(Ts::Constraint, start, &[ty]).map(Some)
            })?;
            let mut items = self.take_list();
            self.push_child(&mut items, name)?;
            if let Some(constraint) = constraint {
                self.push_child(&mut items, constraint)?;
            }
            let parameter =
                self.ts_node(Ts::TypeParameter, self.node_span(name).start(), &items)?;
            self.recycle_list(items);
            return self.ts_node(Ts::Infer, start, &[parameter]);
        }
        let saved = std::mem::replace(&mut self.ts_disallow_conditional, false);
        let result = self.ts_postfix_type();
        self.ts_disallow_conditional = saved;
        result
    }

    fn ts_postfix_type(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let mut ty = self.ts_type_primary()?;
        loop {
            if self.line_break_before_next()? {
                break;
            }
            if self.ts_symbol('[')? {
                if self.ts_symbol(']')? {
                    ty = self.ts_node(Ts::Array, start, &[ty])?;
                } else {
                    if !self.ts_can_start_type()? {
                        return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBracket));
                    }
                    let index = self.ts_type(true)?;
                    self.ts_expect(']')?;
                    ty = self.ts_node(Ts::IndexedAccess, start, &[ty, index])?;
                }
            } else if self.ts_symbol('!')? {
                ty = self.ts_node(Ts::JSDoc, start, &[ty])?;
            } else if self.at_punctuator(Punctuator::Question)?
                && !self.lookahead(|parser| {
                    parser.ts_expect('?')?;
                    parser.ts_can_start_type()
                })?
            {
                self.ts_expect('?')?;
                ty = self.ts_node(Ts::JSDoc, start, &[ty])?;
            } else {
                break;
            }
        }
        Ok(ty)
    }

    fn ts_type_primary(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if (self.at_word("asserts")? || self.at_word("this")?)
            && self.lookahead(|parser| {
                let asserts = parser.at_word("asserts")?;
                parser.scan_identifier()?;
                Ok(!parser.line_break_before_next()?
                    && if asserts {
                        parser.identifier_can_start_here()?
                    } else {
                        parser.at_word("is")?
                    })
            })?
        {
            return self.ts_return_type();
        }
        if self.at_char('(')? {
            self.ts_expect('(')?;
            let ty = self.ts_type(true)?;
            self.ts_expect(')')?;
            return self.ts_node(Ts::Parenthesized, start, &[ty]);
        }
        if self.at_char('{')? {
            return self.ts_object_type();
        }
        if self.at_char('[')? {
            return self.ts_tuple();
        }
        if self.at_char('`')? {
            return self.ts_template_type();
        }
        if self.at_char('*')? {
            self.ts_expect('*')?;
            return self.ts_node(Ts::JSDoc, start, &[]);
        }
        if self.at_char('!')? || self.at_char('?')? {
            let ch = self.peek_char()?.unwrap();
            self.ts_symbol(ch)?;
            if ch == '?' && matches!(self.peek_char()?, Some(',' | '}' | ')' | '>' | '=' | '|')) {
                return self.ts_node(Ts::JSDoc, start, &[]);
            }
            let ty = if ch == '!' {
                self.enter()?;
                let result = self.ts_type_primary();
                self.leave();
                result?
            } else {
                self.ts_type(!self.ts_disallow_conditional)?
            };
            return self.ts_node(Ts::JSDoc, start, &[ty]);
        }
        if self.at_word("function")? {
            return self.ts_jsdoc_function();
        }
        if self.consume_word("typeof")?.is_some() {
            let name = if self.at_word("import")? {
                self.ts_import_type()?
            } else {
                self.ts_entity_name()?
            };
            let mut items = self.take_list();
            self.push_child(&mut items, name)?;
            if !self.line_break_before_next()? && self.at_char('<')? {
                let args = self.ts_type_arguments()?;
                self.push_child(&mut items, args)?;
            }
            let result = self.ts_node(Ts::Query, start, &items);
            self.recycle_list(items);
            return result;
        }
        if self.at_word("import")? {
            return self.ts_import_type();
        }
        if matches!(self.peek_char()?, Some('\'' | '"')) {
            let literal = self.parse_string()?.0;
            return self.ts_node(Ts::Literal, start, &[literal]);
        }
        let negative = self.ts_symbol('-')?;
        if self.peek_char()?.is_some_and(|ch| ch.is_ascii_digit())
            || (self.at_char('.')?
                && self
                    .text
                    .as_bytes()
                    .get(self.cursor + 1)
                    .is_some_and(u8::is_ascii_digit))
        {
            let literal = self.parse_number()?;
            return self.ts_node(Ts::Literal, start, &[literal]);
        }
        if negative {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedType));
        }
        // A literal type is a single token. The expression parser would read a
        // following `=>` as an arrow with an invalid parameter, but after a
        // return type, as in `(): T | null => value`, it belongs to the
        // enclosing arrow function.
        let literal = if self.at_word("null")? {
            Some(SyntaxNodeKind::NullLiteral)
        } else if self.at_word("true")? {
            Some(SyntaxNodeKind::BooleanLiteral(true))
        } else if self.at_word("false")? {
            Some(SyntaxNodeKind::BooleanLiteral(false))
        } else {
            None
        };
        if let Some(kind) = literal {
            let token = self.scan_identifier()?;
            let literal = self.add_node(kind, token.span)?;
            return self.ts_node(Ts::Literal, start, &[literal]);
        }
        if !self.identifier_can_start_here()? {
            return Err(self.error(self.cursor, ParseErrorKind::ExpectedType));
        }
        let token = self.peek_identifier()?;
        if !token.has_escape
            && matches!(
                self.slice(token.span),
                "any"
                    | "unknown"
                    | "string"
                    | "number"
                    | "boolean"
                    | "bigint"
                    | "symbol"
                    | "void"
                    | "undefined"
                    | "never"
                    | "object"
                    | "this"
            )
            && !self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.at_char('.')
            })?
        {
            self.consume_token(token.span)?;
            return self.ts_node(Ts::Keyword, start, &[]);
        }
        let name = self.ts_entity_name()?;
        let mut items = self.take_list();
        self.push_child(&mut items, name)?;
        if !self.line_break_before_next()? && self.at_char('<')? {
            let args = self.ts_type_arguments()?;
            self.push_child(&mut items, args)?;
        }
        let result = self.ts_node(Ts::Reference, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_function_type(&mut self) -> Result<Option<NodeId>, ParseError> {
        let start = self.cursor;
        let mut items = self.take_list();
        let abstract_modifier = self.at_word("abstract")?;
        if abstract_modifier {
            let modifier = self.ts_modifier()?;
            self.push_child(&mut items, modifier)?;
        }
        let constructor = self.consume_word("new")?.is_some();
        if abstract_modifier && !constructor {
            self.recycle_list(items);
            return Ok(None);
        }
        if let Some(params) = self.ts_type_parameters()? {
            self.push_child(&mut items, params)?;
        }
        if !self.at_char('(')? {
            self.recycle_list(items);
            return Ok(None);
        }
        let parameters = self.ts_parameters(false)?;
        self.push_child(&mut items, parameters)?;
        if self
            .consume_punctuator(Punctuator::Arrow, self.goal_after_expression())?
            .is_none()
        {
            self.recycle_list(items);
            return Ok(None);
        }
        let saved = std::mem::replace(&mut self.ts_disallow_conditional, false);
        let ty = self.ts_return_type();
        self.ts_disallow_conditional = saved;
        let ty = ty?;
        self.push_child(&mut items, ty)?;
        let result = self.ts_node(
            if constructor {
                Ts::ConstructorType
            } else {
                Ts::FunctionType
            },
            start,
            &items,
        );
        self.recycle_list(items);
        result.map(Some)
    }

    fn ts_tuple(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('[')?;
        let mut items = self.take_list();
        while !self.at_char(']')? {
            self.skip_trivia()?;
            let element_start = self.cursor;
            let rest = self
                .consume_punctuator(Punctuator::Ellipsis, self.goal_after_expression())?
                .is_some();
            if !rest && !self.ts_can_start_type()? {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedType));
            }
            let named = self.lookahead(|parser| {
                if !parser.identifier_can_start_here()? {
                    return Ok(false);
                }
                parser.scan_identifier()?;
                parser.ts_symbol('?')?;
                parser.at_char(':')
            })?;
            let mut children = self.take_list();
            let mut optional = false;
            if named {
                let name = self.ts_name(false)?;
                self.push_child(&mut children, name)?;
                optional = self.ts_symbol('?')?;
                self.ts_expect(':')?;
            }
            let ty = self.ts_type(true)?;
            self.push_child(&mut children, ty)?;
            if !named {
                optional = self.ts_symbol('?')?;
            }
            optional |= !named
                && matches!(
                    self.node_kind(ty),
                    SyntaxNodeKind::TypeScript {
                        kind: Ts::JSDoc,
                        ..
                    }
                )
                && self.slice(self.node_span(ty)).ends_with('?');
            let element = self.ts_node(
                Ts::TupleElement { optional, rest },
                element_start,
                &children,
            )?;
            self.recycle_list(children);
            self.push_child(&mut items, element)?;
            if !self.ts_symbol(',')? {
                break;
            }
        }
        self.ts_expect(']')?;
        let result = self.ts_node(Ts::Tuple, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_jsdoc_function(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        let token = self.scan_identifier()?;
        if !self.ts_symbol('(')? {
            let name = self.add_node(
                SyntaxNodeKind::IdentifierName {
                    has_escape: token.has_escape,
                    reserved_word: token.reserved_word,
                },
                token.span,
            )?;
            return self.ts_node(Ts::Reference, start, &[name]);
        }
        let mut items = self.take_list();
        while !self.at_char(')')? {
            self.skip_trivia()?;
            let start = self.cursor;
            let mut children = self.take_list();
            if self.at_word("this")? || self.at_word("new")? {
                let name = self.ts_name(false)?;
                self.push_child(&mut children, name)?;
                self.ts_expect(':')?;
            }
            let rest = self
                .consume_punctuator(Punctuator::Ellipsis, self.goal_after_expression())?
                .is_some();
            let ty = self.ts_return_type()?;
            self.push_child(&mut children, ty)?;
            let optional = self.ts_symbol('=')?;
            let param = self.ts_node(Ts::Parameter { optional, rest }, start, &children)?;
            self.recycle_list(children);
            self.push_child(&mut items, param)?;
            if !self.ts_symbol(',')? {
                break;
            }
        }
        self.ts_expect(')')?;
        if let Some(annotation) = self.ts_annotation()? {
            self.push_child(&mut items, annotation)?;
        }
        let result = self.ts_node(Ts::JSDoc, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_template_type(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        let mut items = self.take_list();
        let mut position = TemplatePosition::Start;
        loop {
            let token = scan_js_template(
                self.source,
                self.cursor,
                TemplateContext {
                    position,
                    mode: TemplateMode::Untagged,
                },
                self.limits.lexical,
            )
            .map_err(super::parse_lexical_error)?;
            self.consume_token(token.span)?;
            let children = self.append_children(&[])?;
            let segment = self.add_node(
                SyntaxNodeKind::TypeScript {
                    kind: Ts::TemplateSegment,
                    children,
                },
                token.span,
            )?;
            self.push_child(&mut items, segment)?;
            if matches!(
                token.kind,
                TemplateKind::NoSubstitution | TemplateKind::Tail
            ) {
                break;
            }
            let ty = self.ts_type(true)?;
            self.push_child(&mut items, ty)?;
            self.skip_trivia()?;
            position = TemplatePosition::Continuation;
        }
        let result = self.ts_node(Ts::Template, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_import_type(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_word("import")?;
        self.ts_expect('(')?;
        let mut items = self.take_list();
        let argument = self.ts_type(true)?;
        self.push_child(&mut items, argument)?;
        if self.ts_symbol(',')? {
            let options = self.parse_assignment_with_in()?;
            self.push_child(&mut items, options)?;
            self.ts_symbol(',')?;
        }
        self.ts_expect(')')?;
        if self.ts_symbol('.')? {
            let name = self.ts_entity_name()?;
            self.push_child(&mut items, name)?;
        }
        if !self.line_break_before_next()? && self.at_char('<')? {
            let args = self.ts_type_arguments()?;
            self.push_child(&mut items, args)?;
        }
        let result = self.ts_node(Ts::ImportType, start, &items);
        self.recycle_list(items);
        result
    }

    // An object type ends at its own right brace, also when the type is
    // inside a template substitution, as in `${value as { a: T }}`.
    fn ts_object_type(&mut self) -> Result<NodeId, ParseError> {
        self.within_braces(|parser| {
            let mapped = parser.lookahead(|parser| {
                parser.ts_expect('{')?;
                parser.ts_symbol('+')?;
                parser.ts_symbol('-')?;
                parser.consume_word("readonly")?;
                if !parser.ts_symbol('[')? || !parser.identifier_can_start_here()? {
                    return Ok(false);
                }
                parser.scan_identifier()?;
                parser.at_word("in")
            })?;
            if mapped {
                return parser.ts_mapped_type();
            }
            parser.ts_members(Ts::TypeLiteral)
        })
    }

    pub(super) fn ts_members(&mut self, kind: Ts) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('{')?;
        let mut items = self.take_list();
        while !self.at_char('}')? {
            if self.at_end()? {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBrace));
            }
            let member = self.ts_type_member()?;
            self.push_child(&mut items, member)?;
        }
        self.ts_expect('}')?;
        let result = self.ts_node(kind, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_mapped_type(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('{')?;
        let mut items = self.take_list();
        if self.at_char('+')? || self.at_char('-')? || self.at_word("readonly")? {
            let modifier_start = self.cursor;
            self.ts_symbol('+')?;
            self.ts_symbol('-')?;
            self.ts_word("readonly")?;
            let modifier = self.ts_node(Ts::Modifier, modifier_start, &[])?;
            self.push_child(&mut items, modifier)?;
        }
        self.ts_expect('[')?;
        let parameter_start = self.cursor;
        let name = self.ts_name(true)?;
        self.ts_word("in")?;
        let constraint = self.ts_type(true)?;
        let parameter = self.ts_node(Ts::TypeParameter, parameter_start, &[name, constraint])?;
        self.push_child(&mut items, parameter)?;
        if self.at_word("as")? {
            let start = self.cursor;
            self.scan_identifier()?;
            let ty = self.ts_type(true)?;
            let remap = self.ts_node(Ts::NameRemapping, start, &[ty])?;
            self.push_child(&mut items, remap)?;
        }
        self.ts_expect(']')?;
        if self.at_char('+')? || self.at_char('-')? || self.at_char('?')? {
            let start = self.cursor;
            self.ts_symbol('+')?;
            self.ts_symbol('-')?;
            self.ts_expect('?')?;
            let modifier = self.ts_node(Ts::Modifier, start, &[])?;
            self.push_child(&mut items, modifier)?;
        }
        if let Some(ty) = self.ts_annotation()? {
            self.push_child(&mut items, ty)?;
        }
        self.consume_semicolon()?;
        while !self.at_char('}')? {
            let member = self.ts_type_member()?;
            self.push_child(&mut items, member)?;
        }
        self.ts_expect('}')?;
        let result = self.ts_node(Ts::Mapped, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_modifier(&mut self) -> Result<NodeId, ParseError> {
        let token = self.scan_identifier()?;
        self.ts_node(Ts::Modifier, token.span.start(), &[])
    }

    pub(super) fn ts_member_modifiers(
        &mut self,
        items: &mut Vec<NodeId>,
    ) -> Result<(), ParseError> {
        self.ts_modifiers(items, false).map(|_| ())
    }

    /// Parses the modifiers of a class element and reports whether they
    /// include `static`. `static` and `async` normally stay with the element,
    /// which records them, but a `static` followed by a TypeScript-only
    /// modifier, as in `static readonly`, becomes a modifier node too, and
    /// the caller passes the result on to the element.
    pub(super) fn ts_class_modifiers(
        &mut self,
        items: &mut Vec<NodeId>,
    ) -> Result<bool, ParseError> {
        self.ts_modifiers(items, true)
    }

    fn ts_modifiers(
        &mut self,
        items: &mut Vec<NodeId>,
        preserve_runtime: bool,
    ) -> Result<bool, ParseError> {
        let mut is_static = false;
        while let Some(token) = self.peek_name()? {
            if preserve_runtime && self.is_unescaped(token, "async") {
                break;
            }
            if preserve_runtime && self.slice(token.span) == "static" {
                let before_typescript_modifier = !token.has_escape
                    && !is_static
                    && self.lookahead(|parser| {
                        parser.scan_identifier()?;
                        Ok(parser.peek_name()?.is_some_and(|next| {
                            !next.has_escape
                                && matches!(
                                    parser.slice(next.span),
                                    "readonly"
                                        | "public"
                                        | "private"
                                        | "protected"
                                        | "abstract"
                                        | "declare"
                                        | "override"
                                        | "accessor"
                                )
                        }))
                    })?;
                if !before_typescript_modifier {
                    break;
                }
                is_static = true;
            }
            if token.has_escape
                || !matches!(
                    self.slice(token.span),
                    "readonly"
                        | "public"
                        | "private"
                        | "protected"
                        | "static"
                        | "abstract"
                        | "declare"
                        | "override"
                        | "accessor"
                        | "async"
                )
            {
                break;
            }
            // Like the compiler, `static` may be followed by a line break,
            // as it may in JavaScript; the other modifiers may not.
            let after_line_break = is_static && self.slice(token.span) == "static";
            let is_modifier = self.lookahead(|parser| {
                parser.scan_identifier()?;
                Ok((after_line_break || !parser.line_break_before_next()?)
                    && (parser.identifier_can_start_here()?
                        || parser.at_char('[')?
                        || matches!(parser.peek_char()?, Some('\'' | '"' | '@'))))
            })?;
            if !is_modifier {
                break;
            }
            let modifier = self.ts_modifier()?;
            self.push_child(items, modifier)?;
        }
        Ok(is_static)
    }

    pub(super) fn ts_type_member(&mut self) -> Result<NodeId, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let mut items = self.take_list();
        self.ts_member_modifiers(&mut items)?;
        let construct = self.at_word("new")?
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                Ok(parser.at_char('(')? || parser.at_char('<')?)
            })?;
        if construct {
            self.scan_identifier()?;
        }
        if construct || self.at_char('(')? || self.at_char('<')? {
            self.ts_signature_tail(&mut items)?;
            self.ts_member_end()?;
            let result = self.ts_node(
                if construct {
                    Ts::ConstructSignature
                } else {
                    Ts::CallSignature
                },
                start,
                &items,
            );
            self.recycle_list(items);
            return result;
        }
        if self.ts_is_index_signature()? {
            self.ts_expect('[')?;
            let params = self.ts_parameter_items(']', false)?;
            for item in params {
                self.push_child(&mut items, item)?;
            }
            self.ts_expect(']')?;
            if let Some(ty) = self.ts_annotation()? {
                self.push_child(&mut items, ty)?;
            }
            self.ts_member_end()?;
            let result = self.ts_node(Ts::IndexSignature, start, &items);
            self.recycle_list(items);
            return result;
        }
        if (self.at_word("get")? || self.at_word("set")?)
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.identifier_can_start_here()
            })?
        {
            let modifier = self.ts_modifier()?;
            self.push_child(&mut items, modifier)?;
        }
        let key = self.parse_property_key(false)?.node;
        self.push_child(&mut items, key)?;
        let optional = self.ts_symbol('?')?;
        let kind = if self.at_char('(')? || self.at_char('<')? {
            self.ts_signature_tail(&mut items)?;
            Ts::MethodSignature { optional }
        } else {
            if let Some(ty) = self.ts_annotation()? {
                self.push_child(&mut items, ty)?;
            }
            if self.ts_symbol('=')? {
                let value = self.parse_assignment_with_in()?;
                self.push_child(&mut items, value)?;
            }
            Ts::PropertySignature { optional }
        };
        self.ts_member_end()?;
        let result = self.ts_node(kind, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_is_index_signature(&mut self) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            if !parser.ts_symbol('[')? {
                return Ok(false);
            }
            if parser
                .consume_punctuator(Punctuator::Ellipsis, parser.goal_after_expression())?
                .is_some()
            {
                return Ok(true);
            }
            if !parser.identifier_can_start_here()? {
                return Ok(false);
            }
            parser.scan_identifier()?;
            Ok(parser.at_char(':')? || parser.at_char(',')?)
        })
    }

    fn ts_signature_tail(&mut self, items: &mut Vec<NodeId>) -> Result<(), ParseError> {
        if let Some(generics) = self.ts_type_parameters()? {
            self.push_child(items, generics)?;
        }
        let parameters = self.ts_parameters(false)?;
        self.push_child(items, parameters)?;
        if let Some(ty) = self.ts_annotation()? {
            self.push_child(items, ty)?;
        }
        Ok(())
    }

    pub(super) fn ts_member_end(&mut self) -> Result<(), ParseError> {
        if self.ts_symbol(',')? {
            return Ok(());
        }
        self.consume_semicolon()
    }

    pub(super) fn ts_parameters(&mut self, bindings: bool) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('(')?;
        let items = self.ts_parameter_items(')', bindings)?;
        self.ts_expect(')')?;
        let parameters = self.append_children(&items)?;
        self.recycle_list(items);
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::FormalParameters { parameters }, span)
    }

    fn ts_parameter_items(&mut self, end: char, bindings: bool) -> Result<Vec<NodeId>, ParseError> {
        let mut items = self.take_list();
        while !self.at_char(end)? {
            self.skip_trivia()?;
            let start = self.cursor;
            let mut children = self.take_list();
            while self.at_char('@')? {
                let decorator = self.ts_decorator()?;
                self.push_child(&mut children, decorator)?;
            }
            self.ts_member_modifiers(&mut children)?;
            let rest = self
                .consume_punctuator(Punctuator::Ellipsis, self.goal_after_expression())?
                .is_some();
            let name = if self.at_word("this")? {
                self.ts_name(false)?
            } else if bindings {
                self.parse_binding_target(true)?
            } else if self.at_char('{')? || self.at_char('[')? {
                let mark = self.bound_names.len();
                let name = self.parse_binding_target(true)?;
                self.bound_names.truncate(mark);
                name
            } else {
                self.ts_name(true)?
            };
            self.push_child(&mut children, name)?;
            let optional = self.ts_symbol('?')?;
            if let Some(ty) = self.ts_annotation()? {
                self.push_child(&mut children, ty)?;
            }
            if self.ts_symbol('=')? {
                let value = self.parse_assignment_with_in()?;
                self.push_child(&mut children, value)?;
            }
            let parameter = self.ts_node(Ts::Parameter { optional, rest }, start, &children)?;
            self.recycle_list(children);
            self.push_child(&mut items, parameter)?;
            if !self.ts_symbol(',')? {
                break;
            }
        }
        Ok(items)
    }

    pub(super) fn ts_decorator(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('@')?;
        let primary = self.parse_primary(false)?;
        let expression = self.parse_postfix(primary, super::expression::PostfixMode::Full)?;
        self.ts_node(Ts::Decorator, start, &[expression])
    }

    pub(super) fn ts_typed_binding(
        &mut self,
        binding: NodeId,
        allow_optional: bool,
        allow_definite: bool,
    ) -> Result<NodeId, ParseError> {
        let start = self.node_span(binding).start();
        let optional = allow_optional && self.ts_symbol('?')?;
        let definite = allow_definite && self.ts_symbol('!')?;
        let annotation = self.ts_annotation()?;
        if !optional && !definite && annotation.is_none() {
            return Ok(binding);
        }
        let mut items = self.take_list();
        self.push_child(&mut items, binding)?;
        if let Some(annotation) = annotation {
            self.push_child(&mut items, annotation)?;
        }
        let result = self.ts_node(Ts::TypedBinding { optional, definite }, start, &items);
        self.recycle_list(items);
        result
    }

    // The operand of a TypeScript wrapper around an assignment target: a
    // typed binding or non-null expression, and with `assertions` also an
    // `as`, `satisfies`, or angle-bracket assertion.
    pub(super) fn ts_target_operand(&self, id: NodeId, assertions: bool) -> Option<NodeId> {
        let SyntaxNodeKind::TypeScript { kind, children } = self.node_kind(id) else {
            return None;
        };
        let index = match kind {
            Ts::TypedBinding { .. } | Ts::NonNull => 0,
            Ts::As | Ts::Satisfies if assertions => 0,
            Ts::Assertion if assertions => 1,
            Ts::As
            | Ts::Satisfies
            | Ts::Assertion
            | Ts::Keyword
            | Ts::Reference
            | Ts::QualifiedName
            | Ts::TypeArguments
            | Ts::TypeParameters { .. }
            | Ts::TypeParameter
            | Ts::Constraint
            | Ts::DefaultType
            | Ts::Parenthesized
            | Ts::Union
            | Ts::Intersection
            | Ts::Conditional
            | Ts::Operator
            | Ts::Infer
            | Ts::Array
            | Ts::IndexedAccess
            | Ts::Tuple
            | Ts::TupleElement { .. }
            | Ts::Literal
            | Ts::Template
            | Ts::TemplateSegment
            | Ts::Query
            | Ts::ImportType
            | Ts::TypeLiteral
            | Ts::Mapped
            | Ts::NameRemapping
            | Ts::PropertySignature { .. }
            | Ts::MethodSignature { .. }
            | Ts::CallSignature
            | Ts::ConstructSignature
            | Ts::IndexSignature
            | Ts::FunctionType
            | Ts::ConstructorType
            | Ts::Predicate { .. }
            | Ts::Annotation
            | Ts::Parameter { .. }
            | Ts::Modifier
            | Ts::Decorator
            | Ts::TypeAlias
            | Ts::Interface
            | Ts::Heritage
            | Ts::Enum
            | Ts::EnumMember
            | Ts::Namespace
            | Ts::NamespaceBody
            | Ts::ImportEquals
            | Ts::ExternalModuleReference
            | Ts::TypeOnly
            | Ts::ExportAssignment
            | Ts::NamespaceExport
            | Ts::ModifiedDeclaration
            | Ts::GenericDeclaration
            | Ts::BodylessFunction
            | Ts::Instantiation
            | Ts::JSDoc => return None,
        };
        self.children.get(children.range().start + index).copied()
    }

    pub(super) fn ts_unwrap_binding(&self, mut id: NodeId) -> NodeId {
        while let SyntaxNodeKind::TypeScript {
            kind: Ts::TypedBinding { .. } | Ts::NonNull | Ts::As | Ts::Assertion | Ts::Satisfies,
            children,
        } = self.node_kind(id)
        {
            let index = if matches!(
                self.node_kind(id),
                SyntaxNodeKind::TypeScript {
                    kind: Ts::Assertion,
                    ..
                }
            ) {
                1
            } else {
                0
            };
            let Some(child) = self.children.get(children.range().start + index) else {
                break;
            };
            id = *child;
        }
        id
    }

    pub(super) fn ts_declaration(&mut self, in_list: bool) -> Result<Option<NodeId>, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        let is_type = self.at_word("type")?
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                Ok(!parser.line_break_before_next()? && parser.identifier_can_start_here()?)
            })?;
        if is_type
            || self.at_word("interface")?
            || self.at_word("enum")?
            || self.at_word("namespace")?
            || self.at_word("module")?
            || self.at_word("global")?
        {
            if !in_list {
                return Err(self.error(start, ParseErrorKind::DeclarationInStatementPosition));
            }
            let node = if is_type {
                self.ts_type_alias()?
            } else if self.at_word("interface")? {
                self.ts_interface()?
            } else if self.at_word("enum")? {
                self.ts_enum()?
            } else if (self.at_word("global")?
                && !self.lookahead(|parser| {
                    parser.scan_identifier()?;
                    parser.at_char('{')
                })?)
                || ((self.at_word("module")? || self.at_word("namespace")?)
                    && !self.lookahead(|parser| {
                        parser.scan_identifier()?;
                        Ok(!parser.line_break_before_next()?
                            && (parser.identifier_can_start_here()?
                                || matches!(parser.peek_char()?, Some('\'' | '"'))))
                    })?)
            {
                return Ok(None);
            } else {
                self.ts_namespace()?
            };
            return Ok(Some(node));
        }
        if self.at_word("import")? && self.ts_is_import_equals()? {
            return self.ts_import_equals().map(Some);
        }
        let modified = self.at_char('@')?
            || ((self.at_word("declare")? || self.at_word("abstract")?)
                && self.lookahead(|parser| {
                    parser.scan_identifier()?;
                    Ok(!parser.line_break_before_next()? && parser.identifier_can_start_here()?)
                })?)
            || (self.at_word("const")?
                && self.lookahead(|parser| {
                    parser.scan_identifier()?;
                    parser.at_word("enum")
                })?);
        if modified {
            if !in_list {
                return Err(self.error(start, ParseErrorKind::DeclarationInStatementPosition));
            }
            let mut items = self.take_list();
            while self.at_char('@')? {
                let decorator = self.ts_decorator()?;
                self.push_child(&mut items, decorator)?;
            }
            let saved_ambient = self.ambient;
            while self.at_word("declare")?
                || self.at_word("abstract")?
                || (self.at_word("const")?
                    && self.lookahead(|parser| {
                        parser.scan_identifier()?;
                        parser.at_word("enum")
                    })?)
            {
                self.ambient |= self.at_word("declare")?;
                let modifier = self.ts_modifier()?;
                self.push_child(&mut items, modifier)?;
            }
            let declaration = self.parse_module_item_declaration();
            self.ambient = saved_ambient;
            let declaration = declaration?;
            self.push_child(&mut items, declaration)?;
            let result = self.ts_node(Ts::ModifiedDeclaration, start, &items);
            self.recycle_list(items);
            return result.map(Some);
        }
        Ok(None)
    }

    fn ts_type_alias(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_word("type")?;
        let mut items = self.take_list();
        let name = self.ts_name(true)?;
        self.push_child(&mut items, name)?;
        if let Some(params) = self.ts_type_parameters()? {
            self.push_child(&mut items, params)?;
        }
        self.ts_expect('=')?;
        let ty = if self.at_word("intrinsic")?
            && !self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.at_char('.')
            })? {
            let token = self.scan_identifier()?;
            self.ts_node(Ts::Keyword, token.span.start(), &[])?
        } else {
            self.ts_type(true)?
        };
        self.push_child(&mut items, ty)?;
        self.consume_semicolon()?;
        let result = self.ts_node(Ts::TypeAlias, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_interface(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_word("interface")?;
        let mut items = self.take_list();
        let name = self.ts_name(true)?;
        self.push_child(&mut items, name)?;
        if let Some(params) = self.ts_type_parameters()? {
            self.push_child(&mut items, params)?;
        }
        if let Some(heritage) = self.ts_heritage("extends")? {
            self.push_child(&mut items, heritage)?;
        }
        let members = self.ts_members(Ts::TypeLiteral)?;
        self.push_child(&mut items, members)?;
        let result = self.ts_node(Ts::Interface, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_heritage(&mut self, word: &str) -> Result<Option<NodeId>, ParseError> {
        if !self.at_word(word)? {
            return Ok(None);
        }
        let start = self.cursor;
        self.scan_identifier()?;
        let mut items = self.take_list();
        loop {
            let primary = self.parse_primary(false)?;
            let name = self.parse_postfix(primary, super::expression::PostfixMode::NewCallee)?;
            let name = self.ts_heritage_arguments(name)?;
            self.push_child(&mut items, name)?;
            if !self.ts_symbol(',')? {
                break;
            }
        }
        let result = self.ts_node(Ts::Heritage, start, &items);
        self.recycle_list(items);
        result.map(Some)
    }

    fn ts_enum(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_word("enum")?;
        let mut items = self.take_list();
        let name = self.ts_name(true)?;
        self.push_child(&mut items, name)?;
        self.ts_expect('{')?;
        while !self.at_char('}')? {
            self.skip_trivia()?;
            let member_start = self.cursor;
            let key = self.parse_property_key(false)?.node;
            let mut children = self.take_list();
            self.push_child(&mut children, key)?;
            if self.ts_symbol('=')? {
                let value = self.parse_assignment_with_in()?;
                self.push_child(&mut children, value)?;
            }
            let member = self.ts_node(Ts::EnumMember, member_start, &children)?;
            self.recycle_list(children);
            self.push_child(&mut items, member)?;
            if !self.ts_symbol(',')? {
                break;
            }
        }
        self.ts_expect('}')?;
        let result = self.ts_node(Ts::Enum, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_namespace(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        let global = self.at_word("global")?;
        let token = self.scan_identifier()?;
        let name = if global {
            self.add_node(
                SyntaxNodeKind::IdentifierName {
                    has_escape: token.has_escape,
                    reserved_word: token.reserved_word,
                },
                token.span,
            )?
        } else if matches!(self.peek_char()?, Some('\'' | '"')) {
            self.parse_string()?.0
        } else {
            self.ts_name(true)?
        };
        self.ts_namespace_rest(start, name)
    }

    fn ts_namespace_rest(&mut self, start: usize, name: NodeId) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.ts_namespace_rest_inner(start, name);
        self.leave();
        result
    }

    fn ts_namespace_rest_inner(
        &mut self,
        start: usize,
        name: NodeId,
    ) -> Result<NodeId, ParseError> {
        let mut items = self.take_list();
        self.push_child(&mut items, name)?;
        if self.ts_symbol('.')? {
            let next = self.ts_name(true)?;
            let nested = self.ts_namespace_rest(self.node_span(next).start(), next)?;
            self.push_child(&mut items, nested)?;
        } else if self.at_char('{')? {
            let start = self.cursor;
            self.ts_expect('{')?;
            self.push_scope(super::scope::ScopeKind::Block)?;
            let module = std::mem::take(&mut self.module);
            let mut statements = self.take_list();
            while !self.at_char('}')? {
                if self.at_end()? {
                    return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBrace));
                }
                let statement = self.parse_module_item_declaration()?;
                self.push_child(&mut statements, statement)?;
            }
            self.ts_expect('}')?;
            self.module = module;
            self.pop_scope();
            let body = self.ts_node(Ts::NamespaceBody, start, &statements)?;
            self.recycle_list(statements);
            self.push_child(&mut items, body)?;
        } else {
            self.consume_semicolon()?;
        }
        let result = self.ts_node(Ts::Namespace, start, &items);
        self.recycle_list(items);
        result
    }

    fn ts_is_import_equals(&mut self) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            parser.ts_word("import")?;
            parser.consume_word("type")?;
            if !parser.identifier_can_start_here()? {
                return Ok(false);
            }
            parser.scan_identifier()?;
            parser.at_char('=')
        })
    }

    fn ts_import_equals(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_word("import")?;
        let mut items = self.take_list();
        if self.at_word("type")? {
            let modifier = self.ts_modifier()?;
            self.push_child(&mut items, modifier)?;
        }
        let name = self.ts_name(true)?;
        self.push_child(&mut items, name)?;
        self.ts_expect('=')?;
        let reference = if self.at_word("require")?
            && self.lookahead(|parser| {
                parser.scan_identifier()?;
                parser.at_char('(')
            })? {
            let start = self.cursor;
            self.scan_identifier()?;
            self.ts_expect('(')?;
            if !matches!(self.peek_char()?, Some('\'' | '"')) {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedModuleSpecifier));
            }
            let argument = self.parse_string()?.0;
            self.ts_expect(')')?;
            self.ts_node(Ts::ExternalModuleReference, start, &[argument])?
        } else {
            self.ts_entity_name()?
        };
        self.push_child(&mut items, reference)?;
        self.consume_semicolon()?;
        let result = self.ts_node(Ts::ImportEquals, start, &items);
        self.recycle_list(items);
        result
    }

    pub(super) fn ts_import_type_only(&mut self) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            if parser.consume_word("type")?.is_none() {
                return Ok(false);
            }
            Ok(parser.at_char('{')?
                || parser.at_char('*')?
                || (parser.identifier_can_start_here()? && !parser.at_word("from")?))
        })
    }

    pub(super) fn ts_specifier_type_only(&mut self) -> Result<bool, ParseError> {
        self.lookahead(|parser| {
            if parser.consume_word("type")?.is_none() {
                return Ok(false);
            }
            if !parser.identifier_can_start_here()? {
                return Ok(false);
            }
            if !parser.at_word("as")? {
                return Ok(true);
            }
            parser.scan_identifier()?;
            if parser.at_word("as")? {
                parser.scan_identifier()?;
                return Ok(parser.identifier_can_start_here()?
                    || matches!(parser.peek_char()?, Some('\'' | '"')));
            }
            Ok(!parser.identifier_can_start_here()?
                && !matches!(parser.peek_char()?, Some('\'' | '"')))
        })
    }

    pub(super) fn ts_arrow_head(
        &mut self,
        start: usize,
        is_async: bool,
    ) -> Result<Option<(NodeId, usize)>, ParseError> {
        let mark = self.bound_names.len();
        let mut items = self.take_list();
        let generic_start = self.cursor;
        let generics = self.ts_type_parameters()?;
        if let Some(generics) = generics {
            if self.jsx {
                let SyntaxNodeKind::TypeScript {
                    kind: Ts::TypeParameters { trailing_comma },
                    children,
                } = self.node_kind(generics)
                else {
                    return Err(self.error(generic_start, ParseErrorKind::InvalidTypeScriptSyntax));
                };
                let parameters = &self.children[children.range()];
                let constrained = parameters.first().is_some_and(|&id| {
                    if let SyntaxNodeKind::TypeScript { children, .. } = self.node_kind(id) {
                        self.children[children.range()].iter().any(|&id| {
                            matches!(
                                self.node_kind(id),
                                SyntaxNodeKind::TypeScript {
                                    kind: Ts::Constraint | Ts::DefaultType,
                                    ..
                                }
                            )
                        })
                    } else {
                        false
                    }
                });
                if parameters.len() <= 1 && !trailing_comma && !constrained {
                    self.recycle_list(items);
                    return Ok(None);
                }
            }
            self.push_child(&mut items, generics)?;
        }
        if !self.at_char('(')? {
            self.recycle_list(items);
            return Ok(None);
        }
        let saved_context = self.context;
        self.context.await_keyword = is_async;
        self.context.yield_keyword = false;
        let parameters = self.ts_parameters(true);
        self.context = saved_context;
        let parameters = parameters?;
        self.push_child(&mut items, parameters)?;
        if let Some(annotation) = self.ts_annotation()? {
            self.push_child(&mut items, annotation)?;
        }
        if self.peek_arrow()?.is_none() || self.line_break_before {
            self.recycle_list(items);
            return Ok(None);
        }
        let node = if items.len() == 1 {
            parameters
        } else {
            self.ts_node(Ts::GenericDeclaration, generic_start.max(start), &items)?
        };
        self.recycle_list(items);
        Ok(Some((node, mark)))
    }

    pub(super) fn ts_try_arrow(
        &mut self,
        start: usize,
        is_async: bool,
        allowed: bool,
    ) -> Result<Option<NodeId>, ParseError> {
        let Some((parameters, mark)) = self
            .ts_try_once(Speculation::ArrowHead { start, is_async }, |parser| {
                parser.ts_arrow_head(start, is_async)
            })?
        else {
            return Ok(None);
        };
        if !allowed {
            return Err(self.error(start, ParseErrorKind::UnexpectedArrow));
        }
        self.parse_ts_arrow_body(start, parameters, mark, is_async)
            .map(Some)
    }

    pub(super) fn ts_assertion(&mut self) -> Result<NodeId, ParseError> {
        let start = self.cursor;
        self.ts_expect('<')?;
        let ty = if self.at_word("const")? {
            let token = self.scan_identifier()?;
            self.ts_node(Ts::Keyword, token.span.start(), &[])?
        } else {
            self.ts_type(true)?
        };
        self.ts_expect('>')?;
        let expression = self.parse_expression(15)?;
        self.ts_node(Ts::Assertion, start, &[ty, expression])
    }

    pub(super) fn ts_instantiation(
        &mut self,
        expression: NodeId,
        force: bool,
    ) -> Result<Option<NodeId>, ParseError> {
        let start = self.node_span(expression).start();
        let args = if force {
            Some(self.ts_type_arguments()?)
        } else {
            self.ts_try_once(Speculation::Instantiation, |parser| {
                let args = parser.ts_type_arguments()?;
                // The pinned compiler re-scans the closing `>` together with
                // a directly following `>` or `=` into one operator, so such
                // a list is no type argument list: `f<T>= x` and
                // `a < b, c >= d` compare.
                if parser.text.as_bytes().get(parser.cursor) == Some(&b'=') {
                    return Ok(None);
                }
                // TypeScript's `canFollowTypeArgumentsInExpression` decides
                // whether `a < b > c` compares or instantiates: a call, a
                // tagged template, or a property access takes the type
                // arguments, and so does any token that cannot start an
                // expression, any binary operator (including `=`, `in`,
                // `as`, and `satisfies`), or any token after a line break.
                // A follower that starts with `>` keeps the comparison, as
                // do `<`, `+`, and `-` in their single-character forms,
                // while `<=` and `<<` arrive as one token and instantiate.
                if parser.at_punctuator(Punctuator::LeftParen)?
                    || parser.at_punctuator(Punctuator::Dot)?
                    || parser.at_char('`')?
                {
                    return Ok(Some(args));
                }
                if parser.at_char('>')?
                    || parser.at_punctuator(Punctuator::LessThan)?
                    || parser.at_punctuator(Punctuator::Plus)?
                    || parser.at_punctuator(Punctuator::Minus)?
                {
                    return Ok(None);
                }
                if parser.line_break_before
                    || parser.ts_binary_operator_follows()?
                    || !parser.can_start_expression()?
                {
                    return Ok(Some(args));
                }
                Ok(None)
            })?
        };
        match args {
            Some(args) => self
                .ts_node(Ts::Instantiation, start, &[expression, args])
                .map(Some),
            None => Ok(None),
        }
    }

    // Completes a heritage expression with its type arguments. A heritage
    // clause is not an expression, so its type arguments are not subject to
    // the `a < b > c` disambiguation: `class A extends B<T> {}` and
    // `implements C<T> {` keep the type arguments even though `{` could
    // begin an expression. The pinned compiler parses heritage clauses
    // separately and reaches the same reading.
    pub(super) fn ts_heritage_arguments(
        &mut self,
        expression: NodeId,
    ) -> Result<NodeId, ParseError> {
        if !self.typescript || !self.at_char('<')? {
            return Ok(expression);
        }
        Ok(self
            .ts_instantiation(expression, true)?
            .unwrap_or(expression))
    }

    // Reports whether the token that follows a type argument list is a
    // binary operator. `as` and `satisfies` count here even though the
    // expression parser consumes them separately, matching `isBinaryOperator2`
    // in the pinned compiler's `canFollowTypeArgumentsInExpression`.
    fn ts_binary_operator_follows(&mut self) -> Result<bool, ParseError> {
        if self.at_word("as")? || self.at_word("satisfies")? {
            return Ok(true);
        }
        self.at_binary_operator()
    }

    // TypeScript rejects a property access that directly follows an
    // instantiation expression, because its type arguments must be
    // parenthesized first; `f<T>.x`, `f<T>?.x`, and `new f<T>.x` are errors
    // while `(f<T>).x`, `f<T>[x]`, and `f<T>()` stay valid. The pinned
    // compiler reports the same error while parsing the property access.
    pub(super) fn ts_check_property_access(
        &self,
        object: NodeId,
        offset: usize,
    ) -> Result<(), ParseError> {
        if matches!(
            self.node_kind(object),
            SyntaxNodeKind::TypeScript {
                kind: Ts::Instantiation,
                ..
            }
        ) {
            return Err(self.error(offset, ParseErrorKind::InstantiationPropertyAccess));
        }
        Ok(())
    }
}
