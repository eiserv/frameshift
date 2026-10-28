//! Conversion rules of migrate v0, promoted from the E2 driver (`D-101`).
//!
//! Each rule renders one source file into one output file or returns the
//! reason why the file is outside the v0 subset. Rules never guess: an
//! import must resolve through the module graph to a file the plan writes,
//! and every construct outside the recognized shapes refuses the file.

use std::collections::BTreeMap;

use frameshift_core::{SourcePath, SourceText, Span};
use frameshift_emit::{
    Anchor, AstroComponent, AttributeItem, AttributeName, AttributeValue, ComponentName,
    ComponentReference, ComponentStrategy, Element, ElementResolver, EmittedText, Expression,
    ExpressionPiece, FrontmatterItem, HtmlTagName, ImportItem, JsIdentifier, JsxConversion,
    NamedImport, PrintOptions, PropMember, PropType, RegionOrigin, SourceId, TagName, TemplateNode,
    convert_jsx, print_astro_component,
};
use frameshift_modules::{ModuleGraph, Resolution};
use frameshift_syntax::{
    ChildList, FunctionKind, JsDialect, NodeId, SourceGoal, SyntaxNodeKind, SyntaxTree,
    TemplatePart, TypeScriptKind, VariableKind, parse_js_program,
};
use frameshift_transform::{Edit, EditOrder, apply_edits};

use crate::limits::Limits;

// Props of `next/link` that change navigation behavior and have no HTML equivalent.
const NEXT_LINK_PROPS: &[&str] = &[
    "as",
    "legacyBehavior",
    "locale",
    "onNavigate",
    "passHref",
    "prefetch",
    "replace",
    "scroll",
    "shallow",
];

/// Why a file stays outside the v0 subset. The text names no paths.
pub(crate) type Refusal = String;
type Result<T> = std::result::Result<T, Refusal>;

fn refuse<T>(message: impl Into<String>) -> Result<T> {
    Err(message.into())
}

/// How an output file can be imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetKind {
    /// An Astro component, imported as a default binding with its extension.
    Astro,
    /// JavaScript or TypeScript, imported without its extension.
    Script,
    /// A stylesheet or asset, imported with its extension.
    Asset,
}

/// A file that the plan writes, and how importers refer to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) output: String,
    pub(crate) kind: TargetKind,
}

/// Static metadata of a layout or page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub(crate) title: Option<Title>,
    pub(crate) description: Option<(String, Span)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Title {
    Plain(String, Span),
    Template {
        default: String,
        default_span: Span,
        template: String,
    },
}

/// The facts of the root layout that pages resolve their metadata against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootFacts {
    pub(crate) metadata: Metadata,
}

/// What a converted file is in the Astro project.
pub(crate) enum Role<'a> {
    /// A page wrapped in its nearest layout.
    Page {
        layout_name: &'a str,
        layout_output: &'a str,
        /// The props that the nearest layout accepts.
        layout_props: &'a [&'static str],
        root: &'a RootFacts,
        root_source: &'a SourceText,
    },
    /// A root layout that owns the document head.
    RootLayout,
    /// A nested layout that wraps its parent.
    NestedLayout {
        parent_name: &'a str,
        parent_output: &'a str,
        props: &'a [&'static str],
    },
    /// A component converted to its own Astro file.
    Component,
}

/// Shared inputs of every rule.
pub(crate) struct Context<'a> {
    pub(crate) graph: &'a ModuleGraph,
    pub(crate) targets: &'a BTreeMap<SourcePath, Target>,
    pub(crate) options: PrintOptions,
    pub(crate) limits: &'a Limits,
}

/// The JavaScript dialect of a source extension, if v0 handles it.
pub(crate) fn dialect(path: &str) -> Option<JsDialect> {
    let extension = path.rsplit_once('.')?.1;
    match extension {
        "tsx" => Some(JsDialect::Tsx),
        "ts" if !path.ends_with(".d.ts") => Some(JsDialect::TypeScript),
        "jsx" | "js" | "mjs" => Some(JsDialect::Jsx),
        _ => None,
    }
}

fn parse<'t>(text: &'t SourceText, path: &str, limits: &Limits) -> Result<SyntaxTree<'t>> {
    let Some(dialect) = dialect(path) else {
        return refuse("the file extension is not converted by migrate v0");
    };
    parse_js_program(text, SourceGoal::Module, dialect, limits.parse)
        .map_err(|_| "the file does not parse strictly".to_owned())
}

/// The specifier that the output file at `from` uses for `target`.
pub(crate) fn specifier(from: &str, target: &Target) -> String {
    let relative = relative_path(directory(from), &target.output);
    if target.kind == TargetKind::Script {
        for extension in [".tsx", ".ts", ".jsx", ".js"] {
            if let Some(stem) = relative.strip_suffix(extension) {
                return stem.to_owned();
            }
        }
    }
    relative
}

/// A relative specifier from one output directory to an output path.
pub(crate) fn relative_path(from_directory: &str, to: &str) -> String {
    let from: Vec<&str> = from_directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let target: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&target[common..]);
    let joined = parts.join("/");
    if joined.starts_with("../") {
        joined
    } else {
        format!("./{joined}")
    }
}

pub(crate) fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn kind(tree: &SyntaxTree<'_>, id: NodeId) -> Result<SyntaxNodeKind> {
    tree.node(id)
        .map(|node| node.kind())
        .ok_or_else(|| "a syntax node is outside the tree".to_owned())
}

fn node_span(tree: &SyntaxTree<'_>, id: NodeId) -> Result<Span> {
    tree.node(id)
        .map(|node| node.span())
        .ok_or_else(|| "a syntax node is outside the tree".to_owned())
}

