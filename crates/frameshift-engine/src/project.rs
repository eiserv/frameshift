//! Source facts assembled for assessment, without assigning migration status.
//!
//! The model owns the App Router analysis and a complete selected-package
//! inventory. Extension kinds are evidence, never file disposition or unit
//! classification. Route dependencies are computed on demand, so shared
//! layouts do not duplicate the graph in every route record.

use std::collections::BTreeSet;

use frameshift_bindings::{Bindings, Reference, Target};
use frameshift_core::{SourcePath, Span};
use frameshift_evaluate::EvaluateError;
use frameshift_input::{EntryKind, Snapshot};
use frameshift_modules::{Edge, FileKind, ModuleGraph, ModuleState, Resolution};
use frameshift_nextjs::{
    AppAnalysis, AppError, ComponentError, ComponentGraph, ConfigFile, ConfigurationFacts,
    Convention, EnvironmentAnalysis, FactError, FactLimits, FrameworkModuleFacts,
    analyze_app_facts_with, classify_environment, link_components, summarize_components,
    summarize_environment,
};
use frameshift_syntax::{NodeId, SyntaxNodeKind as N, SyntaxTree, TypeScriptKind};

use crate::limits::Limits;

/// One accounted entry of the selected package, in original root coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    /// Exact source path, including the package prefix.
    pub path: SourcePath,
    /// Admission, exclusion, or directory evidence from discovery.
    pub state: FileState,
    /// Extension classification for admitted files only. It does not imply
    /// that an asset can be copied or a module can be transformed.
    pub kind: Option<FileKind>,
    /// Next.js convention, when the App Router recognizes this file.
    pub convention: Option<Convention>,
}

/// Whether discovery admitted the path for reading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// A regular file within discovery limits.
    Admitted,
    /// A directory, ignored path, or excluded entry with its original reason.
    Other(EntryKind),
}

/// The implemented source model of one App Router package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    package: Option<SourcePath>,
    files: Vec<ProjectFile>,
    app: AppAnalysis,
    configuration: ConfigurationFacts,
    framework: Vec<FrameworkModuleFacts>,
    semantics: Vec<ModuleFacts>,
    components: ComponentGraph,
    environment: EnvironmentAnalysis,
    max_dependency_work: u64,
}

/// Why source-model construction or a dependency query failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectError {
    /// Invalid relations between run limits.
    Limits(crate::limits::LimitsError),
    /// The app is not lexically inside the selected package.
    OutsidePackage,
    /// App Router or module analysis failed; no model is returned.
    App(AppError),
    /// Static evaluation of configuration or convention exports failed
    /// operationally, rather than yielding unknown values.
    Evaluate(EvaluateError),
    /// A route index does not belong to this model.
    InvalidRoute,
    /// Component linking or a dependency query exhausted `modules.max-work`.
    WorkLimit,
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Limits(e) => e.fmt(f),
            Self::OutsidePackage => f.write_str("app directory is outside the selected package"),
            Self::App(e) => e.fmt(f),
            Self::Evaluate(e) => e.fmt(f),
            Self::InvalidRoute => f.write_str("route index is outside the project model"),
            Self::WorkLimit => f.write_str("project dependency work limit reached"),
        }
    }
}

impl std::error::Error for ProjectError {}

/// Builds one source model from an existing snapshot. The caller selects
/// the package, app directory, page extensions, and the configuration file
/// that detection found explicitly; this API does not guess configuration
/// or framework identity. `None` establishes that no configuration exists.
/// Source files are parsed once by the Next.js fact analysis, which also
/// evaluates the configuration and convention exports (`D-097`); files
/// outside its graph are inventoried, not read. Incomplete discovery and
/// invalid roots fail without a partial model. Component linking charges a
/// fresh `modules.max-work` budget; its exhaustion also returns no model.
pub fn build(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    config: Option<&ConfigFile>,
    limits: &Limits,
) -> Result<Project, ProjectError> {
    build_with(
        snapshot,
        package,
        app,
        extensions,
        config,
        limits,
        &mut |_, _, _| {},
    )
}

