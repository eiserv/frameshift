use std::collections::BTreeSet;

use frameshift_core::{FindingLimits, PathLimits, SourcePath, SourceText, Span, TextLimits};
use frameshift_input::{EntryKind, FileRead, Snapshot};
use frameshift_package::LoadLimits;
use frameshift_syntax::{
    JsDialect, JsonLimits, NodeId, ParseError, ParseLimits, SourceGoal, SyntaxNodeKind as N,
    SyntaxTree, TypeScriptKind as T, parse_js_program,
};

use crate::extends::Links;
use crate::{
    ConfigIssue, ConfigProblem, FileKind, ModuleError, ModuleLimits, PackageManifest, Resolution,
    ResolutionConfig, ResolutionMode, Resolver, UnresolvedReason, Work, below, config, parent,
};

/// All bounds used to build one graph, without runtime environment inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphLimits {
    /// File, edge, inheritance, and cumulative work bounds.
    pub modules: ModuleLimits,
    /// Source decoding bounds.
    pub text: TextLimits,
    /// Per-file parser bounds.
    pub parse: ParseLimits,
    /// Configuration JSONC bounds.
    pub json: JsonLimits,
    /// Canonical configuration path bounds.
    pub path: PathLimits,
    /// Workspace loading bounds for package-based configuration inheritance
    /// (`D-098`).
    pub package: LoadLimits,
    /// Storage bounds of the workspace loader's findings, which the graph
    /// does not retain.
    pub finding: FindingLimits,
}

/// Syntactic origin of a dependency reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Static import, including side-effect imports.
    Import,
    /// Named or star re-export with a source.
    ReExport,
    /// Dynamic `import()` expression.
    DynamicImport,
    /// TypeScript `import("...")` type.
    ImportType,
    /// TypeScript `import name = require("...")`.
    ImportEquals,
    /// Call that resembles CommonJS but has no established binding yet.
    RequireCandidate,
}

/// One dependency occurrence in original source coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Syntactic origin.
    pub kind: EdgeKind,
    /// Exact specifier expression span, including quotes when present.
    pub span: Span,
    /// Decoded literal, absent for computed or unrepresentable values.
    pub specifier: Option<String>,
    /// True only for explicitly type-only syntax or declaration files.
    pub type_only: bool,
    /// Source/emitted import form. Bundler package maps use its conditions;
    /// Node10 records it without changing target lookup.
    pub mode: ResolutionMode,
    /// Established target or explicit unresolved evidence.
    pub resolution: Resolution,
}

/// Parse state of one admitted source module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleState {
    /// Complete strict parse; this does not establish binding or runtime validity.
    Parsed,
    /// A syntax error; no dependencies are extracted from the erroneous file.
    SyntaxError(ParseError),
    /// Source text is not accepted UTF-8 or exceeds the decoding bound.
    InvalidText,
    /// The snapshot could not read the unchanged file.
    Unreadable,
}

/// A source module and its dependencies, ordered by original source span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleNode {
    /// Exact root-relative path.
    pub path: SourcePath,
    /// Module or declaration-file classification.
    pub kind: FileKind,
    /// Explicit strict parse outcome.
    pub state: ModuleState,
    /// Every extracted reference; empty after any parse error.
    pub edges: Vec<Edge>,
    /// The module's directive prologue, such as `"use client"`.
    pub directives: Vec<Directive>,
    /// Directive prologues of function bodies, such as an inline
    /// `"use server"`, in source order.
    pub function_directives: Vec<Directive>,
}

/// One directive of a prologue: a string literal statement before any other
/// statement of a program or function body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directive {
    /// The decoded value, absent when it is not representable.
    pub value: Option<String>,
    /// Whether the spelling contains an escape or line continuation, which
    /// ECMAScript's `"use strict"` rule and framework directives treat as a
    /// different directive.
    pub escaped: bool,
    /// The string literal span, including quotes.
    pub span: Span,
    /// The enclosing function body, absent for the module prologue.
    pub function: Option<NodeId>,
}

/// Deterministic source dependency graph, with nodes in source-path byte order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleGraph {
    nodes: Vec<ModuleNode>,
    config: ResolutionConfig,
}

impl ModuleGraph {
    /// Modules in canonical path order, independently of discovery order.
    pub fn nodes(&self) -> &[ModuleNode] {
        &self.nodes
    }

    /// The selected configuration with its inheritance.
    pub fn configuration(&self) -> &ResolutionConfig {
        &self.config
    }

    /// Configuration problems that prevented resolution.
    pub fn configuration_issues(&self) -> &[ConfigIssue] {
        self.config.issues()
    }

