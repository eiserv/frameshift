//! Framework-independent syntax primitives for the E1 parser experiment.
//! These scanners do not validate a file or authorize transformations.

mod identifier;
mod lexical;
mod numeric;
mod punctuator;
mod template;
mod unicode;
#[rustfmt::skip]
mod unicode_tables;

pub use identifier::{
    IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass, scan_js_identifier,
};
pub use lexical::{
    Comment, CommentKind, LexicalError, LexicalErrorKind, LexicalLimits, Trivia, scan_js_string,
    scan_js_trivia,
};
pub use numeric::{NumericBase, NumericKind, NumericLiteral, scan_js_number};
pub use punctuator::{
    GreaterThanMode, JsLexicalGoal, Punctuator, PunctuatorContext, PunctuatorToken,
    scan_js_punctuator,
};
pub use template::{
    TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken, scan_js_template,
};
pub use unicode::{is_js_identifier_part, is_js_identifier_start};
pub use unicode_tables::UNICODE_VERSION;
