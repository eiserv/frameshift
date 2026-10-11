//! Public facade for Frameshift's implemented, in-memory text operations.
//!
//! This initial M0 API exposes only source positions and span rewriting. It
//! contains no project inspection, parsing, migration, execution, or verification
//! pipeline. Future CLI code must depend on this facade rather than lower layers.

pub use frameshift_core::{Position, SourceText, Span, TextError, TextLimits};
pub use frameshift_transform::{
    AppliedEdit, Edit, EditLimits, EditOrder, Origin, ProvenanceRegion, RewriteError,
    RewrittenText, apply_edits,
};