/// Builds the source model while sharing each strict parse with an in-memory
/// consumer. The visitor must not execute source or perform filesystem I/O.
/// Its observations have the same canonical path order as the module graph.
pub fn build_with(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    config: Option<&ConfigFile>,
    limits: &Limits,
    visitor: &mut dyn FnMut(&SourcePath, &SyntaxTree<'_>, &Bindings),
) -> Result<Project, ProjectError> {
    limits.validate().map_err(ProjectError::Limits)?;
    if !inside(app, package) {
        return Err(ProjectError::OutsidePackage);
    }
    let mut semantics = Vec::new();
    let mut summaries = Vec::new();
    let mut environments = Vec::new();
    let facts = analyze_app_facts_with(
        snapshot,
        package,
        app,
        extensions,
        config,
        FactLimits {
            graph: limits.modules(),
            evaluate: limits.evaluate,
        },
        &mut |path, tree, bindings| {
            visitor(path, tree, bindings);
            semantics.push(module_facts(path, tree, bindings));
            summaries.push(summarize_components(path, tree, bindings));
            environments.push(summarize_environment(path, tree, bindings));
            Ok(())
        },
    )
    .map_err(|e| match e {
        FactError::App(e) => ProjectError::App(e),
        FactError::Evaluate(e) => ProjectError::Evaluate(e),
    })?;
    let app = facts.app;
    let components = link_components(&app.graph, &app.routes, &summaries, limits.modules.max_work)
        .map_err(|e| match e {
            ComponentError::WorkLimit => ProjectError::WorkLimit,
        })?;
    let environment = classify_environment(&app.graph, &app.contexts, &components, &environments);
    let inventory = snapshot.inventory();
    let mut files: Vec<_> = inventory
        .files()
        .as_slice()
        .iter()
        .filter(|path| inside(path, package))
        .map(|path| ProjectFile {
            path: path.clone(),
            state: FileState::Admitted,
            kind: Some(FileKind::of(path)),
            convention: None,
        })
        .chain(
            inventory
                .entries()
                .iter()
                .filter(|entry| inside(entry.path(), package))
                .map(|entry| ProjectFile {
                    path: entry.path().clone(),
                    state: FileState::Other(entry.kind().clone()),
                    kind: None,
                    convention: None,
                }),
        )
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for file in &mut files {
        file.convention = app
            .routes
            .files
            .binary_search_by(|f| f.path.cmp(&file.path))
            .ok()
            .map(|i| app.routes.files[i].convention);
    }
    Ok(Project {
        package: package.cloned(),
        files,
        app,
        configuration: facts.configuration,
        framework: facts.modules,
        semantics,
        components,
        environment,
        max_dependency_work: limits.modules.max_work,
    })
}

pub(crate) fn inside(path: &SourcePath, package: Option<&SourcePath>) -> bool {
    package.is_none_or(|root| {
        path.as_str()
            .strip_prefix(root.as_str())
            .is_some_and(|tail| tail.starts_with('/'))
    })
}

/// One JSX opening tag and the callable that lexically contains it.
/// A render site does not prove that the callable executes or that a local
/// binding is a component. Closing tags are not counted again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderSite {
    /// Exact opening-tag span.
    pub span: Span,
    /// Enclosing function, arrow, or method, absent at module level.
    /// Computed method keys retain their enclosing callable's owner.
    pub owner: Option<Span>,
    /// Original tag name, including any namespace or member suffix.
    pub spelling: String,
    /// Binding provenance of the root JSX name. Absent for intrinsic,
    /// namespaced, and `this` names; never guessed from capitalization.
    pub reference: Option<Reference>,
}

