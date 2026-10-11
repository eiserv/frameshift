//! Pure source rewriting for Frameshift's output layer.
//!
//! Edits reference immutable original text. This crate does not parse source,
//! select migration rules, write files, or execute code.

mod edits;

pub use edits::{
    AppliedEdit, Edit, EditLimits, EditOrder, Origin, ProvenanceRegion, RewriteError,
    RewrittenText, apply_edits,
};
