//! Framework-independent text, paths, identifiers, findings, SHA-256, limits,
//! and deterministic collections for Frameshift.
//!
//! These types operate on bytes, text, and path values in memory. They perform no filesystem,
//! process, environment, or network access.

mod collection;
mod finding;
mod hash;
mod identifier;
mod limit;
mod path;
mod text;
mod unit;

pub use collection::{CollectionError, DenseId, DenseMap, DenseSlots, KeyTable};
pub use finding::{
    Finding, FindingCollector, FindingContext, FindingError, FindingId, FindingLimits, FindingList,
    FindingLocation, FindingText, Severity, TextLocation,
};
pub use hash::{HashError, HashLimits, Sha256Digest, Sha256Hasher};
pub use identifier::{FeatureId, FindingCode, IdentifierError, IdentifierLimits, RuleId};
pub use limit::{Budget, BudgetExceeded, RunLimits};
pub use path::{OutputPath, PathError, PathLimits, SourcePath};
pub use text::{Position, SourceText, Span, TextError, TextLimits};
pub use unit::{UnitId, UnitKind};
