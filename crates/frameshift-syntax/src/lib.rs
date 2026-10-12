//! Framework-independent syntax primitives for the E1 parser experiment.
//! These scanners do not validate a file or authorize transformations.

mod ast;
mod identifier;
mod lexical;
mod numeric;
mod parser;
mod punctuator;
mod regexp;
mod regexp_pattern;
mod regexp_property;
mod template;
mod unicode;
mod value;
#[rustfmt::skip]
mod unicode_tables;

pub use ast::{
    AssignmentOperator, BinaryOperator, ChildList, NodeId, SyntaxNode, SyntaxNodeKind, SyntaxTree,
    TemplatePart, TemplatePartList, UnaryOperator, UpdateOperator,
};
pub use identifier::{
    IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass, scan_js_identifier,
};
pub use lexical::{
    Comment, CommentKind, LexicalError, LexicalErrorKind, LexicalLimits, Trivia, scan_js_string,
    scan_js_trivia,
};
pub use numeric::{NumericBase, NumericKind, NumericLiteral, scan_js_number};
pub use parser::{ParseError, ParseErrorKind, ParseLimits, parse_js_expression};
pub use punctuator::{
    GreaterThanMode, JsLexicalGoal, Punctuator, PunctuatorContext, PunctuatorToken,
    scan_js_punctuator,
};
pub use regexp::{
    RegExpFlag, RegExpFlags, RegExpLiteral, RegExpToken, scan_js_regexp, validate_js_regexp,
};
pub use regexp_pattern::RegExpPatternError;
pub use template::{
    TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken, scan_js_template,
};
pub use unicode::{is_js_identifier_part, is_js_identifier_start};
pub use unicode_tables::UNICODE_VERSION;