    /// Finds a node by its exact path.
    pub fn node(&self, path: &SourcePath) -> Option<&ModuleNode> {
        self.nodes
            .binary_search_by(|node| node.path.cmp(path))
            .ok()
            .map(|i| &self.nodes[i])
    }

    /// Iteratively computes the local module closure, including `start`.
    /// Results are sorted by source path. Assets and external or unresolved
    /// references have no graph node; callers must inspect their edge evidence.
    /// `include_types` controls explicit type-only edges, not inferred erasure.
    pub fn closure(
        &self,
        start: &SourcePath,
        include_types: bool,
    ) -> Result<Vec<&ModuleNode>, ModuleError> {
        let index = self
            .nodes
            .binary_search_by(|node| node.path.cmp(start))
            .map_err(|_| ModuleError::InvalidRoot)?;
        let mut seen = BTreeSet::new();
        let mut pending = vec![index];
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            for edge in &self.nodes[index].edges {
                if edge.type_only && !include_types {
                    continue;
                }
                if let Resolution::Local { path, .. } = &edge.resolution
                    && let Ok(next) = self.nodes.binary_search_by(|node| node.path.cmp(path))
                    && !seen.contains(&next)
                {
                    pending.push(next);
                }
            }
        }
        Ok(seen.into_iter().map(|i| &self.nodes[i]).collect())
    }
}

/// Builds the selected package's graph, reading every source only through the
/// snapshot. A package directory is root-relative; `None` selects the run root.
/// Configuration selection prefers its `tsconfig.json` over `jsconfig.json`.
/// Relative inheritance stays within the run root. Local dependencies stay
/// inside the selected package; sibling workspace code is not analyzed.
/// An incomplete inventory or changed snapshot returns no graph. `.cjs` uses
/// Script goal; TypeScript surfaces, including `.cts` and `.d.cts`, use Module
/// goal with their implementation or ambient dialect.
pub fn build_module_graph(
    snapshot: &Snapshot,
    package_root: Option<&SourcePath>,
    limits: GraphLimits,
) -> Result<ModuleGraph, ModuleError> {
    build_module_graph_with(snapshot, package_root, limits, &mut |_, _| Ok(()))
}

