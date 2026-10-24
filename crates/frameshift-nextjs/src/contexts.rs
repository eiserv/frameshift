//! Render contexts: which modules run in the React Server Components graph,
//! which in the client bundle, propagated over the runtime edges of the
//! module graph from the App Router's convention files.
//!
//! Next.js starts every segment convention file in the server context. A
//! module whose prologue holds `"use client"` and that is reached from the
//! server context is a client boundary: it and everything it imports run in
//! the client context. A `"use server"` module exposes server actions; it
//! runs on the server from either context. Type-only edges are erased and
//! do not propagate.

use std::collections::BTreeSet;

use frameshift_core::{SourcePath, Span};
use frameshift_modules::{
    Directive, FileKind, ModuleGraph, ModuleState, Resolution, UnresolvedReason,
};

use crate::routes::{AppRoutes, Convention};

/// The contexts a module runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleContext {
    /// The module.
    pub path: SourcePath,
    /// Whether it runs in the server components graph.
    pub server: bool,
    /// Whether it runs in the client bundle.
    pub client: bool,
    /// Whether its prologue holds `"use client"`.
    pub use_client: bool,
    /// Whether its prologue holds `"use server"`.
    pub use_server: bool,
}

/// A server-to-client crossing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boundary {
    /// The importing server module, absent for a convention file that is
    /// itself a client module.
    pub importer: Option<SourcePath>,
    /// The import specifier span in the importer.
    pub span: Option<Span>,
    /// The client module.
    pub client: SourcePath,
}

/// Evidence that limits or contradicts the propagation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextIssue {
    /// A reached module has no strict parse, so its imports are unknown.
    Unparsed {
        /// The module.
        module: SourcePath,
    },
    /// A runtime reference of a reached module has no established target.
    Unresolved {
        /// The importing module.
        module: SourcePath,
        /// The specifier span.
        span: Span,
        /// Why it is unresolved.
        reason: UnresolvedReason,
    },
    /// A `"use client"` or `"use server"` spelling with an escape, which
    /// Next.js does not treat as the directive.
    EscapedDirective {
        /// The module.
        module: SourcePath,
        /// The literal span.
        span: Span,
    },
    /// A client module imports `server-only`; Next.js fails the build.
    ServerOnlyInClient {
        /// The module.
        module: SourcePath,
        /// The specifier span.
        span: Span,
    },
    /// A server module imports `client-only`; Next.js fails the build.
    ClientOnlyInServer {
        /// The module.
        module: SourcePath,
        /// The specifier span.
        span: Span,
    },
    /// A function body holds an inline `"use server"` action.
    InlineServerAction {
        /// The module.
        module: SourcePath,
        /// The directive span.
        span: Span,
    },
}

/// The render contexts of every module reached from the routes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderContexts {
    /// Reached modules, in path order.
    pub modules: Vec<ModuleContext>,
    /// Server-to-client crossings, in path order of the client module.
    pub boundaries: Vec<Boundary>,
    /// External packages imported from the client context, by module.
    pub client_packages: Vec<(SourcePath, String)>,
    /// Evidence, in module path order.
    pub issues: Vec<ContextIssue>,
}

impl RenderContexts {
    /// The contexts of one module, if the routes reach it.
    pub fn module(&self, path: &SourcePath) -> Option<&ModuleContext> {
        self.modules
            .binary_search_by(|m| m.path.cmp(path))
            .ok()
            .map(|i| &self.modules[i])
    }
}

pub(crate) fn has(directives: &[Directive], value: &str) -> bool {
    directives
        .iter()
        .any(|d| !d.escaped && d.value.as_deref() == Some(value))
}

