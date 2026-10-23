//! Snapshot-only module resolution and source-spanned dependency graphs.
//!
//! The explicit profile follows TypeScript 6.0.3 bundler source lookup for
//! local files, package `imports`, self-references through `exports`, and
//! local directory entry points. Installed package contents, custom bundler
//! configuration, and binding analysis are outside this contract. Resolution never establishes that an
//! import can execute, that a dependency is declared, or that migration is safe.

mod config;
mod graph;
mod package;
mod resolve;

use std::fmt;

pub use config::{ConfigIssue, ConfigProblem, ResolutionConfig};
pub use graph::{
    Edge, EdgeKind, GraphLimits, ModuleGraph, ModuleNode, ModuleState, build_module_graph,
    collect_dependencies,
};
pub use package::PackageManifest;
pub use resolve::{FileKind, Resolution, ResolutionMode, Resolver, UnresolvedReason};

/// Explicit limits shared by configuration loading and graph construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleLimits {
    /// Maximum admitted paths in the resolver. Defaults to 100,000.
    pub max_files: usize,
    /// Maximum graph edges, including unresolved references. Defaults to 1,000,000.
    pub max_edges: usize,
    /// Maximum active configuration inheritance depth. Defaults to 32.
    pub max_config_depth: usize,
    /// Maximum charged source bytes, path probes, and matching bytes. Defaults to 2^28.
    pub max_work: u64,
}

impl Default for ModuleLimits {
    fn default() -> Self {
        Self {
            max_files: 100_000,
            max_edges: 1_000_000,
            max_config_depth: 32,
            max_work: 1 << 28,
        }
    }
}

/// A stopped operation, without source-derived text or partial graph results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleError {
    /// The admitted-path limit was reached.
    FileLimit,
    /// The edge limit was reached.
    EdgeLimit,
    /// The deterministic work limit was reached.
    WorkLimit,
    /// The requested source or package directory is not admitted.
    InvalidRoot,
    /// The inventory is incomplete, so lookup precedence cannot be established.
    IncompleteInventory,
    /// Storage could not be reserved.
    AllocationFailed,
    /// The snapshot changed or otherwise became untrustworthy.
    Snapshot(frameshift_input::SnapshotError),
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FileLimit => "module file limit reached",
            Self::EdgeLimit => "module edge limit reached",
            Self::WorkLimit => "module work limit reached",
            Self::InvalidRoot => "module source or package root is not admitted",
            Self::IncompleteInventory => "module resolution requires a complete inventory",
            Self::AllocationFailed => "module allocation failed",
            Self::Snapshot(_) => "module snapshot failed",
        })
    }
}

impl std::error::Error for ModuleError {}

struct Work {
    remaining: u64,
}

impl Work {
    fn charge(&mut self, count: usize) -> Result<(), ModuleError> {
        self.remaining = self
            .remaining
            .checked_sub(count as u64)
            .ok_or(ModuleError::WorkLimit)?;
        Ok(())
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn below(path: &str, root: &str) -> bool {
    root.is_empty()
        || path
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('/'))
}

// Module references permit dot segments, unlike canonical stored paths. Only
// this boundary resolves them; it never interprets native filesystem syntax.
fn join(base: &str, reference: &str) -> Option<String> {
    if reference.starts_with('/')
        || reference.contains(['\\', '\0', ':'])
        || reference.split('/').any(|part| part.is_empty())
    {
        return None;
    }
    let mut parts: Vec<_> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in reference.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
