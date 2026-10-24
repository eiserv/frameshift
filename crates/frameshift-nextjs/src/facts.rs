//! Next.js source facts, without compatibility assessment or execution.

use std::collections::BTreeMap;

use frameshift_bindings::{Bindings, ExportName, ImportedName, Meaning, Target};
use frameshift_core::{SourcePath, Span};
use frameshift_evaluate::{
    EvaluateError, EvaluateLimits, Evaluator, Object, Reason, Unknown, Value,
};
use frameshift_input::Snapshot;
use frameshift_modules::{GraphLimits, ModuleError, ModuleState, Resolution};
use frameshift_syntax::{NodeId, SyntaxNodeKind as N, SyntaxTree, TypeScriptKind as T};

use crate::{
    AppAnalysis, AppError, ConfigFile, ConfigLoad, Convention, analyze_app_with, app_routes,
};

/// Existing graph and evaluator bounds for one fact analysis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FactLimits {
    /// Snapshot, parser, and module bounds.
    pub graph: GraphLimits,
    /// Cumulative static evaluation bounds.
    pub evaluate: EvaluateLimits,
}

/// Source facts of an App Router package, with no target-side decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppFacts {
    /// Routes, strict parse outcomes, import graph, and render contexts.
    pub app: AppAnalysis,
    /// The selected configuration, including load uncertainty.
    pub configuration: ConfigurationFacts,
    /// Every successfully parsed module, in path order.
    pub modules: Vec<FrameworkModuleFacts>,
}

/// A failure stops analysis without a partial result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactError {
    /// App Router or graph construction failed.
    App(AppError),
    /// Static evaluation failed operationally, rather than yielding an unknown value.
    Evaluate(EvaluateError),
}

impl std::fmt::Display for FactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::App(error) => error.fmt(f),
            Self::Evaluate(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FactError {}

/// Evaluated configuration. A missing file means the caller established
/// absence through detection; an unloadable file never means defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationFacts {
    /// The detector's selected file and its loading evidence.
    pub file: Option<ConfigFile>,
    /// The original evaluated value. Absent only when loading is uncertain
    /// or unsupported. Unknown values and incomplete objects are retained.
    pub value: Option<Value>,
}

/// Evidence for one property, without treating uncertainty as absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigProperty<'a> {
    /// A value, which may itself have unknown parts.
    Value(&'a Value),
    /// A complete object establishes that the property is absent.
    Absent,
    /// The object or its property set cannot be established.
    Unknown,
    /// A known non-object occupies a required object position.
    Invalid,
}

/// Established page-extension configuration, without migration eligibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageExtensions {
    /// Ordered suffixes, including multipart or non-code suffixes.
    Known(Vec<String>),
    /// Loading, property presence, or an element is not established.
    Unknown,
    /// A known setting has an invalid shape or path-like suffix.
    Invalid,
}

/// A configuration concern whose runtime resolver is not modeled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundlerSetting {
    /// The webpack hook.
    Webpack,
    /// Current Turbopack options, including aliases, rules, and extensions.
    Turbopack,
    /// The older `experimental.turbo` spelling.
    LegacyTurbo,
}

