//! Framework-independent text and canonical path primitives for Frameshift.
//!
//! These types operate on immutable text and path values in memory. They perform no filesystem,
//! process, environment, or network access.

mod path;
mod text;

pub use path::{OutputPath, PathError, PathLimits, SourcePath};
pub use text::{Position, SourceText, Span, TextError, TextLimits};
