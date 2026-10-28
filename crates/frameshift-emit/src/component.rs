//! Conversion of React function components into Astro components.
//!
//! A function whose body is a sequence of declarations and expression
//! statements followed by a single `return` of markup becomes frontmatter
//! and a template: the statements are copied in order, and the markup is
//! converted by [`convert_jsx`](crate::convert_jsx)'s rules. Everything the
//! frontmatter cannot reproduce exactly fails closed with a typed error. The
//! conversion knows the syntax only; which parameter reads become
//! `Astro.params`, and whether every name used is available to the
//! generated component, are the caller's facts.

use frameshift_core::Span;
use frameshift_syntax::{NodeId, SyntaxNodeKind, SyntaxTree, TypeScriptKind, VariableKind};

use crate::astro::{AstroComponent, Expression, ExpressionPiece, FrontmatterItem};
use crate::jsx::{
    Assumption, ConvertError, ConvertErrorKind, ElementResolver, JsxConversion, convert_markup,
};
use crate::output::{Anchor, SourceId};

/// How a converted component receives its parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropsSource {
    /// The parameter's object pattern is copied into
    /// `const pattern = Astro.props;`.
    AstroProps,
    /// The parameter holds framework values, such as a page's route
    /// parameters, that the caller replaces through
    /// [`ComponentConversion::astro_params`]. The parameter is dropped, so
    /// its pattern may not evaluate a default value or a computed key, and
    /// no name it declares may occur outside a replaced span.
    Dropped,
}

/// Options of one component conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentConversion<'a> {
    /// How the component receives its parameter.
    pub props: PropsSource,
    /// Disjoint spans of expressions inside the function body whose source
    /// is replaced by `Astro.params`, in any order. Every span must be
    /// reached by a copied statement or expression.
    pub astro_params: &'a [Span],
    /// Maximum nesting of elements and expressions. Defaults to 256.
    pub max_depth: usize,
}

impl Default for ComponentConversion<'_> {
    fn default() -> Self {
        Self {
            props: PropsSource::AstroProps,
            astro_params: &[],
            max_depth: 256,
        }
    }
}

/// A converted component and the assumptions it depends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvertedComponent {
    /// The component, without imports, which the caller adds.
    pub component: AstroComponent,
    /// Assumptions of the converted markup, in source order.
    pub assumptions: Vec<Assumption>,
    /// Source spans that the component copies verbatim, in source order.
    /// Every comment inside the function lies in one of them.
    pub copied: Vec<Span>,
}