impl ConfigurationFacts {
    /// Looks up a nested property. Missing properties are absent only in a
    /// complete object. An empty path addresses the configuration value.
    pub fn property(&self, path: &[&str]) -> ConfigProperty<'_> {
        let Some(mut value) = self.value.as_ref() else {
            return ConfigProperty::Unknown;
        };
        for key in path {
            match value {
                Value::Object(object) => match object.get(key) {
                    Some(next) => value = next,
                    None if object.complete => return ConfigProperty::Absent,
                    None => return ConfigProperty::Unknown,
                },
                Value::Unknown(_) => return ConfigProperty::Unknown,
                Value::Undefined
                | Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::BigInt(_)
                | Value::String(_)
                | Value::Array(_) => return ConfigProperty::Invalid,
            }
        }
        ConfigProperty::Value(value)
    }

    /// Page extensions are defaults only when their absence is established.
    /// Known unsupported-language suffixes remain facts, not parser support.
    pub fn page_extensions(&self) -> PageExtensions {
        match self.property(&["pageExtensions"]) {
            ConfigProperty::Absent => PageExtensions::Known(
                ["tsx", "ts", "jsx", "js"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            ),
            ConfigProperty::Unknown | ConfigProperty::Value(Value::Unknown(_)) => {
                PageExtensions::Unknown
            }
            ConfigProperty::Value(Value::Array(items)) if !items.is_empty() => {
                let mut extensions = Vec::new();
                for item in items {
                    match item {
                        Value::String(extension)
                            if !extension.is_empty()
                                && !extension.starts_with('.')
                                && !extension
                                    .chars()
                                    .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':')) =>
                        {
                            extensions.push(extension.clone())
                        }
                        Value::Unknown(_) => return PageExtensions::Unknown,
                        Value::Undefined
                        | Value::Null
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::BigInt(_)
                        | Value::String(_)
                        | Value::Array(_)
                        | Value::Object(_) => return PageExtensions::Invalid,
                    }
                }
                PageExtensions::Known(extensions)
            }
            ConfigProperty::Invalid
            | ConfigProperty::Value(
                Value::Undefined
                | Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::BigInt(_)
                | Value::String(_)
                | Value::Array(_)
                | Value::Object(_),
            ) => PageExtensions::Invalid,
        }
    }

    /// All unmodeled bundler concerns, including uncertainty and invalid
    /// shapes. Even an empty declared customization is retained. This does
    /// not alter the graph's explicit TypeScript source-lookup profile.
    pub fn bundler_settings(&self) -> Vec<(BundlerSetting, ConfigProperty<'_>)> {
        [
            (BundlerSetting::Webpack, self.property(&["webpack"])),
            (BundlerSetting::Turbopack, self.property(&["turbopack"])),
            (
                BundlerSetting::LegacyTurbo,
                self.property(&["experimental", "turbo"]),
            ),
        ]
        .into_iter()
        .filter(|(_, property)| *property != ConfigProperty::Absent)
        .collect()
    }
}

/// Kind of convention export; this records source roles, not validity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacetKind {
    /// Route segment configuration such as `dynamic` or `revalidate`.
    Segment,
    /// Static metadata or viewport data.
    Metadata,
    /// A generator export. Functions are never invoked.
    Generator,
    /// An HTTP method of a route handler.
    Handler,
    /// The default export of a generated metadata route.
    MetadataRoute,
}

/// An explicit runtime export of a convention file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportFact {
    /// Its Next.js role.
    pub kind: FacetKind,
    /// Exact exported name.
    pub name: ExportName,
    /// Original export declaration or specifier.
    pub span: Span,
    /// Its safely evaluated value, including all unknown parts and reasons.
    pub value: Value,
}

/// How a proven import is used syntactically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportedUseKind {
    /// A direct call, including optional calls.
    Call,
    /// A JSX opening tag; its matching closing tag is not counted again.
    Render,
    /// Another value reference, including escaping the value to other code.
    Reference,
}

/// One value-meaning reference whose root binding is a direct import.
/// This includes configuration plugin wrappers. It is import provenance,
/// not proof of runtime execution; type queries may use value meanings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedUse {
    /// The complete call, opening tag, or reference/member expression.
    pub span: Span,
    /// The root identifier reference.
    pub reference: Span,
    /// Original import specifier span.
    pub source: Span,
    /// The graph edge's established target or unresolved evidence. Absent
    /// only if no corresponding import edge exists; never guessed from text.
    pub resolution: Option<Resolution>,
    /// Decoded import source, including external configuration wrappers.
    pub specifier: String,
    /// Export selected by the import, independent of its local spelling.
    pub imported: ImportedName,
    /// Member path after the imported value. `None` is a computed property
    /// with no literal key, never a guessed API name.
    pub members: Vec<Option<String>>,
    /// The syntactic use.
    pub kind: ImportedUseKind,
}

