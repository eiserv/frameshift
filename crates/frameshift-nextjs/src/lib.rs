//! The Next.js source adapter (`nextjs`): whether the selected package is a
//! Next.js project, which Next.js version it uses, which configuration file
//! Next.js loads, and which routers it has (`D-074`).
//!
//! The App Router analysis (`D-092`) adds the route tree from the
//! inventory, the exports of every convention file, and render contexts
//! propagated over the module graph; it reads files only through the
//! snapshot and evaluates nothing. The component graph (`D-096`) resolves
//! JSX render sites through bindings and module edges to the definitions,
//! packages, or explicit reasons they render.
//!
//! Detection combines evidence, as section 7 of ARCHITECTURE.md requires:
//! a `next` dependency and at least one structural signal, a
//! configuration file or route files. It reads no file content; the
//! inventory, the manifest, and the root lockfile suffice. Framework
//! knowledge lives here, not in the package or input crates.

mod app;
mod components;
mod contexts;
mod detect;
mod routes;

#[cfg(test)]
mod tests;

pub use app::{AppAnalysis, AppError, FileExports, analyze_app, analyze_app_with};
pub use components::{
    BrowserImport, ComponentError, ComponentGraph, ComponentSummary, Definition, DefinitionKind,
    Entry, Rendered, Resolved, Site, SsrOption, UnknownReason, Wrapper, link_components,
    summarize_components,
};
pub use contexts::{Boundary, ContextIssue, ModuleContext, RenderContexts, render_contexts};
pub use detect::{
    ConfigFile, ConfigLoad, DetectError, Detection, NextDependency, Router, RouterDirectory,
    RouterState, VALIDATED_MAJORS, Verdict, VersionEvidence, detect,
};
pub use routes::{
    AppRoute, AppRoutes, Convention, ConventionFile, Interception, Metadata, RouteError,
    RouteIssue, RouteKind, Segment, SegmentKind, app_routes, convention, pattern, segment,
};

/// The source adapter identifier.
pub const ADAPTER_ID: &str = "nextjs";
