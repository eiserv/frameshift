//! Environment classification (`D-096`): where each module is evaluated,
//! and which environment variables, Node.js APIs, and browser globals it uses.
//!
//! Next.js evaluates server-context modules on the server, which for static
//! output means during the build, and prerenders client components there
//! too. Client-context modules also run in the browser. A `next/dynamic`
//! loader with a literal `ssr: false` is the one import that only the
//! browser performs, so prerendering does not follow it. Uses are facts
//! from binding analysis and the module graph: a name counts only when no
//! binding of the module declares it, and a module only through the graph's
//! own resolution. Nothing is evaluated, and no environment file is read.

use std::collections::BTreeSet;

use frameshift_bindings::{BindingId, Bindings, DeclarationKind as K, Meaning, Target};
use frameshift_core::{SourcePath, Span};
use frameshift_modules::{EdgeKind, FileKind, ModuleGraph, ModuleState, Resolution};
use frameshift_syntax::{
    NodeId, SyntaxNodeKind as N, SyntaxTree, TypeScriptKind as T, UnaryOperator,
};

use crate::components::ComponentGraph;
use crate::contexts::{RenderContexts, has};

/// Node.js built-in modules, as `module.builtinModules` lists them in the
/// pinned Node.js 22.22.3. A `node:` specifier is built-in by the resolver's
/// own classification; a bare specifier is built-in only when it equals one
/// of these names exactly, because Node.js prefers built-ins to packages.
pub const NODE_BUILTINS: &[&str] = &[
    "_http_agent",
    "_http_client",
    "_http_common",
    "_http_incoming",
    "_http_outgoing",
    "_http_server",
    "_stream_duplex",
    "_stream_passthrough",
    "_stream_readable",
    "_stream_transform",
    "_stream_wrap",
    "_stream_writable",
    "_tls_common",
    "_tls_wrap",
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// Node.js globals that browsers do not define: the CommonJS module scope
/// and Node.js-specific objects. `process.env` reads are variable uses.
pub const NODE_GLOBALS: &[&str] = &[
    "Buffer",
    "__dirname",
    "__filename",
    "clearImmediate",
    "exports",
    "global",
    "module",
    "process",
    "require",
    "setImmediate",
];

/// Browser globals of the `Window` that the pinned Node.js 22.22.3 does not
/// define by default. The list is deliberately not exhaustive: it names the
/// document, navigation, storage, layout, and observer APIs that server
/// evaluation commonly lacks. `navigator`, `fetch`, `URL`, `crypto`, and
/// other names Node.js also defines are not browser-only.
pub const BROWSER_GLOBALS: &[&str] = &[
    "Element",
    "HTMLElement",
    "IntersectionObserver",
    "MutationObserver",
    "Node",
    "ResizeObserver",
    "SVGElement",
    "XMLHttpRequest",
    "alert",
    "cancelAnimationFrame",
    "cancelIdleCallback",
    "confirm",
    "customElements",
    "devicePixelRatio",
    "document",
    "getComputedStyle",
    "getSelection",
    "history",
    "indexedDB",
    "innerHeight",
    "innerWidth",
    "localStorage",
    "location",
    "matchMedia",
    "outerHeight",
    "outerWidth",
    "pageXOffset",
    "pageYOffset",
    "prompt",
    "requestAnimationFrame",
    "requestIdleCallback",
    "screen",
    "scrollBy",
    "scrollTo",
    "scrollX",
    "scrollY",
    "self",
    "sessionStorage",
    "visualViewport",
    "window",
];

/// The Next.js prefix of environment variables that it inlines into
/// client bundles.
pub const PUBLIC_PREFIX: &str = "NEXT_PUBLIC_";

/// How a `process.env` read is spelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableAccess {
    /// A member of `process.env`, such as `process.env.NAME`.
    Direct,
    /// A member of a binding that holds `process.env`.
    Alias,
    /// A property of an object pattern that destructures `process.env` or
    /// an alias of it.
    Destructured,
    /// The object escapes as a whole: passed, spread, stored, or bound by a
    /// rest pattern or a written binding. Every variable may be read.
    Whole,
}

/// One environment-variable read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableUse {
    /// The member access, pattern property, or escaping expression.
    pub span: Span,
    /// The decoded variable name; `None` for a computed name or a whole-object
    /// use, which may read any variable.
    pub name: Option<String>,
    /// How it is read.
    pub access: VariableAccess,
    /// Whether it is inside a function, method, or instance field, so that
    /// it runs when called rather than when the module is evaluated.
    pub deferred: bool,
}