/// Next.js import and export evidence from one strict tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameworkModuleFacts {
    /// The source module.
    pub path: SourcePath,
    /// Explicit recognized convention exports, in source order.
    pub exports: Vec<ExportFact>,
    /// `export *` declarations whose full convention names are not established.
    pub star_exports: Vec<Span>,
    /// Binding-proven import uses, in source order. Local aliases and
    /// re-export chains are not inferred as direct framework provenance.
    pub import_uses: Vec<ImportedUse>,
}

impl FrameworkModuleFacts {
    /// Direct uses of the `next` package and its subpaths. Names alone,
    /// globals, type-only imports, and other packages never match.
    pub fn api_uses(&self) -> impl Iterator<Item = &ImportedUse> {
        self.import_uses
            .iter()
            .filter(|usage| {
                (usage.specifier == "next" || usage.specifier.starts_with("next/"))
                    && matches!(&usage.resolution, Some(Resolution::External { package }) if package == "next")
            })
    }
}

struct Work(u64);

impl Work {
    fn charge(&mut self, units: usize) -> Result<(), ModuleError> {
        self.0 = self
            .0
            .checked_sub(units as u64)
            .ok_or(ModuleError::WorkLimit)?;
        Ok(())
    }
}

fn facet(name: &ExportName, convention: Convention) -> Option<FacetKind> {
    let ExportName::Named(name) = name else {
        return (*name == ExportName::Default && matches!(convention, Convention::Metadata(_)))
            .then_some(FacetKind::MetadataRoute);
    };
    match name.as_str() {
        "dynamic" | "dynamicParams" | "revalidate" | "fetchCache" | "runtime"
        | "preferredRegion" | "maxDuration" | "experimental_ppr" => Some(FacetKind::Segment),
        "metadata" | "viewport" => Some(FacetKind::Metadata),
        "generateStaticParams"
        | "generateMetadata"
        | "generateViewport"
        | "generateSitemaps"
        | "generateImageMetadata" => Some(FacetKind::Generator),
        "GET" | "HEAD" | "OPTIONS" | "POST" | "PUT" | "DELETE" | "PATCH"
            if convention == Convention::Route =>
        {
            Some(FacetKind::Handler)
        }
        _ => None,
    }
}

fn unwrap(tree: &SyntaxTree<'_>, mut node: NodeId) -> NodeId {
    loop {
        let kind = tree.node(node).expect("tree-owned node").kind();
        if let N::Parenthesized { expression } = kind {
            node = expression;
        } else if let N::TypeScript {
            kind: T::As | T::Satisfies | T::NonNull | T::Instantiation,
            children,
        } = kind
        {
            let Some(first) = tree.children(children).and_then(|c| c.first()) else {
                return node;
            };
            node = *first;
        } else if let N::TypeScript {
            kind: T::Assertion,
            children,
        } = kind
        {
            // An angle-bracket assertion stores its type first and the
            // asserted expression second.
            let Some(inner) = tree.children(children).and_then(|c| c.get(1)) else {
                return node;
            };
            node = *inner;
        } else {
            return node;
        }
    }
}

