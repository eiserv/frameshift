//! npm ecosystem facts of a project: SemVer versions and npm ranges
//! (`D-070`).
//!
//! This crate interprets package metadata without framework knowledge:
//! manifests, package managers, lockfiles, and workspaces (`D-069` to
//! `D-073`). It performs no I/O of its own and reads files only through
//! the snapshot store.

mod load;
mod lockfile;
mod manager;
mod manifest;
mod range;
mod spec;
mod version;
mod workspace;

pub use load::{
    LoadError, LoadLimits, LockfileState, ManifestState, PackageDirectory, PackageLimits,
    load_package,
};
pub use lockfile::{
    Declared, LockedPackage, LockedWorkspace, Lockfile, LockfileError, LockfileErrorKind,
    Resolution,
};
pub use manager::{
    Detection, DetectionSource, IgnoredLockfile, IgnoredReason, LockfileKind, PackageManagerError,
    PackageManagerKind, PackageManagerSpec, detect,
};
pub use manifest::{
    Bin, Browser, BrowserTarget, Browserslist, Dependency, DependencyKind, Element, Entry, Exports,
    Field, IssueKind, ManifestError, ManifestField, ManifestIssue, ModuleType, OtherMember,
    Override, PackageManifest,
};
pub use spec::DependencySpec;
pub use workspace::{
    DeclarationFile, Member, PatternRejection, ReleaseMembers, SelectedPackage, SelectionError,
    Workspace, WorkspaceLimits, WorkspaceSemantics, load_workspace, release_members,
};

pub use range::{Comparator, Operator, RangeError, VersionRange};
pub use version::{Identifier, MAX_VERSION_BYTES, MAX_VERSION_NUMBER, Version, VersionError};
