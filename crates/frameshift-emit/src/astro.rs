//! A document model for Astro components and its canonical printer.
//!
//! Rules build an [`AstroComponent`] from converted markup and generated
//! pieces; [`print_astro_component`] writes it with explicit indentation and
//! line endings and records the provenance of every output byte. The printer
//! relies on two template behaviors of the pinned Astro compiler, observed
//! with the `astro7-preact10` profile: whitespace that contains a line break
//! between markup nodes is dropped, as in JSX, outside whitespace-sensitive
//! elements; and character references, quoted attribute values, and text
//! without line breaks pass through to the HTML output unchanged.

use std::fmt;

use frameshift_core::{SourceText, Span};

use crate::html::{escape_attribute, escape_text, is_void_element, is_whitespace_sensitive};
use crate::names::{AttributeName, ComponentName, HtmlTagName, JsIdentifier};
use crate::output::{Anchor, BuildError, EmitLimits, EmittedText, OutputBuilder};

/// An Astro component file: optional frontmatter, then the template.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AstroComponent {
    /// Frontmatter items in output order. The frontmatter fences are omitted
    /// when this is empty.
    pub frontmatter: Vec<FrontmatterItem>,
    /// Top-level template nodes.
    pub template: Vec<TemplateNode>,
}

/// One item of the frontmatter script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontmatterItem {
    /// A generated import declaration.
    Import(ImportItem),
    /// Source code copied verbatim, such as a statement and its leading
    /// comments. It must not contain a line that starts with `---`.
    Copied(Anchor),
    /// A generated `interface Props { ... }` declaration.
    PropsInterface {
        /// Members in output order.
        members: Vec<PropMember>,
        /// Construct responsible for the declaration.
        anchor: Anchor,
    },
    /// A generated `const { ... } = Astro.props;` declaration.
    PropsBinding {
        /// Bound property names in output order.
        names: Vec<JsIdentifier>,
        /// Construct responsible for the declaration.
        anchor: Anchor,
    },
}

/// A generated import declaration. Without bindings it is a side-effect
/// import such as `import '../styles/globals.css';`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportItem {
    /// Default import binding, if any.
    pub default: Option<JsIdentifier>,
    /// Named imports in output order.
    pub named: Vec<NamedImport>,
    /// Module specifier, printed as an escaped string literal.
    pub specifier: String,
    /// Construct responsible for the import.
    pub anchor: Anchor,
}

/// A named import `imported` or `imported as local`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedImport {
    /// Exported name.
    pub imported: JsIdentifier,
    /// Local binding, if it differs from the exported name.
    pub local: Option<JsIdentifier>,
}

/// A member of a generated `Props` interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropMember {
    /// Property name.
    pub name: JsIdentifier,
    /// Declared type.
    pub ty: PropType,
    /// Whether the property is optional.
    pub optional: bool,
}

/// A type that a generated `Props` member can declare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropType {
    /// `string`.
    String,
}

/// A node of an Astro template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateNode {
    /// An HTML element or a component.
    Element(Element),
    /// A fragment `<>...</>`.
    Fragment {
        /// Child nodes.
        children: Vec<TemplateNode>,
        /// Construct responsible for the fragment.
        anchor: Anchor,
    },
    /// Text, escaped when printed.
    Text {
        /// The text value.
        value: String,
        /// Construct responsible for the text.
        anchor: Anchor,
    },
    /// An expression `{...}`.
    Expression(Expression),
    /// A slot `<slot />` or `<slot name="..." />`.
    Slot {
        /// Slot name, absent for the default slot.
        name: Option<String>,
        /// Construct responsible for the slot.
        anchor: Anchor,
    },
}

/// An HTML element or a component with attributes and children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// Tag name.
    pub name: TagName,
    /// Attributes, spreads, and directives in output order.
    pub attributes: Vec<AttributeItem>,
    /// Child nodes.
    pub children: Vec<TemplateNode>,
    /// Construct responsible for the element.
    pub anchor: Anchor,
}