/// Builds the graph as [`build_module_graph`] does, and lets `visit` inspect
/// each strict syntax tree once, in path order, before its arena is dropped.
/// Later analyses, such as binding analysis or framework fact extraction,
/// share the parse this way. An error from `visit` returns no graph.
pub fn build_module_graph_with(
    snapshot: &Snapshot,
    package_root: Option<&SourcePath>,
    limits: GraphLimits,
    visit: &mut dyn FnMut(&SourcePath, &SyntaxTree<'_>) -> Result<(), ModuleError>,
) -> Result<ModuleGraph, ModuleError> {
    let inventory = snapshot.inventory();
    let root = package_root.map_or("", SourcePath::as_str);
    let (mut resolver, configuration) = resolver_and_config(snapshot, package_root, limits)?;
    let mut nodes = Vec::new();
    let mut edge_count = 0;
    for (id, path) in inventory.files().iter() {
        let kind = FileKind::of(path);
        if !below(path.as_str(), root) || !matches!(kind, FileKind::Module | FileKind::Declaration)
        {
            continue;
        }
        let mut node = ModuleNode {
            path: path.clone(),
            kind,
            state: ModuleState::Parsed,
            edges: Vec::new(),
            directives: Vec::new(),
            function_directives: Vec::new(),
        };
        match snapshot.read(id).map_err(ModuleError::Snapshot)? {
            FileRead::Unreadable(_) => node.state = ModuleState::Unreadable,
            FileRead::Content(file) => {
                resolver.work.charge(file.bytes().len())?;
                match SourceText::from_bytes(file.bytes().to_vec(), limits.text) {
                    Err(_) => node.state = ModuleState::InvalidText,
                    Ok(source) => {
                        let (goal, dialect) =
                            source_kind(path).expect("module and declaration files have a kind");
                        match parse_js_program(&source, goal, dialect, limits.parse) {
                            Err(error) => node.state = ModuleState::SyntaxError(error),
                            Ok(tree) => {
                                node.edges = collect_dependencies(
                                    &tree,
                                    path,
                                    &mut resolver,
                                    limits.modules.max_edges - edge_count,
                                )?;
                                edge_count += node.edges.len();
                                (node.directives, node.function_directives) =
                                    directives(&tree, &mut resolver.work)?;
                                visit(path, &tree)?;
                            }
                        }
                    }
                }
            }
        }
        nodes
            .try_reserve(1)
            .map_err(|_| ModuleError::AllocationFailed)?;
        nodes.push(node);
    }
    Ok(ModuleGraph {
        nodes,
        config: configuration,
    })
}

/// Extracts dependency facts from an existing strict syntax tree without
/// reparsing it, so later semantic stages can share one parse per file.
/// The caller associates the tree with its admitted source `path`.
/// `max_edges` is the remaining graph-wide edge allowance. Work charges stay
/// cumulative in `resolver`; failure returns no partial edge list.
/// Declaration-file paths make every reference type-only.
pub fn collect_dependencies(
    tree: &SyntaxTree<'_>,
    path: &SourcePath,
    resolver: &mut Resolver,
    max_edges: usize,
) -> Result<Vec<Edge>, ModuleError> {
    if !resolver.files.contains_key(path.as_str()) || !below(path.as_str(), &resolver.root) {
        return Err(ModuleError::InvalidRoot);
    }
    let source = tree.source();
    let declaration = FileKind::of(path) == FileKind::Declaration;
    // The third flag marks the direct child of a whole-declaration `type`
    // modifier, the only place a `resolution-mode` attribute applies.
    let mut pending = vec![(tree.root(), declaration, false)];
    let mut edges = Vec::new();
    while let Some((id, mut type_only, explicit)) = pending.pop() {
        resolver.work.charge(1)?;
        let node = tree.node(id).expect("parser-owned node");
        let explicit_child = matches!(
            node.kind(),
            N::TypeScript {
                kind: T::TypeOnly,
                ..
            }
        );
        if matches!(
            node.kind(),
            N::TypeScript {
                kind: T::TypeOnly | T::ImportType,
                ..
            }
        ) {
            type_only = true;
        }
        if let N::TypeScript {
            kind: T::ImportEquals,
            children,
        } = node.kind()
        {
            type_only |= tree
                .children(children)
                .expect("parser-owned list")
                .iter()
                .any(|&id| {
                    tree.node(id).is_some_and(|n| {
                        matches!(
                            n.kind(),
                            N::TypeScript {
                                kind: T::Modifier,
                                ..
                            }
                        ) && source.slice(n.span()).ok() == Some("type")
                    })
                });
        }
        let mut mode = None;
        let reference: Option<(NodeId, EdgeKind)> =
            if let N::ImportDeclaration {
                source, attributes, ..
            } = node.kind()
            {
                if explicit {
                    mode = attributes.and_then(|id| attribute_mode(tree, id));
                }
                Some((source, EdgeKind::Import))
            } else if let N::ExportNamed {
                source: Some(source),
                attributes,
                ..
            }
            | N::ExportAll {
                source, attributes, ..
            } = node.kind()
            {
                if explicit {
                    mode = attributes.and_then(|id| attribute_mode(tree, id));
                }
                Some((source, EdgeKind::ReExport))
            } else if let N::ImportCall { source, .. } = node.kind() {
                Some((source, EdgeKind::DynamicImport))
            } else if let N::TypeScript {
                kind: T::ImportType | T::ExternalModuleReference,
                children,
            } = node.kind()
            {
                if let Some(&options) = tree.children(children).and_then(|c| c.get(1)) {
                    mode = import_type_mode(tree, options);
                }
                tree.children(children)
                    .and_then(|c| c.first())
                    .copied()
                    .map(|id| {
                        (
                            id,
                            if matches!(
                                node.kind(),
                                N::TypeScript {
                                    kind: T::ImportType,
                                    ..
                                }
                            ) {
                                EdgeKind::ImportType
                            } else {
                                EdgeKind::ImportEquals
                            },
                        )
                    })
            } else if let N::Call {
                callee, arguments, ..
            } = node.kind()
            {
                let callee = tree.node(callee).expect("parser-owned callee");
                if matches!(
                    callee.kind(),
                    N::Identifier {
                        has_escape: true,
                        ..
                    }
                ) || matches!(callee.kind(), N::Identifier { .. })
                    && source.slice(callee.span()).ok() == Some("require")
                {
                    tree.children(arguments)
                        .and_then(|c| c.first())
                        .copied()
                        .map(|id| (id, EdgeKind::RequireCandidate))
                } else {
                    None
                }
            } else {
                None
            };
        if let Some((mut target, kind)) = reference {
            if let N::TypeScript {
                kind: T::Literal,
                children,
            } = tree.node(target).expect("parser-owned target").kind()
                && let Some(&id) = tree.children(children).and_then(|c| c.first())
            {
                target = id;
            }
            if edges.len() >= max_edges {
                return Err(ModuleError::EdgeLimit);
            }
            let specifier = tree.string_literal_value(target);
            let mode = mode.unwrap_or_else(|| resolver.mode(path, kind));
            let resolution = if kind == EdgeKind::RequireCandidate {
                Resolution::Unresolved(UnresolvedReason::BindingAnalysis)
            } else if let Some(value) = &specifier {
                resolver.resolve_in_mode(path, value, mode)?
            } else {
                Resolution::Unresolved(UnresolvedReason::Computed)
            };
            edges
                .try_reserve(1)
                .map_err(|_| ModuleError::AllocationFailed)?;
            edges.push(Edge {
                kind,
                span: tree.node(target).expect("parser-owned target").span(),
                specifier,
                type_only,
                mode,
                resolution,
            });
        }
        let first_child = pending.len();
        for child in tree.child_nodes(id).expect("parser-owned children") {
            pending.push((child, type_only, explicit_child));
        }
        pending[first_child..].reverse();
    }
    edges.sort_by_key(|edge| (edge.span.start(), edge.span.end(), edge.kind));
    Ok(edges)
}

/// A type-only declaration's `with { "resolution-mode": ... }` override:
/// exactly one string-keyed attribute with the value `import` or `require`.
/// Any other attribute list keeps the default mode, as in the reference.
fn attribute_mode(tree: &SyntaxTree<'_>, attributes: NodeId) -> Option<ResolutionMode> {
    let N::ImportAttributes { attributes } = tree.node(attributes)?.kind() else {
        return None;
    };
    let [attribute] = tree.children(attributes)? else {
        return None;
    };
    let N::ImportAttribute { key, value } = tree.node(*attribute)?.kind() else {
        return None;
    };
    mode_value(tree, key, value)
}

/// The override of `import("x", { with: { "resolution-mode": ... } })`.
fn import_type_mode(tree: &SyntaxTree<'_>, options: NodeId) -> Option<ResolutionMode> {
    let single = |id: NodeId| -> Option<(NodeId, NodeId)> {
        let N::ObjectExpression { properties, .. } = tree.node(id)?.kind() else {
            return None;
        };
        let [property] = tree.children(properties)? else {
            return None;
        };
        let N::Property {
            key,
            value,
            computed: false,
        } = tree.node(*property)?.kind()
        else {
            return None;
        };
        Some((key, value))
    };
    let (key, attributes) = single(options)?;
    let key = tree.node(key)?;
    if !matches!(
        key.kind(),
        N::IdentifierName {
            has_escape: false,
            ..
        }
    ) || !matches!(tree.source().slice(key.span()).ok()?, "with" | "assert")
    {
        return None;
    }
    let (key, value) = single(attributes)?;
    mode_value(tree, key, value)
}

fn mode_value(tree: &SyntaxTree<'_>, key: NodeId, value: NodeId) -> Option<ResolutionMode> {
    if tree.string_literal_value(key)? != "resolution-mode" {
        return None;
    }
    match tree.string_literal_value(value)?.as_str() {
        "import" => Some(ResolutionMode::Import),
        "require" => Some(ResolutionMode::Require),
        _ => None,
    }
}

/// The module prologue and every function-body prologue of a strict tree,
/// found with an explicit stack because trees can be as deep as their nodes.
pub(crate) fn directives(
    tree: &SyntaxTree<'_>,
    work: &mut Work,
) -> Result<(Vec<Directive>, Vec<Directive>), ModuleError> {
    let mut module = Vec::new();
    let mut functions = Vec::new();
    let mut pending = vec![tree.root()];
    while let Some(id) = pending.pop() {
        work.charge(1)?;
        let kind = tree.node(id).expect("parser-owned node").kind();
        let prologue = if let N::Program { body, .. } = kind {
            Some((body, None))
        } else if let N::FunctionBody { body, .. } = kind {
            Some((body, Some(id)))
        } else {
            None
        };
        if let Some((body, function)) = prologue {
            for &statement in tree.children(body).expect("parser-owned list") {
                let N::Directive { expression } =
                    tree.node(statement).expect("parser-owned node").kind()
                else {
                    break;
                };
                let span = tree.node(expression).expect("parser-owned node").span();
                let spelling = tree.source().slice(span).unwrap_or("");
                let target = if function.is_some() {
                    &mut functions
                } else {
                    &mut module
                };
                target
                    .try_reserve(1)
                    .map_err(|_| ModuleError::AllocationFailed)?;
                target.push(Directive {
                    value: tree.string_literal_value(expression),
                    escaped: spelling.contains('\\'),
                    span,
                    function,
                });
            }
        }
        pending
            .try_reserve(8)
            .map_err(|_| ModuleError::AllocationFailed)?;
        pending.extend(tree.child_nodes(id).expect("parser-owned children"));
    }
    functions.sort_by_key(|d: &Directive| (d.span.start(), d.span.end()));
    Ok((module, functions))
}

/// The resolver that [`build_module_graph`] uses for the selected package:
/// its `tsconfig.json` or `jsconfig.json` with inheritance, every excluded
/// entry, and every manifest that can govern the package. Later analyses,
/// such as the static evaluator, resolve references exactly as the graph does.
pub fn snapshot_resolver(
    snapshot: &Snapshot,
    package_root: Option<&SourcePath>,
    limits: GraphLimits,
) -> Result<Resolver, ModuleError> {
    resolver_and_config(snapshot, package_root, limits).map(|(resolver, _)| resolver)
}

fn resolver_and_config(
    snapshot: &Snapshot,
    package_root: Option<&SourcePath>,
    limits: GraphLimits,
) -> Result<(Resolver, ResolutionConfig), ModuleError> {
    let inventory = snapshot.inventory();
    if !inventory.is_complete() {
        return Err(ModuleError::IncompleteInventory);
    }
    if inventory.files().len() > limits.modules.max_files {
        return Err(ModuleError::FileLimit);
    }
    let root = package_root.map_or("", SourcePath::as_str);
    if let Some(path) = package_root
        && !inventory
            .entry(path)
            .is_some_and(|e| matches!(e.kind(), EntryKind::Directory))
    {
        return Err(ModuleError::InvalidRoot);
    }
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    let mut work = Work {
        remaining: limits.modules.max_work,
    };
    let mut configuration = ResolutionConfig::default();
    for filename in ["tsconfig.json", "jsconfig.json"] {
        let path = SourcePath::new(&format!("{prefix}{filename}"), limits.path)
            .map_err(|_| ModuleError::InvalidRoot)?;
        if inventory.files().find(&path).is_some() {
            configuration = config::load(
                snapshot,
                &path,
                limits,
                &mut Vec::new(),
                &mut Links::new(),
                &mut work,
            )?;
            break;
        }
        if inventory.entry(&path).is_some() {
            configuration.issues.push(ConfigIssue {
                path,
                problem: ConfigProblem::Unreadable,
            });
            break;
        }
    }
    let files = inventory.files().as_slice().to_vec();
    let mut resolver = Resolver::new(files, package_root, configuration.clone(), limits.modules)?;
    resolver
        .work
        .charge((limits.modules.max_work - work.remaining) as usize)?;
    for entry in inventory.entries() {
        resolver.work.charge(entry.path().as_str().len() + 1)?;
        if !matches!(entry.kind(), EntryKind::Directory) {
            resolver.excluded.insert(entry.path().as_str().to_owned());
        }
    }
    // Manifests inside the package supply directory entry points and nested
    // scopes; those above it can be the package's own scope.
    for (id, path) in inventory.files().iter() {
        let directory = parent(path.as_str());
        if path.as_str().rsplit('/').next() != Some("package.json")
            || !(below(directory, root) || directory == root || below(root, directory))
        {
            continue;
        }
        let manifest = match snapshot.read(id).map_err(ModuleError::Snapshot)? {
            FileRead::Unreadable(_) => None,
            FileRead::Content(file) => {
                resolver.work.charge(file.bytes().len())?;
                SourceText::from_bytes(file.bytes().to_vec(), limits.text)
                    .ok()
                    .and_then(|source| PackageManifest::parse(&source, limits.json))
            }
        };
        resolver.add_manifest(path, manifest)?;
    }
    Ok((resolver, configuration))
}

/// The source goal and dialect that the graph parses a module path with,
/// or `None` for a path that is not a module or declaration file: `.cjs`
/// uses Script goal, every other surface Module goal.
pub fn source_kind(path: &SourcePath) -> Option<(SourceGoal, JsDialect)> {
    let kind = FileKind::of(path);
    let text = path.as_str();
    let dialect = match kind {
        FileKind::Declaration => JsDialect::TypeScriptDeclaration,
        FileKind::Module if text.ends_with(".tsx") => JsDialect::Tsx,
        FileKind::Module
            if text.ends_with(".ts") || text.ends_with(".mts") || text.ends_with(".cts") =>
        {
            JsDialect::TypeScript
        }
        FileKind::Module if text.ends_with(".js") || text.ends_with(".jsx") => JsDialect::Jsx,
        FileKind::Module => JsDialect::JavaScript,
        FileKind::Stylesheet | FileKind::Json | FileKind::Asset => return None,
    };
    let goal = if text.ends_with(".cjs") {
        SourceGoal::Script
    } else {
        SourceGoal::Module
    };
    Some((goal, dialect))
}
