//! Framework-independent text primitives for Frameshift.
//!
//! These types operate on immutable text in memory. They perform no filesystem,
//! process, environment, or network access.

mod text;

pub use text::{Position, SourceText, Span, TextError, TextLimits};