/// Propagates render contexts from the convention files of `routes`, and
/// from package-level middleware, proxy, and instrumentation files, over
/// `graph`. Modules outside the graph, such as stylesheets and assets, have
/// no context of their own.
pub fn render_contexts(graph: &ModuleGraph, routes: &AppRoutes) -> RenderContexts {
    let nodes = graph.nodes();
    let index = |path: &SourcePath| nodes.binary_search_by(|n| n.path.cmp(path)).ok();
    let mut server = vec![false; nodes.len()];
    let mut client = vec![false; nodes.len()];
    let mut boundaries = Vec::new();
    let mut pending: Vec<(usize, bool)> = Vec::new();
    for file in &routes.files {
        if matches!(file.convention, Convention::Metadata(_))
            && !matches!(
                FileKind::of(&file.path),
                FileKind::Module | FileKind::Declaration
            )
        {
            continue;
        }
        let Some(i) = index(&file.path) else { continue };
        if has(&nodes[i].directives, "use client") {
            boundaries.push(Boundary {
                importer: None,
                span: None,
                client: file.path.clone(),
            });
            pending.push((i, true));
        } else {
            pending.push((i, false));
        }
    }
    let mut issues = Vec::new();
    let mut client_packages = BTreeSet::new();
    while let Some((i, in_client)) = pending.pop() {
        let state = if in_client { &mut client } else { &mut server };
        if state[i] {
            continue;
        }
        state[i] = true;
        let node = &nodes[i];
        if !matches!(node.state, ModuleState::Parsed) {
            issues.push(ContextIssue::Unparsed {
                module: node.path.clone(),
            });
            continue;
        }
        for edge in &node.edges {
            if edge.type_only {
                continue;
            }
            match &edge.resolution {
                Resolution::Local { path, .. } => {
                    let Some(target) = index(path) else { continue };
                    let directives = &nodes[target].directives;
                    let to_client = if has(directives, "use server") {
                        false
                    } else if !in_client && has(directives, "use client") {
                        boundaries.push(Boundary {
                            importer: Some(node.path.clone()),
                            span: Some(edge.span),
                            client: path.clone(),
                        });
                        true
                    } else {
                        in_client
                    };
                    pending.push((target, to_client));
                }
                Resolution::External { package } => {
                    if in_client {
                        client_packages.insert((node.path.clone(), package.clone()));
                        if package == "server-only" {
                            issues.push(ContextIssue::ServerOnlyInClient {
                                module: node.path.clone(),
                                span: edge.span,
                            });
                        }
                    } else if package == "client-only" {
                        issues.push(ContextIssue::ClientOnlyInServer {
                            module: node.path.clone(),
                            span: edge.span,
                        });
                    }
                }
                Resolution::NodeBuiltin => {}
                Resolution::Unresolved(reason) => {
                    issues.push(ContextIssue::Unresolved {
                        module: node.path.clone(),
                        span: edge.span,
                        reason: *reason,
                    });
                }
            }
        }
    }
    let mut modules = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if !server[i] && !client[i] {
            continue;
        }
        for directive in node.directives.iter().chain(&node.function_directives) {
            let value = directive.value.as_deref();
            if directive.escaped && matches!(value, Some("use client" | "use server")) {
                issues.push(ContextIssue::EscapedDirective {
                    module: node.path.clone(),
                    span: directive.span,
                });
            }
        }
        for directive in &node.function_directives {
            if !directive.escaped && directive.value.as_deref() == Some("use server") {
                issues.push(ContextIssue::InlineServerAction {
                    module: node.path.clone(),
                    span: directive.span,
                });
            }
        }
        modules.push(ModuleContext {
            path: node.path.clone(),
            server: server[i],
            client: client[i],
            use_client: has(&node.directives, "use client"),
            use_server: has(&node.directives, "use server"),
        });
    }
    boundaries.sort_by(|a, b| {
        (&a.client, &a.importer, a.span.map(|s| s.start())).cmp(&(
            &b.client,
            &b.importer,
            b.span.map(|s| s.start()),
        ))
    });
    boundaries.dedup();
    issues.sort_by(|a, b| issue_key(a).cmp(&issue_key(b)));
    issues.dedup();
    RenderContexts {
        modules,
        boundaries,
        client_packages: client_packages.into_iter().collect(),
        issues,
    }
}

// Identical issues from a module reached in both contexts sort together.
fn issue_key(issue: &ContextIssue) -> (&SourcePath, usize, u8) {
    match issue {
        ContextIssue::Unparsed { module } => (module, 0, 0),
        ContextIssue::Unresolved { module, span, .. } => (module, span.start() + 1, 1),
        ContextIssue::EscapedDirective { module, span } => (module, span.start() + 1, 2),
        ContextIssue::ServerOnlyInClient { module, span } => (module, span.start() + 1, 3),
        ContextIssue::ClientOnlyInServer { module, span } => (module, span.start() + 1, 4),
        ContextIssue::InlineServerAction { module, span } => (module, span.start() + 1, 5),
    }
}
