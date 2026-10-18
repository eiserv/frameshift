//! The input layer: read-only discovery of a project's files, `.gitignore`
//! matching with Git's pattern semantics, and the read-once snapshot store
//! (`D-064`, `D-065`, `D-067`).
//!
//! This is the only product crate that reads the source filesystem. It
//! lists directories, reads link targets, opens `.gitignore` files during
//! discovery, and opens admitted files only when the snapshot store reads
//! them; it never follows a link below the root, never writes, starts
//! processes, or accesses the network.

mod gitignore;
mod inventory;
mod native;
mod snapshot;
mod walk;

pub use gitignore::{GitignoreFile, IgnoreDecision, IgnoreError, IgnoreLimits};
pub use inventory::{
    Entry, EntryKind, IgnoreReason, Inventory, LinkReference, LinkTarget, NotTraversed, Stop,
    Unresolved,
};
pub use snapshot::{
    FileRead, Snapshot, SnapshotError, SnapshotFile, SnapshotId, SnapshotLimits, snapshot,
};
pub use walk::{DiscoveryError, DiscoveryLimits, IoFailure, discover};
