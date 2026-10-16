//! The input layer: `.gitignore` matching with Git's pattern semantics
//! (`D-065`).
//!
//! This crate will hold the only product code that reads the source
//! filesystem; matching itself performs no I/O.

mod gitignore;

pub use gitignore::{GitignoreFile, IgnoreDecision, IgnoreError, IgnoreLimits};