/// Binding and JSX facts collected from the graph's one strict parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleFacts {
    /// Source module.
    pub path: SourcePath,
    /// Scopes, declarations, imports, references, and exports. Node IDs are
    /// local to this file; `spans` retains their original coordinates.
    pub bindings: Bindings,
    /// Spans indexed by syntax-node ID, without retaining another syntax tree.
    pub spans: Vec<Span>,
    /// Render sites in source order. Binding provenance plus imports is the
    /// component-graph evidence; imported definitions and HOCs are not guessed.
    pub renders: Vec<RenderSite>,
    /// Direct `process.env` accesses with global-name provenance. No
    /// environment file or host variable is read; aliases are not inferred.
    pub environment: Vec<EnvironmentUse>,
    /// Opening tags of intrinsic `style` elements with a `jsx` attribute,
    /// the spelling that styled-jsx compiles, in source order.
    pub styled_jsx: Vec<Span>,
    /// Non-computed or literal `searchParams` member reads and object
    /// pattern keys, in source order. This is property-name evidence for
    /// later filtering by convention and render context, not a binding.
    pub search_params: Vec<Span>,
}

/// A direct access through a global (possibly ambient) `process` name.
/// This is syntax and binding evidence, not proof of a Node.js runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentUse {
    /// Span of `process.env` or its immediate property access.
    pub span: Span,
    /// Decoded literal property name. `None` means computed or whole-object
    /// use, never absence of environment dependence.
    pub name: Option<String>,
}

impl EnvironmentUse {
    /// Next.js public-prefix classification, absent for unknown property names.
    pub fn is_public(&self) -> Option<bool> {
        self.name
            .as_ref()
            .map(|name| name.starts_with("NEXT_PUBLIC_"))
    }
}

fn module_facts(path: &SourcePath, tree: &SyntaxTree<'_>, bindings: &Bindings) -> ModuleFacts {
    let mut renders = Vec::new();
    let mut environment = Vec::new();
    let mut styled_jsx = Vec::new();
    let mut search_params = Vec::new();
    let mut pending = vec![(tree.root(), None, None)];
    while let Some((id, owner, parent)) = pending.pop() {
        let node = tree.node(id).expect("parser-owned node");
        let inherited_owner = owner;
        let owner = if matches!(
            node.kind(),
            N::FunctionDeclaration { .. }
                | N::FunctionExpression { .. }
                | N::ArrowFunction { .. }
                | N::MethodDefinition { .. }
        ) {
            Some(node.span())
        } else {
            owner
        };
        if let N::Member {
            object,
            property,
            computed,
            ..
        } = node.kind()
            && member_name(tree, property, computed).as_deref() == Some("env")
            && let Some(reference) = bindings.reference(object)
            && reference.name == "process"
            && reference.target == Target::Global
        {
            let (span, name) = parent
                .and_then(|parent| {
                    let outer = tree.node(parent)?;
                    if let N::Member {
                        object,
                        property,
                        computed,
                        ..
                    } = outer.kind()
                        && object == id
                    {
                        Some((outer.span(), member_name(tree, property, computed)))
                    } else {
                        None
                    }
                })
                .unwrap_or((node.span(), None));
            environment.push(EnvironmentUse { span, name });
        }
        let search = if let N::Member {
            property, computed, ..
        } = node.kind()
        {
            (member_name(tree, property, computed).as_deref() == Some("searchParams"))
                .then_some(property)
        } else if let N::PatternProperty {
            key,
            computed: false,
            ..
        } = node.kind()
        {
            (property_key(tree, key).as_deref() == Some("searchParams")).then_some(key)
        } else if let N::ShorthandPatternProperty { target, .. } = node.kind() {
            (tree.identifier_value(target).as_deref() == Some("searchParams")).then_some(target)
        } else {
            None
        };
        if let Some(key) = search {
            search_params.push(tree.node(key).expect("parser-owned key").span());
        }
        if let N::JsxOpeningElement {
            name, attributes, ..
        } = node.kind()
            && jsx_name(tree, name) == Some("style")
            && tree
                .children(attributes)
                .unwrap_or_default()
                .iter()
                .any(|&attribute| {
                    matches!(tree.node(attribute).unwrap().kind(),
                        N::JsxAttribute { name, .. } if jsx_name(tree, name) == Some("jsx"))
                })
        {
            styled_jsx.push(node.span());
        }
        if let N::JsxOpeningElement { name, .. } = node.kind() {
            let name_span = tree.node(name).expect("parser-owned name").span();
            let mut root = name;
            if let N::TypeScript {
                kind: TypeScriptKind::Instantiation,
                children,
            } = tree.node(root).unwrap().kind()
                && let Some(first) = tree.children(children).and_then(|nodes| nodes.first())
            {
                root = *first;
            }
            while let N::JsxMemberExpression { object, .. } = tree.node(root).unwrap().kind() {
                root = object;
            }
            renders.push(RenderSite {
                span: node.span(),
                owner,
                spelling: tree.source().slice(name_span).unwrap().to_owned(),
                reference: bindings.reference(root).cloned(),
            });
        }
        pending.extend(
            tree.child_nodes(id)
                .expect("parser-owned children")
                .map(|child| {
                    // Computed method keys execute outside the method body.
                    let child_owner = if matches!(node.kind(), N::MethodDefinition { key, computed: true, .. } if key == child) {
                        inherited_owner
                    } else { owner };
                    (child, child_owner, Some(id))
                }),
        );
    }
    environment.sort_by_key(|usage| usage.span.start());
    renders.sort_by_key(|site| site.span.start());
    styled_jsx.sort_by_key(|span| span.start());
    search_params.sort_by_key(|span| span.start());
    ModuleFacts {
        path: path.clone(),
        bindings: bindings.clone(),
        spans: tree.nodes().iter().map(|node| node.span()).collect(),
        renders,
        environment,
        styled_jsx,
        search_params,
    }
}