impl VariableUse {
    /// Whether the name carries the public prefix, absent for unknown names.
    pub fn is_public(&self) -> Option<bool> {
        self.name
            .as_ref()
            .map(|name| name.starts_with(PUBLIC_PREFIX))
    }
}

/// The kind of a runtime API use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApiKind {
    /// A Node.js built-in module, imported or required.
    NodeModule,
    /// A Node.js global, see [`NODE_GLOBALS`].
    NodeGlobal,
    /// A browser global, see [`BROWSER_GLOBALS`].
    BrowserGlobal,
}

/// One use of a runtime-specific API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiUse {
    /// The identifier reference or module specifier.
    pub span: Span,
    /// What is used.
    pub kind: ApiKind,
    /// The global name, or the decoded module specifier.
    pub name: String,
    /// Whether it is inside a function, method, or instance field.
    pub deferred: bool,
    /// Whether the reference is the direct operand of `typeof`, which never
    /// throws for an undeclared name.
    pub typeof_guard: bool,
}

/// Where a reached module is evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Runtime {
    /// Only on the server, which for static output is the build.
    Build,
    /// Only in the browser.
    Browser,
    /// On both sides.
    Shared,
}

/// The environment facts of one module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleEnvironment {
    /// The module.
    pub path: SourcePath,
    /// Whether it has a strict parse. Without one its uses are unknown, not
    /// absent.
    pub parsed: bool,
    /// Whether the server evaluates it: in the server context, or in the
    /// client context through an import that prerendering follows.
    pub build: bool,
    /// Whether the browser evaluates it: in the client context.
    pub browser: bool,
    /// Environment-variable reads, in source order.
    pub variables: Vec<VariableUse>,
    /// Node.js and browser API uses, in source order.
    pub apis: Vec<ApiUse>,
}

impl ModuleEnvironment {
    /// Where the module is evaluated, absent when no route reaches it.
    pub fn runtime(&self) -> Option<Runtime> {
        match (self.build, self.browser) {
            (true, true) => Some(Runtime::Shared),
            (true, false) => Some(Runtime::Build),
            (false, true) => Some(Runtime::Browser),
            (false, false) => None,
        }
    }
}

/// The environment facts of every implementation module of the package.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentAnalysis {
    /// Modules of the graph, without declaration files, in path order.
    pub modules: Vec<ModuleEnvironment>,
}

impl EnvironmentAnalysis {
    /// The facts of one module.
    pub fn module(&self, path: &SourcePath) -> Option<&ModuleEnvironment> {
        self.modules
            .binary_search_by(|m| m.path.cmp(path))
            .ok()
            .map(|i| &self.modules[i])
    }
}

/// The environment facts of one strict module, collected while its tree is
/// alive and consumed by [`classify_environment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentSummary {
    path: SourcePath,
    variables: Vec<VariableUse>,
    globals: Vec<ApiUse>,
    // `require` calls of the global name with a literal specifier.
    requires: Vec<(Span, String, bool)>,
    // Specifier spans of `import()` calls inside functions.
    deferred_imports: BTreeSet<Span>,
}

impl EnvironmentSummary {
    /// The summarized module.
    pub fn path(&self) -> &SourcePath {
        &self.path
    }
}