fn node_text<'t>(tree: &SyntaxTree<'t>, id: NodeId) -> Result<&'t str> {
    let span = node_span(tree, id)?;
    tree.source()
        .slice(span)
        .map_err(|_| "a syntax span is outside the source".to_owned())
}

fn list(tree: &SyntaxTree<'_>, list: ChildList) -> Result<Vec<NodeId>> {
    tree.children(list)
        .map(<[NodeId]>::to_vec)
        .ok_or_else(|| "a child list is outside the tree".to_owned())
}

fn string_value(tree: &SyntaxTree<'_>, id: NodeId) -> Result<String> {
    tree.string_literal_value(id)
        .ok_or_else(|| "a string literal was expected".to_owned())
}

fn contains_jsx(tree: &SyntaxTree<'_>, id: NodeId) -> Result<bool> {
    if matches!(
        kind(tree, id)?,
        SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
    ) {
        return Ok(true);
    }
    for child in tree.child_nodes(id).into_iter().flatten() {
        if contains_jsx(tree, child)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn program_body(tree: &SyntaxTree<'_>) -> Result<Vec<NodeId>> {
    let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root())? else {
        return refuse("the tree root is not a program");
    };
    list(tree, body)
}

// The span from the first to the last comment between `start` and `end`.
fn comments_between(tree: &SyntaxTree<'_>, start: usize, end: usize) -> Option<Span> {
    let inside: Vec<Span> = tree
        .comments()
        .iter()
        .map(|comment| comment.span)
        .filter(|comment| comment.start() >= start && comment.end() <= end)
        .collect();
    let first = inside.first()?;
    let last = inside.last()?;
    Span::new(first.start(), last.end()).ok()
}

fn identifier(name: &str) -> Result<JsIdentifier> {
    JsIdentifier::new(name).ok_or_else(|| "a binding is not a plain identifier".to_owned())
}

fn tag(name: &str) -> HtmlTagName {
    HtmlTagName::new(name).unwrap_or_else(|| unreachable!("constant tag name {name}"))
}

fn attribute(name: &str) -> AttributeName {
    AttributeName::new(name).unwrap_or_else(|| unreachable!("constant attribute name {name}"))
}

/// The resolution of the import whose specifier literal is `source`.
fn resolution<'g>(
    graph: &'g ModuleGraph,
    path: &SourcePath,
    tree: &SyntaxTree<'_>,
    source: NodeId,
) -> Result<&'g Resolution> {
    let span = node_span(tree, source)?;
    graph
        .node(path)
        .and_then(|node| node.edges.iter().find(|edge| edge.span == span))
        .map(|edge| &edge.resolution)
        .ok_or_else(|| "an import has no module graph edge".to_owned())
}

enum Import {
    Link { local: String },
    Dropped,
    Generated(ImportItem),
}

struct Analysis {
    imports: Vec<Import>,
    copies: Vec<Span>,
    metadata: Option<Metadata>,
    function: Span,
    props: Vec<String>,
    returned: NodeId,
}

struct Resolver {
    link: Option<String>,
    components: Vec<(String, ComponentName)>,
}

impl ElementResolver for Resolver {
    fn resolve(&mut self, reference: ComponentReference<'_>) -> Option<ComponentStrategy> {
        if self.link.as_deref() == Some(reference.spelling) {
            return HtmlTagName::new("a").map(ComponentStrategy::Html);
        }
        self.components
            .iter()
            .find(|(local, _)| local == reference.spelling)
            .map(|(_, name)| ComponentStrategy::Component {
                name: name.clone(),
                directive: None,
            })
    }
}

/// The name of the single function component that a component file exports.
pub(crate) fn component_name(path: &str, text: &SourceText, limits: &Limits) -> Result<String> {
    let tree = parse(text, path, limits)?;
    let mut names = Vec::new();
    for item in program_body(&tree)? {
        let declaration = match kind(&tree, item)? {
            SyntaxNodeKind::ExportDefault { declaration }
            | SyntaxNodeKind::ExportDeclaration { declaration } => declaration,
            _ => continue,
        };
        if let SyntaxNodeKind::FunctionDeclaration {
            name: Some(name), ..
        } = kind(&tree, declaration)?
        {
            names.push(node_text(&tree, name)?.to_owned());
        } else {
            return refuse("a component module exports something other than a function");
        }
    }
    match names.as_slice() {
        [name] if ComponentName::new(name).is_some() => Ok(name.clone()),
        [_] => refuse("the exported function is not a component name"),
        _ => refuse("a component module must export exactly one named function component"),
    }
}

/// Reads the static metadata of a root layout.
pub(crate) fn root_facts(
    path: &SourcePath,
    text: &SourceText,
    limits: &Limits,
) -> Result<RootFacts> {
    let tree = parse(text, path.as_str(), limits)?;
    let mut metadata = Metadata::default();
    for item in program_body(&tree)? {
        if let SyntaxNodeKind::ExportDeclaration { declaration } = kind(&tree, item)?
            && let Some(found) = metadata_export(&tree, declaration, true)?
        {
            metadata = found;
        }
    }
    Ok(RootFacts { metadata })
}