/// The spelling of a plain JSX identifier; namespaced and member names have none.
fn jsx_name<'a>(tree: &'a SyntaxTree<'_>, name: NodeId) -> Option<&'a str> {
    let node = tree.node(name)?;
    matches!(node.kind(), N::JsxIdentifier)
        .then(|| tree.source().slice(node.span()).ok())
        .flatten()
}

/// A literal object-pattern key: an identifier name or a string literal.
fn property_key(tree: &SyntaxTree<'_>, key: NodeId) -> Option<String> {
    tree.identifier_value(key)
        .or_else(|| tree.string_literal_value(key))
}

fn member_name(tree: &SyntaxTree<'_>, property: NodeId, computed: bool) -> Option<String> {
    if computed {
        tree.string_literal_value(property)
    } else {
        tree.identifier_value(property)
    }
}

/// A source-spanned runtime edge of a reached module. External, built-in,
/// unresolved, declaration, stylesheet, JSON, and asset targets all remain
/// explicit in `edge.resolution`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DependencyEdge<'a> {
    /// The importing module.
    pub importer: &'a SourcePath,
    /// The original graph edge; no type-only edge is included.
    pub edge: &'a Edge,
}

/// The runtime closure rooted at a route file and its ordered convention
/// chain. This is dependency evidence, not a readiness or execution claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteDependencies<'a> {
    /// Reached local files, sorted and deduplicated, including non-code roots
    /// and local stylesheet, JSON, and asset targets.
    pub files: Vec<&'a SourcePath>,
    /// Runtime edges, in importer path and source order. Duplicate imports
    /// remain distinct evidence.
    pub edges: Vec<DependencyEdge<'a>>,
    /// Reached graph modules without a strict parse, in path order. Their
    /// absent edges must never be treated as an empty dependency set.
    pub unparsed: Vec<&'a SourcePath>,
}

impl Project {
    /// Selected package; `None` denotes the run root.
    pub fn package(&self) -> Option<&SourcePath> {
        self.package.as_ref()
    }

    /// Every discovery entry below the package, including ignored and
    /// excluded entries. Unrepresentable names remain discovery evidence
    /// in the snapshot inventory, because they have no canonical path.
    pub fn files(&self) -> &[ProjectFile] {
        &self.files
    }

    /// Framework-specific facts with the original route and context issues.
    pub fn nextjs(&self) -> &AppAnalysis {
        &self.app
    }

    /// Strict per-module bindings and JSX render evidence, in path order.
    /// Files without a strict parse have no facts entry; inspect graph state.
    pub fn semantics(&self) -> &[ModuleFacts] {
        &self.semantics
    }

    /// JSX render sites resolved through bindings, imports, re-exports, and
    /// recognized wrappers to definitions, packages, or explicit reasons,
    /// with the rendered defaults of App Router conventions (`D-096`).
    pub fn components(&self) -> &ComponentGraph {
        &self.components
    }