/// Converts the function component at `function` into an Astro component.
///
/// The function is a declaration, expression, or arrow function, not a
/// generator, with no parameter or one object pattern parameter, optionally
/// with a TypeScript annotation. Its body is a concise markup expression, or
/// statements followed by a final `return` of markup. Each statement before
/// it is a `var`, `let`, or `const` declaration or an expression statement
/// without markup and without a `return` outside nested functions; each
/// becomes a frontmatter statement, and comments between them are copied
/// as their own frontmatter lines. A `children` property of the pattern is
/// the children binding of the markup and may not occur in the statements.
///
/// The whole function fails on `this`, `super`, `arguments`, `new.target`,
/// `import.meta`, the name `Astro`, the function's own name, and calls of
/// functions named like React hooks, wherever they occur, and on any comment
/// that the component would drop. These checks are conservative: they read spellings, never scopes.
pub fn convert_component(
    tree: &SyntaxTree<'_>,
    source: SourceId,
    function: NodeId,
    conversion: &ComponentConversion<'_>,
    resolver: &mut dyn ElementResolver,
) -> Result<ConvertedComponent, ConvertError> {
    let checker = Checker { tree, source };
    let function_span = checker.span(function)?;
    let Some((parameters, body)) = checker.function_parts(function)? else {
        return Err(checker.error(ConvertErrorKind::UnsupportedFunction, function_span));
    };
    let pattern = checker.pattern(parameters)?;
    // The statement area runs from the body's opening brace to the `return`.
    let (statements, root, statement_area) =
        if let SyntaxNodeKind::FunctionBody { body: list, .. } = checker.kind(body)? {
            let items = checker.list(list)?;
            let body_span = checker.span(body)?;
            let unsupported = checker.error(ConvertErrorKind::UnsupportedBody, body_span);
            let Some((&last, rest)) = items.split_last() else {
                return Err(unsupported);
            };
            let SyntaxNodeKind::ReturnStatement {
                argument: Some(argument),
            } = checker.kind(last)?
            else {
                return Err(unsupported);
            };
            let area = checker.between(body_span.start(), checker.span(last)?.start())?;
            (rest.to_vec(), argument, Some(area))
        } else {
            (Vec::new(), body, None)
        };
    let root_span = checker.span(root)?;
    let root_kind = checker.kind(checker.unparenthesize(root)?)?;
    if !matches!(
        root_kind,
        SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
    ) {
        return Err(checker.error(ConvertErrorKind::NotJsx, root_span));
    }

    let mut substitutions = conversion.astro_params.to_vec();
    substitutions.sort_by_key(|span| (span.start(), span.end()));
    let mut statement_spans = Vec::with_capacity(statements.len());
    for &statement in &statements {
        statement_spans.push(checker.span(statement)?);
    }
    for (index, span) in substitutions.iter().enumerate() {
        let overlaps = index > 0 && substitutions[index - 1].end() > span.start();
        let placed = statement_spans
            .iter()
            .chain([&root_span])
            .any(|outer| outer.start() <= span.start() && span.end() <= outer.end());
        if overlaps || !placed {
            return Err(checker.error(ConvertErrorKind::UnsupportedSubstitution, *span));
        }
    }

    let children = match pattern {
        Some(pattern) => checker.children_binding(pattern)?,
        None => None,
    };
    let dropped = match (pattern, conversion.props) {
        (Some(pattern), PropsSource::Dropped) => {
            checker.check_inert(pattern)?;
            checker.declared_names(pattern)?
        }
        (Some(_), PropsSource::AstroProps) | (None, _) => Vec::new(),
    };
    // The function's own binding does not exist in the Astro component.
    let mut dropped = dropped;
    let own_name = checker.own_name(function)?;
    if let Some(name) = own_name {
        dropped.push(
            checker
                .tree
                .identifier_value(name)
                .ok_or_else(|| checker.invalid())?,
        );
    }
    checker.check_function(function, own_name, &substitutions, &dropped)?;
    for &statement in &statements {
        checker.check_statement(statement, children.as_deref())?;
    }

    let (markup, mut reached) = convert_markup(
        tree,
        source,
        root,
        &JsxConversion {
            children_binding: children.as_deref(),
            max_depth: conversion.max_depth,
        },
        resolver,
        &substitutions,
    )?;

    let mut frontmatter = Vec::new();
    let mut copied = markup.copied;
    if let (Some(pattern), PropsSource::AstroProps) = (pattern, conversion.props) {
        let span = checker.span(pattern)?;
        frontmatter.push(FrontmatterItem::PropsPattern(checker.anchor(span)));
        copied.push(span);
    }
    // Comments between the body's opening brace and the `return` that lie
    // outside every statement become their own lines. Every other comment
    // must lie in a copied span; the markup conversion has checked the
    // comments of the root itself.
    let mut loose = Vec::new();
    for comment in tree.comments() {
        let span = comment.span;
        if span.start() < function_span.start() || span.end() > function_span.end() {
            continue;
        }
        let within = |outer: &Span| outer.start() <= span.start() && span.end() <= outer.end();
        let pattern_copied = matches!(conversion.props, PropsSource::AstroProps)
            && pattern.is_some_and(|pattern| checker.span(pattern).is_ok_and(|p| within(&p)));
        if substitutions.iter().any(within) {
            return Err(checker.error(ConvertErrorKind::CommentInMarkup, span));
        }
        if within(&root_span) || statement_spans.iter().any(within) || pattern_copied {
            continue;
        }
        if statement_area.is_some_and(|area| within(&area)) {
            loose.push(span);
        } else {
            return Err(checker.error(ConvertErrorKind::CommentInMarkup, span));
        }
    }
    let mut loose = loose.into_iter().peekable();
    for (&statement, &span) in statements.iter().zip(&statement_spans) {
        while let Some(comment) = loose.next_if(|comment| comment.end() <= span.start()) {
            frontmatter.push(FrontmatterItem::Copied(checker.anchor(comment)));
            copied.push(comment);
        }
        let mut pieces = Vec::new();
        let mut offset = span.start();
        for &substitution in substitutions
            .iter()
            .filter(|s| span.start() <= s.start() && s.end() <= span.end())
        {
            if substitution.start() > offset {
                let gap = checker.between(offset, substitution.start())?;
                pieces.push(ExpressionPiece::Source(gap));
                copied.push(gap);
            }
            pieces.push(ExpressionPiece::AstroParams);
            reached.push(substitution);
            offset = substitution.end();
        }
        if span.end() > offset {
            let gap = checker.between(offset, span.end())?;
            pieces.push(ExpressionPiece::Source(gap));
            copied.push(gap);
        }
        frontmatter.push(FrontmatterItem::Statement(Expression {
            pieces,
            anchor: checker.anchor(checker.span(statement)?),
        }));
    }
    for comment in loose {
        frontmatter.push(FrontmatterItem::Copied(checker.anchor(comment)));
        copied.push(comment);
    }
    copied.sort_by_key(|span| (span.start(), span.end()));

    reached.sort_by_key(|span| (span.start(), span.end()));
    if reached != substitutions {
        let missed = substitutions
            .iter()
            .find(|span| {
                reached
                    .binary_search_by_key(&(span.start(), span.end()), |s| (s.start(), s.end()))
                    .is_err()
            })
            .copied()
            .unwrap_or(function_span);
        return Err(checker.error(ConvertErrorKind::UnsupportedSubstitution, missed));
    }
    Ok(ConvertedComponent {
        component: AstroComponent {
            frontmatter,
            template: markup.nodes,
        },
        assumptions: markup.assumptions,
        copied,
    })
}

