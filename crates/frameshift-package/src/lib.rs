//! npm ecosystem facts of a project: SemVer versions and npm ranges
//! (`D-070`).
//!
//! This crate interprets package metadata without framework knowledge. It
//! performs no I/O of its own.

mod manifest;
mod range;
mod spec;
mod version;

pub use manifest::{
    Bin, Browser, BrowserTarget, Browserslist, Dependency, DependencyKind, Element, Entry, Exports,
    Field, IssueKind, ManifestError, ManifestField, ManifestIssue, ModuleType, OtherMember,
    Override, PackageManifest,
};
pub use spec::DependencySpec;

pub use range::{Comparator, Operator, RangeError, VersionRange};
pub use version::{Identifier, MAX_VERSION_BYTES, MAX_VERSION_NUMBER, Version, VersionError};
