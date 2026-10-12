//! Reinterpretation of cover expressions as patterns, in place.
//!
//! Array and object literals, parenthesized lists before `=>`, and parameter
//! lists are parsed as expressions first. Once `=`, `=>`, or a declaration
//! decides their role, the same arena nodes change kind while keeping their
//! IDs and spans. Conversion fails closed: a node kind that is not a valid
//! target in the requested mode is rejected, so the helpers test the kinds
//! they accept with `if let` and reject everything else.

use frameshift_core::Span;

use super::{ParseError, ParseErrorKind, Parser, StrictModeError, scope::Declaration};
use crate::{
    ast::{AssignmentOperator, ChildList, NodeId, SyntaxNodeKind},
    value::identifier_chars,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TargetMode {
    // Destructuring assignment and for-in or for-of targets: references,
    // including member expressions.
    Assignment,
    // Arrow function parameters: names are declared.
    ArrowParameters,
    // Variable, function parameter, and catch bindings: names are declared.
    Binding,
}

impl TargetMode {
    fn invalid(self) -> ParseErrorKind {
        match self {
            Self::Assignment => ParseErrorKind::InvalidAssignmentTarget,
            Self::ArrowParameters => ParseErrorKind::InvalidArrowParameters,
            Self::Binding => ParseErrorKind::InvalidBindingTarget,
        }
    }

    fn binds(self) -> bool {
        match self {
            Self::Assignment => false,
            Self::ArrowParameters | Self::Binding => true,
        }
    }
}

impl<'source> Parser<'source> {
    pub(super) fn prepare_assignment_target(
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
    // is a valid target in sloppy mode only, where it fails at runtime.
    pub(super) fn check_simple_target(
        &self,
        id: NodeId,
        web_compat: bool,
    ) -> Result<(), ParseError> {
        let mut current = self.ts_unwrap_binding(id);
        while let SyntaxNodeKind::Parenthesized { expression } = self.node_kind(current) {
            current = expression;
        }
        let strict = self.context.strict;
        let kind = self.node_kind(current);
        if matches!(kind, SyntaxNodeKind::Identifier { .. }) {
            return self.check_restricted_name(self.node_span(current), strict);
        }
        if matches!(kind, SyntaxNodeKind::Member { .. }) && !self.is_optional_chain(current) {
            return Ok(());
        }
        if matches!(kind, SyntaxNodeKind::Call { .. })
            && web_compat
            && !self.is_optional_chain(current)
        {
            return self.strict_mode_error(
                self.node_span(id).start(),
                strict,
                StrictModeError::CallAssignmentTarget,
            );
        }
        Err(self.error(
            self.node_span(id).start(),
            ParseErrorKind::InvalidAssignmentTarget,
        ))
    }

    // Reinterprets a parenthesized cover list or parameter list. Bound names
    // are appended to `bound_names`.
    pub(super) fn convert_parameters(
        &mut self,
        parameters: &[NodeId],
        trailing_comma: bool,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        for (index, &parameter) in parameters.iter().enumerate() {
            let last = index + 1 == parameters.len();
            self.convert_list_element(parameter, last, trailing_comma, mode)?;
        }
        Ok(())
    }

    // Reports whether a converted parameter list is a simple list of names.
    pub(super) fn is_simple_parameter_list(&self, parameters: &[NodeId]) -> bool {
        parameters.iter().all(|&parameter| {
            matches!(
                self.node_kind(parameter),
                SyntaxNodeKind::BindingIdentifier { .. }
            )
        })
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
        let kind = self.node_kind(element);
        if kind == SyntaxNodeKind::Elision {
            return Ok(());
        }
        if let SyntaxNodeKind::Spread { argument } | SyntaxNodeKind::RestElement { argument } = kind
        {
            if !last || trailing_comma {
                return Err(self.error(
                    self.node_span(element).start(),
                    ParseErrorKind::InvalidRestElement,
                ));
            }
            self.convert_rest_argument(argument, false, mode)?;
            self.set_kind(element, SyntaxNodeKind::RestElement { argument });
            return Ok(());
        }
        self.convert_element(element, mode)
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
            if mode.binds() {
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
            if mode.binds() {
                self.convert_target(target, mode)?;
            }
            return Ok(());
        }
        self.convert_target(element, mode)
    }

    // Converts a target without a default. Patterns produced by an earlier
    // assignment conversion are walked again when they become bindings.
    pub(super) fn convert_target(
        &mut self,
        id: NodeId,
        mode: TargetMode,
    ) -> Result<(), ParseError> {
        let kind = self.node_kind(id);
        if let SyntaxNodeKind::Identifier { has_escape, .. } = kind {
            let span = self.node_span(id);
            if mode.binds() {
                self.set_kind(id, SyntaxNodeKind::BindingIdentifier { has_escape });
                return self.record_bound_name(span);
            }
            return self.check_restricted_name(span, self.context.strict);
        }
        if mode == TargetMode::Assignment {
            if matches!(kind, SyntaxNodeKind::Member { .. }) && !self.is_optional_chain(id) {
                return Ok(());
            }
            if matches!(kind, SyntaxNodeKind::Parenthesized { .. }) {
                return self.check_simple_target(id, false);
            }
        }
        if let SyntaxNodeKind::ArrayExpression {
            elements,
            trailing_comma,
        } = kind
        {
            self.convert_array_elements(elements, trailing_comma, mode)?;
            self.set_kind(
                id,
                SyntaxNodeKind::ArrayPattern {
                    elements,
                    trailing_comma,
                },
            );
            return Ok(());
        }
        if let SyntaxNodeKind::ObjectExpression {
            properties,
            trailing_comma,
        } = kind
        {
            self.convert_object_properties(properties, trailing_comma, mode)?;
            self.set_kind(
                id,
                SyntaxNodeKind::ObjectPattern {
                    properties,
                    trailing_comma,
                },
            );
            return Ok(());
        }
        // An earlier assignment conversion produced this pattern; bindings
        // walk it again to declare its names.
        if let SyntaxNodeKind::ArrayPattern {
            elements,
            trailing_comma,
        } = kind
        {
            if mode.binds() {
                return self.convert_array_elements(elements, trailing_comma, mode);
            }
            return Ok(());
        }
        if let SyntaxNodeKind::ObjectPattern {
            properties,
            trailing_comma,
        } = kind
        {
            if mode.binds() {
                return self.convert_object_properties(properties, trailing_comma, mode);
            }
            return Ok(());
        }
        Err(self.error(self.node_span(id).start(), mode.invalid()))
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
            let kind = self.node_kind(property);
            if let SyntaxNodeKind::Property {
                key,
                value,
                computed,
            } = kind
            {
                self.convert_element(value, mode)?;
                self.set_kind(
                    property,
                    SyntaxNodeKind::PatternProperty {
                        key,
                        value,
                        computed,
                    },
                );
            } else if let SyntaxNodeKind::PatternProperty { value, .. } = kind {
                self.convert_element(value, mode)?;
            } else if let SyntaxNodeKind::ShorthandProperty { identifier } = kind {
                self.convert_target(identifier, mode)?;
                self.set_kind(
                    property,
                    SyntaxNodeKind::ShorthandPatternProperty {
                        target: identifier,
                        default: None,
                    },
                );
            } else if let SyntaxNodeKind::ShorthandPatternProperty { target, .. } = kind {
                self.convert_target(target, mode)?;
            } else if let SyntaxNodeKind::Spread { argument }
            | SyntaxNodeKind::RestElement { argument } = kind
            {
                if index + 1 != last || trailing_comma {
                    return Err(self.error(
                        self.node_span(property).start(),
                        ParseErrorKind::InvalidRestElement,
                    ));
                }
                self.convert_rest_argument(argument, true, mode)?;
                self.set_kind(property, SyntaxNodeKind::RestElement { argument });
            } else {
                // Methods cannot be destructuring targets.
                return Err(self.error(self.node_span(property).start(), mode.invalid()));
            }
        }
        Ok(())
    }

    pub(super) fn record_bound_name(&mut self, span: Span) -> Result<(), ParseError> {
        self.bound_names
            .try_reserve(1)
            .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
        self.bound_names.push(span);
        Ok(())
    }

    // Returns the offset of the earliest second declaration of a name among
    // `bound_names[from..]`, comparing StringValues so that escaped and plain
    // spellings collide.
    pub(super) fn first_duplicate_bound_name(
        &self,
        from: usize,
    ) -> Result<Option<usize>, ParseError> {
        let names = &self.bound_names[from..];
        if names.len() < 2 {
            return Ok(None);
        }
        let mut order = Vec::new();
        order
            .try_reserve_exact(names.len())
            .map_err(|_| self.error(names[0].start(), ParseErrorKind::AllocationFailed))?;
        order.extend(0..names.len());
        let value = |index: usize| identifier_chars(self.slice(names[index]));
        // Sorting keeps the check O(n log n) for hostile parameter counts.
        order.sort_unstable_by(|&a, &b| value(a).cmp(value(b)).then(a.cmp(&b)));
        Ok(order
            .windows(2)
            .filter(|pair| value(pair[0]).eq(value(pair[1])))
            .map(|pair| names[pair[1]].start())
            .min())
    }

    // Validates the names bound by a declaration of known strictness, from
    // `bound_names[from..]`, and declares them in the current scope.
    pub(super) fn declare_bound_names(
        &mut self,
        from: usize,
        declaration: Declaration,
        lexical: bool,
    ) -> Result<(), ParseError> {
        let strict = self.context.strict;
        for index in from..self.bound_names.len() {
            let span = self.bound_names[index];
            self.check_restricted_name(span, strict)?;
            let name = self.name_of(span);
            if lexical && name == "let" {
                return Err(self.error(span.start(), ParseErrorKind::LetBindingName));
            }
            self.declare(name, span.start(), declaration)?;
        }
        Ok(())
    }

    pub(super) fn is_optional_chain(&self, id: NodeId) -> bool {
        let mut current = id;
        loop {
            let kind = self.node_kind(current);
            if let SyntaxNodeKind::Member {
                optional, object, ..
            } = kind
            {
                if optional {
                    return true;
                }
                current = object;
            } else if let SyntaxNodeKind::Call {
                optional, callee, ..
            } = kind
            {
                if optional {
                    return true;
                }
                current = callee;
            } else if let SyntaxNodeKind::TypeScript {
                kind: crate::TypeScriptKind::NonNull | crate::TypeScriptKind::Instantiation,
                children,
            } = kind
            {
                current = self.children[children.range().start];
            } else {
                return false;
            }
        }
    }
}
