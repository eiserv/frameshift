//! Public facade for Frameshift's implemented, in-memory text operations.
//!
//! This M0 API exposes source positions, lexical primitives, and span rewriting.
//! It contains no project inspection, full parser, migration, or verification
//! pipeline. Future CLI code must depend on this facade rather than lower layers.

pub use frameshift_core::{Position, SourceText, Span, TextError, TextLimits};
pub use frameshift_syntax::{
    Comment, CommentKind, GreaterThanMode, JsLexicalGoal, LexicalError, LexicalErrorKind,
    LexicalLimits, NumericBase, NumericKind, NumericLiteral, Punctuator, PunctuatorContext,
    PunctuatorToken, TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken,
    Trivia, scan_js_number, scan_js_punctuator, scan_js_string, scan_js_template, scan_js_trivia,
};
pub use frameshift_transform::{
    AppliedEdit, Edit, EditLimits, EditOrder, Origin, ProvenanceRegion, RewriteError,
    RewrittenText, apply_edits,
};
