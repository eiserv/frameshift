//! Functions, arrow functions, and methods: their context boundaries,
//! parameter lists, and bodies.
//!
//! Parameter names are scanned before the body's directive prologue decides
//! whether the function is strict, so the strict-mode rules for the function
//! name and its parameters run after the prologue.

use frameshift_core::Span;

use super::{
    Context, CoverMarks, JumpTargets, ParseError, ParseErrorKind, Parser, Permission, Strictness,
    expression::{CoverList, PropertyKey},
    pattern::TargetMode,
    scope::{Declaration, ScopeKind},
};
use crate::{
    ast::{FunctionKind, MethodKind, NodeId, SyntaxNodeKind},
    identifier::{IdentifierKind, IdentifierToken},
    punctuator::Punctuator,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FunctionPlacement {
    // A declaration in a statement list; the name is required.
    Declaration,
    // A function expression; the name is optional and local to the function.
    Expression,
    // `export default function`; the name is optional.
    ExportDefault,
}

// How a parameter list is validated.
#[derive(Clone, Copy)]
struct ParameterRules {
    // Arrow functions and methods never permit duplicate names.
    unique: bool,
    accessor: Option<MethodKind>,
}

// A parsed parameter list awaiting the body's strictness.
struct Parameters {
    node: NodeId,
    // Start of this list's names in `bound_names`.
    mark: usize,
    simple: bool,
    rules: ParameterRules,
}

// Parser state replaced at a function boundary and restored afterwards.
pub(super) struct FunctionFrame<'source> {
    context: Context,
    jumps: JumpTargets<'source>,
    cover: CoverMarks,
}

/// Placement of a method inside a class, if any.
#[derive(Clone, Copy)]
pub(super) struct MethodClass {
    pub(super) derived: bool,
}

