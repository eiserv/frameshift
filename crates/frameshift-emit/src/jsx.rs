//! Conversion of React JSX into the Astro template model.
//!
//! The conversion preserves what React renders to static HTML for the subset
//! of section 9.3 of the migration specification and fails closed with a
//! typed error for everything else. Where the result depends on runtime
//! values that the syntax cannot show, the conversion succeeds and records
//! an [`Assumption`] that the calling rule must classify. The converter knows
//! React's JSX semantics, which every planned source with JSX shares; it has
//! no knowledge of a source framework, whose components the caller resolves
//! through an [`ElementResolver`].

use std::fmt;

use frameshift_core::Span;
use frameshift_syntax::{
    BinaryOperator, ChildList, NodeId, SyntaxNodeKind, SyntaxTree, TemplatePart,
    decode_jsx_character_reference,
};

use crate::astro::{
    AttributeItem, AttributeValue, Element, Expression, ExpressionPiece, TagName, TemplateNode,
};
use crate::html::{
    HtmlAttribute, html_attribute, is_boolean_attribute, is_foreign_root, is_raw_text_element,
    is_void_element,
};
use crate::names::{AttributeName, ComponentName, HtmlTagName};
use crate::output::{Anchor, SourceId};

/// Options of one JSX conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsxConversion<'a> {
    /// The identifier bound to the component's `children` prop, if any. A
    /// child expression that is exactly this identifier becomes the default
    /// `<slot />`; any other use of the name is an error.
    pub children_binding: Option<&'a str>,
    /// Maximum nesting of elements and expressions. Defaults to 256.
    pub max_depth: usize,
}

impl Default for JsxConversion<'_> {
    fn default() -> Self {
        Self {
            children_binding: None,
            max_depth: 256,
        }
    }
}

/// A component reference in JSX, such as `Link` or `Menu.Item`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentReference<'a> {
    /// The element name as spelled in the source.
    pub spelling: &'a str,
    /// Span of the element name.
    pub span: Span,
}

/// How the caller renders a component reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComponentStrategy {
    /// Render a component under this name, hydrated with `directive` if any.
    Component {
        /// Component name in the output.
        name: ComponentName,
        /// Client directive of an island.
        directive: Option<ClientDirective>,
    },
    /// Render an HTML element instead, such as `a` for a link component.
    /// The element's attributes follow the HTML attribute rules. The caller
    /// has validated the component's props.
    Html(HtmlTagName),
}

/// An Astro client directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientDirective {
    /// `client:load`, which hydrates on page load.
    Load,
}

/// Resolves component references to rendering strategies.
pub trait ElementResolver {
    /// Returns the strategy for `reference`, or `None` when the reference is
    /// unknown, which fails the conversion.
    fn resolve(&mut self, reference: ComponentReference<'_>) -> Option<ComponentStrategy>;
}

/// Converted markup and the assumptions it depends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvertedMarkup {
    /// Template nodes in source order. A root fragment contributes its children.
    pub nodes: Vec<TemplateNode>,
    /// Assumptions in source order.
    pub assumptions: Vec<Assumption>,
    /// Source spans that the template copies verbatim, in source order. Every
    /// comment inside the converted JSX lies in one of them.
    pub copied: Vec<Span>,
}

/// A runtime fact that the converted markup assumes and the syntax does not
/// prove.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assumption {
    /// What is assumed.
    pub kind: AssumptionKind,
    /// Construct that depends on it.
    pub anchor: Anchor,
}

/// The kinds of [`Assumption`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssumptionKind {
    /// A child expression never evaluates to `true`, which React renders as
    /// nothing and Astro as the text `true`.
    ChildNotTrue,
    /// An attribute expression of an HTML element evaluates to a string, a
    /// number, `null`, or `undefined`, which React and Astro render alike,
    /// and not to a boolean, an array, or an object.
    AttributeValueSerialization,
    /// A spread on an HTML element contains only HTML attribute names, which
    /// Astro renders as written while React maps React prop names.
    SpreadHtmlNames,
    /// A prop of a hydrated component is a value that Astro can serialize.
    SerializableIslandProp,
    /// A removed `key` expression has no side effects.
    DiscardedKey,
}

impl AssumptionKind {
    /// Returns a stable kebab-case identifier for reports.
    pub const fn id(self) -> &'static str {
        match self {
            Self::ChildNotTrue => "child-not-true",
            Self::AttributeValueSerialization => "attribute-value-serialization",
            Self::SpreadHtmlNames => "spread-html-names",
            Self::SerializableIslandProp => "serializable-island-prop",
            Self::DiscardedKey => "discarded-key",
        }
    }
}

/// A rejected conversion. No partial markup is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvertError {
    /// Why the construct is not converted.
    pub kind: ConvertErrorKind,
    /// The construct.
    pub anchor: Anchor,
}