/// The tag name of an [`Element`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagName {
    /// An HTML element.
    Html(HtmlTagName),
    /// An Astro or framework component.
    Component(ComponentName),
}

/// An attribute, directive, or spread of an [`Element`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeItem {
    /// `name`, `name="text"`, or `name={expression}`.
    Named {
        /// Attribute or directive name.
        name: AttributeName,
        /// Attribute value.
        value: AttributeValue,
        /// Construct responsible for the attribute.
        anchor: Anchor,
    },
    /// `{...expression}`.
    Spread(Expression),
}

/// The value of a named attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    /// No value, as in `disabled` or `client:load`.
    Present,
    /// A quoted text value, escaped when printed.
    Text(String),
    /// An expression value.
    Expression(Expression),
}

/// A template expression assembled from source slices, nested markup, and
/// generated code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expression {
    /// Pieces in output order.
    pub pieces: Vec<ExpressionPiece>,
    /// Construct responsible for the expression. Source pieces are spans of
    /// this anchor's source.
    pub anchor: Anchor,
}

/// A piece of an [`Expression`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionPiece {
    /// Source code copied verbatim from the expression anchor's source.
    Source(Span),
    /// Converted markup, printed inline.
    Markup(TemplateNode),
    /// A generated identifier reference.
    Identifier(JsIdentifier),
    /// The generated literal `true`.
    True,
}

/// Indentation of generated lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indent {
    /// This many spaces per level, from 1 to 16.
    Spaces(u8),
    /// One tab per level.
    Tab,
}

/// Line ending of generated lines. Copied source keeps its own line endings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

/// Explicit formatting of printed components. The default is two spaces and
/// LF; callers pass the project's dominant style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintOptions {
    /// Indentation per nesting level.
    pub indent: Indent,
    /// Line ending of generated lines.
    pub line_ending: LineEnding,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            indent: Indent::Spaces(2),
            line_ending: LineEnding::Lf,
        }
    }
}

/// A rejected print. No partial output is returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintError {
    /// The indentation width is outside 1 to 16 spaces.
    InvalidIndent,
    /// An anchor or source piece names an unknown source or an invalid span.
    InvalidAnchor(Anchor),
    /// Copied frontmatter contains a line starting with `---`, which would
    /// end the frontmatter early.
    FrontmatterFence(Anchor),
    /// Text or an attribute value contains U+0000, which HTML cannot carry.
    NulCharacter(Anchor),
    /// A void HTML element has children.
    VoidElementChildren(Anchor),
    /// Markup nests deeper than the configured limit.
    TooDeep(Anchor),
    /// The output would exceed the configured byte limit.
    OutputTooLarge {
        /// Configured limit.
        limit: usize,
    },
    /// Storage for the output could not be reserved.
    AllocationFailed,
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIndent => f.write_str("indentation must be 1 to 16 spaces or a tab"),
            Self::InvalidAnchor(anchor) => write!(f, "invalid anchor {}", describe(anchor)),
            Self::FrontmatterFence(anchor) => {
                write!(
                    f,
                    "copied frontmatter at {} contains `---`",
                    describe(anchor)
                )
            }
            Self::NulCharacter(anchor) => write!(f, "U+0000 in text at {}", describe(anchor)),
            Self::VoidElementChildren(anchor) => {
                write!(f, "void element with children at {}", describe(anchor))
            }
            Self::TooDeep(anchor) => write!(f, "markup too deep at {}", describe(anchor)),
            Self::OutputTooLarge { limit } => write!(f, "output exceeds byte limit {limit}"),
            Self::AllocationFailed => f.write_str("could not allocate output"),
        }
    }
}

impl std::error::Error for PrintError {}

fn describe(anchor: &Anchor) -> String {
    format!(
        "source {} bytes {}..{}",
        anchor.source.0,
        anchor.span.start(),
        anchor.span.end()
    )
}