fn import_uses(
    tree: &SyntaxTree<'_>,
    bindings: &Bindings,
    work: &mut Work,
) -> Result<Vec<ImportedUse>, ModuleError> {
    work.charge(tree.nodes().len())?;
    let mut parents = vec![None; tree.nodes().len()];
    let mut pending = vec![tree.root()];
    while let Some(node) = pending.pop() {
        for child in tree.child_nodes(node).expect("tree-owned node") {
            parents[child.index()] = Some(node);
            pending.push(child);
        }
    }
    let span = |node| tree.node(node).expect("tree-owned node").span();
    let mut uses = Vec::new();
    for reference in bindings.references() {
        work.charge(1)?;
        if !reference.read || !reference.meaning.overlaps(Meaning::VALUE) {
            continue;
        }
        let Target::Binding(id) = reference.target else {
            continue;
        };
        let Some(import) = &bindings.binding(id).import else {
            continue;
        };
        let Some(specifier) = &import.specifier else {
            continue;
        };
        if import.type_only {
            continue;
        }
        let mut node = reference.node;
        let mut members = Vec::new();
        let mut kind = ImportedUseKind::Reference;
        while let Some(parent) = parents[node.index()] {
            work.charge(1)?;
            let parent_kind = tree.node(parent).expect("tree-owned node").kind();
            if let N::Member {
                object,
                property,
                computed,
                ..
            } = parent_kind
                && object == node
            {
                work.charge(span(property).len())?;
                members.push(if computed {
                    tree.string_literal_value(unwrap(tree, property))
                } else {
                    tree.identifier_value(property)
                });
                node = parent;
            } else if let N::JsxMemberExpression { object, property } = parent_kind
                && object == node
            {
                work.charge(span(property).len())?;
                members.push(tree.identifier_value(property));
                node = parent;
            } else if let N::Parenthesized { expression } = parent_kind
                && expression == node
            {
                node = parent;
            } else if matches!(parent_kind, N::TypeScript { .. })
                && unwrap(tree, parent) == unwrap(tree, node)
            {
                node = parent;
            } else if let N::Call { callee, .. } = parent_kind
                && callee == node
            {
                node = parent;
                kind = ImportedUseKind::Call;
                break;
            } else if matches!(parent_kind, N::JsxOpeningElement { .. }) {
                node = parent;
                kind = ImportedUseKind::Render;
                break;
            } else if matches!(parent_kind, N::JsxClosingElement { .. }) {
                kind = ImportedUseKind::Render;
                break;
            } else {
                break;
            }
        }
        if kind == ImportedUseKind::Render
            && !matches!(
                tree.node(node).expect("tree-owned node").kind(),
                N::JsxOpeningElement { .. }
            )
        {
            continue;
        }
        work.charge(specifier.len())?;
        if let ImportedName::Named(name) = &import.imported {
            work.charge(name.len())?;
        }
        uses.try_reserve(1)
            .map_err(|_| ModuleError::AllocationFailed)?;
        uses.push(ImportedUse {
            span: span(node),
            reference: span(reference.node),
            source: span(import.source),
            resolution: None,
            specifier: specifier.clone(),
            imported: import.imported.clone(),
            members,
            kind,
        });
    }
    uses.sort_by_key(|usage| {
        (
            usage.span.start(),
            usage.span.end(),
            usage.reference.start(),
        )
    });
    Ok(uses)
}

/// Extracts configuration, convention exports, and direct-import API uses
/// alongside one App Router analysis. All modules share its strict parse and
/// bindings. Configuration imports and export values use the bounded static
/// evaluator, which never invokes generators, hooks, or plugin wrappers.
///
/// `config` is detection evidence, and `extensions` is an explicit caller
/// choice. Configuration facts never silently change route discovery or
/// claim that the source-lookup graph models custom runtime bundlers.
pub fn analyze_app_facts(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    config: Option<&ConfigFile>,
    limits: FactLimits,
) -> Result<AppFacts, FactError> {
    analyze_app_facts_with(
        snapshot,
        package,
        app,
        extensions,
        config,
        limits,
        &mut |_, _, _| Ok(()),
    )
}

