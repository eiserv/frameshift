//! One App Router analysis: the route tree, the module graph with one parse
//! per file, the exports of every convention file, and render contexts.

use std::collections::BTreeSet;

use frameshift_bindings::{BindingError, Bindings, ExportKind, ExportName, analyze};
use frameshift_core::SourcePath;
use frameshift_input::Snapshot;
use frameshift_modules::{GraphLimits, ModuleError, ModuleGraph, build_module_graph_with};
use frameshift_syntax::SyntaxTree;

use crate::contexts::{RenderContexts, render_contexts};
use crate::routes::{AppRoutes, RouteError, app_routes};

/// The exports of one convention file, which name its Next.js facets such
/// as `generateStaticParams`, `metadata`, segment configuration, or route
/// handler methods. Their values are not evaluated here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExports {
    /// The convention file.
    pub path: SourcePath,
    /// Exported names, sorted, without `export *` targets.
    pub names: Vec<ExportName>,
    /// Whether an `export *` adds names that only its source module knows.
    pub star: bool,
    /// Whether the default export is an expression rather than a named
    /// local declaration.
    pub default_expression: bool,
}

impl FileExports {
    /// Whether the file exports this name.
    pub fn exports(&self, name: &str) -> bool {
        self.names
            .iter()
            .any(|n| matches!(n, ExportName::Named(x) if x == name))
    }

    /// Whether the file has a default export.
    pub fn has_default(&self) -> bool {
        self.names.contains(&ExportName::Default)
    }
}

/// The App Router analysis of one package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppAnalysis {
    /// The route tree.
    pub routes: AppRoutes,
    /// The package's module graph.
    pub graph: ModuleGraph,
    /// Exports of every convention file that parsed, in path order.
    pub exports: Vec<FileExports>,
    /// Render contexts over the graph.
    pub contexts: RenderContexts,
}

/// Why an analysis stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppError {
    /// Route discovery failed.
    Routes(RouteError),
    /// Graph construction failed.
    Modules(ModuleError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Routes(e) => e.fmt(f),
            Self::Modules(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for AppError {}

/// Analyzes the App Router of the selected package: its routes below `app`
/// with the given page extensions, the module graph of `package`, the
/// exports of every convention file, and the render contexts. Every file is
/// read through the snapshot and parsed once.
pub fn analyze_app(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    limits: GraphLimits,
) -> Result<AppAnalysis, AppError> {
    analyze_app_with(
        snapshot,
        package,
        app,
        extensions,
        limits,
        &mut |_, _, _| Ok(()),
    )
}

/// As `analyze_app`, also exposing each strict tree and its binding analysis
/// while it is alive. The visitor runs once per successfully parsed module,
/// in path order, including non-convention files. No erroneous tree is exposed.
/// A visitor failure stops construction without a partial analysis.
pub fn analyze_app_with(
    snapshot: &Snapshot,
    package: Option<&SourcePath>,
    app: &SourcePath,
    extensions: &[&str],
    limits: GraphLimits,
    visit: &mut dyn FnMut(&SourcePath, &SyntaxTree<'_>, &Bindings) -> Result<(), ModuleError>,
) -> Result<AppAnalysis, AppError> {
    let routes =
        app_routes(snapshot.inventory(), package, app, extensions).map_err(AppError::Routes)?;
    let conventions: BTreeSet<&SourcePath> = routes.files.iter().map(|f| &f.path).collect();
    let mut exports = Vec::new();
    let graph = build_module_graph_with(snapshot, package, limits, &mut |path, tree| {
        let bindings = analyze(tree).map_err(|e| match e {
            BindingError::AllocationFailed | BindingError::TooLarge => {
                ModuleError::AllocationFailed
            }
        })?;
        visit(path, tree, &bindings)?;
        if !conventions.contains(path) {
            return Ok(());
        }
        let mut names = Vec::new();
        let mut star = false;
        let mut default_expression = false;
        for export in bindings.exports() {
            match &export.name {
                ExportName::Star => star = true,
                name @ (ExportName::Default | ExportName::Named(_) | ExportName::Assignment) => {
                    if *name == ExportName::Default
                        && matches!(export.kind, ExportKind::Expression(_))
                    {
                        default_expression = true;
                    }
                    names.push(name.clone());
                }
            }
        }
        names.sort();
        names.dedup();
        exports.push(FileExports {
            path: path.clone(),
            names,
            star,
            default_expression,
        });
        Ok(())
    })
    .map_err(AppError::Modules)?;
    let contexts = render_contexts(&graph, &routes);
    Ok(AppAnalysis {
        routes,
        graph,
        exports,
        contexts,
    })
}