/// The reasons of a [`ConvertError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertErrorKind {
    /// The node is not a JSX element or fragment, or the tree is inconsistent.
    NotJsx,
    /// A namespaced element or attribute name such as `svg:rect`.
    NamespacedName,
    /// An HTML element name with uppercase letters or other characters.
    UnsupportedElementName,
    /// The resolver does not know the component.
    UnresolvedComponent,
    /// An `svg` or `math` element, whose attribute mapping is not verified.
    ForeignContent,
    /// Children of `script`, `style`, `textarea`, `title`, or `noscript`.
    RawTextChildren,
    /// A void element such as `img` with children.
    VoidElementChildren,
    /// An attribute without a verified mapping, such as an event handler,
    /// `ref`, `children`, or an unknown camelCase name.
    UnsupportedAttribute,
    /// A valueless attribute that React would render as `true` on an
    /// attribute that is neither boolean nor `data-*` or `aria-*`.
    ValuelessAttribute,
    /// An attribute string that contains a line break, which JSX compilers
    /// normalize differently.
    MultilineAttributeString,
    /// A JSX element or fragment used as an attribute value.
    MarkupAttributeValue,
    /// A `dangerouslySetInnerHTML` value other than `{{ __html: value }}`,
    /// or one combined with children.
    UnsupportedInnerHtml,
    /// A `style` value other than an object literal with string values and
    /// keys whose CSS names React and Astro derive alike.
    UnsupportedStyle,
    /// A spread child `{...items}`.
    SpreadChild,
    /// The children binding is used other than as a child expression, or it
    /// is shadowed inside the markup.
    ChildrenOutsideSlot,
    /// A character reference in text that decodes to whitespace, which JSX
    /// compilers trim in different orders.
    WhitespaceReference,
    /// Text with a tab inside a line, a line or paragraph separator, a
    /// control character, or a reference to a surrogate.
    UnsupportedTextCharacter,
    /// A comment in markup outside every copied expression, which the
    /// conversion would drop.
    CommentInMarkup,
    /// Markup nests deeper than the configured limit.
    TooDeep,
}

impl ConvertErrorKind {
    /// Returns a stable kebab-case identifier for reports.
    pub const fn id(self) -> &'static str {
        match self {
            Self::NotJsx => "not-jsx",
            Self::NamespacedName => "namespaced-name",
            Self::UnsupportedElementName => "unsupported-element-name",
            Self::UnresolvedComponent => "unresolved-component",
            Self::ForeignContent => "foreign-content",
            Self::RawTextChildren => "raw-text-children",
            Self::VoidElementChildren => "void-element-children",
            Self::UnsupportedAttribute => "unsupported-attribute",
            Self::ValuelessAttribute => "valueless-attribute",
            Self::MultilineAttributeString => "multiline-attribute-string",
            Self::MarkupAttributeValue => "markup-attribute-value",
            Self::UnsupportedInnerHtml => "unsupported-inner-html",
            Self::UnsupportedStyle => "unsupported-style",
            Self::SpreadChild => "spread-child",
            Self::ChildrenOutsideSlot => "children-outside-slot",
            Self::WhitespaceReference => "whitespace-reference",
            Self::UnsupportedTextCharacter => "unsupported-text-character",
            Self::CommentInMarkup => "comment-in-markup",
            Self::TooDeep => "too-deep",
        }
    }
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at source {} bytes {}..{}",
            self.kind.id(),
            self.anchor.source.0,
            self.anchor.span.start(),
            self.anchor.span.end()
        )
    }
}

impl std::error::Error for ConvertError {}

/// Converts the JSX element or fragment at `node`, which may be wrapped in
/// parentheses, into Astro template nodes.
///
/// Text follows React's JSX whitespace rules and XHTML character references.
/// HTML attributes follow React's rendering rules for the supported names
/// (`className` becomes `class`, `htmlFor` becomes `for`, `key` is removed,
/// `dangerouslySetInnerHTML` becomes `set:html`). Expression containers are
/// copied verbatim with any JSX inside them converted, so comments in them
/// are preserved. Every comment of the converted range must be copied.
pub fn convert_jsx(
    tree: &SyntaxTree<'_>,
    source: SourceId,
    node: NodeId,
    conversion: &JsxConversion<'_>,
    resolver: &mut dyn ElementResolver,
) -> Result<ConvertedMarkup, ConvertError> {
    let mut converter = Converter {
        tree,
        source,
        conversion,
        resolver,
        assumptions: Vec::new(),
        copied: Vec::new(),
    };
    let root = converter.unparenthesize(node)?;
    let root_span = converter.span(root)?;
    if let Some(binding) = conversion.children_binding {
        converter.check_children_binding(root, binding)?;
    }
    let root_kind = converter.kind(root)?;
    let nodes = if matches!(root_kind, SyntaxNodeKind::JsxElement { .. }) {
        vec![converter.element(root, 0)?]
    } else if let SyntaxNodeKind::JsxFragment { children, .. } = root_kind {
        converter.children(children, 0)?
    } else {
        return Err(converter.error(ConvertErrorKind::NotJsx, root_span));
    };
    converter
        .copied
        .sort_by_key(|span| (span.start(), span.end()));
    converter.check_comments(root_span)?;
    converter
        .assumptions
        .sort_by_key(|assumption| (assumption.anchor.span.start(), assumption.anchor.span.end()));
    Ok(ConvertedMarkup {
        nodes,
        assumptions: converter.assumptions,
        copied: converter.copied,
    })
}

struct Converter<'t, 'r, 'c> {
    tree: &'t SyntaxTree<'t>,
    source: SourceId,
    conversion: &'c JsxConversion<'c>,
    resolver: &'r mut dyn ElementResolver,
    assumptions: Vec<Assumption>,
    copied: Vec<Span>,
}

impl<'t> Converter<'t, '_, '_> {
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

