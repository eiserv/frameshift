//! Classes: heritage, elements, static blocks, and private names.
//!
//! All parts of a class are strict mode code. Private names may be used
//! before their declaration, so references are resolved when the class body
//! ends; unresolved names move to the enclosing class.

use super::{
    Context, Goal, ParseError, ParseErrorKind, Parser, Permission, Strictness,
    expression::{PostfixMode, PropertyKey},
    function::MethodClass,
    names::Name,
    scope::{Declaration, ScopeKind},
    statement::ListKind,
};
use crate::{
    ast::{MethodKind, NodeId, SyntaxNodeKind},
    identifier::{IdentifierKind, IdentifierToken},
    lookup::LookupMap,
    punctuator::Punctuator,
    value::{identifier_equals, string_literal_equals},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClassPlacement {
    // A declaration in a statement list; the name is required.
    Declaration,
    // A class expression; the name is optional and local to the class.
    Expression,
    // `export default class`; the name is optional.
    ExportDefault,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateKind {
    Field,
    Method,
    Getter,
    Setter,
    // A getter and setter pair.
    Accessor,
}

#[derive(Clone, Copy)]
struct PrivateDeclaration {
    kind: PrivateKind,
    is_static: bool,
}

#[derive(Default)]
pub(super) struct ClassScope<'source> {
    declared: LookupMap<Name<'source>, PrivateDeclaration>,
    pub(super) references: Vec<(Name<'source>, usize)>,
}

impl<'source> Parser<'source> {
    // Records a private name reference for validation when its class ends.
    pub(super) fn use_private_name(&mut self, token: IdentifierToken) -> Result<(), ParseError> {
        let offset = token.span.start();
        let name = self.name_of(token.name_span);
        let Some(class) = self.classes.last_mut() else {
            let kind = if self.goal == Goal::Unknown {
                ParseErrorKind::PrivateIdentifierRequiresContext
            } else {
                ParseErrorKind::UndeclaredPrivateName
            };
            return Err(self.error(offset, kind));
        };
        class.references.try_reserve(1).map_err(|_| ParseError {
            offset,
            kind: ParseErrorKind::AllocationFailed,
        })?;
        class.references.push((name, offset));
        Ok(())
    }

    // Parses a class after `class`, starting at the optional name.
    pub(super) fn parse_class(
        &mut self,
        start: usize,
        placement: ClassPlacement,
    ) -> Result<NodeId, ParseError> {
        self.enter()?;
        let strict = std::mem::replace(&mut self.context.strict, Strictness::Strict);
        let result = self.parse_class_inner(start, placement);
        self.context.strict = strict;
        self.leave();
        result
    }

    fn parse_class_inner(
        &mut self,
        start: usize,
        placement: ClassPlacement,
    ) -> Result<NodeId, ParseError> {
        let mut name = match self.peek_name()? {
            Some(token)
                if token.kind == IdentifierKind::Name && !self.is_unescaped(token, "extends") =>
            {
                self.consume_token(token.span)?;
                self.check_name(token)?;
                self.check_binding_strictness(token.span, Strictness::Strict)?;
                if placement != ClassPlacement::Expression {
                    let name = self.name_of(token.span);
                    self.declare(name, token.span.start(), Declaration::Lexical)?;
                }
                Some(self.add_node(
                    SyntaxNodeKind::BindingIdentifier {
                        has_escape: token.has_escape,
                    },
                    token.span,
                )?)
            }
            Some(_) | None => {
                if placement == ClassPlacement::Declaration {
                    return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
                }
                None
            }
        };
        if self.typescript
            && let Some(parameters) = self.ts_type_parameters()?
        {
            let start = name.map_or(self.node_span(parameters).start(), |id| {
                self.node_span(id).start()
            });
            let mut items = self.take_list();
            if let Some(name) = name {
                self.push_child(&mut items, name)?;
            }
            self.push_child(&mut items, parameters)?;
            name = Some(self.ts_node(crate::TypeScriptKind::GenericDeclaration, start, &items)?);
            self.recycle_list(items);
        }
        let mut super_class = if self.consume_word("extends")?.is_some() {
            let heritage = self.parse_primary(false)?;
            let heritage = self.parse_postfix(heritage, PostfixMode::Full)?;
            Some(self.ts_heritage_arguments(heritage)?)
        } else {
            None
        };
        let derived = super_class.is_some();
        if self.typescript
            && let Some(implements) = self.ts_heritage("implements")?
        {
            let start = super_class.map_or(self.node_span(implements).start(), |id| {
                self.node_span(id).start()
            });
            let mut items = self.take_list();
            if let Some(base) = super_class {
                self.push_child(&mut items, base)?;
            }
            self.push_child(&mut items, implements)?;
            super_class =
                Some(self.ts_node(crate::TypeScriptKind::GenericDeclaration, start, &items)?);
            self.recycle_list(items);
        }
        if !self.at_char('{')? {
            return Err(self.error(
                self.cursor,
                ParseErrorKind::ExpectedPunctuator(Punctuator::LeftBrace),
            ));
        }
        self.classes
            .try_reserve(1)
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.classes.push(ClassScope::default());
        let body = self.within_braces(|parser| parser.parse_class_body(derived));
        let resolved = self.resolve_private_names();
        let body = body?;
        resolved?;
        let span = self.source_span(start, self.node_span(body).end())?;
        let kind = match placement {
            ClassPlacement::Declaration | ClassPlacement::ExportDefault => {
                SyntaxNodeKind::ClassDeclaration {
                    name,
                    super_class,
                    body,
                }
            }
            ClassPlacement::Expression => SyntaxNodeKind::ClassExpression {
                name,
                super_class,
                body,
            },
        };
        self.add_node(kind, span)
    }

    // Pops the innermost class scope and validates its private name references.
    fn resolve_private_names(&mut self) -> Result<(), ParseError> {
        let Some(class) = self.classes.pop() else {
            return Ok(());
        };
        let mut unresolved = None;
        for (name, offset) in class.references {
            if class.declared.contains_key(&name) {
                continue;
            }
            match self.classes.last_mut() {
                Some(outer) => {
                    outer.references.try_reserve(1).map_err(|_| ParseError {
                        offset,
                        kind: ParseErrorKind::AllocationFailed,
                    })?;
                    outer.references.push((name, offset));
                }
                None => {
                    unresolved = Some(unresolved.map_or(offset, |first: usize| first.min(offset)));
                }
            }
        }
        match unresolved {
            Some(offset) => {
                let kind = if self.goal == Goal::Unknown {
                    ParseErrorKind::PrivateIdentifierRequiresContext
                } else {
                    ParseErrorKind::UndeclaredPrivateName
                };
                Err(self.error(offset, kind))
            }
            None => Ok(()),
        }
    }

    fn parse_class_body(&mut self, derived: bool) -> Result<NodeId, ParseError> {
        let open = self.expect_punctuator(Punctuator::LeftBrace, self.goal_after_expression())?;
        let mut elements = self.take_list();
        let mut constructor_seen = false;
        loop {
            if self.at_char('}')? {
                break;
            }
            if self.at_end()? {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedClosingBrace));
            }
            if let Some(element) =
                self.parse_class_element(derived, &mut constructor_seen, false)?
            {
                self.push_child(&mut elements, element)?;
            }
        }
        let close = self.expect_punctuator(Punctuator::RightBrace, self.goal_after_expression())?;
        let elements = self.finish_list(elements)?;
        let span = self.source_span(open.span.start(), close.span.end())?;
        self.add_node(SyntaxNodeKind::ClassBody { elements }, span)
    }

    // `modifier_static` reports a `static` that the TypeScript modifiers
    // before this element consumed.
    fn parse_class_element(
        &mut self,
        derived: bool,
        constructor_seen: &mut bool,
        modifier_static: bool,
    ) -> Result<Option<NodeId>, ParseError> {
        self.skip_trivia()?;
        let start = self.cursor;
        if self.typescript {
            let mut modifiers = self.take_list();
            while self.at_char('@')? {
                let decorator = self.ts_decorator()?;
                self.push_child(&mut modifiers, decorator)?;
            }
            let is_static = self.ts_class_modifiers(&mut modifiers)? || modifier_static;
            if !modifiers.is_empty() {
                self.enter()?;
                let element = self.parse_class_element(derived, constructor_seen, is_static);
                self.leave();
                let element = element?.ok_or_else(|| {
                    self.error(self.cursor, ParseErrorKind::InvalidTypeScriptSyntax)
                })?;
                self.push_child(&mut modifiers, element)?;
                let result = self.ts_node(
                    crate::TypeScriptKind::ModifiedDeclaration,
                    start,
                    &modifiers,
                );
                self.recycle_list(modifiers);
                return result.map(Some);
            }
            self.recycle_list(modifiers);
            if self.ts_is_index_signature()? {
                return self.ts_type_member().map(Some);
            }
        }
        if self
            .consume_punctuator(Punctuator::Semicolon, self.goal_after_expression())?
            .is_some()
        {
            return Ok(None);
        }
        let mut is_static = modifier_static;
        if let Some(token) = self.peek_name()?
            && self.is_unescaped(token, "static")
        {
            let next = self.lookahead(|parser| {
                parser.consume_token(token.span)?;
                parser.element_after_static()
            })?;
            match next {
                AfterStatic::Block => {
                    self.consume_token(token.span)?;
                    return self.parse_static_block(start).map(Some);
                }
                AfterStatic::Element => {
                    self.consume_token(token.span)?;
                    is_static = true;
                }
                AfterStatic::Name => {}
            }
        }
        let mut kind = MethodKind::Method;
        if self
            .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
            .is_some()
        {
            kind = MethodKind::Generator;
        } else if let Some(token) = self.peek_name()?
            && token.kind == IdentifierKind::Name
            && !token.has_escape
            && let Some(modifier) = self.method_modifier(token)?
        {
            self.consume_token(token.span)?;
            kind = modifier;
            if modifier == MethodKind::Async
                && self
                    .consume_punctuator(Punctuator::Star, self.goal_for_expression_start())?
                    .is_some()
            {
                kind = MethodKind::AsyncGenerator;
            }
        }
        let mut key = self.parse_property_key(true)?;
        let private = matches!(
            self.node_kind(key.node),
            SyntaxNodeKind::PrivateIdentifier { .. }
        );
        if private && self.private_key_is(key, "constructor") {
            return Err(self.error(
                self.node_span(key.node).start(),
                ParseErrorKind::InvalidClassElementName,
            ));
        }
        if self.typescript {
            key.node = self.ts_typed_binding(key.node, true, true)?;
        }
        if self.at_punctuator(Punctuator::LeftParen)? || (self.typescript && self.at_char('<')?) {
            if !is_static && self.public_key_is(key, "constructor") {
                if kind != MethodKind::Method {
                    return Err(self.error(
                        self.node_span(key.node).start(),
                        ParseErrorKind::InvalidConstructor,
                    ));
                }
                if *constructor_seen && !self.typescript {
                    return Err(self.error(
                        self.node_span(key.node).start(),
                        ParseErrorKind::DuplicateConstructor,
                    ));
                }
                *constructor_seen = true;
                kind = MethodKind::Constructor;
            }
            if is_static && self.public_key_is(key, "prototype") {
                return Err(self.error(
                    self.node_span(key.node).start(),
                    ParseErrorKind::InvalidClassElementName,
                ));
            }
            if private {
                let private_kind = match kind {
                    MethodKind::Getter => PrivateKind::Getter,
                    MethodKind::Setter => PrivateKind::Setter,
                    MethodKind::Method
                    | MethodKind::Generator
                    | MethodKind::Async
                    | MethodKind::AsyncGenerator
                    | MethodKind::Constructor => PrivateKind::Method,
                };
                self.declare_private_name(key, private_kind, is_static)?;
            }
            return self
                .parse_method(start, key, kind, is_static, Some(MethodClass { derived }))
                .map(Some);
        }
        if kind != MethodKind::Method {
            return Err(self.error(
                self.cursor,
                ParseErrorKind::ExpectedPunctuator(Punctuator::LeftParen),
            ));
        }
        if self.public_key_is(key, "constructor")
            || (is_static && self.public_key_is(key, "prototype"))
        {
            return Err(self.error(
                self.node_span(key.node).start(),
                ParseErrorKind::InvalidClassElementName,
            ));
        }
        if private {
            self.declare_private_name(key, PrivateKind::Field, is_static)?;
        }
        self.parse_field(start, key, is_static).map(Some)
    }

    // Classifies what follows `static` in a class body.
    fn element_after_static(&mut self) -> Result<AfterStatic, ParseError> {
        if let Some(token) = self.peek_punctuator(self.goal_for_expression_start())? {
            if token.kind == Punctuator::LeftBrace {
                return Ok(AfterStatic::Block);
            }
            if matches!(token.kind, Punctuator::LeftBracket | Punctuator::Star) {
                return Ok(AfterStatic::Element);
            }
            return Ok(AfterStatic::Name);
        }
        let Some(ch) = self.peek_char()? else {
            return Ok(AfterStatic::Name);
        };
        if ch.is_ascii_digit()
            || matches!(ch, '.' | '\'' | '"')
            || self.identifier_can_start_here()?
        {
            return Ok(AfterStatic::Element);
        }
        Ok(AfterStatic::Name)
    }

    // Reports whether a non-computed, non-private key has the StringValue `name`.
    fn public_key_is(&self, key: PropertyKey, name: &str) -> bool {
        if key.computed {
            return false;
        }
        let node = self.ts_unwrap_binding(key.node);
        let spelling = self.slice(self.node_span(node));
        let kind = self.node_kind(node);
        if matches!(kind, SyntaxNodeKind::IdentifierName { .. }) {
            return identifier_equals(spelling, name);
        }
        kind == SyntaxNodeKind::StringLiteral && string_literal_equals(spelling, name)
    }

    fn private_key_is(&self, key: PropertyKey, name: &str) -> bool {
        let spelling = self.slice(self.node_span(self.ts_unwrap_binding(key.node)));
        spelling
            .strip_prefix('#')
            .is_some_and(|spelling| identifier_equals(spelling, name))
    }

    fn declare_private_name(
        &mut self,
        key: PropertyKey,
        kind: PrivateKind,
        is_static: bool,
    ) -> Result<(), ParseError> {
        let span = self.node_span(self.ts_unwrap_binding(key.node));
        let offset = span.start();
        let name = self.name_of(
            frameshift_core::Span::new(span.start() + 1, span.end())
                .map_err(|error| self.error(offset, ParseErrorKind::InternalInvariant(error)))?,
        );
        let duplicate = self.error(offset, ParseErrorKind::DuplicatePrivateName);
        let Some(class) = self.classes.last_mut() else {
            return Ok(());
        };
        match class.declared.get_mut(&name) {
            None => {
                class
                    .declared
                    .insert(name, PrivateDeclaration { kind, is_static });
                Ok(())
            }
            // A getter and a setter with the same placement may share a name.
            Some(existing)
                if existing.is_static == is_static
                    && matches!(
                        (existing.kind, kind),
                        (PrivateKind::Getter, PrivateKind::Setter)
                            | (PrivateKind::Setter, PrivateKind::Getter)
                    ) =>
            {
                existing.kind = PrivateKind::Accessor;
                Ok(())
            }
            Some(_) => Err(duplicate),
        }
    }

    // A field initializer is evaluated like a method body without parameters,
    // so `yield` and `await` are not keywords there. A static initializer
    // also reserves `await`, as a static block does; V8 enforces this, while
    // the specification's grammar states no such early error, so this is the
    // stricter, fail-closed reading.
    fn parse_field(
        &mut self,
        start: usize,
        key: PropertyKey,
        is_static: bool,
    ) -> Result<NodeId, ParseError> {
        let value = if self
            .consume_punctuator(Punctuator::Assign, self.goal_after_expression())?
            .is_some()
        {
            let context = Context {
                strict: Strictness::Strict,
                in_function: false,
                yield_keyword: false,
                await_keyword: false,
                await_reserved: is_static,
                in_parameters: false,
                new_target: Permission::Allowed,
                super_property: Permission::Allowed,
                super_call: Permission::Denied,
                arguments_forbidden: true,
            };
            let frame = self.enter_function(context);
            let value = self.parse_assignment_with_in();
            self.exit_function(frame);
            Some(value?)
        } else {
            None
        };
        self.consume_semicolon()?;
        let span = self.span_from(start)?;
        self.add_node(
            SyntaxNodeKind::FieldDefinition {
                key: key.node,
                value,
                is_static,
                computed: key.computed,
            },
            span,
        )
    }

    // `static { ... }` after the consumed `static`.
    fn parse_static_block(&mut self, start: usize) -> Result<NodeId, ParseError> {
        let context = Context {
            strict: Strictness::Strict,
            in_function: false,
            yield_keyword: false,
            await_keyword: false,
            await_reserved: true,
            in_parameters: false,
            new_target: Permission::Allowed,
            super_property: Permission::Allowed,
            super_call: Permission::Denied,
            arguments_forbidden: true,
        };
        let frame = self.enter_function(context);
        self.push_scope(ScopeKind::StaticBlock)?;
        let result = self.within_braces(|parser| {
            parser.expect_punctuator(Punctuator::LeftBrace, parser.goal_after_expression())?;
            let mut items = parser.take_list();
            parser.parse_statement_list(ListKind::StaticBlock, &mut items)?;
            parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            parser.finish_list(items)
        });
        self.pop_scope();
        self.exit_function(frame);
        let body = result?;
        let span = self.span_from(start)?;
        self.add_node(SyntaxNodeKind::StaticBlock { body }, span)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterStatic {
    Block,
    Element,
    Name,
}
