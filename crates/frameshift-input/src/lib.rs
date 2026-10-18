//! The input layer: read-only discovery of a project's files and
//! `.gitignore` matching with Git's pattern semantics (`D-064`, `D-065`).
//!
//! This is the only product crate that reads the source filesystem. It
//! lists directories, reads link targets, and opens `.gitignore` files; it
//! never follows a link below the root, never opens any other file, and
//! never writes, starts processes, or accesses the network.

mod gitignore;
mod inventory;
mod native;
mod walk;

pub use gitignore::{GitignoreFile, IgnoreDecision, IgnoreError, IgnoreLimits};
pub use inventory::{
    Entry, EntryKind, IgnoreReason, Inventory, LinkReference, LinkTarget, NotTraversed, Stop,
    Unresolved,
};
pub use walk::{DiscoveryError, DiscoveryLimits, IoFailure, discover};
