//! Pure assessment predicates over source-model and strict syntax evidence.

use crate::{
    features::{Detail, Location, Occurrence},
    project::Project,
};
use frameshift_bindings::Bindings;
use frameshift_catalog::Status;
use frameshift_core::{SourcePath, Span};
use frameshift_nextjs::{ConfigProperty, Convention, Metadata};
use frameshift_syntax::{NodeId, SyntaxNodeKind as N, SyntaxTree};
use std::collections::BTreeMap;

/// Why an assessment has its status. Reasons never contain source snippets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The catalog fixes this feature's V1 behavior.
    Fixed,
    /// A source-model predicate establishes this mapping.
    Proven(&'static str),
    /// The documented mapping changes behavior or needs runtime knowledge.
    Review(&'static str),
    /// Evidence needed for an assessment precondition is absent.
    Missing(&'static str),
}
impl Reason {
    /// Human-readable explanation of the deterministic classification.
    pub fn message(&self) -> &str {
        match self {
            Self::Fixed => "The compatibility matrix fixes this feature's V1 status.",
            Self::Proven(s) | Self::Review(s) | Self::Missing(s) => s,
        }
    }
}

/// One occurrence with its assessment, kept even when it blocks a unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssessedOccurrence {
    /// Original detection evidence.
    pub occurrence: Occurrence,
    /// Exactly one status, or `None` for explicitly out-of-scope features.
    pub status: Option<Status>,
    /// Stable full assessment rule ID; absent when no predicate is established.
    pub rule: Option<String>,
    /// Why this result follows from the evidence.
    pub reason: Reason,
    /// Owning counting unit, absent only for inventory-only occurrences.
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Syntax {
    pub modules: BTreeMap<SourcePath, Module>,
    pub link_hrefs_need_review: bool,
    automatic_routes: BTreeMap<SourcePath, std::collections::BTreeSet<String>>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Module {
    pub functions: BTreeMap<Span, Function>,
    page_function: Option<Span>,
    pub tags: BTreeMap<Span, Tag>,
    pub calls: BTreeMap<Span, Option<usize>>,
    pub handler_request: Option<bool>,
    pub route: super::route::Summary,
}
#[derive(Clone, Debug)]
pub(crate) struct Function {
    pub asynchronous: bool,
    pub astro: bool,
    pub assumptions: bool,
    // The conversion as a page whose parameter is the framework's page props,
    // for a default export with a parameter. Route correlation selects it.
    page: Option<Conversion>,
}
#[derive(Clone, Copy, Debug)]
struct Conversion {
    astro: bool,
    assumptions: bool,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Tag {
    pub attrs: BTreeMap<String, Option<String>>,
    pub literal_href: bool,
    pub local_image_url: bool,
    pub image_import: Option<Span>,
    pub static_image: bool,
    pub image_unoptimized: Option<Option<bool>>,
    pub spread: bool,
    pub jsx_props: bool,
    pub dynamic_props: bool,
    pub promise_props: bool,
}
struct Components;
impl frameshift_emit::ElementResolver for Components {
    fn resolve(
        &mut self,
        _: frameshift_emit::ComponentReference<'_>,
    ) -> Option<frameshift_emit::ComponentStrategy> {
        // Only checks printer syntax. Graph provenance and the conversion
        // fixed point are checked separately before assigning a strategy.
        Some(frameshift_emit::ComponentStrategy::Component {
            name: frameshift_emit::ComponentName::new("Component")?,
            directive: None,
        })
    }
}
impl Syntax {
    pub fn correlate_images(&mut self, project: &Project) {
        for (path, module) in &mut self.modules {
            let images: std::collections::BTreeSet<_> = project.modules().node(path).into_iter()
                .flat_map(|n| &n.edges).filter(|e| !e.type_only)
                .filter(|e| matches!(&e.resolution, frameshift_modules::Resolution::Local { path, kind: frameshift_modules::FileKind::Asset } if path.as_str().rsplit_once('.').is_some_and(|(_, ext)| ["png", "jpg", "jpeg", "gif", "webp", "avif"].contains(&ext))))
                .map(|e| e.span).collect();
            for tag in module.tags.values_mut() {
                tag.static_image = tag.image_import.is_some_and(|span| images.contains(&span));
            }
        }
    }
    pub fn correlate_routes(&mut self, project: &Project) {
        let mut routes: BTreeMap<SourcePath, std::collections::BTreeSet<String>> = BTreeMap::new();
        // `Astro.params` equals Next.js `params` only for the values of plain
        // dynamic segments; catch-all values are joined strings in Astro.
        let mut pages: BTreeMap<&SourcePath, bool> = BTreeMap::new();
        for route in &project.nextjs().routes.routes {
            if route.kind != frameshift_nextjs::RouteKind::Page {
                continue;
            }
            let exact = self.modules.get(&route.file).is_some_and(|module| {
                module.route.parameter_keys().is_some_and(|mut keys| {
                    keys.all(|key| {
                        route.segments.iter().any(|segment| {
                            matches!(&segment.kind, frameshift_nextjs::SegmentKind::Dynamic(p) if p == key)
                        })
                    })
                })
            });
            *pages.entry(&route.file).or_insert(true) &= exact;
        }
        for (path, exact) in pages {
            if let Some(module) = self.modules.get_mut(path)
                && let Some(span) = module.page_function
                && let Some(function) = module.functions.get_mut(&span)
                && let Some(page) = function.page
            {
                function.astro = page.astro && exact;
                function.assumptions = page.assumptions;
            }
        }
        for route in &project.nextjs().routes.routes {
            if super::route::automatic(route, self) {
                routes
                    .entry(route.file.clone())
                    .or_default()
                    .insert(route.pattern.clone());
            }
        }
        self.automatic_routes = routes;
    }
    fn automatic_route(&self, occurrence: &Occurrence) -> bool {
        let Detail::Segment { pattern, .. } = &occurrence.detail else {
            return false;
        };
        occurrence
            .location
            .path()
            .and_then(|p| self.automatic_routes.get(p))
            .is_some_and(|routes| routes.contains(pattern))
    }
    pub fn correlate_links(
        &mut self,
        occurrences: &[Occurrence],
        excluded_files: &std::collections::BTreeSet<&SourcePath>,
    ) {
        self.link_hrefs_need_review = occurrences
            .iter()
            .filter(|o| o.feature.id == "nextjs.api.link")
            .filter(|o| {
                !o.location
                    .path()
                    .is_some_and(|p| excluded_files.contains(p))
            })
            .any(|o| {
                self.tag(&o.location)
                    .is_none_or(|tag| tag.spread || !tag.literal_href)
            });
    }
    pub fn collect(&mut self, path: &SourcePath, tree: &SyntaxTree<'_>, bindings: &Bindings) {
        let mut module = Module {
            route: super::route::summarize(tree, bindings),
            ..Module::default()
        };
        let references = References::new(tree, bindings);
        let page = module.route.page();
        module.page_function = page.and_then(|(node, _)| Some(tree.node(node)?.span()));
        let mut stack = vec![tree.root()];
        while let Some(id) = stack.pop() {
            if let Some(function) = function_summary(tree, &references, id, page) {
                module
                    .functions
                    .insert(tree.node(id).unwrap().span(), function);
            }
            stack.extend(tree.child_nodes(id).into_iter().flatten());
        }
        let written: std::collections::BTreeSet<_> = bindings
            .references()
            .iter()
            .filter(|r| r.write)
            .filter_map(|r| {
                if let frameshift_bindings::Target::Binding(id) = r.target {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        for node in tree.nodes() {
            if let N::JsxOpeningElement { attributes, .. } = node.kind() {
                let mut tag = Tag::default();
                for &attr in tree.children(attributes).unwrap_or_default() {
                    if let N::JsxAttribute { name, value } = tree.node(attr).unwrap().kind() {
                        let name = tree
                            .source()
                            .slice(tree.node(name).unwrap().span())
                            .unwrap()
                            .to_owned();
                        let known = value.and_then(|v| literal(tree, v));
                        if name == "href" {
                            tag.literal_href = value.is_some_and(|v| literal_href(tree, v));
                        }
                        if name == "src" {
                            tag.local_image_url = known.as_ref().is_some_and(|s| {
                                s.starts_with('/') && !s.starts_with("//") && !s.contains('\\')
                            });
                            tag.image_import =
                                value.and_then(|v| image_import(tree, bindings, &written, v));
                        }
                        if name == "unoptimized" {
                            tag.image_unoptimized = Some(match value {
                                None => Some(true),
                                Some(v) => {
                                    if let N::BooleanLiteral(b) =
                                        tree.node(unwrap(tree, v)).unwrap().kind()
                                    {
                                        Some(b)
                                    } else {
                                        None
                                    }
                                }
                            });
                        }
                        let jsx = value.is_some_and(|v| jsx_value(tree, v));
                        tag.jsx_props |= name != "children" && jsx;
                        tag.dynamic_props |= value.is_some_and(|v| !serializable(tree, v)) && !jsx;
                        tag.promise_props |= value.is_some_and(|v| promise(tree, bindings, v));
                        tag.attrs.insert(name, known);
                    } else if matches!(
                        tree.node(attr).unwrap().kind(),
                        N::JsxSpreadAttribute { .. }
                    ) {
                        tag.spread = true;
                    }
                }
                module.tags.insert(node.span(), tag);
            }
            if let N::Call {
                callee, arguments, ..
            } = node.kind()
            {
                module.calls.insert(
                    tree.node(unwrap(tree, callee)).unwrap().span(),
                    tree.children(arguments).and_then(|args| {
                        args.iter()
                            .all(|&id| !matches!(tree.node(id).unwrap().kind(), N::Spread { .. }))
                            .then_some(args.len())
                    }),
                );
            }
        }
        module.handler_request = handler_request(tree, bindings);
        self.modules.insert(path.clone(), module);
    }
    pub fn function(&self, path: &SourcePath, span: Span) -> Option<&Function> {
        self.modules.get(path)?.functions.get(&span)
    }
    pub fn tag(&self, location: &Location) -> Option<&Tag> {
        let Location::Text { path, span } = location else {
            return None;
        };
        self.modules.get(path)?.tags.get(span)
    }
}
fn handler_request(tree: &SyntaxTree<'_>, bindings: &Bindings) -> Option<bool> {
    use frameshift_bindings::{ExportKind, ExportName, Target};
    let export = bindings
        .exports()
        .iter()
        .find(|e| e.name == ExportName::Named("GET".into()) && !e.type_only)?;
    let ExportKind::Local(Target::Binding(binding)) = export.kind else {
        return None;
    };
    let binding = bindings.binding(binding);
    let [declaration] = binding.declarations.as_slice() else {
        return None;
    };
    let mut node = declaration.node;
    if let N::VariableDeclarator {
        init: Some(init), ..
    } = tree.node(node)?.kind()
    {
        node = unwrap(tree, init)
    }
    let parameters = if let N::FunctionDeclaration { parameters, .. }
    | N::FunctionExpression { parameters, .. }
    | N::ArrowFunction { parameters, .. } = tree.node(node)?.kind()
    {
        parameters
    } else {
        return None;
    };
    if bindings.references().iter().any(|r| {
        if let Target::Binding(id) = r.target {
            bindings.binding(id).declarations.iter().any(|d| {
                d.kind == frameshift_bindings::DeclarationKind::Arguments && d.node == node
            })
        } else {
            false
        }
    }) {
        return Some(true);
    }
    let Some(first) = tree.child_nodes(parameters)?.next() else {
        return Some(false);
    };
    let span = tree.node(first)?.span();
    Some(bindings.references().iter().any(|reference| {
        if let Target::Binding(id) = reference.target {
            bindings
                .binding(id)
                .declarations
                .iter()
                .filter_map(|d| d.name)
                .filter_map(|id| tree.node(id))
                .any(|n| span.start() <= n.span().start() && n.span().end() <= span.end())
        } else {
            false
        }
    }))
}
// References sorted by source position, for the references inside one
// function without scanning the module's references again.
struct References<'a> {
    bindings: &'a Bindings,
    spans: Vec<(Span, usize)>,
}
impl<'a> References<'a> {
    fn new(tree: &SyntaxTree<'_>, bindings: &'a Bindings) -> Self {
        let mut spans: Vec<_> = bindings
            .references()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((tree.node(r.node)?.span(), i)))
            .collect();
        spans.sort_by_key(|(span, i)| (span.start(), span.end(), *i));
        Self { bindings, spans }
    }
    fn within(&self, outer: Span) -> impl Iterator<Item = &'a frameshift_bindings::Reference> {
        let first = self
            .spans
            .partition_point(|(span, _)| span.start() < outer.start());
        let bindings = self.bindings;
        self.spans[first..]
            .iter()
            .take_while(move |(span, _)| span.start() < outer.end())
            .filter(move |(span, _)| span.end() <= outer.end())
            .map(move |&(_, i)| &bindings.references()[i])
    }
}
fn function_summary(
    tree: &SyntaxTree<'_>,
    references: &References<'_>,
    id: NodeId,
    page: Option<(NodeId, Option<&[Span]>)>,
) -> Option<Function> {
    let kind = tree.node(id)?.kind();
    let (parameters, asynchronous, generator) =
        if let N::FunctionDeclaration {
            parameters, kind, ..
        }
        | N::FunctionExpression {
            parameters, kind, ..
        } = kind
        {
            (parameters, kind.is_async(), kind.is_generator())
        } else if let N::ArrowFunction {
            parameters,
            is_async,
            ..
        } = kind
        {
            (parameters, is_async, false)
        } else {
            return None;
        };
    let component = conversion(
        tree,
        references,
        id,
        frameshift_emit::PropsSource::AstroProps,
        &[],
    );
    // A page's parameter holds route values, never component props.
    let page = page
        .filter(|(node, _)| *node == id)
        .filter(|_| {
            tree.child_nodes(parameters)
                .is_some_and(|mut p| p.next().is_some())
        })
        .map(|(_, substitutions)| match substitutions {
            Some(spans) => conversion(
                tree,
                references,
                id,
                frameshift_emit::PropsSource::Dropped,
                spans,
            ),
            None => Conversion {
                astro: false,
                assumptions: false,
            },
        });
    Some(Function {
        asynchronous: asynchronous || generator,
        astro: component.astro,
        assumptions: component.assumptions,
        page,
    })
}
// The candidate shape is exactly what the printer converts and prints. The
// printer reads syntax only, so binding facts add what it cannot see.
fn conversion(
    tree: &SyntaxTree<'_>,
    references: &References<'_>,
    id: NodeId,
    props: frameshift_emit::PropsSource,
    astro_params: &[Span],
) -> Conversion {
    let rejected = Conversion {
        astro: false,
        assumptions: false,
    };
    let Ok(converted) = frameshift_emit::convert_component(
        tree,
        frameshift_emit::SourceId(0),
        id,
        &frameshift_emit::ComponentConversion {
            props,
            astro_params,
            ..Default::default()
        },
        &mut Components,
    ) else {
        return rejected;
    };
    let printed = frameshift_emit::print_astro_component(
        &converted.component,
        &[tree.source()],
        frameshift_emit::PrintOptions::default(),
        frameshift_emit::EmitLimits::default(),
    );
    if printed.is_err() || !portable_references(tree, references, id) {
        return rejected;
    }
    Conversion {
        astro: true,
        assumptions: !converted.assumptions.is_empty(),
    }
}
// Whether every reference inside the function keeps its meaning when the
// function becomes an Astro component whose module-level dependencies are
// imported: no dynamic scope, implicit `arguments`, or direct `eval`; no
// write outside the function; no capture of an enclosing function or block;
// and no value from React itself, whose hooks and element APIs need a React
// render.
fn portable_references(tree: &SyntaxTree<'_>, references: &References<'_>, id: NodeId) -> bool {
    use frameshift_bindings::{DeclarationKind, Meaning, ScopeKind, Target};
    let Some(span) = tree.node(id).map(|n| n.span()) else {
        return false;
    };
    references
        .within(span)
        .all(|reference| match reference.target {
            Target::Dynamic => false,
            Target::Global => {
                !reference.write && reference.name != "eval" && reference.name != "arguments"
            }
            Target::Binding(target) => {
                let binding = references.bindings.binding(target);
                if binding
                    .declarations
                    .iter()
                    .any(|d| d.kind == DeclarationKind::Arguments)
                {
                    return false;
                }
                let react = binding.import.as_ref().is_some_and(|import| {
                    !import.type_only
                        && import.specifier.as_deref().is_some_and(|s| {
                            ["react", "react-dom"].iter().any(|p| {
                                s == *p || s.strip_prefix(p).is_some_and(|r| r.starts_with('/'))
                            })
                        })
                });
                let element_name = tree.node(reference.node).is_some_and(|n| {
                    matches!(
                        n.kind(),
                        N::JsxIdentifier
                            | N::JsxMemberExpression { .. }
                            | N::JsxNamespacedName { .. }
                    )
                });
                if react && reference.meaning.overlaps(Meaning::VALUE) && !element_name {
                    return false;
                }
                let scope = references.bindings.scope(binding.scope);
                let local = tree.node(scope.node).is_some_and(|n| {
                    span.start() <= n.span().start() && n.span().end() <= span.end()
                });
                local
                    || (!reference.write
                        && matches!(scope.kind, ScopeKind::Module | ScopeKind::Script))
            }
        })
}
pub(super) fn unwrap(tree: &SyntaxTree<'_>, mut id: NodeId) -> NodeId {
    loop {
        if let N::JsxExpressionContainer {
            expression: Some(e),
        }
        | N::Parenthesized { expression: e } = tree.node(id).unwrap().kind()
        {
            id = e
        } else if let N::TypeScript { kind, children } = tree.node(id).unwrap().kind() {
            let children = tree.children(children).unwrap_or_default();
            let erased = matches!(
                kind,
                frameshift_syntax::TypeScriptKind::Parameter { .. }
                    | frameshift_syntax::TypeScriptKind::As
                    | frameshift_syntax::TypeScriptKind::Satisfies
                    | frameshift_syntax::TypeScriptKind::NonNull
                    | frameshift_syntax::TypeScriptKind::Instantiation
            );
            let next = if erased {
                children.first()
            } else if matches!(kind, frameshift_syntax::TypeScriptKind::Assertion) {
                children.get(1)
            } else {
                None
            };
            if let Some(&next) = next {
                id = next
            } else {
                return id;
            }
        } else {
            return id;
        }
    }
}
fn jsx_value(tree: &SyntaxTree<'_>, id: NodeId) -> bool {
    matches!(
        tree.node(unwrap(tree, id)).unwrap().kind(),
        N::JsxElement { .. } | N::JsxFragment { .. }
    )
}
fn literal(tree: &SyntaxTree<'_>, id: NodeId) -> Option<String> {
    let id = unwrap(tree, id);
    let node = tree.node(id)?;
    if let Some(s) = tree.string_literal_value(id) {
        Some(s)
    } else if matches!(node.kind(), N::JsxStringLiteral) {
        jsx_string(tree.source().slice(node.span()).ok()?)
    } else if let N::BooleanLiteral(value) = node.kind() {
        Some(value.to_string())
    } else if matches!(node.kind(), N::NumberLiteral { .. }) {
        Some(tree.source().slice(node.span()).ok()?.to_owned())
    } else {
        None
    }
}
fn jsx_string(quoted: &str) -> Option<String> {
    let text = quoted.get(1..quoted.len().checked_sub(1)?)?;
    let mut out = String::new();
    let mut at = 0;
    while at < text.len() {
        if let Some(reference) = frameshift_syntax::decode_jsx_character_reference(text, at).ok()? {
            out.push(reference.as_char()?);
            at = reference.span.end()
        } else {
            let ch = text[at..].chars().next()?;
            out.push(ch);
            at += ch.len_utf8()
        }
    }
    Some(out)
}
fn literal_href(tree: &SyntaxTree<'_>, id: NodeId) -> bool {
    let id = unwrap(tree, id);
    if matches!(
        tree.node(id).unwrap().kind(),
        N::StringLiteral | N::JsxStringLiteral
    ) {
        return literal(tree, id).is_some();
    }
    matches!(tree.node(id).unwrap().kind(), N::ObjectExpression { .. }) && serializable(tree, id)
}
fn image_import(
    tree: &SyntaxTree<'_>,
    bindings: &Bindings,
    written: &std::collections::BTreeSet<frameshift_bindings::BindingId>,
    id: NodeId,
) -> Option<Span> {
    use frameshift_bindings::{ImportedName, Target};
    let reference = bindings.reference(unwrap(tree, id))?;
    let Target::Binding(id) = reference.target else {
        return None;
    };
    let import = bindings.binding(id).import.as_ref()?;
    if import.type_only
        || !(matches!(&import.imported, ImportedName::Default)
            || matches!(&import.imported, ImportedName::Named(n) if n == "default"))
    {
        return None;
    }
    if written.contains(&id) {
        return None;
    }
    Some(tree.node(import.source)?.span())
}
fn image_automatic(tag: &Tag, project: &Project) -> bool {
    use frameshift_evaluate::Value;
    let default_loader = matches!(
        project.configuration().property(&["images", "loader"]),
        ConfigProperty::Absent
    ) || matches!(project.configuration().property(&["images", "loader"]), ConfigProperty::Value(Value::String(s)) if s == "default");
    let default_loader = default_loader
        && matches!(
            project.configuration().property(&["images", "loaderFile"]),
            ConfigProperty::Absent
        );
    let unoptimized = match tag.image_unoptimized {
        Some(Some(b)) => b,
        Some(None) => return false,
        None => match project.configuration().property(&["images", "unoptimized"]) {
            ConfigProperty::Value(Value::Bool(b)) => *b,
            ConfigProperty::Absent => false,
            ConfigProperty::Value(_) | ConfigProperty::Unknown | ConfigProperty::Invalid => {
                return false;
            }
        },
    };
    if tag.spread
        || ["fill", "placeholder", "loader"]
            .iter()
            .any(|k| tag.attrs.contains_key(*k))
    {
        return false;
    }
    let static_image = tag.static_image
        && !matches!(
            project
                .configuration()
                .property(&["images", "disableStaticImages"]),
            ConfigProperty::Value(Value::Bool(true))
                | ConfigProperty::Unknown
                | ConfigProperty::Invalid
        );
    (unoptimized && (tag.local_image_url || static_image)) || (default_loader && static_image)
}
fn serializable(tree: &SyntaxTree<'_>, id: NodeId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        let id = unwrap(tree, id);
        let node = tree.node(id).unwrap();
        if matches!(
            node.kind(),
            N::NullLiteral
                | N::BooleanLiteral(_)
                | N::NumberLiteral { .. }
                | N::StringLiteral
                | N::JsxStringLiteral
        ) {
            continue;
        }
        if let N::ObjectExpression { properties, .. } = node.kind() {
            for &p in tree.children(properties).unwrap_or_default() {
                if let N::Property {
                    value,
                    computed: false,
                    ..
                } = tree.node(p).unwrap().kind()
                {
                    pending.push(value)
                } else {
                    return false;
                }
            }
        } else if let N::ArrayExpression { elements, .. } = node.kind() {
            pending.extend(tree.children(elements).unwrap_or_default())
        } else {
            return false;
        }
    }
    true
}
fn promise(tree: &SyntaxTree<'_>, bindings: &Bindings, id: NodeId) -> bool {
    let id = unwrap(tree, id);
    let node = tree.node(id).unwrap();
    let (callee, is_new) = if let N::New { callee, .. } = node.kind() {
        (callee, true)
    } else if let N::Call { callee, .. } = node.kind() {
        (callee, false)
    } else {
        return false;
    };
    let root = if !is_new {
        if let N::Member {
            object,
            property,
            computed: false,
            ..
        } = tree.node(callee).unwrap().kind()
        {
            if !tree.identifier_value(property).is_some_and(|p| {
                ["resolve", "reject", "all", "allSettled", "race", "any"].contains(&p.as_str())
            }) {
                return false;
            }
            object
        } else {
            return false;
        }
    } else {
        callee
    };
    bindings
        .reference(root)
        .is_some_and(|r| r.target == frameshift_bindings::Target::Global && r.name == "Promise")
}

pub(crate) fn classify(
    occurrence: Occurrence,
    project: &Project,
    syntax: &Syntax,
) -> AssessedOccurrence {
    use Status::{Auto, Review, Unknown, Unsupported};
    let mut result = AssessedOccurrence {
        status: occurrence.feature.v1.fixed(),
        rule: None,
        reason: Reason::Fixed,
        unit: None,
        occurrence,
    };
    if matches!(
        result.occurrence.feature.v1,
        frameshift_catalog::V1::OutOfScope
    ) {
        return result;
    }
    if result.status.is_some() {
        result.rule = Some(format!(
            "{}/assessment.{}",
            frameshift_catalog::PATH,
            result.occurrence.feature.id.replace('.', "-")
        ));
        return result;
    }
    let at = &result.occurrence.location;
    let tag = syntax.tag(at);
    if result.occurrence.feature.id == "env.reference"
        && matches!(&result.occurrence.detail,Detail::Variable{name} if name!="NODE_ENV"&&!name.starts_with("NEXT_PUBLIC_"))
        && at
            .path()
            .and_then(|p| project.environment().module(p))
            .is_some_and(|m| m.browser)
    {
        result.status = Some(Unknown);
        result.reason = Reason::Missing(
            "A private environment variable's browser and prerender availability is not established.",
        );
        return result;
    }

    let decision=match result.occurrence.feature.id {
        "nextjs.file.layout" => Some(if at.path().is_some_and(|p|project.nextjs().contexts.modules.iter().any(|m|&m.path==p&&m.client)) {(Unsupported,"Client layouts cannot preserve the layout chain in static output.")} else {(Auto,"The layout remains in the static layout chain.")}),
        "nextjs.file.not-found"=>Some(if at.path().is_some_and(|p|p.as_str().rsplit_once('/').is_some_and(|(parent,_)|parent==project.nextjs().routes.app.as_str())) {(Auto,"A root not-found page maps to the static 404 page.")}else{(Review,"A nested not-found page cannot select a host-level 404 independently.")}),
        "env.reference"=> if let Detail::Variable{name}=&result.occurrence.detail {at.path().and_then(|p|project.environment().module(p)).map(|m| {
                if name=="NODE_ENV" || m.browser {(Review,"Environment naming and deployment settings require review.")} else {(Auto,"A build-time environment reference maps to import.meta.env.")}
            }) }else {None},
        "nextjs.route.dynamic"|"nextjs.route.catch-all"=>Some(if syntax.automatic_route(&result.occurrence) {(Auto,"One independent parameter generator and recognized page parameter uses map to static paths.")} else {(Review,"Parameter composition, captured declarations, or unrecognized parameter uses require review.")}),
        "nextjs.config.base-path"=>Some(if syntax.link_hrefs_need_review {(Review,"Computed Next Link hrefs need review when adding the base prefix.")}else{(Auto,"No computed Next Link href is present in the analyzed render sites.")}),
        "nextjs.api.link"=>tag.map(|tag| if !tag.spread && tag.literal_href && !["prefetch","replace","scroll"].iter().any(|p|tag.attrs.contains_key(*p)) {(Auto,"The literal href maps to an anchor without dropped navigation props.")}else{(Review,"Computed hrefs, spread props, or dropped navigation props require review.")}),
        "nextjs.api.image"=>tag.map(|tag| if image_automatic(tag, project) && !matches!(&result.occurrence.detail,Detail::Api{specifier,..} if specifier=="next/legacy/image") {(Auto,"The local image source has an established preserving or static-asset mapping.")}else{(Review,"Remote sources, loaders, optimization, legacy layout, or dynamic props require review.")}),
        "nextjs.api.script"=>tag.and_then(|tag| if tag.spread {None} else {match tag.attrs.get("strategy") {Some(Some(s)) if s=="worker"=>Some((Unsupported,"The worker script strategy has no V1 mapping.")),Some(None)=>None,Some(Some(s)) if !["afterInteractive","beforeInteractive","lazyOnload"].contains(&s.as_str())=>None,Some(Some(_))|None=>Some((Review,"Script placement and loading behavior require review."))}}),
        "react.server-component"=> if let Location::Text{path,span}=at {syntax.function(path,*span).map(|f|if f.asynchronous&&!f.astro {(Unsupported,"An async component outside the proven Astro subset cannot render through Preact.")}else{(Auto,"The component has an Astro or synchronous static Preact strategy.")})} else {None},
        "css.tailwind"=>Some((Review,"Tailwind and PostCSS configuration must be reviewed against the target integration.")),
        "npm.manifest"=>Some((Review,"Framework dependency replacement and project scripts require review.")),
        "react.api.start-transition"|"react.hook.use-transition"|"react.hook.use-deferred-value"|"react.hook.use-insertion-effect"|"react.hook.use-sync-external-store"=>Some((Review,"The pinned compat implementation has scheduling or style-insertion differences.")),
        "nextjs.fetch"=>if let Location::Text{path,span}=at { syntax.modules.get(path).and_then(|m|m.calls.get(span).copied()).map(|n| match n {Some(n) if n<2 =>(Auto,"A fetch without options remains a build-time fetch."),Some(_)=>(Review,"Fetch options and build-time snapshot semantics require review."),None=>(Review,"Spread fetch arguments may supply options; cache and snapshot semantics require review.")}) }else{None},
        "nextjs.metadata.route"=>at.path().and_then(|p|project.nextjs().routes.files.iter().find(|f|&f.path==p)).map(|f|if matches!(f.convention,Convention::Metadata(Metadata::OpenGraphImage|Metadata::TwitterImage)) {(Unsupported,"Generated social images require a runtime image renderer.")}else{(Review,"Generated metadata is retained as a build-time endpoint with review.")}),
        _=>None,
    };
    match decision {
        Some((status, message)) => {
            result.status = Some(status);
            result.reason = if status == Review {
                Reason::Review(message)
            } else {
                Reason::Proven(message)
            };
            result.rule = Some(format!(
                "{}/assessment.{}",
                frameshift_catalog::PATH,
                result.occurrence.feature.id.replace('.', "-")
            ));
        }
        None => {
            result.status = Some(Unknown);
            result.reason = Reason::Missing(
                "The available facts do not establish the assessment rule's preconditions.",
            );
        }
    }
    // A function's syntactic candidacy is not sufficient for the Astro
    // fixed point; model::assess revisits async components after linking it.
    result
}