impl From<BuildError> for PrintError {
    fn from(error: BuildError) -> Self {
        match error {
            BuildError::InvalidAnchor(anchor) => Self::InvalidAnchor(anchor),
            BuildError::OutputTooLarge { limit } => Self::OutputTooLarge { limit },
            BuildError::AllocationFailed => Self::AllocationFailed,
        }
    }
}

/// Prints `component` canonically.
///
/// Generated lines use `options`. Children of an element are placed on
/// separate indented lines only when none of them is text and the element is
/// not whitespace-sensitive (`pre`, `textarea`, `listing`); otherwise they
/// are printed inline, so no whitespace is added next to text. Markup inside
/// expressions is printed inline. Text is escaped for HTML and Astro (`&`,
/// `<`, `>`, `{`, `}`, tab, line breaks, and U+00A0 become references), and
/// attribute text escapes `&`, `"`, tab, and line breaks. Source pieces and
/// copied frontmatter are emitted verbatim. The file ends with a line ending.
pub fn print_astro_component(
    component: &AstroComponent,
    sources: &[&SourceText],
    options: PrintOptions,
    limits: EmitLimits,
) -> Result<EmittedText, PrintError> {
    let indent = match options.indent {
        Indent::Spaces(width @ 1..=16) => " ".repeat(usize::from(width)),
        Indent::Spaces(_) => return Err(PrintError::InvalidIndent),
        Indent::Tab => "\t".to_owned(),
    };
    let eol = match options.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut printer = Printer {
        out: OutputBuilder::new(sources, limits),
        sources,
        indent,
        eol,
        max_depth: limits.max_depth,
    };
    printer.frontmatter(&component.frontmatter)?;
    printer.template(&component.template)?;
    Ok(printer.out.finish())
}

struct Printer<'s> {
    out: OutputBuilder<'s>,
    sources: &'s [&'s SourceText],
    indent: String,
    eol: &'static str,
    max_depth: usize,
}