/// Summarizes one strict module. Work is linear in its nodes and references.
pub fn summarize_environment(
    path: &SourcePath,
    tree: &SyntaxTree<'_>,
    bindings: &Bindings,
) -> EnvironmentSummary {
    let count = tree.nodes().len();
    let mut parents: Vec<Option<NodeId>> = vec![None; count];
    let mut deferred = vec![false; count];
    let mut requires = Vec::new();
    let mut deferred_imports = BTreeSet::new();
    let mut pending = vec![(tree.root(), false)];
    let kind = |id: NodeId| tree.node(id).expect("parser-owned node").kind();
    let span = |id: NodeId| tree.node(id).expect("parser-owned node").span();
    while let Some((id, inside)) = pending.pop() {
        deferred[id.index()] = inside;
        let node = kind(id);
        if let N::ImportCall { source, .. } = node
            && inside
        {
            deferred_imports.insert(span(source));
        }
        if let N::Call {
            callee, arguments, ..
        } = node
            && let N::Identifier { .. } = kind(callee)
            && bindings
                .reference(callee)
                .is_some_and(|r| r.target == Target::Global && r.name == "require")
            && let Some(&first) = tree.children(arguments).unwrap_or_default().first()
            && let Some(specifier) = tree.string_literal_value(first)
        {
            requires.push((span(first), specifier, inside));
        }
        let function = matches!(
            node,
            N::FunctionDeclaration { .. } | N::FunctionExpression { .. } | N::ArrowFunction { .. }
        );
        for child in tree.child_nodes(id).expect("parser-owned node") {
            parents[child.index()] = Some(id);
            // Computed keys and static field values run with the class;
            // method bodies and instance fields run later.
            let child_inside = if function {
                true
            } else if let N::MethodDefinition { key, computed, .. } = node {
                if computed && key == child {
                    inside
                } else {
                    true
                }
            } else if let N::FieldDefinition {
                value: Some(value),
                is_static: false,
                ..
            } = node
            {
                inside || value == child
            } else {
                inside
            };
            pending.push((child, child_inside));
        }
    }
    let mut references: Vec<Vec<NodeId>> = vec![Vec::new(); bindings.bindings().len()];
    let mut written = vec![false; bindings.bindings().len()];
    for reference in bindings.references() {
        if let Target::Binding(b) = reference.target {
            references[b.index()].push(reference.node);
            written[b.index()] |= reference.write;
        }
    }
    let mut summary = Summary {
        tree,
        bindings,
        parents,
        deferred,
        references,
        written,
        variables: Vec::new(),
        aliases: BTreeSet::new(),
    };
    let mut globals = Vec::new();
    for reference in bindings.references() {
        if reference.target != Target::Global
            || !reference.meaning.overlaps(Meaning::VALUE)
            || summary.type_query(reference.node)
        {
            continue;
        }
        let name = reference.name.as_str();
        if name == "process"
            && let Some(env) = summary.env_member(reference.node)
        {
            summary.env(env, VariableAccess::Direct);
            continue;
        }
        let kind = if NODE_GLOBALS.contains(&name) {
            ApiKind::NodeGlobal
        } else if BROWSER_GLOBALS.contains(&name) {
            ApiKind::BrowserGlobal
        } else {
            continue;
        };
        globals.push(ApiUse {
            span: span(reference.node),
            kind,
            name: name.to_owned(),
            deferred: summary.deferred[reference.node.index()],
            typeof_guard: summary.typeof_operand(reference.node),
        });
    }
    let mut variables = summary.variables;
    variables.sort_by_key(|v| (v.span, v.access));
    globals.sort_by_key(|g| g.span);
    requires.sort();
    EnvironmentSummary {
        path: path.clone(),
        variables,
        globals,
        requires,
        deferred_imports,
    }
}

struct Summary<'a, 't> {
    tree: &'a SyntaxTree<'t>,
    bindings: &'a Bindings,
    parents: Vec<Option<NodeId>>,
    deferred: Vec<bool>,
    references: Vec<Vec<NodeId>>,
    written: Vec<bool>,
    variables: Vec<VariableUse>,
    aliases: BTreeSet<BindingId>,
}

impl Summary<'_, '_> {
    fn kind(&self, id: NodeId) -> N {
        self.tree.node(id).expect("parser-owned node").kind()
    }