/// As [`analyze_app_facts`], also visiting each strict tree with its bindings.
/// This composes framework facts with other source-model analyses without
/// another graph parse. A visitor failure returns no partial facts.
pub fn analyze_app_facts_with(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    config: Option<&ConfigFile>,
    limits: FactLimits,
    visit: &mut dyn FnMut(&SourcePath, &SyntaxTree<'_>, &Bindings) -> Result<(), ModuleError>,
) -> Result<AppFacts, FactError> {
    let routes = app_routes(snapshot.inventory(), package, app, extensions)
        .map_err(|error| FactError::App(AppError::Routes(error)))?;
    let conventions: BTreeMap<_, _> = routes
        .files
        .iter()
        .map(|file| (&file.path, file.convention))
        .collect();
    let mut evaluator = Evaluator::new(snapshot, package, limits.graph, limits.evaluate)
        .map_err(FactError::Evaluate)?;
    let mut modules = Vec::new();
    let mut configuration = None;
    let mut failure = None;
    let mut work = Work(limits.graph.modules.max_work);
    let analysis = analyze_app_with(
        snapshot,
        package,
        app,
        extensions,
        limits.graph,
        &mut |path, tree, bindings| {
            let result = (|| {
                visit(path, tree, bindings)
                    .map_err(|error| FactError::App(AppError::Modules(error)))?;
                let imports = import_uses(tree, bindings, &mut work)
                    .map_err(|error| FactError::App(AppError::Modules(error)))?;
                if config.is_some_and(|file| file.path == *path && file.load == ConfigLoad::Loaded)
                {
                    let declares_default = bindings
                        .exports()
                        .iter()
                        .any(|export| !export.type_only && export.name == ExportName::Default);
                    let mut value = evaluator
                        .tree_export(path, tree, bindings, &ExportName::Default)
                        .map_err(FactError::Evaluate)?;
                    if !declares_default {
                        value = evaluator
                            .tree_export(path, tree, bindings, &ExportName::Assignment)
                            .map_err(FactError::Evaluate)?;
                    }
                    configuration = Some(value);
                }
                let mut exports = Vec::new();
                let mut star_exports = Vec::new();
                if let Some(&convention) = conventions.get(path) {
                    for export in bindings.exports().iter().filter(|export| !export.type_only) {
                        let span = tree.node(export.node).expect("tree-owned export").span();
                        if export.name == ExportName::Star {
                            star_exports.push(span);
                        } else if let Some(kind) = facet(&export.name, convention) {
                            exports.push(ExportFact {
                                kind,
                                name: export.name.clone(),
                                span,
                                value: evaluator
                                    .tree_export(path, tree, bindings, &export.name)
                                    .map_err(FactError::Evaluate)?,
                            });
                        }
                    }
                    exports.sort_by_key(|export| (export.span.start(), export.span.end()));
                }
                modules.push(FrameworkModuleFacts {
                    path: path.clone(),
                    exports,
                    star_exports,
                    import_uses: imports,
                });
                Ok::<(), FactError>(())
            })();
            result.map_err(|error| {
                failure = Some(error);
                ModuleError::AllocationFailed
            })
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    let app = analysis.map_err(FactError::App)?;
    for module in &mut modules {
        let Some(node) = app.graph.node(&module.path) else {
            continue;
        };
        for usage in &mut module.import_uses {
            work.charge(1)
                .map_err(|error| FactError::App(AppError::Modules(error)))?;
            if let Ok(index) = node
                .edges
                .binary_search_by_key(&(usage.source.start(), usage.source.end()), |edge| {
                    (edge.span.start(), edge.span.end())
                })
            {
                let resolution = &node.edges[index].resolution;
                let bytes = match resolution {
                    Resolution::Local { path, .. } => path.as_str().len(),
                    Resolution::External { package } => package.len(),
                    Resolution::NodeBuiltin | Resolution::Unresolved(_) => 1,
                };
                work.charge(bytes)
                    .map_err(|error| FactError::App(AppError::Modules(error)))?;
                usage.resolution = Some(resolution.clone());
            }
        }
    }
    let value = match config {
        None => Some(Value::Object(Object {
            entries: Vec::new(),
            complete: true,
            unknown_properties: Vec::new(),
        })),
        Some(file) if file.load != ConfigLoad::Loaded => None,
        Some(file) => match configuration {
            Some(value) => Some(value),
            None => Some(
                if app
                    .graph
                    .node(&file.path)
                    .is_some_and(|node| node.state != ModuleState::Parsed)
                {
                    Value::Unknown(Unknown {
                        reason: Reason::Syntax,
                        module: file.path.clone(),
                        span: None,
                    })
                } else {
                    evaluator.config(&file.path).map_err(FactError::Evaluate)?
                },
            ),
        },
    };
    Ok(AppFacts {
        app,
        configuration: ConfigurationFacts {
            file: config.cloned(),
            value,
        },
        modules,
    })
}