    fn text(&self, span: Span) -> Result<&'t str, ConvertError> {
        self.tree
            .source()
            .slice(span)
            .map_err(|_| self.error(ConvertErrorKind::NotJsx, span))
    }

    fn node_text(&self, id: NodeId) -> Result<&'t str, ConvertError> {
        self.text(self.span(id)?)
    }

    fn list(&self, list: ChildList) -> Result<Vec<NodeId>, ConvertError> {
        self.tree
            .children(list)
            .map(<[NodeId]>::to_vec)
            .ok_or_else(|| self.invalid())
    }

    fn assume(&mut self, kind: AssumptionKind, span: Span) {
        let anchor = self.anchor(span);
        self.assumptions.push(Assumption { kind, anchor });
    }

    fn unparenthesize(&self, mut id: NodeId) -> Result<NodeId, ConvertError> {
        while let SyntaxNodeKind::Parenthesized { expression } = self.kind(id)? {
            id = expression;
        }
        Ok(id)
    }

    fn depth_check(&self, depth: usize, span: Span) -> Result<(), ConvertError> {
        if depth > self.conversion.max_depth {
            Err(self.error(ConvertErrorKind::TooDeep, span))
        } else {
            Ok(())
        }
    }

    fn element(&mut self, id: NodeId, depth: usize) -> Result<TemplateNode, ConvertError> {
        let span = self.span(id)?;
        self.depth_check(depth, span)?;
        let SyntaxNodeKind::JsxElement {
            opening, children, ..
        } = self.kind(id)?
        else {
            return Err(self.error(ConvertErrorKind::NotJsx, span));
        };
        let SyntaxNodeKind::JsxOpeningElement {
            name, attributes, ..
        } = self.kind(opening)?
        else {
            return Err(self.error(ConvertErrorKind::NotJsx, span));
        };
        let name_span = self.span(name)?;
        let (tag, directive) = self.element_name(name)?;
        let children = self.list(children)?;
        let attributes = self.list(attributes)?;
        let mut items = Vec::new();
        if let Some(ClientDirective::Load) = directive {
            items.push(AttributeItem::Named {
                name: attribute_name("client:load"),
                value: AttributeValue::Present,
                anchor: self.anchor(name_span),
            });
        }
        let converted_children = match &tag {
            TagName::Html(html) => {
                let html = html.as_str();
                if is_foreign_root(html) {
                    return Err(self.error(ConvertErrorKind::ForeignContent, span));
                }
                if !children.is_empty() && is_raw_text_element(html) {
                    return Err(self.error(ConvertErrorKind::RawTextChildren, span));
                }
                if !children.is_empty() && is_void_element(html) {
                    return Err(self.error(ConvertErrorKind::VoidElementChildren, span));
                }
                let has_inner_html = self.html_attributes(&attributes, &mut items, depth)?;
                if has_inner_html && !children.is_empty() {
                    return Err(self.error(ConvertErrorKind::UnsupportedInnerHtml, span));
                }
                self.child_nodes(&children, depth)?
            }
            TagName::Component(_) => {
                self.component_attributes(&attributes, &mut items, directive.is_some(), depth)?;
                self.child_nodes(&children, depth)?
            }
        };
        Ok(TemplateNode::Element(Element {
            name: tag,
            attributes: items,
            children: converted_children,
            anchor: self.anchor(span),
        }))
    }

    fn element_name(
        &mut self,
        name: NodeId,
    ) -> Result<(TagName, Option<ClientDirective>), ConvertError> {
        let span = self.span(name)?;
        let spelling = self.text(span)?;
        let kind = self.kind(name)?;
        if matches!(kind, SyntaxNodeKind::JsxNamespacedName { .. }) {
            return Err(self.error(ConvertErrorKind::NamespacedName, span));
        }
        let identifier = matches!(kind, SyntaxNodeKind::JsxIdentifier);
        if !identifier && !matches!(kind, SyntaxNodeKind::JsxMemberExpression { .. }) {
            return Err(self.error(ConvertErrorKind::NotJsx, span));
        }
        // React treats a name that starts with a lowercase ASCII letter as an
        // HTML element and every other name as a component reference.
        if identifier && spelling.starts_with(|ch: char| ch.is_ascii_lowercase()) {
            return HtmlTagName::new(spelling)
                .map(|tag| (TagName::Html(tag), None))
                .ok_or_else(|| self.error(ConvertErrorKind::UnsupportedElementName, span));
        }
        let reference = ComponentReference { spelling, span };
        match self.resolver.resolve(reference) {
            Some(ComponentStrategy::Component { name, directive }) => {
                Ok((TagName::Component(name), directive))
            }
            Some(ComponentStrategy::Html(tag)) => Ok((TagName::Html(tag), None)),
            None => Err(self.error(ConvertErrorKind::UnresolvedComponent, span)),
        }
    }

    // Returns the attribute's name spelling, rejecting namespaced names.
    fn attribute_spelling(&self, name: NodeId, span: Span) -> Result<&'t str, ConvertError> {
        if !matches!(self.kind(name)?, SyntaxNodeKind::JsxIdentifier) {
            return Err(self.error(ConvertErrorKind::NamespacedName, span));
        }
        self.node_text(name)
    }

    // Returns whether `dangerouslySetInnerHTML` was converted.
    fn html_attributes(
        &mut self,
        attributes: &[NodeId],
        items: &mut Vec<AttributeItem>,
        depth: usize,
    ) -> Result<bool, ConvertError> {
        let mut inner_html = false;
        for &attribute in attributes {
            let span = self.span(attribute)?;
            let kind = self.kind(attribute)?;
            if let SyntaxNodeKind::JsxSpreadAttribute { argument } = kind {
                self.assume(AssumptionKind::SpreadHtmlNames, span);
                items.push(AttributeItem::Spread(self.spread(span, argument, depth)?));
                continue;
            }
            let SyntaxNodeKind::JsxAttribute { name, value } = kind else {
                return Err(self.error(ConvertErrorKind::NotJsx, span));
            };
            let spelling = self.attribute_spelling(name, span)?;
            match html_attribute(spelling) {
                HtmlAttribute::Key => self.discard_key(value)?,
                HtmlAttribute::Drop => {}
                HtmlAttribute::Unsupported => {
                    return Err(self.error(ConvertErrorKind::UnsupportedAttribute, span));
                }
                HtmlAttribute::InnerHtml => {
                    items.push(self.inner_html(span, value, depth)?);
                    inner_html = true;
                }
                HtmlAttribute::Style => items.push(self.style(span, value, depth)?),
                HtmlAttribute::Same => {
                    let html = spelling.to_owned();
                    items.push(self.html_attribute(span, &html, value, depth)?);
                }
                HtmlAttribute::Rename(html) => {
                    items.push(self.html_attribute(span, html, value, depth)?);
                }
            }
        }
        Ok(inner_html)
    }

    fn html_attribute(
        &mut self,
        span: Span,
        html: &str,
        value: Option<NodeId>,
        depth: usize,
    ) -> Result<AttributeItem, ConvertError> {
        let name = AttributeName::new(html)
            .ok_or_else(|| self.error(ConvertErrorKind::UnsupportedAttribute, span))?;
        let value = match value {
            None if is_boolean_attribute(html) => AttributeValue::Present,
            None if html.starts_with("data-") || html.starts_with("aria-") => {
                AttributeValue::Text("true".to_owned())
            }
            None => return Err(self.error(ConvertErrorKind::ValuelessAttribute, span)),
            Some(value) => {
                let kind = self.kind(value)?;
                if matches!(kind, SyntaxNodeKind::JsxStringLiteral) {
                    AttributeValue::Text(self.attribute_string(value)?)
                } else if let SyntaxNodeKind::JsxExpressionContainer {
                    expression: Some(expression),
                } = kind
                {
                    if !self.is_string_or_number(expression)? {
                        self.assume(AssumptionKind::AttributeValueSerialization, span);
                    }
                    AttributeValue::Expression(self.attribute_container(span, value, depth)?)
                } else {
                    return Err(self.error(ConvertErrorKind::MarkupAttributeValue, span));
                }
            }
        };
        Ok(AttributeItem::Named {
            name,
            value,
            anchor: self.anchor(span),
        })
    }

    fn component_attributes(
        &mut self,
        attributes: &[NodeId],
        items: &mut Vec<AttributeItem>,
        hydrated: bool,
        depth: usize,
    ) -> Result<(), ConvertError> {
        for &attribute in attributes {
            let span = self.span(attribute)?;
            let kind = self.kind(attribute)?;
            if let SyntaxNodeKind::JsxSpreadAttribute { argument } = kind {
                if hydrated {
                    self.assume(AssumptionKind::SerializableIslandProp, span);
                }
                items.push(AttributeItem::Spread(self.spread(span, argument, depth)?));
                continue;
            }
            let SyntaxNodeKind::JsxAttribute { name, value } = kind else {
                return Err(self.error(ConvertErrorKind::NotJsx, span));
            };
            let spelling = self.attribute_spelling(name, span)?;
            if spelling == "key" {
                self.discard_key(value)?;
                continue;
            }
            if matches!(spelling, "ref" | "children") {
                return Err(self.error(ConvertErrorKind::UnsupportedAttribute, span));
            }
            let name = AttributeName::new(spelling)
                .ok_or_else(|| self.error(ConvertErrorKind::UnsupportedAttribute, span))?;
            let value = match value {
                None => AttributeValue::Expression(Expression {
                    pieces: vec![ExpressionPiece::True],
                    anchor: self.anchor(span),
                }),
                Some(value) => {
                    let kind = self.kind(value)?;
                    if matches!(kind, SyntaxNodeKind::JsxStringLiteral) {
                        AttributeValue::Text(self.attribute_string(value)?)
                    } else if let SyntaxNodeKind::JsxExpressionContainer {
                        expression: Some(expression),
                    } = kind
                    {
                        if hydrated && !self.is_literal(expression)? {
                            self.assume(AssumptionKind::SerializableIslandProp, span);
                        }
                        AttributeValue::Expression(self.attribute_container(span, value, depth)?)
                    } else {
                        return Err(self.error(ConvertErrorKind::MarkupAttributeValue, span));
                    }
                }
            };
            items.push(AttributeItem::Named {
                name,
                value,
                anchor: self.anchor(span),
            });
        }
        Ok(())
    }

    fn discard_key(&mut self, value: Option<NodeId>) -> Result<(), ConvertError> {
        let Some(value) = value else {
            return Ok(());
        };
        if let SyntaxNodeKind::JsxExpressionContainer {
            expression: Some(expression),
        } = self.kind(value)?
            && !self.is_side_effect_free(expression)?
        {
            let span = self.span(value)?;
            self.assume(AssumptionKind::DiscardedKey, span);
        }
        Ok(())
    }

    // The single expression of `attribute={expression}`.
    fn container_expression(&self, value: Option<NodeId>) -> Result<Option<NodeId>, ConvertError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if let SyntaxNodeKind::JsxExpressionContainer { expression } = self.kind(value)? {
            Ok(expression)
        } else {
            Ok(None)
        }
    }

    fn inner_html(
        &mut self,
        span: Span,
        value: Option<NodeId>,
        depth: usize,
    ) -> Result<AttributeItem, ConvertError> {
        let unsupported = self.error(ConvertErrorKind::UnsupportedInnerHtml, span);
        let object = self.container_expression(value)?.ok_or(unsupported)?;
        let SyntaxNodeKind::ObjectExpression { properties, .. } = self.kind(object)? else {
            return Err(unsupported);
        };
        let properties = self.list(properties)?;
        let [property] = properties.as_slice() else {
            return Err(unsupported);
        };
        let property_kind = self.kind(*property)?;
        let html = if let SyntaxNodeKind::Property {
            key,
            value,
            computed: false,
        } = property_kind
            && self.is_key(key, "__html")?
        {
            value
        } else if let SyntaxNodeKind::ShorthandProperty { identifier } = property_kind
            && self.node_text(identifier)? == "__html"
        {
            identifier
        } else {
            return Err(unsupported);
        };
        let html_span = self.span(html)?;
        let expression = self.expression_pieces(html_span, html, html_span, depth)?;
        Ok(AttributeItem::Named {
            name: attribute_name("set:html"),
            value: AttributeValue::Expression(expression),
            anchor: self.anchor(span),
        })
    }

    fn style(
        &mut self,
        span: Span,
        value: Option<NodeId>,
        depth: usize,
    ) -> Result<AttributeItem, ConvertError> {
        let unsupported = self.error(ConvertErrorKind::UnsupportedStyle, span);
        let object = self.container_expression(value)?.ok_or(unsupported)?;
        let SyntaxNodeKind::ObjectExpression { properties, .. } = self.kind(object)? else {
            return Err(unsupported);
        };
        for property in self.list(properties)? {
            let SyntaxNodeKind::Property {
                key,
                value,
                computed: false,
            } = self.kind(property)?
            else {
                return Err(unsupported);
            };
            let key_kind = self.kind(key)?;
            let key_name = if let SyntaxNodeKind::IdentifierName {
                has_escape: false, ..
            } = key_kind
            {
                self.node_text(key)?.to_owned()
            } else {
                self.tree.string_literal_value(key).ok_or(unsupported)?
            };
            let value_text = self.tree.string_literal_value(value).ok_or(unsupported)?;
            if !is_portable_style_key(&key_name) || value_text.is_empty() {
                return Err(unsupported);
            }
        }
        let Some(container) = value else {
            return Err(unsupported);
        };
        Ok(AttributeItem::Named {
            name: attribute_name("style"),
            value: AttributeValue::Expression(self.container(container, depth)?),
            anchor: self.anchor(span),
        })
    }

    fn is_key(&self, key: NodeId, expected: &str) -> Result<bool, ConvertError> {
        let kind = self.kind(key)?;
        if let SyntaxNodeKind::IdentifierName {
            has_escape: false, ..
        } = kind
        {
            return Ok(self.node_text(key)? == expected);
        }
        Ok(self.tree.string_literal_value(key).as_deref() == Some(expected))
    }

    fn attribute_string(&self, value: NodeId) -> Result<String, ConvertError> {
        let span = self.span(value)?;
        let spelling = self.text(span)?;
        let content = spelling
            .get(1..spelling.len().saturating_sub(1))
            .unwrap_or_default();
        if content.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            return Err(self.error(ConvertErrorKind::MultilineAttributeString, span));
        }
        decode_references(content).map_err(|kind| self.error(kind, span))
    }

    fn child_nodes(
        &mut self,
        children: &[NodeId],
        depth: usize,
    ) -> Result<Vec<TemplateNode>, ConvertError> {
        let mut nodes = Vec::new();
        for &child in children {
            let span = self.span(child)?;
            let kind = self.kind(child)?;
            if matches!(kind, SyntaxNodeKind::JsxText) {
                let value =
                    cook_jsx_text(self.text(span)?).map_err(|kind| self.error(kind, span))?;
                if !value.is_empty() {
                    nodes.push(TemplateNode::Text {
                        value,
                        anchor: self.anchor(span),
                    });
                }
            } else if matches!(kind, SyntaxNodeKind::JsxElement { .. }) {
                nodes.push(self.element(child, depth + 1)?);
            } else if let SyntaxNodeKind::JsxFragment { children, .. } = kind {
                self.depth_check(depth + 1, span)?;
                nodes.extend(self.children(children, depth + 1)?);
            } else if let SyntaxNodeKind::JsxExpressionContainer { expression } = kind {
                let Some(expression) = expression else {
                    // An empty container renders nothing; its comments are kept.
                    if self.has_comment(span) {
                        nodes.push(TemplateNode::Expression(self.container(child, depth)?));
                    }
                    continue;
                };
                if self.is_children_binding(expression)? {
                    if self.has_comment(span) {
                        return Err(self.error(ConvertErrorKind::CommentInMarkup, span));
                    }
                    nodes.push(TemplateNode::Slot {
                        name: None,
                        anchor: self.anchor(span),
                    });
                    continue;
                }
                if !self.is_safe_child(expression)? {
                    self.assume(AssumptionKind::ChildNotTrue, span);
                }
                nodes.push(TemplateNode::Expression(self.container(child, depth)?));
            } else if matches!(kind, SyntaxNodeKind::JsxSpreadChild { .. }) {
                return Err(self.error(ConvertErrorKind::SpreadChild, span));
            } else {
                return Err(self.error(ConvertErrorKind::NotJsx, span));
            }
        }
        Ok(nodes)
    }

    fn children(
        &mut self,
        list: ChildList,
        depth: usize,
    ) -> Result<Vec<TemplateNode>, ConvertError> {
        let children = self.list(list)?;
        self.child_nodes(&children, depth)
    }

    // The interior of an expression container, between its braces.
    fn container(&mut self, container: NodeId, depth: usize) -> Result<Expression, ConvertError> {
        let span = self.span(container)?;
        let inner = Span::new(span.start() + 1, span.end() - 1)
            .map_err(|_| self.error(ConvertErrorKind::NotJsx, span))?;
        self.expression_pieces(span, container, inner, depth)
    }

    // An attribute value container, which must not contain markup: React
    // elements as prop values become named slots in later rules.
    fn attribute_container(
        &mut self,
        attribute: Span,
        container: NodeId,
        depth: usize,
    ) -> Result<Expression, ConvertError> {
        let expression = self.container(container, depth)?;
        if expression
            .pieces
            .iter()
            .any(|piece| matches!(piece, ExpressionPiece::Markup(_)))
        {
            return Err(self.error(ConvertErrorKind::MarkupAttributeValue, attribute));
        }
        Ok(expression)
    }

    // The argument of a spread attribute `{...argument}`. Comments elsewhere
    // inside the braces are not copied, so the comment check rejects them.
    fn spread(
        &mut self,
        attribute: Span,
        argument: NodeId,
        depth: usize,
    ) -> Result<Expression, ConvertError> {
        let argument_span = self.span(argument)?;
        let expression = self.expression_pieces(attribute, argument, argument_span, depth)?;
        if expression
            .pieces
            .iter()
            .any(|piece| matches!(piece, ExpressionPiece::Markup(_)))
        {
            return Err(self.error(ConvertErrorKind::MarkupAttributeValue, attribute));
        }
        Ok(expression)
    }

    // Copies `inner` verbatim, converting the outermost JSX nodes inside it.
    // `start` is the smallest node that contains `inner`.
    fn expression_pieces(
        &mut self,
        anchor_span: Span,
        start: NodeId,
        inner: Span,
        depth: usize,
    ) -> Result<Expression, ConvertError> {
        self.depth_check(depth, anchor_span)?;
        let mut markup = Vec::new();
        self.find_markup(start, inner, &mut markup)?;
        let mut pieces = Vec::new();
        let mut offset = inner.start();
        for jsx in markup {
            let span = self.span(jsx)?;
            if span.start() > offset {
                let gap = Span::new(offset, span.start()).map_err(|_| self.invalid())?;
                self.copied.push(gap);
                pieces.push(ExpressionPiece::Source(gap));
            }
            let kind = self.kind(jsx)?;
            let node = if let SyntaxNodeKind::JsxFragment { children, .. } = kind {
                TemplateNode::Fragment {
                    children: self.children(children, depth + 1)?,
                    anchor: self.anchor(span),
                }
            } else {
                self.element(jsx, depth + 1)?
            };
            pieces.push(ExpressionPiece::Markup(node));
            offset = span.end();
        }
        if inner.end() > offset {
            let rest = Span::new(offset, inner.end()).map_err(|_| self.invalid())?;
            self.copied.push(rest);
            pieces.push(ExpressionPiece::Source(rest));
        }
        Ok(Expression {
            pieces,
            anchor: self.anchor(anchor_span),
        })
    }

    // Collects, in source order, the outermost JSX nodes inside `range`. The
    // walk uses an explicit stack, because expression trees such as member
    // chains are not bounded by the parser's nesting limit.
    fn find_markup(
        &self,
        start: NodeId,
        range: Span,
        found: &mut Vec<NodeId>,
    ) -> Result<(), ConvertError> {
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            let span = self.span(id)?;
            if span.end() <= range.start() || span.start() >= range.end() {
                continue;
            }
            let inside = span.start() >= range.start() && span.end() <= range.end();
            if inside
                && matches!(
                    self.kind(id)?,
                    SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
                )
            {
                found.push(id);
                continue;
            }
            let children: Vec<NodeId> = self
                .tree
                .child_nodes(id)
                .ok_or_else(|| self.invalid())?
                .collect();
            stack.extend(children.into_iter().rev());
        }
        Ok(())
    }

    fn has_comment(&self, span: Span) -> bool {
        let comments = self.tree.comments();
        let first = comments.partition_point(|comment| comment.span.start() < span.start());
        comments
            .get(first)
            .is_some_and(|comment| comment.span.end() <= span.end())
    }

    fn check_comments(&self, root: Span) -> Result<(), ConvertError> {
        let comments = self.tree.comments();
        let first = comments.partition_point(|comment| comment.span.start() < root.start());
        for comment in &comments[first..] {
            let span = comment.span;
            if span.start() >= root.end() {
                break;
            }
            let index = self
                .copied
                .partition_point(|copied| copied.start() <= span.start());
            let covered = index > 0
                && self
                    .copied
                    .get(index - 1)
                    .is_some_and(|copied| copied.end() >= span.end());
            if !covered {
                return Err(self.error(ConvertErrorKind::CommentInMarkup, span));
            }
        }
        Ok(())
    }

    fn is_children_binding(&self, expression: NodeId) -> Result<bool, ConvertError> {
        let Some(binding) = self.conversion.children_binding else {
            return Ok(false);
        };
        Ok(matches!(
            self.kind(expression)?,
            SyntaxNodeKind::Identifier {
                has_escape: false,
                ..
            }
        ) && self.node_text(expression)? == binding)
    }

    // Rejects any use of the children binding other than a whole child
    // expression, and any rebinding of its name inside the markup. Escaped
    // names are rejected, because they may spell the binding. The walk uses
    // an explicit stack, like `find_markup`.
    fn check_children_binding(&self, root: NodeId, binding: &str) -> Result<(), ConvertError> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let span = self.span(id)?;
            let kind = self.kind(id)?;
            let named = |has_escape: bool| -> Result<bool, ConvertError> {
                Ok(has_escape || self.text(span)? == binding)
            };
            let uses_binding = if let SyntaxNodeKind::Identifier { has_escape, .. } = kind {
                named(has_escape)?
            } else if let SyntaxNodeKind::BindingIdentifier { has_escape } = kind {
                named(has_escape)?
            } else {
                false
            };
            if uses_binding {
                return Err(self.error(ConvertErrorKind::ChildrenOutsideSlot, span));
            }
            let child_list = if let SyntaxNodeKind::JsxElement { children, .. } = kind {
                Some(self.list(children)?)
            } else if let SyntaxNodeKind::JsxFragment { children, .. } = kind {
                Some(self.list(children)?)
            } else {
                None
            };
            for child in self.tree.child_nodes(id).ok_or_else(|| self.invalid())? {
                // A whole child expression that is the binding becomes the slot.
                let allowed = if let Some(children) = &child_list
                    && children.contains(&child)
                    && let SyntaxNodeKind::JsxExpressionContainer {
                        expression: Some(expression),
                    } = self.kind(child)?
                {
                    self.is_children_binding(expression)?
                } else {
                    false
                };
                if !allowed {
                    stack.push(child);
                }
            }
        }
        Ok(())
    }

    // Child expressions whose value can never be `true`. Every rule requires
    // all of its operands to be safe, so a worklist decides the conjunction
    // without recursion.
    fn is_safe_child(&self, expression: NodeId) -> Result<bool, ConvertError> {
        let mut pending = vec![expression];
        while let Some(id) = pending.pop() {
            let kind = self.kind(id)?;
            if matches!(
                kind,
                SyntaxNodeKind::StringLiteral
                    | SyntaxNodeKind::NumberLiteral { .. }
                    | SyntaxNodeKind::TemplateLiteral { .. }
                    | SyntaxNodeKind::NullLiteral
                    | SyntaxNodeKind::BooleanLiteral(false)
                    | SyntaxNodeKind::JsxElement { .. }
                    | SyntaxNodeKind::JsxFragment { .. }
            ) {
                continue;
            }
            if let SyntaxNodeKind::Parenthesized { expression } = kind {
                pending.push(expression);
            } else if let SyntaxNodeKind::Conditional {
                consequent,
                alternate,
                ..
            } = kind
            {
                pending.extend([consequent, alternate]);
            } else if let SyntaxNodeKind::Binary {
                operator,
                left,
                right,
            } = kind
            {
                match operator {
                    // A falsy left operand is never `true`.
                    BinaryOperator::LogicalAnd | BinaryOperator::Comma => pending.push(right),
                    BinaryOperator::LogicalOr | BinaryOperator::Nullish => {
                        pending.extend([left, right]);
                    }
                    // Arithmetic and bitwise operators produce numbers or strings.
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Remainder
                    | BinaryOperator::Exponent
                    | BinaryOperator::BitwiseOr
                    | BinaryOperator::BitwiseXor
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::ShiftRightUnsigned => {}
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::StrictEqual
                    | BinaryOperator::StrictNotEqual
                    | BinaryOperator::LessThan
                    | BinaryOperator::GreaterThan
                    | BinaryOperator::LessThanEqual
                    | BinaryOperator::GreaterThanEqual
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => return Ok(false),
                }
            } else if let SyntaxNodeKind::Call {
                callee,
                arguments,
                optional: false,
            } = kind
                && let Some(body) = self.mapped_body(callee, arguments)?
            {
                pending.push(body);
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }

    // The expression body of the arrow in `items.map((item) => body)`, whose
    // result is an array of `body` values.
    fn mapped_body(
        &self,
        callee: NodeId,
        arguments: ChildList,
    ) -> Result<Option<NodeId>, ConvertError> {
        let SyntaxNodeKind::Member {
            property,
            computed: false,
            optional: false,
            ..
        } = self.kind(callee)?
        else {
            return Ok(None);
        };
        if self.node_text(property)? != "map" {
            return Ok(None);
        }
        let arguments = self.list(arguments)?;
        let Some(&callback) = arguments.first() else {
            return Ok(None);
        };
        let SyntaxNodeKind::ArrowFunction { body, .. } = self.kind(callback)? else {
            return Ok(None);
        };
        if matches!(self.kind(body)?, SyntaxNodeKind::FunctionBody { .. }) {
            return Ok(None);
        }
        Ok(Some(body))
    }

    fn is_string_or_number(&self, id: NodeId) -> Result<bool, ConvertError> {
        Ok(matches!(
            self.kind(id)?,
            SyntaxNodeKind::StringLiteral
                | SyntaxNodeKind::NumberLiteral { .. }
                | SyntaxNodeKind::TemplateLiteral { .. }
        ))
    }

    fn is_literal(&self, id: NodeId) -> Result<bool, ConvertError> {
        let kind = self.kind(id)?;
        if let SyntaxNodeKind::TemplateLiteral { parts } = kind {
            let parts = self
                .tree
                .template_parts(parts)
                .ok_or_else(|| self.invalid())?;
            return Ok(parts
                .iter()
                .all(|part| matches!(part, TemplatePart::Segment(_))));
        }
        Ok(matches!(
            kind,
            SyntaxNodeKind::StringLiteral
                | SyntaxNodeKind::NumberLiteral { .. }
                | SyntaxNodeKind::BooleanLiteral(_)
                | SyntaxNodeKind::NullLiteral
        ))
    }

    // Member chains are walked iteratively, because the parser builds them
    // without nesting, so their depth is bounded only by the node limit.
    fn is_side_effect_free(&self, mut id: NodeId) -> Result<bool, ConvertError> {
        loop {
            let kind = self.kind(id)?;
            if let SyntaxNodeKind::Member {
                object,
                computed: false,
                ..
            } = kind
            {
                id = object;
                continue;
            }
            return Ok(matches!(
                kind,
                SyntaxNodeKind::Identifier { .. }
                    | SyntaxNodeKind::StringLiteral
                    | SyntaxNodeKind::NumberLiteral { .. }
            ));
        }
    }
}

fn attribute_name(name: &str) -> AttributeName {
    AttributeName::new(name).unwrap_or_else(|| unreachable!("constant attribute name {name}"))
}

// React derives CSS property names from camelCase keys by inserting hyphens,
// with a leading hyphen for vendor prefixes and `ms` lowered to `-ms-`;
// Astro derives the same names except for the `ms` prefix. Custom
// properties keep their spelling in both.
fn is_portable_style_key(key: &str) -> bool {
    if key.starts_with("--") {
        return key.len() > 2;
    }
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let ms_prefix =
        key.starts_with("ms") && key[2..].starts_with(|ch: char| ch.is_ascii_uppercase());
    first.is_ascii_alphabetic() && chars.all(|ch| ch.is_ascii_alphabetic()) && !ms_prefix
}

/// Applies React's JSX whitespace rules to raw JSX text and decodes its
/// character references, as Babel's `cleanJSXElementLiteralChild` does.
///
/// Lines are split at CR, LF, and CRLF. Spaces and tabs are trimmed at the
/// start of every line but the first and at the end of every line but the
/// last, lines left empty are dropped, and the rest are joined with one
/// space. A tab that survives trimming, a line or paragraph separator, a
/// control character, and a reference that decodes to whitespace or a
/// surrogate fail, because the JSX compilers treat them differently.
pub(crate) fn cook_jsx_text(raw: &str) -> Result<String, ConvertErrorKind> {
    if raw.contains(['\u{2028}', '\u{2029}']) {
        return Err(ConvertErrorKind::UnsupportedTextCharacter);
    }
    let lines: Vec<&str> = raw
        .split("\r\n")
        .flat_map(|line| line.split(['\r', '\n']))
        .collect();
    let is_blank = |line: &str| line.chars().all(|ch| ch == ' ' || ch == '\t');
    let last_non_empty = lines.iter().rposition(|line| !is_blank(line));
    let mut cooked = String::new();
    for (index, line) in lines.iter().enumerate() {
        let mut trimmed: &str = line;
        if index != 0 {
            trimmed = trimmed.trim_start_matches([' ', '\t']);
        }
        if index + 1 != lines.len() {
            trimmed = trimmed.trim_end_matches([' ', '\t']);
        }
        if trimmed.is_empty() {
            continue;
        }
        cooked.push_str(&decode_references(trimmed)?);
        if Some(index) != last_non_empty {
            cooked.push(' ');
        }
    }
    if cooked.contains('\t') {
        return Err(ConvertErrorKind::UnsupportedTextCharacter);
    }
    Ok(cooked)
}

// Decodes the XHTML character references that JSX compilers decode.
fn decode_references(text: &str) -> Result<String, ConvertErrorKind> {
    let mut out = String::with_capacity(text.len());
    let mut offset = 0;
    while let Some(relative) = text[offset..].find('&') {
        let start = offset + relative;
        out.push_str(&text[offset..start]);
        match decode_jsx_character_reference(text, start)
            .map_err(|_| ConvertErrorKind::UnsupportedTextCharacter)?
        {
            Some(reference) => {
                let ch = reference
                    .as_char()
                    .ok_or(ConvertErrorKind::UnsupportedTextCharacter)?;
                if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    return Err(ConvertErrorKind::WhitespaceReference);
                }
                if ch.is_control() {
                    return Err(ConvertErrorKind::UnsupportedTextCharacter);
                }
                out.push(ch);
                offset = reference.span.end();
            }
            None => {
                out.push('&');
                offset = start + 1;
            }
        }
    }
    out.push_str(&text[offset..]);
    if out.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err(ConvertErrorKind::UnsupportedTextCharacter);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsx_text_follows_react_whitespace_rules() {
        assert_eq!(cook_jsx_text("Hello").unwrap(), "Hello");
        assert_eq!(cook_jsx_text("  two  spaces  ").unwrap(), "  two  spaces  ");
        assert_eq!(cook_jsx_text("\n  Hello\n  ").unwrap(), "Hello");
        assert_eq!(cook_jsx_text("Clicked ").unwrap(), "Clicked ");
        assert_eq!(cook_jsx_text("\n  a\n\n   b  \n").unwrap(), "a b");
        assert_eq!(
            cook_jsx_text("trailing   \r\n   next").unwrap(),
            "trailing next"
        );
        assert_eq!(cook_jsx_text("a\r\rb").unwrap(), "a b");
        assert_eq!(cook_jsx_text("\n\t\tindented\n").unwrap(), "indented");
        assert_eq!(cook_jsx_text("\n   \n").unwrap(), "");
    }

    #[test]
    fn jsx_text_decodes_xhtml_references_only() {
        assert_eq!(cook_jsx_text("a &amp; b").unwrap(), "a & b");
        assert_eq!(
            cook_jsx_text("&copy; &#169; &#xA9;").unwrap(),
            "\u{a9} \u{a9} \u{a9}"
        );
        assert_eq!(cook_jsx_text("&nbsp;").unwrap(), "\u{a0}");
        // HTML decodes these legacy and HTML5-only names; JSX keeps them.
        assert_eq!(
            cook_jsx_text("&copy b &ampx &NotEqual;").unwrap(),
            "&copy b &ampx &NotEqual;"
        );
    }

    #[test]
    fn ambiguous_text_fails_closed() {
        for (raw, kind) in [
            ("a\tb", ConvertErrorKind::UnsupportedTextCharacter),
            ("a&#32;", ConvertErrorKind::WhitespaceReference),
            ("&#10;", ConvertErrorKind::WhitespaceReference),
            ("&#xD800;", ConvertErrorKind::UnsupportedTextCharacter),
            ("&#0;", ConvertErrorKind::UnsupportedTextCharacter),
            ("a\u{2028}b", ConvertErrorKind::UnsupportedTextCharacter),
            ("a\u{7}b", ConvertErrorKind::UnsupportedTextCharacter),
        ] {
            assert_eq!(cook_jsx_text(raw), Err(kind), "{raw:?}");
        }
    }

    #[test]
    fn style_keys_must_map_alike() {
        assert!(is_portable_style_key("color"));
        assert!(is_portable_style_key("backgroundColor"));
        assert!(is_portable_style_key("WebkitTransition"));
        assert!(is_portable_style_key("--accent"));
        assert!(!is_portable_style_key("msTransform"));
        assert!(!is_portable_style_key("--"));
        assert!(!is_portable_style_key("margin-top"));
        assert!(!is_portable_style_key(""));
    }
}