    /// Where each module is evaluated, from render contexts and browser-only
    /// imports, with its environment-variable reads, including aliases and
    /// destructuring, and its Node.js and browser API uses (`D-096`).
    /// `ModuleFacts::environment` keeps the direct reads alone.
    pub fn environment(&self) -> &EnvironmentAnalysis {
        &self.environment
    }

    /// The evaluated configuration with its loading evidence and unknown
    /// parts (`D-097`). Unknown values are never defaults.
    pub fn configuration(&self) -> &ConfigurationFacts {
        &self.configuration
    }

    /// Convention exports and direct-import uses of every strictly parsed
    /// module, in path order (`D-097`).
    pub fn framework(&self) -> &[FrameworkModuleFacts] {
        &self.framework
    }

    /// The shared module graph, without a copy per route.
    pub fn modules(&self) -> &ModuleGraph {
        &self.app.graph
    }

    /// Computes the runtime closure for one route index from `nextjs().routes`.
    /// The route file and every enclosing convention file are roots. Explicit
    /// type-only edges are excluded; declaration targets are retained as
    /// evidence but never traversed as runtime implementations. Traversal is
    /// iterative and cycle-safe. Each root, first-visited path, and examined
    /// edge consumes one unit of a fresh `modules.max-work` query budget.
    /// Reaching the limit returns no partial closure. Framework runtime
    /// dispatch and component rendering are not inferred from imports.
    pub fn route_dependencies(&self, index: usize) -> Result<RouteDependencies<'_>, ProjectError> {
        let route = self
            .app
            .routes
            .routes
            .get(index)
            .ok_or(ProjectError::InvalidRoute)?;
        let mut work = self.max_dependency_work;
        let mut charge = || {
            work = work.checked_sub(1).ok_or(ProjectError::WorkLimit)?;
            Ok(())
        };
        let mut pending = Vec::new();
        for path in std::iter::once(&route.file).chain(route.chain.iter().map(|file| &file.path)) {
            charge()?;
            pending.push(path);
        }
        let mut files = BTreeSet::new();
        let mut visited = BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !files.insert(path) {
                continue;
            }
            charge()?;
            let Some(node) = self.app.graph.node(path) else {
                continue;
            };
            visited.insert(&node.path);
            if node.kind == FileKind::Declaration {
                continue;
            }
            for edge in &node.edges {
                charge()?;
                if !edge.type_only
                    && let Resolution::Local { path, .. } = &edge.resolution
                {
                    pending.push(path);
                }
            }
        }
        let mut edges = Vec::new();
        let mut unparsed = Vec::new();
        for path in visited {
            let node = self.app.graph.node(path).expect("graph-owned path");
            if node.state != ModuleState::Parsed {
                unparsed.push(path);
            }
            if node.kind == FileKind::Declaration {
                continue;
            }
            edges.extend(
                node.edges
                    .iter()
                    .filter(|edge| !edge.type_only)
                    .map(|edge| DependencyEdge {
                        importer: &node.path,
                        edge,
                    }),
            );
        }
        Ok(RouteDependencies {
            files: files.into_iter().collect(),
            edges,
            unparsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_work_boundary_is_inclusive() {
        use frameshift_core::{FindingCollector, FindingLimits, PathLimits};
        let root = std::env::temp_dir().join("frameshift-project-query-budget");
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("app/page.tsx"), "export default () => null;").unwrap();
        let limits = Limits::default();
        let snapshot = frameshift_input::snapshot(
            &root,
            limits.snapshot(),
            &mut FindingCollector::new(FindingLimits::default()),
        )
        .unwrap();
        let mut model = build(
            &snapshot,
            None,
            &SourcePath::new("app", PathLimits::default()).unwrap(),
            &["tsx"],
            None,
            &limits,
        )
        .unwrap();
        // One root and its first visit, with no dependency edge.
        for work in 0..2 {
            model.max_dependency_work = work;
            assert_eq!(model.route_dependencies(0), Err(ProjectError::WorkLimit));
        }
        model.max_dependency_work = 2;
        assert_eq!(model.route_dependencies(0).unwrap().files.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
