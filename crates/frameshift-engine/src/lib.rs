//! Public facade for Frameshift's implemented, in-memory text operations.
//!
//! This M0 API exposes source positions, lexical primitives, a bounded
//! expression parser, regular-expression validation, pinned Unicode identifier
//! classification, and span rewriting. It contains no project inspection,
//! complete parser, migration, or verification pipeline. Future CLI code must
//! depend on this facade rather than lower layers.

pub use frameshift_core::{Position, SourceText, Span, TextError, TextLimits};
pub use frameshift_syntax::{
    AssignmentOperator, BinaryOperator, ChildList, Comment, CommentKind, GreaterThanMode,
    IdentifierKind, IdentifierToken, JsLexicalGoal, LexicalError, LexicalErrorKind, LexicalLimits,
    NodeId, NumericBase, NumericKind, NumericLiteral, ParseError, ParseErrorKind, ParseLimits,
    Punctuator, PunctuatorContext, PunctuatorToken, RegExpFlag, RegExpFlags, RegExpLiteral,
    RegExpPatternError, RegExpToken, ReservedWord, ReservedWordClass, SyntaxNode, SyntaxNodeKind,
    SyntaxTree, TemplateContext, TemplateKind, TemplateMode, TemplatePart, TemplatePartList,
    TemplatePosition, TemplateToken, Trivia, UNICODE_VERSION, UnaryOperator, UpdateOperator,
    is_js_identifier_part, is_js_identifier_start, parse_js_expression, scan_js_identifier,
    scan_js_number, scan_js_punctuator, scan_js_regexp, scan_js_string, scan_js_template,
    scan_js_trivia, validate_js_regexp,
};
pub use frameshift_transform::{
    AppliedEdit, Edit, EditLimits, EditOrder, Origin, ProvenanceRegion, RewriteError,
    RewrittenText, apply_edits,
};