    fn span(&self, id: NodeId) -> Span {
        self.tree.node(id).expect("parser-owned node").span()
    }

    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents[id.index()]
    }

    fn ts_children(&self, id: NodeId) -> &[NodeId] {
        if let N::TypeScript { children, .. } = self.kind(id) {
            self.tree.children(children).unwrap_or_default()
        } else {
            &[]
        }
    }

    /// Whether a reference names a value in a type query, which is erased.
    fn type_query(&self, mut node: NodeId) -> bool {
        while let Some(parent) = self.parent(node)
            && let N::TypeScript { kind, .. } = self.kind(parent)
        {
            if !matches!(kind, T::QualifiedName) {
                return matches!(kind, T::Query);
            }
            node = parent;
        }
        false
    }

    /// Whether a node is the direct operand of `typeof`, through parentheses.
    fn typeof_operand(&self, mut node: NodeId) -> bool {
        while let Some(parent) = self.parent(node) {
            if let N::Parenthesized { .. } = self.kind(parent) {
                node = parent;
                continue;
            }
            return matches!(
                self.kind(parent),
                N::Unary {
                    operator: UnaryOperator::Typeof,
                    ..
                }
            );
        }
        false
    }

    /// The `process.env` member whose object is this `process` reference.
    fn env_member(&self, process: NodeId) -> Option<NodeId> {
        let parent = self.parent(process)?;
        let N::Member {
            object,
            property,
            computed,
            ..
        } = self.kind(parent)
        else {
            return None;
        };
        (object == process && self.member_name(property, computed).as_deref() == Some("env"))
            .then_some(parent)
    }

    fn member_name(&self, property: NodeId, computed: bool) -> Option<String> {
        if computed {
            self.tree.string_literal_value(property)
        } else {
            self.tree.identifier_value(property)
        }
    }

    /// Climbs from an expression through parentheses and erased TypeScript
    /// syntax to the node that uses its value.
    fn used(&self, mut node: NodeId) -> (NodeId, Option<NodeId>) {
        loop {
            let Some(parent) = self.parent(node) else {
                return (node, None);
            };
            let kind = self.kind(parent);
            let transparent = if let N::Parenthesized { .. } = kind {
                true
            } else if let N::TypeScript { kind, .. } = kind {
                let children = self.ts_children(parent);
                if matches!(kind, T::NonNull | T::As | T::Satisfies) {
                    children.first() == Some(&node)
                } else {
                    matches!(kind, T::Assertion) && children.get(1) == Some(&node)
                }
            } else {
                false
            };
            if !transparent {
                return (node, Some(parent));
            }
            node = parent;
        }
    }

    fn push(&mut self, span: Span, name: Option<String>, access: VariableAccess, at: NodeId) {
        let deferred = self.deferred[at.index()];
        self.variables.push(VariableUse {
            span,
            name,
            access,
            deferred,
        });
    }

    /// Records the uses of an expression that evaluates to `process.env`,
    /// following aliases iteratively.
    fn env(&mut self, expression: NodeId, access: VariableAccess) {
        let mut pending = vec![(expression, access)];
        while let Some((expression, access)) = pending.pop() {
            let (node, parent) = self.used(expression);
            let Some(parent) = parent else {
                self.push(self.span(node), None, VariableAccess::Whole, node);
                continue;
            };
            let parent_kind = self.kind(parent);
            if let N::Member {
                object,
                property,
                computed,
                ..
            } = parent_kind
                && object == node
            {
                let name = self.member_name(property, computed);
                self.push(self.span(parent), name, access, parent);
                continue;
            }
            let N::VariableDeclarator {
                target,
                init: Some(init),
            } = parent_kind
            else {
                self.push(self.span(node), None, VariableAccess::Whole, node);
                continue;
            };
            if init != node {
                self.push(self.span(node), None, VariableAccess::Whole, node);
                continue;
            }
            let mut pattern = target;
            if let N::TypeScript {
                kind: T::TypedBinding { .. },
                ..
            } = self.kind(pattern)
                && let Some(&first) = self.ts_children(pattern).first()
            {
                pattern = first;
            }
            let pattern_kind = self.kind(pattern);
            if let N::BindingIdentifier { .. } = pattern_kind {
                match self.alias(pattern) {
                    Some(binding) => {
                        if self.aliases.insert(binding) {
                            for &reference in &self.references[binding.index()] {
                                pending.push((reference, VariableAccess::Alias));
                            }
                        }
                    }
                    None => self.push(self.span(parent), None, VariableAccess::Whole, parent),
                }
            } else if let N::ObjectPattern { properties, .. } = pattern_kind {
                let properties = self.tree.children(properties).unwrap_or_default().to_vec();
                for property in properties {
                    let kind = self.kind(property);
                    let name = if let N::ShorthandPatternProperty { target, .. } = kind {
                        Some(self.tree.identifier_value(target))
                    } else if let N::PatternProperty { key, computed, .. } = kind {
                        Some(if computed {
                            self.tree.string_literal_value(key)
                        } else {
                            self.tree
                                .identifier_value(key)
                                .or_else(|| self.tree.string_literal_value(key))
                        })
                    } else {
                        None
                    };
                    match name {
                        Some(name) => self.push(
                            self.span(property),
                            name,
                            VariableAccess::Destructured,
                            property,
                        ),
                        None => {
                            self.push(self.span(property), None, VariableAccess::Whole, property);
                        }
                    }
                }
            } else {
                self.push(self.span(parent), None, VariableAccess::Whole, parent);
            }
        }
    }

    /// The binding of a declarator name when it is an unwritten, single
    /// `const`, `let`, or `var` declaration that can hold the object.
    fn alias(&self, name: NodeId) -> Option<BindingId> {
        let binding = self.bindings.declared(name)?;
        let data = self.bindings.binding(binding);
        let [declaration] = data.declarations.as_slice() else {
            return None;
        };
        (matches!(declaration.kind, K::Const | K::Let | K::Var) && !self.written[binding.index()])
            .then_some(binding)
    }
}