impl<'source> Parser<'source> {
    pub(super) fn enter_function(&mut self, context: Context) -> FunctionFrame<'source> {
        FunctionFrame {
            context: std::mem::replace(&mut self.context, context),
            jumps: std::mem::take(&mut self.jumps),
            cover: std::mem::take(&mut self.cover),
        }
    }

    pub(super) fn exit_function(&mut self, frame: FunctionFrame<'source>) {
        self.context = frame.context;
        self.jumps = frame.jumps;
        self.cover = frame.cover;
    }

    // Parses a function after `function`, `*`, and `async`, starting at the
    // optional name.
    pub(super) fn parse_function(
        &mut self,
        start: usize,
        kind: FunctionKind,
        placement: FunctionPlacement,
    ) -> Result<NodeId, ParseError> {
        self.enter()?;
        let result = self.parse_function_inner(start, kind, placement);
        self.leave();
        result
    }

    fn parse_function_inner(
        &mut self,
        start: usize,
        kind: FunctionKind,
        placement: FunctionPlacement,
    ) -> Result<NodeId, ParseError> {
        let name = if self.identifier_can_start_here()? {
            let token = self.scan_identifier()?;
            if token.kind == IdentifierKind::PrivateName {
                return Err(self.error(token.span.start(), ParseErrorKind::UnexpectedPrivateName));
            }
            match placement {
                FunctionPlacement::Declaration | FunctionPlacement::ExportDefault => {
                    self.check_name(token)?;
                    let name = self.name_of(token.span);
                    self.declare(
                        name,
                        token.span.start(),
                        Declaration::Function {
                            plain: kind == FunctionKind::Normal,
                        },
                    )?;
                }
                // An expression's name follows the function's own `yield`
                // and `await` rules.
                FunctionPlacement::Expression => {
                    let saved = self.context;
                    self.context.yield_keyword = kind.is_generator();
                    self.context.await_keyword = kind.is_async();
                    self.context.await_reserved = false;
                    let checked = self.check_name(token);
                    self.context = saved;
                    checked?;
                }
            }
            Some(token)
        } else {
            if placement == FunctionPlacement::Declaration {
                return Err(self.error(self.cursor, ParseErrorKind::ExpectedIdentifier));
            }
            None
        };
        let name_node = match name {
            Some(token) => Some(self.add_node(
                SyntaxNodeKind::BindingIdentifier {
                    has_escape: token.has_escape,
                },
                token.span,
            )?),
            None => None,
        };
        let context = Context {
            strict: self.context.strict,
            in_function: true,
            yield_keyword: kind.is_generator(),
            await_keyword: kind.is_async(),
            await_reserved: false,
            in_parameters: true,
            new_target: Permission::Allowed,
            super_property: Permission::Denied,
            super_call: Permission::Denied,
            arguments_forbidden: false,
        };
        let frame = self.enter_function(context);
        // TypeScript overloads and ambient declarations omit the body; a
        // function expression always has one, as in the pinned compiler.
        let result = self.parse_parameters_and_body(
            name.map(|token| token.span),
            ParameterRules {
                unique: false,
                accessor: None,
            },
            placement != FunctionPlacement::Expression,
        );
        self.exit_function(frame);
        let (parameters, body) = result?;
        let span = self.source_span(start, self.node_span(body).end())?;
        let node = match placement {
            FunctionPlacement::Declaration | FunctionPlacement::ExportDefault => {
                SyntaxNodeKind::FunctionDeclaration {
                    name: name_node,
                    parameters,
                    body,
                    kind,
                }
            }
            FunctionPlacement::Expression => SyntaxNodeKind::FunctionExpression {
                name: name_node,
                parameters,
                body,
                kind,
            },
        };
        self.add_node(node, span)
    }

    // Parses an object literal or class method after its key. The current
    // context must already be the class's when `class` is present.
    pub(super) fn parse_method(
        &mut self,
        start: usize,
        key: PropertyKey,
        kind: MethodKind,
        is_static: bool,
        class: Option<MethodClass>,
    ) -> Result<NodeId, ParseError> {
        self.enter()?;
        let function = kind.function_kind();
        let derived = class.is_some_and(|class| class.derived);
        let context = Context {
            strict: self.context.strict,
            in_function: true,
            yield_keyword: function.is_generator(),
            await_keyword: function.is_async(),
            await_reserved: false,
            in_parameters: true,
            new_target: Permission::Allowed,
            super_property: Permission::Allowed,
            super_call: if kind == MethodKind::Constructor && derived {
                Permission::Allowed
            } else {
                Permission::Denied
            },
            arguments_forbidden: false,
        };
        let frame = self.enter_function(context);
        let accessor = match kind {
            MethodKind::Getter | MethodKind::Setter => Some(kind),
            MethodKind::Method
            | MethodKind::Generator
            | MethodKind::Async
            | MethodKind::AsyncGenerator
            | MethodKind::Constructor => None,
        };
        let result = self.parse_parameters_and_body(
            None,
            ParameterRules {
                unique: true,
                accessor,
            },
            true,
        );
        self.exit_function(frame);
        self.leave();
        let (parameters, body) = result?;
        let span = self.source_span(start, self.node_span(body).end())?;

        self.add_node(
            SyntaxNodeKind::MethodDefinition {
                key: key.node,
                parameters,
                body,
                kind,
                is_static,
                computed: key.computed,
            },
            span,
        )
    }

    // Parses `( parameters ) { body }` in the function's context and scope.
    // In the TypeScript dialects, `bodyless` permits a signature without a
    // body.
    fn parse_parameters_and_body(
        &mut self,
        name: Option<Span>,
        rules: ParameterRules,
        bodyless: bool,
    ) -> Result<(NodeId, NodeId), ParseError> {
        self.push_scope(ScopeKind::Function)?;
        let result = self.parse_parameters_and_body_in_scope(name, rules, bodyless);
        self.pop_scope();
        result
    }

    fn parse_parameters_and_body_in_scope(
        &mut self,
        name: Option<Span>,
        rules: ParameterRules,
        bodyless: bool,
    ) -> Result<(NodeId, NodeId), ParseError> {
        let generics = if self.typescript {
            self.ts_type_parameters()?
        } else {
            None
        };
        let mut parameters = self.parse_formal_parameters(rules)?;
        if let Some(generics) = generics {
            parameters.node = self.ts_node(
                crate::TypeScriptKind::GenericDeclaration,
                self.node_span(generics).start(),
                &[generics, parameters.node],
            )?;
        }
        let annotation = if self.typescript {
            self.ts_annotation()?
        } else {
            None
        };
        self.context.in_parameters = false;
        if self.typescript && bodyless && !self.at_char('{')? {
            // Without a return type, the node spans the semicolon, or nothing
            // where a semicolon is inserted.
            let start = match annotation {
                Some(annotation) => self.node_span(annotation).start(),
                None if self.at_char(';')? => self.cursor,
                None => self.last_end,
            };
            self.consume_semicolon()?;
            self.bound_names.truncate(parameters.mark);
            let items = annotation.into_iter().collect::<Vec<_>>();
            let body = self.ts_node(crate::TypeScriptKind::BodylessFunction, start, &items)?;
            return Ok((parameters.node, body));
        }
        if !self.at_punctuator(Punctuator::LeftBrace)? {
            return Err(self.error(
                self.cursor,
                ParseErrorKind::ExpectedPunctuator(Punctuator::LeftBrace),
            ));
        }
        let mut body = self.parse_function_body(name, &parameters)?;
        if let Some(annotation) = annotation {
            body = self.ts_node(
                crate::TypeScriptKind::GenericDeclaration,
                self.node_span(annotation).start(),
                &[annotation, body],
            )?;
        }
        Ok((parameters.node, body))
    }

    fn parse_formal_parameters(&mut self, rules: ParameterRules) -> Result<Parameters, ParseError> {
        if self.typescript {
            let mark = self.bound_names.len();
            let node = self.ts_parameters(true)?;
            self.declare_parameters(mark)?;
            return Ok(Parameters {
                node,
                mark,
                simple: true,
                rules,
            });
        }
        let list = self.parse_cover_list(Punctuator::LeftParen)?;
        let valid_arity = match rules.accessor {
            Some(MethodKind::Getter) => list.elements.is_empty(),
            Some(MethodKind::Setter) => {
                list.elements.len() == 1
                    && !list.trailing_comma
                    && !matches!(
                        self.node_kind(list.elements[0]),
                        SyntaxNodeKind::Spread { .. }
                    )
            }
            Some(
                MethodKind::Method
                | MethodKind::Generator
                | MethodKind::Async
                | MethodKind::AsyncGenerator
                | MethodKind::Constructor,
            )
            | None => true,
        };
        if !valid_arity {
            return Err(self.error(list.open.start(), ParseErrorKind::InvalidAccessorParameters));
        }
        let parameters = self.finish_parameters(&list, TargetMode::Binding, rules);
        self.recycle_list(list.elements);
        parameters
    }

    // Converts a cover list to parameters, adds the node, and declares the names.
    fn finish_parameters(
        &mut self,
        list: &CoverList,
        mode: TargetMode,
        rules: ParameterRules,
    ) -> Result<Parameters, ParseError> {
        let mark = self.bound_names.len();
        self.convert_parameters(&list.elements, list.trailing_comma, mode)?;
        let simple = self.is_simple_parameter_list(&list.elements);
        let children = self.append_children(&list.elements)?;
        let span = self.source_span(list.open.start(), list.close.end())?;
        let node = self.add_node(
            SyntaxNodeKind::FormalParameters {
                parameters: children,
            },
            span,
        )?;
        self.declare_parameters(mark)?;
        Ok(Parameters {
            node,
            mark,
            simple,
            rules,
        })
    }

    fn declare_parameters(&mut self, mark: usize) -> Result<(), ParseError> {
        for index in mark..self.bound_names.len() {
            let span = self.bound_names[index];
            let name = self.name_of(span);
            self.declare(name, span.start(), Declaration::Parameter)?;
        }
        Ok(())
    }

    // Parses a `{ ... }` function body with its directive prologue, then
    // applies the strict-mode rules to the function name and parameters.
    fn parse_function_body(
        &mut self,
        name: Option<Span>,
        parameters: &Parameters,
    ) -> Result<NodeId, ParseError> {
        self.within_braces(|parser| {
            let open = parser
                .expect_punctuator(Punctuator::LeftBrace, parser.goal_for_expression_start())?;
            let mut items = parser.take_list();
            let use_strict = parser.parse_directive_prologue(&mut items)?;
            if let Some(offset) = use_strict
                && !parameters.simple
            {
                return Err(parser.error(offset, ParseErrorKind::UseStrictWithNonSimpleParameters));
            }
            parser.validate_function_names(name, parameters)?;
            parser.parse_statement_list(super::statement::ListKind::FunctionBody, &mut items)?;
            let close =
                parser.expect_punctuator(Punctuator::RightBrace, parser.goal_after_expression())?;
            let body = parser.finish_list(items)?;
            let span = parser.source_span(open.span.start(), close.span.end())?;
            parser.add_node(
                SyntaxNodeKind::FunctionBody {
                    body,
                    strict: parser.context.strict == Strictness::Strict,
                },
                span,
            )
        })
    }

    // Applies the rules that depend on the function's final strictness.
    fn validate_function_names(
        &mut self,
        name: Option<Span>,
        parameters: &Parameters,
    ) -> Result<(), ParseError> {
        let strict = self.context.strict;
        if let Some(name) = name {
            self.check_binding_strictness(name, strict)?;
        }
        for index in parameters.mark..self.bound_names.len() {
            self.check_binding_strictness(self.bound_names[index], strict)?;
        }
        if !self.typescript
            && let Some(offset) = self.first_duplicate_bound_name(parameters.mark)?
        {
            if !parameters.simple || parameters.rules.unique {
                return Err(self.error(offset, ParseErrorKind::DuplicateParameter));
            }
            match strict {
                Strictness::Sloppy => {}
                Strictness::Strict => {
                    return Err(self.error(offset, ParseErrorKind::DuplicateParameter));
                }
                Strictness::Unknown => {
                    return Err(self.error(offset, ParseErrorKind::StrictModeRequiresContext));
                }
            }
        }
        self.bound_names.truncate(parameters.mark);
        Ok(())
    }

    fn arrow_context(&self, is_async: bool) -> Context {
        Context {
            in_function: true,
            yield_keyword: false,
            await_keyword: is_async,
            // An arrow body inside a static block may use `await` as a name.
            await_reserved: false,
            in_parameters: false,
            ..self.context
        }
    }

    pub(super) fn parse_ts_arrow_body(
        &mut self,
        start: usize,
        node: NodeId,
        mark: usize,
        is_async: bool,
    ) -> Result<NodeId, ParseError> {
        self.enter()?;
        let frame = self.enter_function(self.arrow_context(is_async));
        let result = self.with_arrow_scope(|parser| {
            parser.declare_parameters(mark)?;
            let parameters = Parameters {
                node,
                mark,
                simple: true,
                rules: ParameterRules {
                    unique: true,
                    accessor: None,
                },
            };
            parser.parse_arrow_body(start, &parameters, is_async)
        });
        self.exit_function(frame);
        self.leave();
        result
    }

    // `x => body` or `async x => body`, with the name already consumed.
    pub(super) fn parse_arrow_from_identifier(
        &mut self,
        start: usize,
        token: IdentifierToken,
        is_async: bool,
    ) -> Result<NodeId, ParseError> {
        let parameter = self.add_node(
            SyntaxNodeKind::BindingIdentifier {
                has_escape: token.has_escape,
            },
            token.span,
        )?;
        let children = self.append_children(&[parameter])?;
        let node = self.add_node(
            SyntaxNodeKind::FormalParameters {
                parameters: children,
            },
            token.span,
        )?;
        self.enter()?;
        let frame = self.enter_function(self.arrow_context(is_async));
        let result = self.with_arrow_scope(|parser| {
            let mark = parser.bound_names.len();
            parser.record_bound_name(token.span)?;
            parser.declare_parameters(mark)?;
            let parameters = Parameters {
                node,
                mark,
                simple: true,
                rules: ParameterRules {
                    unique: true,
                    accessor: None,
                },
            };
            parser.parse_arrow_body(start, &parameters, is_async)
        });
        self.exit_function(frame);
        self.leave();
        result
    }

    // `(...) => body` or `async (...) => body` from a parsed cover list.
    pub(super) fn parse_arrow_from_cover(
        &mut self,
        start: usize,
        list: CoverList,
        is_async: bool,
    ) -> Result<NodeId, ParseError> {
        self.enter()?;
        let frame = self.enter_function(self.arrow_context(is_async));
        let result = self.with_arrow_scope(move |parser| {
            let parameters = parser.finish_parameters(
                &list,
                TargetMode::ArrowParameters,
                ParameterRules {
                    unique: true,
                    accessor: None,
                },
            );
            parser.recycle_list(list.elements);
            parser.parse_arrow_body(start, &parameters?, is_async)
        });
        self.exit_function(frame);
        self.leave();
        result
    }

    fn with_arrow_scope(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<NodeId, ParseError>,
    ) -> Result<NodeId, ParseError> {
        self.push_scope(ScopeKind::Function)?;
        let result = parse(self);
        self.pop_scope();
        result
    }

    // Consumes `=>` and parses a block or concise body.
    fn parse_arrow_body(
        &mut self,
        start: usize,
        parameters: &Parameters,
        is_async: bool,
    ) -> Result<NodeId, ParseError> {
        self.expect_punctuator(Punctuator::Arrow, self.goal_after_expression())?;
        let body = if self.at_char('{')? {
            self.parse_function_body(None, parameters)?
        } else {
            self.validate_function_names(None, parameters)?;
            self.parse_expression(2)?
        };
        let span = self.source_span(start, self.node_span(body).end())?;
        self.add_node(
            SyntaxNodeKind::ArrowFunction {
                parameters: parameters.node,
                body,
                is_async,
            },
            span,
        )
    }
}
