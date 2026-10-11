//! Framework-independent syntax primitives for the E1 parser experiment.
//! These scanners do not validate a file or authorize transformations.

mod lexical;

pub use lexical::{
    Comment, CommentKind, LexicalError, LexicalErrorKind, LexicalLimits, Trivia, scan_js_string,
    scan_js_trivia,
};