struct Checker<'t> {
    tree: &'t SyntaxTree<'t>,
    source: SourceId,
}

impl Checker<'_> {
    fn anchor(&self, span: Span) -> Anchor {
        Anchor {
            source: self.source,
            span,
        }
    }

    fn error(&self, kind: ConvertErrorKind, span: Span) -> ConvertError {
        ConvertError {
            kind,
            anchor: self.anchor(span),
        }
    }

    fn invalid(&self) -> ConvertError {
        self.error(ConvertErrorKind::NotJsx, self.tree.consumed_span())
    }

    fn span(&self, id: NodeId) -> Result<Span, ConvertError> {
        self.tree
            .node(id)
            .map(|node| node.span())
            .ok_or_else(|| self.invalid())
    }

    fn kind(&self, id: NodeId) -> Result<SyntaxNodeKind, ConvertError> {
        self.tree
            .node(id)
            .map(|node| node.kind())
            .ok_or_else(|| self.invalid())
    }

    fn list(&self, list: frameshift_syntax::ChildList) -> Result<Vec<NodeId>, ConvertError> {
        self.tree
            .children(list)
            .map(<[NodeId]>::to_vec)
            .ok_or_else(|| self.invalid())
    }

    fn between(&self, start: usize, end: usize) -> Result<Span, ConvertError> {
        Span::new(start, end).map_err(|_| self.invalid())
    }

    fn unparenthesize(&self, mut id: NodeId) -> Result<NodeId, ConvertError> {
        while let SyntaxNodeKind::Parenthesized { expression } = self.kind(id)? {
            id = expression;
        }
        Ok(id)
    }

    // The object pattern of the single parameter, if there is one.
    fn pattern(&self, parameters: NodeId) -> Result<Option<NodeId>, ConvertError> {
        let span = self.span(parameters)?;
        let unsupported = self.error(ConvertErrorKind::UnsupportedFunction, span);
        let items: Vec<NodeId> = self
            .tree
            .child_nodes(parameters)
            .ok_or_else(|| self.invalid())?
            .collect();
        let parameter = match items.as_slice() {
            [] => return Ok(None),
            [parameter] => *parameter,
            _ => return Err(unsupported),
        };
        let parameter = if let SyntaxNodeKind::TypeScript {
            kind:
                TypeScriptKind::Parameter {
                    optional: false,
                    rest: false,
                },
            children,
        } = self.kind(parameter)?
        {
            *self.list(children)?.first().ok_or(unsupported)?
        } else {
            parameter
        };
        if matches!(self.kind(parameter)?, SyntaxNodeKind::ObjectPattern { .. }) {
            Ok(Some(parameter))
        } else {
            Err(unsupported)
        }
    }

    // The binding of a top-level `children` property, as `{ children }` or
    // `{ children: name }`.
    fn children_binding(&self, pattern: NodeId) -> Result<Option<String>, ConvertError> {
        let SyntaxNodeKind::ObjectPattern { properties, .. } = self.kind(pattern)? else {
            return Err(self.invalid());
        };
        let mut binding = None;
        for property in self.list(properties)? {
            if let SyntaxNodeKind::ShorthandPatternProperty { target, .. } = self.kind(property)? {
                if self.tree.identifier_value(target).as_deref() == Some("children") {
                    binding = Some("children".to_owned());
                }
            } else if let SyntaxNodeKind::PatternProperty {
                key,
                value,
                computed: false,
            } = self.kind(property)?
                && self.tree.identifier_value(key).as_deref() == Some("children")
            {
                binding = self.tree.identifier_value(value);
            }
        }
        Ok(binding)
    }

    // Rejects a pattern whose binding can run code or throw, which dropping
    // the pattern would skip: a default value, a computed key, or a nested
    // pattern, which destructures a value that may be absent.
    fn check_inert(&self, pattern: NodeId) -> Result<(), ConvertError> {
        let mut stack = vec![pattern];
        while let Some(id) = stack.pop() {
            let kind = self.kind(id)?;
            let nested = id != pattern
                && matches!(
                    kind,
                    SyntaxNodeKind::ObjectPattern { .. } | SyntaxNodeKind::ArrayPattern { .. }
                );
            let evaluates = nested
                || matches!(
                    kind,
                    SyntaxNodeKind::AssignmentPattern { .. }
                        | SyntaxNodeKind::ShorthandPatternProperty {
                            default: Some(_),
                            ..
                        }
                        | SyntaxNodeKind::PatternProperty { computed: true, .. }
                );
            if evaluates {
                return Err(self.error(ConvertErrorKind::UnsupportedFunction, self.span(id)?));
            }
            stack.extend(self.tree.child_nodes(id).ok_or_else(|| self.invalid())?);
        }
        Ok(())
    }

    // Every name that the pattern declares.
    fn declared_names(&self, pattern: NodeId) -> Result<Vec<String>, ConvertError> {
        let mut names = Vec::new();
        let mut stack = vec![pattern];
        while let Some(id) = stack.pop() {
            if matches!(self.kind(id)?, SyntaxNodeKind::BindingIdentifier { .. }) {
                names.push(
                    self.tree
                        .identifier_value(id)
                        .ok_or_else(|| self.invalid())?,
                );
            }
            stack.extend(self.tree.child_nodes(id).ok_or_else(|| self.invalid())?);
        }
        Ok(names)
    }

    // Checks the whole function for constructs whose meaning differs in an
    // Astro component and for dropped names outside the replaced spans. The
    // walk uses an explicit stack, because expression chains are not
    // bounded by the parser's nesting limit.
    fn check_function(
        &self,
        function: NodeId,
        own_name: Option<NodeId>,
        substitutions: &[Span],
        dropped: &[String],
    ) -> Result<(), ConvertError> {
        let mut stack = vec![function];
        while let Some(id) = stack.pop() {
            if Some(id) == own_name {
                continue;
            }
            let span = self.span(id)?;
            let kind = self.kind(id)?;
            let replaced = substitutions
                .iter()
                .any(|s| s.start() <= span.start() && span.end() <= s.end());
            if replaced {
                continue;
            }
            if matches!(
                kind,
                SyntaxNodeKind::This | SyntaxNodeKind::Super | SyntaxNodeKind::MetaProperty { .. }
            ) {
                return Err(self.error(ConvertErrorKind::ContextDependent, span));
            }
            if matches!(
                kind,
                SyntaxNodeKind::Identifier { .. } | SyntaxNodeKind::BindingIdentifier { .. }
            ) {
                let name = self
                    .tree
                    .identifier_value(id)
                    .ok_or_else(|| self.invalid())?;
                if name == "arguments" {
                    return Err(self.error(ConvertErrorKind::ContextDependent, span));
                }
                if name == "Astro" {
                    return Err(self.error(ConvertErrorKind::ReservedName, span));
                }
                if dropped.contains(&name) && self.outside_parameters(function, span)? {
                    return Err(self.error(ConvertErrorKind::DroppedBinding, span));
                }
            }
            if let SyntaxNodeKind::Call { callee, .. } = kind
                && self.hook_like(callee)?
            {
                return Err(self.error(ConvertErrorKind::HookCall, span));
            }
            stack.extend(self.tree.child_nodes(id).ok_or_else(|| self.invalid())?);
        }
        Ok(())
    }

    // The binding identifier of a function declaration or expression.
    fn own_name(&self, function: NodeId) -> Result<Option<NodeId>, ConvertError> {
        let kind = self.kind(function)?;
        if let SyntaxNodeKind::FunctionDeclaration { name, .. }
        | SyntaxNodeKind::FunctionExpression { name, .. } = kind
        {
            Ok(name)
        } else {
            Ok(None)
        }
    }

    // The parameters and body of a function that is not a generator.
    fn function_parts(&self, function: NodeId) -> Result<Option<(NodeId, NodeId)>, ConvertError> {
        let kind = self.kind(function)?;
        if let SyntaxNodeKind::FunctionDeclaration {
            parameters,
            body,
            kind,
            ..
        }
        | SyntaxNodeKind::FunctionExpression {
            parameters,
            body,
            kind,
            ..
        } = kind
        {
            Ok((!kind.is_generator()).then_some((parameters, body)))
        } else if let SyntaxNodeKind::ArrowFunction {
            parameters, body, ..
        } = kind
        {
            Ok(Some((parameters, body)))
        } else {
            Ok(None)
        }
    }

    // Whether `span` lies outside the function's parameter list, where the
    // dropped pattern itself declares its names.
    fn outside_parameters(&self, function: NodeId, span: Span) -> Result<bool, ConvertError> {
        let (parameters, _) = self
            .function_parts(function)?
            .ok_or_else(|| self.invalid())?;
        let list = self.span(parameters)?;
        Ok(span.end() <= list.start() || span.start() >= list.end())
    }

    // React's rules of hooks identify hooks by this spelling. Matching it
    // only rejects, so a hook under another name is the caller's concern.
    fn hook_like(&self, callee: NodeId) -> Result<bool, ConvertError> {
        let callee = self.unparenthesize(callee)?;
        let kind = self.kind(callee)?;
        let name = if matches!(kind, SyntaxNodeKind::Identifier { .. }) {
            self.tree.identifier_value(callee)
        } else if let SyntaxNodeKind::Member {
            property,
            computed: false,
            ..
        } = kind
        {
            self.tree.identifier_value(property)
        } else {
            None
        };
        Ok(name.is_some_and(|name| {
            name.strip_prefix("use").is_some_and(|rest| {
                rest.is_empty()
                    || rest.starts_with(|ch: char| ch.is_ascii_uppercase() || ch.is_ascii_digit())
            })
        }))
    }

    // Checks one statement before the final `return`.
    fn check_statement(
        &self,
        statement: NodeId,
        children: Option<&str>,
    ) -> Result<(), ConvertError> {
        let span = self.span(statement)?;
        let kind = self.kind(statement)?;
        let supported = if let SyntaxNodeKind::VariableDeclaration { kind, .. } = kind {
            matches!(
                kind,
                VariableKind::Var | VariableKind::Let | VariableKind::Const
            )
        } else {
            matches!(kind, SyntaxNodeKind::ExpressionStatement { .. })
        };
        if !supported {
            return Err(self.error(ConvertErrorKind::UnsupportedBody, span));
        }
        // A `return` inside a nested function returns from that function.
        let mut stack = vec![(statement, false)];
        while let Some((id, nested)) = stack.pop() {
            let span = self.span(id)?;
            let kind = self.kind(id)?;
            let markup = matches!(
                kind,
                SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
            );
            let early_return = !nested && matches!(kind, SyntaxNodeKind::ReturnStatement { .. });
            if markup || early_return {
                return Err(self.error(ConvertErrorKind::UnsupportedBody, span));
            }
            if matches!(
                kind,
                SyntaxNodeKind::Identifier { .. } | SyntaxNodeKind::BindingIdentifier { .. }
            ) && children.is_some()
                && self.tree.identifier_value(id).as_deref() == children
            {
                return Err(self.error(ConvertErrorKind::ChildrenOutsideSlot, span));
            }
            let nested = nested
                || matches!(
                    kind,
                    SyntaxNodeKind::FunctionDeclaration { .. }
                        | SyntaxNodeKind::FunctionExpression { .. }
                        | SyntaxNodeKind::ArrowFunction { .. }
                );
            for child in self.tree.child_nodes(id).ok_or_else(|| self.invalid())? {
                stack.push((child, nested));
            }
        }
        Ok(())
    }
}
