//! Framework-independent text, canonical paths, identifiers, and findings for Frameshift.
//!
//! These types operate on immutable text and path values in memory. They perform no filesystem,
//! process, environment, or network access.

mod finding;
mod identifier;
mod path;
mod text;
mod unit;

pub use finding::{
    Finding, FindingCollector, FindingContext, FindingError, FindingId, FindingLimits, FindingList,
    FindingLocation, FindingText, Severity, TextLocation,
};
pub use identifier::{FeatureId, FindingCode, IdentifierError, IdentifierLimits, RuleId};
pub use path::{OutputPath, PathError, PathLimits, SourcePath};
pub use text::{Position, SourceText, Span, TextError, TextLimits};
pub use unit::{UnitId, UnitKind};