/// Classifies every implementation module of the graph: `contexts` supply
/// the server and client contexts, and the component graph's browser-only
/// imports bound prerendering. `summaries` must be in path order; modules
/// without one have no strict parse.
pub fn classify_environment(
    graph: &ModuleGraph,
    contexts: &RenderContexts,
    components: &ComponentGraph,
    summaries: &[EnvironmentSummary],
) -> EnvironmentAnalysis {
    let nodes = graph.nodes();
    let index = |path: &SourcePath| nodes.binary_search_by(|n| n.path.cmp(path)).ok();
    let browser_only: BTreeSet<(&SourcePath, Span)> = components
        .browser_imports
        .iter()
        .map(|import| (&import.path, import.span))
        .collect();
    // Client roots are the boundaries; prerendering follows client edges
    // except browser-only imports, and leaves at `"use server"` modules,
    // which run in the server context.
    let mut prerendered = vec![false; nodes.len()];
    let mut pending: Vec<usize> = contexts
        .boundaries
        .iter()
        .filter(|b| match (&b.importer, b.span) {
            (Some(importer), Some(span)) => !browser_only.contains(&(importer, span)),
            (Some(_), None) | (None, Some(_)) | (None, None) => true,
        })
        .filter_map(|b| index(&b.client))
        .collect();
    while let Some(i) = pending.pop() {
        if prerendered[i] {
            continue;
        }
        prerendered[i] = true;
        let node = &nodes[i];
        if node.state != ModuleState::Parsed {
            continue;
        }
        for edge in &node.edges {
            if edge.type_only || browser_only.contains(&(&node.path, edge.span)) {
                continue;
            }
            if let Resolution::Local { path, .. } = &edge.resolution
                && let Some(target) = index(path)
                && !has(&nodes[target].directives, "use server")
            {
                pending.push(target);
            }
        }
    }
    let summary = |path: &SourcePath| {
        summaries
            .binary_search_by(|s| s.path.cmp(path))
            .ok()
            .map(|i| &summaries[i])
    };
    let mut modules = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if node.kind == FileKind::Declaration {
            continue;
        }
        let context = contexts.module(&node.path);
        let server = context.is_some_and(|c| c.server);
        let client = context.is_some_and(|c| c.client);
        let summary = summary(&node.path);
        let mut apis = Vec::new();
        let mut variables = Vec::new();
        if let Some(summary) = summary {
            variables.clone_from(&summary.variables);
            apis.extend(summary.globals.iter().cloned());
            for edge in &node.edges {
                if edge.type_only {
                    continue;
                }
                let builtin = match &edge.resolution {
                    Resolution::NodeBuiltin => true,
                    Resolution::External { .. } => edge
                        .specifier
                        .as_deref()
                        .is_some_and(|s| NODE_BUILTINS.contains(&s)),
                    Resolution::Local { .. } | Resolution::Unresolved(_) => false,
                };
                if !builtin || edge.kind == EdgeKind::RequireCandidate {
                    continue;
                }
                apis.push(ApiUse {
                    span: edge.span,
                    kind: ApiKind::NodeModule,
                    name: edge.specifier.clone().unwrap_or_default(),
                    deferred: edge.kind == EdgeKind::DynamicImport
                        && summary.deferred_imports.contains(&edge.span),
                    typeof_guard: false,
                });
            }
            for (span, specifier, deferred) in &summary.requires {
                if specifier.starts_with("node:") || NODE_BUILTINS.contains(&specifier.as_str()) {
                    apis.push(ApiUse {
                        span: *span,
                        kind: ApiKind::NodeModule,
                        name: specifier.clone(),
                        deferred: *deferred,
                        typeof_guard: false,
                    });
                }
            }
            apis.sort_by_key(|a| (a.span, a.kind));
            apis.dedup();
        }
        modules.push(ModuleEnvironment {
            path: node.path.clone(),
            parsed: summary.is_some(),
            build: server || client && prerendered[i],
            browser: client,
            variables,
            apis,
        });
    }
    EnvironmentAnalysis { modules }
}