/// Converts a page, layout, or component file into an Astro component.
///
/// Returns the output and the identifiers of the value-dependent
/// assumptions that the conversion relies on, which make it `review`.
pub(crate) fn astro(
    context: &Context<'_>,
    path: &SourcePath,
    text: &SourceText,
    output: &str,
    role: &Role<'_>,
) -> Result<(EmittedText, Vec<&'static str>)> {
    let tree = parse(text, path.as_str(), context.limits)?;
    let source = SourceId(0);
    let analysis = analyze(context, path, &tree, output, role)?;
    let anchor = |span: Span| Anchor { source, span };
    let function_anchor = anchor(analysis.function);
    let mut frontmatter = Vec::new();
    let mut resolver = Resolver {
        link: None,
        components: Vec::new(),
    };
    for import in analysis.imports {
        match import {
            Import::Link { local } => resolver.link = Some(local),
            Import::Dropped => {}
            Import::Generated(item) => {
                for binding in item.default.iter().chain(
                    item.named
                        .iter()
                        .map(|n| n.local.as_ref().unwrap_or(&n.imported)),
                ) {
                    if let Some(name) = ComponentName::new(binding.as_str()) {
                        resolver
                            .components
                            .push((binding.as_str().to_owned(), name));
                    }
                }
                frontmatter.push(FrontmatterItem::Import(item));
            }
        }
    }
    let children = analysis.props.iter().any(|prop| prop == "children");
    let mut props: Vec<String> = analysis
        .props
        .iter()
        .filter(|prop| prop.as_str() != "children")
        .cloned()
        .collect();
    match role {
        Role::Page {
            layout_name,
            layout_output,
            ..
        } => {
            if !analysis.props.is_empty() {
                return refuse("pages that read props are not converted by migrate v0");
            }
            frontmatter.push(FrontmatterItem::Import(ImportItem {
                default: Some(identifier(layout_name)?),
                named: Vec::new(),
                specifier: relative_path(directory(output), layout_output),
                anchor: function_anchor,
            }));
        }
        Role::RootLayout | Role::NestedLayout { .. } => {
            if !props.is_empty() {
                return refuse("layouts that read props other than children are not converted");
            }
            let declared: Vec<&str> = match role {
                Role::NestedLayout {
                    parent_name,
                    parent_output,
                    props,
                } => {
                    if analysis.metadata.is_some() {
                        return refuse("nested layout metadata is not converted by migrate v0");
                    }
                    frontmatter.push(FrontmatterItem::Import(ImportItem {
                        default: Some(identifier(parent_name)?),
                        named: Vec::new(),
                        specifier: relative_path(directory(output), parent_output),
                        anchor: function_anchor,
                    }));
                    props.to_vec()
                }
                Role::RootLayout | Role::Page { .. } | Role::Component => {
                    layout_props(analysis.metadata.as_ref())
                }
            };
            props = declared.iter().map(|prop| (*prop).to_owned()).collect();
            if !props.is_empty() {
                let members = props
                    .iter()
                    .map(|prop| {
                        Ok(PropMember {
                            name: identifier(prop)?,
                            ty: PropType::String,
                            optional: false,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                frontmatter.push(FrontmatterItem::PropsInterface {
                    members,
                    anchor: function_anchor,
                });
            }
        }
        Role::Component => {
            if analysis.metadata.is_some() {
                return refuse("only pages and layouts export metadata");
            }
        }
    }
    if children && matches!(role, Role::Page { .. }) {
        return refuse("pages do not render children");
    }
    if !props.is_empty() {
        frontmatter.push(FrontmatterItem::PropsBinding {
            names: props
                .iter()
                .map(|prop| identifier(prop))
                .collect::<Result<Vec<_>>>()?,
            anchor: function_anchor,
        });
    }
    frontmatter.extend(
        analysis
            .copies
            .iter()
            .map(|span| FrontmatterItem::Copied(anchor(*span))),
    );
    check_links(&tree, resolver.link.as_deref(), analysis.returned)?;
    let conversion = JsxConversion {
        children_binding: children.then_some("children"),
        ..JsxConversion::default()
    };
    let markup = convert_jsx(&tree, source, analysis.returned, &conversion, &mut resolver)
        .map_err(|error| format!("the JSX is outside the converted subset ({error})"))?;
    let mut assumptions: Vec<&'static str> =
        markup.assumptions.iter().map(|a| a.kind.id()).collect();
    assumptions.sort_unstable();
    assumptions.dedup();
    let mut template = markup.nodes;
    let root_source;
    let sources: Vec<&SourceText> = match role {
        Role::RootLayout => {
            template = layout_template(template, analysis.metadata.as_ref(), function_anchor)?;
            vec![text]
        }
        Role::NestedLayout {
            parent_name, props, ..
        } => {
            template = vec![wrap(parent_name, props, template, function_anchor)?];
            vec![text]
        }
        Role::Page {
            layout_name,
            layout_props,
            root,
            root_source: layout_text,
            ..
        } => {
            root_source = *layout_text;
            template = page_template(
                template,
                analysis.metadata.as_ref(),
                layout_name,
                layout_props,
                root,
                function_anchor,
            )?;
            vec![text, root_source]
        }
        Role::Component => vec![text],
    };
    let component = AstroComponent {
        frontmatter,
        template,
    };
    let emitted = print_astro_component(&component, &sources, context.options, context.limits.emit)
        .map_err(|error| format!("the Astro component cannot be printed ({error})"))?;
    emitted
        .verify(&sources)
        .map_err(|error| format!("emitted provenance is inconsistent ({error})"))?;
    check_comments(&tree, source, &emitted)?;
    Ok((emitted, assumptions))
}

/// The props that a layout with this metadata receives from its pages.
pub(crate) fn layout_props(metadata: Option<&Metadata>) -> Vec<&'static str> {
    let mut props = Vec::new();
    if metadata.is_some_and(|m| m.title.is_some()) {
        props.push("title");
    }
    if metadata.is_some_and(|m| m.description.is_some()) {
        props.push("description");
    }
    props
}

// Every comment of a converted module must be copied into the output.
fn check_comments(tree: &SyntaxTree<'_>, source: SourceId, emitted: &EmittedText) -> Result<()> {
    let copied: Vec<Anchor> = emitted
        .regions()
        .iter()
        .filter_map(|region| match region.origin {
            RegionOrigin::Copied(anchor) if anchor.source == source => Some(anchor),
            RegionOrigin::Copied(_) | RegionOrigin::Generated(_) => None,
        })
        .collect();
    for comment in tree.comments() {
        let span = comment.span;
        if !copied
            .iter()
            .any(|anchor| anchor.span.start() <= span.start() && anchor.span.end() >= span.end())
        {
            return refuse("a comment would be dropped by the conversion");
        }
    }
    Ok(())
}

fn analyze(
    context: &Context<'_>,
    path: &SourcePath,
    tree: &SyntaxTree<'_>,
    output: &str,
    role: &Role<'_>,
) -> Result<Analysis> {
    let is_component = matches!(role, Role::Component);
    let mut imports = Vec::new();
    let mut copies = Vec::new();
    let mut metadata = None;
    let mut function = None;
    let mut previous_end = 0;
    for item in program_body(tree)? {
        let item_span = node_span(tree, item)?;
        if let Some(comments) = comments_between(tree, previous_end, item_span.start()) {
            copies.push(comments);
        }
        previous_end = item_span.end();
        match kind(tree, item)? {
            SyntaxNodeKind::ImportDeclaration { .. } => {
                imports.push(import(context, path, tree, item, output)?);
            }
            SyntaxNodeKind::TypeScript {
                kind: TypeScriptKind::TypeOnly,
                children,
            } => {
                // Types have no runtime behavior; Astro strips them anyway.
                let declaration = list(tree, children)?;
                let [declaration] = declaration.as_slice() else {
                    return refuse("an unexpected type-only declaration");
                };
                if !matches!(
                    kind(tree, *declaration)?,
                    SyntaxNodeKind::ImportDeclaration { .. }
                ) {
                    return refuse("only type-only imports are dropped");
                }
                imports.push(Import::Dropped);
            }
            SyntaxNodeKind::ExportDeclaration { declaration } => {
                if let SyntaxNodeKind::FunctionDeclaration { .. } = kind(tree, declaration)?
                    && is_component
                {
                    if function.is_some() {
                        return refuse("a component module exports several functions");
                    }
                    function = Some(declaration);
                } else if is_dropped_segment_option(tree, declaration)? {
                } else if let Some(found) =
                    metadata_export(tree, declaration, matches!(role, Role::RootLayout))?
                {
                    if metadata.is_some() {
                        return refuse("metadata is exported twice");
                    }
                    metadata = Some(found);
                } else {
                    return refuse("an export other than metadata or the component");
                }
            }
            SyntaxNodeKind::ExportDefault { declaration } => {
                if function.is_some() {
                    return refuse("a module exports several functions");
                }
                function = Some(declaration);
            }
            SyntaxNodeKind::VariableDeclaration {
                kind: VariableKind::Const,
                ..
            }
            | SyntaxNodeKind::FunctionDeclaration { .. } => {
                if contains_jsx(tree, item)? {
                    return refuse("module-level JSX is not converted by migrate v0");
                }
                copies.push(item_span);
            }
            _ => return refuse("a module item outside the converted subset"),
        }
    }
    if let Some(comments) = comments_between(tree, previous_end, tree.source().len()) {
        copies.push(comments);
    }
    let Some(function) = function else {
        return refuse("no exported function component");
    };
    let SyntaxNodeKind::FunctionDeclaration {
        name: _,
        parameters,
        body,
        kind: function_kind,
    } = kind(tree, function)?
    else {
        return refuse("the component must be a function declaration");
    };
    if !matches!(function_kind, FunctionKind::Normal | FunctionKind::Async) {
        return refuse("generator components are not converted");
    }
    let props = props(tree, parameters)?;
    let SyntaxNodeKind::FunctionBody {
        body: statements, ..
    } = kind(tree, body)?
    else {
        return refuse("the component has no function body");
    };
    let statements = list(tree, statements)?;
    let Some((&last, leading)) = statements.split_last() else {
        return refuse("the component body is empty");
    };
    let mut previous_end = node_span(tree, body)?.start() + 1;
    for &statement in leading {
        let statement_span = node_span(tree, statement)?;
        if let Some(comments) = comments_between(tree, previous_end, statement_span.start()) {
            copies.push(comments);
        }
        previous_end = statement_span.end();
        let SyntaxNodeKind::VariableDeclaration { .. } = kind(tree, statement)? else {
            return refuse("only declarations may precede the final return");
        };
        if contains_jsx(tree, statement)? {
            return refuse("JSX outside the returned markup is not converted");
        }
        copies.push(statement_span);
    }
    let last_span = node_span(tree, last)?;
    if let Some(comments) = comments_between(tree, previous_end, last_span.start()) {
        copies.push(comments);
    }
    let SyntaxNodeKind::ReturnStatement {
        argument: Some(returned),
    } = kind(tree, last)?
    else {
        return refuse("the body must end with a return of JSX");
    };
    Ok(Analysis {
        imports,
        copies,
        metadata,
        function: node_span(tree, function)?,
        props,
        returned,
    })
}

// Segment options that static output makes irrelevant (MIGRATION_SPEC.md
// section 8): `dynamic` other than `force-dynamic`, and `runtime`.
fn is_dropped_segment_option(tree: &SyntaxTree<'_>, declaration: NodeId) -> Result<bool> {
    let Some((name, init)) = const_export(tree, declaration)? else {
        return Ok(false);
    };
    if !matches!(kind(tree, init)?, SyntaxNodeKind::StringLiteral) {
        return Ok(false);
    }
    let value = string_value(tree, init)?;
    Ok(match name.as_str() {
        "dynamic" => matches!(value.as_str(), "auto" | "force-static" | "error"),
        "runtime" => true,
        _ => false,
    })
}

// The name and initializer of `export const <name> = <init>`.
fn const_export(tree: &SyntaxTree<'_>, declaration: NodeId) -> Result<Option<(String, NodeId)>> {
    let SyntaxNodeKind::VariableDeclaration {
        kind: VariableKind::Const,
        declarators,
    } = kind(tree, declaration)?
    else {
        return Ok(None);
    };
    let declarators = list(tree, declarators)?;
    let [declarator] = declarators.as_slice() else {
        return Ok(None);
    };
    let SyntaxNodeKind::VariableDeclarator {
        target,
        init: Some(init),
    } = kind(tree, *declarator)?
    else {
        return Ok(None);
    };
    let binding = if let SyntaxNodeKind::TypeScript {
        kind: TypeScriptKind::TypedBinding { .. },
        children,
    } = kind(tree, target)?
    {
        match list(tree, children)?.first() {
            Some(first) => *first,
            None => return Ok(None),
        }
    } else {
        target
    };
    if !matches!(
        kind(tree, binding)?,
        SyntaxNodeKind::BindingIdentifier { .. } | SyntaxNodeKind::Identifier { .. }
    ) {
        return Ok(None);
    }
    Ok(Some((node_text(tree, binding)?.to_owned(), init)))
}

fn import(
    context: &Context<'_>,
    path: &SourcePath,
    tree: &SyntaxTree<'_>,
    item: NodeId,
    output: &str,
) -> Result<Import> {
    let SyntaxNodeKind::ImportDeclaration {
        specifiers,
        source,
        attributes: None,
    } = kind(tree, item)?
    else {
        return refuse("import attributes are not converted");
    };
    let written = string_value(tree, source)?;
    let specifiers = list(tree, specifiers)?;
    let anchor = Anchor {
        source: SourceId(0),
        span: node_span(tree, item)?,
    };
    let mut default = None;
    let mut named = Vec::new();
    for &specifier in &specifiers {
        match kind(tree, specifier)? {
            SyntaxNodeKind::ImportDefaultSpecifier { local } => {
                default = Some(identifier(node_text(tree, local)?)?);
            }
            SyntaxNodeKind::ImportSpecifier { imported, local } => {
                let local = identifier(node_text(tree, local)?)?;
                match imported {
                    Some(imported) => named.push(NamedImport {
                        imported: identifier(node_text(tree, imported)?)?,
                        local: Some(local),
                    }),
                    None => named.push(NamedImport {
                        imported: local,
                        local: None,
                    }),
                }
            }
            _ => return refuse("namespace and type-only import specifiers are not converted"),
        }
    }
    match resolution(context.graph, path, tree, source)? {
        Resolution::Local { path: target, .. } => {
            let Some(target) = context.targets.get(target) else {
                return refuse("it imports a file that migrate v0 does not migrate");
            };
            let specifier = specifier(output, target);
            match target.kind {
                TargetKind::Astro => {
                    // An Astro file has only its default export.
                    let local = match (default, named.as_slice()) {
                        (Some(local), []) => local,
                        (None, [only]) => {
                            only.local.clone().unwrap_or_else(|| only.imported.clone())
                        }
                        _ => {
                            return refuse(
                                "a converted component is imported with several bindings",
                            );
                        }
                    };
                    Ok(Import::Generated(ImportItem {
                        default: Some(local),
                        named: Vec::new(),
                        specifier,
                        anchor,
                    }))
                }
                TargetKind::Script | TargetKind::Asset => {
                    if specifiers.is_empty() && target.kind == TargetKind::Script {
                        return refuse("side-effect imports of modules are not converted");
                    }
                    Ok(Import::Generated(ImportItem {
                        default,
                        named,
                        specifier,
                        anchor,
                    }))
                }
            }
        }
        Resolution::External { package } => {
            if written == "next/link" {
                return match (default, named.as_slice()) {
                    (Some(local), []) => Ok(Import::Link {
                        local: local.as_str().to_owned(),
                    }),
                    _ => refuse("`next/link` must be a default import"),
                };
            }
            if written == "server-only" && specifiers.is_empty() {
                // Astro frontmatter is server-only by construction.
                return Ok(Import::Dropped);
            }
            if matches!(package.as_str(), "next" | "react" | "react-dom") {
                return refuse("framework imports other than `next/link` are not converted");
            }
            Ok(Import::Generated(ImportItem {
                default,
                named,
                specifier: written,
                anchor,
            }))
        }
        Resolution::NodeBuiltin => Ok(Import::Generated(ImportItem {
            default,
            named,
            specifier: written,
            anchor,
        })),
        Resolution::Unresolved(_) => refuse("an import does not resolve"),
    }
}

fn props(tree: &SyntaxTree<'_>, parameters: NodeId) -> Result<Vec<String>> {
    let SyntaxNodeKind::FormalParameters { parameters } = kind(tree, parameters)? else {
        return refuse("the component has no parameter list");
    };
    let parameters = list(tree, parameters)?;
    let Some(&parameter) = parameters.first() else {
        return Ok(Vec::new());
    };
    if parameters.len() > 1 {
        return refuse("components take one props parameter");
    }
    let mut pattern = parameter;
    if let SyntaxNodeKind::TypeScript {
        kind:
            TypeScriptKind::Parameter {
                optional: false,
                rest: false,
            },
        children,
    } = kind(tree, parameter)?
    {
        let children = list(tree, children)?;
        let (binding, annotation) = match children.as_slice() {
            [binding] => (*binding, None),
            [binding, annotation] => (*binding, Some(*annotation)),
            _ => return refuse("an unsupported parameter form"),
        };
        if let Some(annotation) = annotation
            && !matches!(
                kind(tree, annotation)?,
                SyntaxNodeKind::TypeScript {
                    kind: TypeScriptKind::Annotation,
                    ..
                }
            )
        {
            return refuse("an unsupported parameter form");
        }
        pattern = binding;
    }
    let SyntaxNodeKind::ObjectPattern { properties, .. } = kind(tree, pattern)? else {
        return refuse("props must be an object pattern");
    };
    let mut names = Vec::new();
    for property in list(tree, properties)? {
        let SyntaxNodeKind::ShorthandPatternProperty {
            target,
            default: None,
        } = kind(tree, property)?
        else {
            return refuse("props must be shorthand properties without defaults");
        };
        names.push(node_text(tree, target)?.to_owned());
    }
    Ok(names)
}

fn metadata_export(
    tree: &SyntaxTree<'_>,
    declaration: NodeId,
    templates: bool,
) -> Result<Option<Metadata>> {
    let Some((name, init)) = const_export(tree, declaration)? else {
        return Ok(None);
    };
    if name != "metadata" {
        return Ok(None);
    }
    let SyntaxNodeKind::ObjectExpression { properties, .. } = kind(tree, init)? else {
        return refuse("metadata must be an object literal");
    };
    let mut metadata = Metadata::default();
    for property in list(tree, properties)? {
        let SyntaxNodeKind::Property {
            key,
            value,
            computed: false,
        } = kind(tree, property)?
        else {
            return refuse("an unsupported metadata property");
        };
        let value_span = node_span(tree, value)?;
        let is_string = matches!(kind(tree, value)?, SyntaxNodeKind::StringLiteral);
        match node_text(tree, key)? {
            "title" if is_string => {
                metadata.title = Some(Title::Plain(string_value(tree, value)?, value_span));
            }
            "title" if templates => metadata.title = Some(title_template(tree, value)?),
            "description" if is_string => {
                metadata.description = Some((string_value(tree, value)?, value_span));
            }
            _ => return refuse("a metadata field outside static title and description"),
        }
    }
    Ok(Some(metadata))
}

fn title_template(tree: &SyntaxTree<'_>, value: NodeId) -> Result<Title> {
    let SyntaxNodeKind::ObjectExpression { properties, .. } = kind(tree, value)? else {
        return refuse("an unsupported title");
    };
    let (mut default, mut template) = (None, None);
    for property in list(tree, properties)? {
        let SyntaxNodeKind::Property {
            key,
            value,
            computed: false,
        } = kind(tree, property)?
        else {
            return refuse("an unsupported title property");
        };
        if !matches!(kind(tree, value)?, SyntaxNodeKind::StringLiteral) {
            return refuse("an unsupported title property");
        }
        let text = string_value(tree, value)?;
        match node_text(tree, key)? {
            "default" => default = Some((text, node_span(tree, value)?)),
            "template" => template = Some(text),
            _ => return refuse("a title field outside default and template"),
        }
    }
    match (default, template) {
        (Some((default, default_span)), Some(template)) if template.matches("%s").count() == 1 => {
            Ok(Title::Template {
                default,
                default_span,
                template,
            })
        }
        _ => refuse("a title template needs `default` and one `%s`"),
    }
}

// Inserts the document head that Next.js renders for the root layout: the
// charset and viewport tags it adds by default, then the metadata.
fn layout_template(
    mut template: Vec<TemplateNode>,
    metadata: Option<&Metadata>,
    function_anchor: Anchor,
) -> Result<Vec<TemplateNode>> {
    let [TemplateNode::Element(html)] = template.as_mut_slice() else {
        return refuse("the root layout must render one `html` element");
    };
    if html.name != TagName::Html(tag("html")) {
        return refuse("the root layout must render one `html` element");
    }
    if html.children.iter().any(|child| {
        matches!(child, TemplateNode::Element(element) if element.name == TagName::Html(tag("head")))
    }) {
        return refuse("a custom `head` is not converted by migrate v0");
    }
    let meta = |attributes: Vec<(&str, AttributeValue)>, anchor: Anchor| {
        TemplateNode::Element(Element {
            name: TagName::Html(tag("meta")),
            attributes: attributes
                .into_iter()
                .map(|(name, value)| AttributeItem::Named {
                    name: attribute(name),
                    value,
                    anchor,
                })
                .collect(),
            children: Vec::new(),
            anchor,
        })
    };
    let text = |value: &str| AttributeValue::Text(value.to_owned());
    let mut head = vec![
        meta(vec![("charset", text("utf-8"))], function_anchor),
        meta(
            vec![
                ("name", text("viewport")),
                ("content", text("width=device-width, initial-scale=1")),
            ],
            function_anchor,
        ),
    ];
    let metadata = metadata.cloned().unwrap_or_default();
    let module_anchor = |span: Span| Anchor {
        source: SourceId(0),
        span,
    };
    if let Some(title) = &metadata.title {
        let title_span = match title {
            Title::Plain(_, span) => *span,
            Title::Template { default_span, .. } => *default_span,
        };
        let anchor = module_anchor(title_span);
        head.push(TemplateNode::Element(Element {
            name: TagName::Html(tag("title")),
            attributes: Vec::new(),
            children: vec![TemplateNode::Expression(Expression {
                pieces: vec![ExpressionPiece::Identifier(identifier("title")?)],
                anchor,
            })],
            anchor,
        }));
    }
    if let Some((_, description_span)) = &metadata.description {
        let anchor = module_anchor(*description_span);
        head.push(meta(
            vec![
                ("name", text("description")),
                (
                    "content",
                    AttributeValue::Expression(Expression {
                        pieces: vec![ExpressionPiece::Identifier(identifier("description")?)],
                        anchor,
                    }),
                ),
            ],
            anchor,
        ));
    }
    html.children.insert(
        0,
        TemplateNode::Element(Element {
            name: TagName::Html(tag("head")),
            attributes: Vec::new(),
            children: head,
            anchor: function_anchor,
        }),
    );
    Ok(template)
}

// A nested layout renders its parent and forwards the metadata props.
fn wrap(
    parent: &str,
    props: &[&'static str],
    children: Vec<TemplateNode>,
    anchor: Anchor,
) -> Result<TemplateNode> {
    let name = ComponentName::new(parent).ok_or_else(|| "a layout name is invalid".to_owned())?;
    let attributes = props
        .iter()
        .map(|prop| {
            Ok(AttributeItem::Named {
                name: attribute(prop),
                value: AttributeValue::Expression(Expression {
                    pieces: vec![ExpressionPiece::Identifier(identifier(prop)?)],
                    anchor,
                }),
                anchor,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(TemplateNode::Element(Element {
        name: TagName::Component(name),
        attributes,
        children,
        anchor,
    }))
}

// Wraps a page in its nearest layout with the metadata that Next.js would
// resolve for it: the page title through the root layout's template, and the
// nearest description.
fn page_template(
    template: Vec<TemplateNode>,
    page: Option<&Metadata>,
    layout_name: &str,
    layout_props: &[&'static str],
    root: &RootFacts,
    function_anchor: Anchor,
) -> Result<Vec<TemplateNode>> {
    let page = page.cloned().unwrap_or_default();
    let page_anchor = |span: Span| Anchor {
        source: SourceId(0),
        span,
    };
    let root_anchor = |span: Span| Anchor {
        source: SourceId(1),
        span,
    };
    let title = match (&page.title, &root.metadata.title) {
        (Some(Title::Plain(title, span)), Some(Title::Template { template, .. })) => {
            Some((template.replacen("%s", title, 1), page_anchor(*span)))
        }
        (Some(Title::Plain(title, span)), Some(Title::Plain(..))) => {
            Some((title.clone(), page_anchor(*span)))
        }
        (
            None,
            Some(Title::Template {
                default,
                default_span,
                ..
            }),
        ) => Some((default.clone(), root_anchor(*default_span))),
        (None, Some(Title::Plain(title, span))) => Some((title.clone(), root_anchor(*span))),
        (None, None) => None,
        (Some(_), None) | (Some(Title::Template { .. }), Some(_)) => {
            return refuse("a page title needs a title in the root layout");
        }
    };
    let description = match (&page.description, &root.metadata.description) {
        (Some((text, span)), Some(_)) => Some((text.clone(), page_anchor(*span))),
        (None, Some((text, span))) => Some((text.clone(), root_anchor(*span))),
        (None, None) => None,
        (Some(_), None) => return refuse("a page description needs one in the root layout"),
    };
    let mut attributes = Vec::new();
    for (prop, value) in [("title", title), ("description", description)] {
        let Some((value, anchor)) = value else {
            continue;
        };
        if !layout_props.contains(&prop) {
            return refuse("the nearest layout does not accept the page metadata");
        }
        attributes.push(AttributeItem::Named {
            name: attribute(prop),
            value: AttributeValue::Text(value),
            anchor,
        });
    }
    let name =
        ComponentName::new(layout_name).ok_or_else(|| "a layout name is invalid".to_owned())?;
    Ok(vec![TemplateNode::Element(Element {
        name: TagName::Component(name),
        attributes,
        children: template,
        anchor: function_anchor,
    })])
}

// Next.js-specific link props and spreads that could carry them are rejected,
// and `href` must be a string, because `next/link` formats URL objects itself.
fn check_links(tree: &SyntaxTree<'_>, link: Option<&str>, root: NodeId) -> Result<()> {
    let Some(link) = link else {
        return Ok(());
    };
    if let SyntaxNodeKind::JsxOpeningElement {
        name, attributes, ..
    } = kind(tree, root)?
        && node_text(tree, name)? == link
    {
        let mut has_href = false;
        for attribute in list(tree, attributes)? {
            let SyntaxNodeKind::JsxAttribute { name, value } = kind(tree, attribute)? else {
                return refuse("spread props on `Link` are not converted");
            };
            let name = node_text(tree, name)?;
            if NEXT_LINK_PROPS.contains(&name) {
                return refuse("a `Link` prop that changes navigation is not converted");
            }
            if name == "href" {
                has_href = true;
                let string = value.is_some_and(|value| {
                    matches!(kind(tree, value), Ok(SyntaxNodeKind::JsxStringLiteral))
                }) || value
                    .map(|value| static_string_container(tree, value))
                    .transpose()?
                    .unwrap_or(false);
                if !string {
                    return refuse("a `Link` href must be a string");
                }
            }
        }
        if !has_href {
            return refuse("a `Link` has no href");
        }
    }
    for child in tree.child_nodes(root).into_iter().flatten() {
        check_links(tree, Some(link), child)?;
    }
    Ok(())
}

fn static_string_container(tree: &SyntaxTree<'_>, value: NodeId) -> Result<bool> {
    let SyntaxNodeKind::JsxExpressionContainer {
        expression: Some(expression),
    } = kind(tree, value)?
    else {
        return Ok(false);
    };
    let expression_kind = kind(tree, expression)?;
    if let SyntaxNodeKind::TemplateLiteral { parts } = expression_kind {
        let parts = tree.template_parts(parts).unwrap_or_default();
        return Ok(parts
            .iter()
            .all(|part| matches!(part, TemplatePart::Segment(_))));
    }
    Ok(matches!(expression_kind, SyntaxNodeKind::StringLiteral))
}

/// Relocates a module that stays JavaScript or TypeScript: every resolved
/// local specifier is rewritten for the new location and `server-only`
/// side-effect imports are removed. Framework imports refuse the file.
pub(crate) fn relocate(
    context: &Context<'_>,
    path: &SourcePath,
    text: &SourceText,
    output: &str,
) -> Result<EmittedText> {
    if path.as_str().ends_with(".d.ts") {
        return Ok(EmittedText::from_rewritten(
            SourceId(0),
            &apply_edits(text, &[], context.limits.edit)
                .map_err(|_| "the declaration file cannot be copied".to_owned())?,
        ));
    }
    let tree = parse(text, path.as_str(), context.limits)?;
    let Some(node) = context.graph.node(path) else {
        return refuse("the module is not in the module graph");
    };
    let mut replacements: Vec<(Span, String)> = Vec::new();
    for edge in &node.edges {
        match &edge.resolution {
            Resolution::Local { path: target, .. } => {
                if edge.type_only {
                    continue;
                }
                let Some(target) = context.targets.get(target) else {
                    return refuse("it imports a file that migrate v0 does not migrate");
                };
                if target.kind == TargetKind::Astro {
                    return refuse("a script module cannot import an Astro component");
                }
                let Some(written) = &edge.specifier else {
                    return refuse("a computed import specifier");
                };
                let new = specifier(output, target);
                if &new != written {
                    let quote = text
                        .slice(edge.span)
                        .ok()
                        .and_then(|s| s.chars().next())
                        .filter(|c| matches!(c, '\'' | '"'))
                        .ok_or_else(|| "an import specifier is not a string literal".to_owned())?;
                    replacements.push((edge.span, format!("{quote}{new}{quote}")));
                }
            }
            Resolution::External { package } => {
                if !edge.type_only && matches!(package.as_str(), "next" | "react-dom") {
                    return refuse("Next.js and React DOM imports are not converted");
                }
            }
            Resolution::NodeBuiltin => {}
            Resolution::Unresolved(_) => {
                if !edge.type_only {
                    return refuse("an import does not resolve");
                }
            }
        }
    }
    // `import 'server-only'` throws outside React Server Components.
    for item in program_body(&tree)? {
        if let SyntaxNodeKind::ImportDeclaration {
            specifiers, source, ..
        } = kind(&tree, item)?
            && list(&tree, specifiers)?.is_empty()
            && string_value(&tree, source)? == "server-only"
        {
            let span = node_span(&tree, item)?;
            let rest = &text.as_str()[span.end()..];
            let newline = rest
                .strip_prefix("\r\n")
                .map(|_| 2)
                .or_else(|| rest.strip_prefix('\n').map(|_| 1))
                .unwrap_or(0);
            let source_span = node_span(&tree, source)?;
            replacements.retain(|(s, _)| *s != source_span);
            replacements.push((
                Span::new(span.start(), span.end() + newline)
                    .map_err(|_| "an invalid span".to_owned())?,
                String::new(),
            ));
        }
    }
    replacements.sort_by_key(|(span, _)| *span);
    let edits: Vec<Edit<'_>> = replacements
        .iter()
        .enumerate()
        .map(|(index, (span, replacement))| Edit {
            span: *span,
            replacement,
            order: EditOrder {
                operation: 0,
                edit: u32::try_from(index).unwrap_or(u32::MAX),
            },
        })
        .collect();
    let rewritten = apply_edits(text, &edits, context.limits.edit)
        .map_err(|error| format!("the import rewrite failed ({error})"))?;
    Ok(EmittedText::from_rewritten(SourceId(0), &rewritten))
}

/// Whether a stylesheet keeps working when it moves: every `url()` and
/// import must be absolute, a data URL, or a remote URL.
pub(crate) fn stylesheet_relocatable(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let absolute = |rest: &str| {
        let rest = rest.trim_start_matches([' ', '\t', '\r', '\n', '"', '\'']);
        (rest.starts_with('/') && !rest.starts_with("//"))
            || rest.starts_with("data:")
            || rest.starts_with("https:")
            || rest.starts_with("http:")
            || rest.starts_with('#')
    };
    for (index, _) in lower.match_indices("url(") {
        if !absolute(&lower[index + 4..]) {
            return false;
        }
    }
    for keyword in ["@import", "@use", "@forward"] {
        for (index, _) in lower.match_indices(keyword) {
            let rest = lower[index + keyword.len()..].trim_start();
            let rest = rest.strip_prefix("url(").unwrap_or(rest);
            if !absolute(rest) || rest.trim_start_matches(['"', '\'']).starts_with('/') {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_specifiers() {
        assert_eq!(
            relative_path("src/pages", "src/layouts/RootLayout.astro"),
            "../layouts/RootLayout.astro"
        );
        assert_eq!(
            relative_path("src/components", "src/components/A.astro"),
            "./A.astro"
        );
        let script = Target {
            output: "src/components/Counter.tsx".into(),
            kind: TargetKind::Script,
        };
        assert_eq!(
            specifier("src/pages/index.astro", &script),
            "../components/Counter"
        );
        let asset = Target {
            output: "src/styles/globals.css".into(),
            kind: TargetKind::Asset,
        };
        assert_eq!(
            specifier("src/layouts/RootLayout.astro", &asset),
            "../styles/globals.css"
        );
    }

    #[test]
    fn stylesheet_references() {
        assert!(stylesheet_relocatable("body { color: red }"));
        assert!(stylesheet_relocatable("a { background: url('/bg.png') }"));
        assert!(stylesheet_relocatable(
            "a { background: url(data:image/png;base64,AA) }"
        ));
        assert!(!stylesheet_relocatable("a { background: url(./bg.png) }"));
        assert!(!stylesheet_relocatable("@import './other.css';"));
        assert!(!stylesheet_relocatable("@import 'tailwindcss';"));
        assert!(stylesheet_relocatable(
            "@import url('https://fonts.example/css');"
        ));
    }
}