impl Printer<'_> {
    fn frontmatter(&mut self, items: &[FrontmatterItem]) -> Result<(), PrintError> {
        let Some(first) = items.first() else {
            return Ok(());
        };
        let fence = format!("---{}", self.eol);
        self.out.push_generated(&fence, item_anchor(first))?;
        let mut previous_was_import = None;
        for item in items {
            let is_import = matches!(item, FrontmatterItem::Import(_));
            if previous_was_import == Some(true) && !is_import {
                self.out.push_generated(self.eol, item_anchor(item))?;
            }
            previous_was_import = Some(is_import);
            self.frontmatter_item(item)?;
        }
        let last = items.last().map_or_else(|| item_anchor(first), item_anchor);
        self.out.push_generated(&fence, last)?;
        Ok(())
    }

    fn frontmatter_item(&mut self, item: &FrontmatterItem) -> Result<(), PrintError> {
        let eol = self.eol;
        match item {
            FrontmatterItem::Import(import) => {
                let mut line = String::from("import ");
                let mut has_binding = false;
                if let Some(default) = &import.default {
                    line.push_str(default.as_str());
                    has_binding = true;
                }
                if !import.named.is_empty() {
                    if has_binding {
                        line.push_str(", ");
                    }
                    line.push_str("{ ");
                    let named: Vec<String> = import
                        .named
                        .iter()
                        .map(|named| match &named.local {
                            Some(local) => format!("{} as {local}", named.imported),
                            None => named.imported.to_string(),
                        })
                        .collect();
                    line.push_str(&named.join(", "));
                    line.push_str(" }");
                    has_binding = true;
                }
                if has_binding {
                    line.push_str(" from ");
                }
                line.push_str(&js_string(&import.specifier));
                line.push(';');
                line.push_str(eol);
                self.out.push_generated(&line, import.anchor)?;
            }
            FrontmatterItem::Copied(anchor) => {
                let text = self
                    .sources
                    .get(anchor.source.0 as usize)
                    .and_then(|source| source.slice(anchor.span).ok())
                    .ok_or(PrintError::InvalidAnchor(*anchor))?;
                let fence = text
                    .split(['\n', '\r'])
                    .any(|line| line.trim_start().starts_with("---"));
                if fence {
                    return Err(PrintError::FrontmatterFence(*anchor));
                }
                self.out.push_copied(*anchor)?;
                self.out.push_generated(eol, *anchor)?;
            }
            FrontmatterItem::PropsInterface { members, anchor } => {
                let mut text = format!("interface Props {{{eol}");
                for member in members {
                    let ty = match member.ty {
                        PropType::String => "string",
                    };
                    let optional = if member.optional { "?" } else { "" };
                    text.push_str(&format!(
                        "{}{}{optional}: {ty};{eol}",
                        self.indent, member.name
                    ));
                }
                text.push_str(&format!("}}{eol}"));
                self.out.push_generated(&text, *anchor)?;
            }
            FrontmatterItem::PropsBinding { names, anchor } => {
                let names: Vec<&str> = names.iter().map(JsIdentifier::as_str).collect();
                let text = format!("const {{ {} }} = Astro.props;{eol}", names.join(", "));
                self.out.push_generated(&text, *anchor)?;
            }
        }
        Ok(())
    }

    fn template(&mut self, nodes: &[TemplateNode]) -> Result<(), PrintError> {
        let block = !nodes.iter().any(is_text);
        for (index, node) in nodes.iter().enumerate() {
            self.node(node, 0, false, 0)?;
            if block || index + 1 == nodes.len() {
                self.out.push_generated(self.eol, node_anchor(node))?;
            }
        }
        Ok(())
    }

    // `inline` is true inside expressions and whitespace-sensitive elements,
    // where no line breaks or indentation may be added.
    fn node(
        &mut self,
        node: &TemplateNode,
        level: usize,
        inline: bool,
        depth: usize,
    ) -> Result<(), PrintError> {
        if depth > self.max_depth {
            return Err(PrintError::TooDeep(node_anchor(node)));
        }
        match node {
            TemplateNode::Element(element) => self.element(element, level, inline, depth),
            TemplateNode::Fragment { children, anchor } => {
                self.out.push_generated("<>", *anchor)?;
                self.children(children, level, inline, false, depth)?;
                self.out.push_generated("</>", *anchor)?;
                Ok(())
            }
            TemplateNode::Text { value, anchor } => {
                if value.contains('\0') {
                    return Err(PrintError::NulCharacter(*anchor));
                }
                self.out.push_generated(&escape_text(value), *anchor)?;
                Ok(())
            }
            TemplateNode::Expression(expression) => self.expression(expression, "{", depth),
            TemplateNode::Slot { name, anchor } => {
                let text = match name {
                    Some(name) => format!("<slot name=\"{}\" />", escape_attribute(name)),
                    None => "<slot />".to_owned(),
                };
                if name.as_deref().is_some_and(|name| name.contains('\0')) {
                    return Err(PrintError::NulCharacter(*anchor));
                }
                self.out.push_generated(&text, *anchor)?;
                Ok(())
            }
        }
    }

    fn element(
        &mut self,
        element: &Element,
        level: usize,
        inline: bool,
        depth: usize,
    ) -> Result<(), PrintError> {
        let anchor = element.anchor;
        let (name, void, sensitive) = match &element.name {
            TagName::Html(name) => (
                name.as_str(),
                is_void_element(name.as_str()),
                is_whitespace_sensitive(name.as_str()),
            ),
            TagName::Component(name) => (name.as_str(), false, false),
        };
        if void && !element.children.is_empty() {
            return Err(PrintError::VoidElementChildren(anchor));
        }
        self.out.push_generated(&format!("<{name}"), anchor)?;
        for attribute in &element.attributes {
            self.attribute(attribute, depth)?;
        }
        if element.children.is_empty() {
            let close = match &element.name {
                TagName::Html(_) if !void => format!("></{name}>"),
                TagName::Html(_) | TagName::Component(_) => " />".to_owned(),
            };
            self.out.push_generated(&close, anchor)?;
            return Ok(());
        }
        self.out.push_generated(">", anchor)?;
        let child_inline = inline || sensitive;
        let block = !child_inline && !element.children.iter().any(is_text);
        self.children(&element.children, level, child_inline, block, depth)?;
        if block {
            let line = format!("{}{}", self.eol, self.indent.repeat(level));
            self.out.push_generated(&line, anchor)?;
        }
        self.out.push_generated(&format!("</{name}>"), anchor)?;
        Ok(())
    }

    fn children(
        &mut self,
        children: &[TemplateNode],
        level: usize,
        inline: bool,
        block: bool,
        depth: usize,
    ) -> Result<(), PrintError> {
        for child in children {
            if block {
                let line = format!("{}{}", self.eol, self.indent.repeat(level + 1));
                self.out.push_generated(&line, node_anchor(child))?;
            }
            self.node(child, level + 1, inline, depth + 1)?;
        }
        Ok(())
    }

    fn attribute(&mut self, attribute: &AttributeItem, depth: usize) -> Result<(), PrintError> {
        match attribute {
            AttributeItem::Named {
                name,
                value,
                anchor,
            } => {
                self.out
                    .push_generated(&format!(" {}", name.as_str()), *anchor)?;
                match value {
                    AttributeValue::Present => {}
                    AttributeValue::Text(text) => {
                        if text.contains('\0') {
                            return Err(PrintError::NulCharacter(*anchor));
                        }
                        let text = format!("=\"{}\"", escape_attribute(text));
                        self.out.push_generated(&text, *anchor)?;
                    }
                    AttributeValue::Expression(expression) => {
                        self.out.push_generated("=", *anchor)?;
                        self.expression(expression, "{", depth)?;
                    }
                }
            }
            AttributeItem::Spread(expression) => {
                self.out.push_generated(" ", expression.anchor)?;
                self.expression(expression, "{...", depth)?;
            }
        }
        Ok(())
    }

    fn expression(
        &mut self,
        expression: &Expression,
        open: &str,
        depth: usize,
    ) -> Result<(), PrintError> {
        let anchor = expression.anchor;
        if depth > self.max_depth {
            return Err(PrintError::TooDeep(anchor));
        }
        self.out.push_generated(open, anchor)?;
        for piece in &expression.pieces {
            match piece {
                ExpressionPiece::Source(span) => self.out.push_copied(Anchor {
                    source: anchor.source,
                    span: *span,
                })?,
                ExpressionPiece::Markup(node) => self.node(node, 0, true, depth + 1)?,
                ExpressionPiece::Identifier(identifier) => {
                    self.out.push_generated(identifier.as_str(), anchor)?;
                }
                ExpressionPiece::True => self.out.push_generated("true", anchor)?,
            }
        }
        self.out.push_generated("}", anchor)?;
        Ok(())
    }
}

fn is_text(node: &TemplateNode) -> bool {
    matches!(node, TemplateNode::Text { .. })
}

fn node_anchor(node: &TemplateNode) -> Anchor {
    match node {
        TemplateNode::Element(element) => element.anchor,
        TemplateNode::Fragment { anchor, .. }
        | TemplateNode::Text { anchor, .. }
        | TemplateNode::Slot { anchor, .. } => *anchor,
        TemplateNode::Expression(expression) => expression.anchor,
    }
}

fn item_anchor(item: &FrontmatterItem) -> Anchor {
    match item {
        FrontmatterItem::Import(import) => import.anchor,
        FrontmatterItem::Copied(anchor)
        | FrontmatterItem::PropsInterface { anchor, .. }
        | FrontmatterItem::PropsBinding { anchor, .. } => *anchor,
    }
}

/// Prints `value` as a single-quoted JavaScript string literal. Backslashes,
/// quotes, and control and line-separator characters are escaped.
pub(crate) fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{{{:04X}}}", u32::from(ch))),
            ch if ch.is_control() => out.push_str(&format!("\\u{{{:04X}}}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}
