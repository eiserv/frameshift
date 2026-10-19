//! Framework-independent JavaScript and TypeScript scanners and parsers for
//! the E1 parser experiment, and the JSON and JSONC parser for manifests,
//! lockfiles, and configuration (`D-068`). A parsed tree does not authorize
//! transformations.

mod ast;
mod identifier;
mod json;
mod jsx;
mod lexical;
mod lookup;
mod numeric;
mod parser;
mod punctuator;
mod reference;
mod regexp;
mod regexp_pattern;
mod regexp_property;
mod template;
mod typescript;
mod unicode;
mod value;
#[rustfmt::skip]
mod entity_tables;
#[rustfmt::skip]
mod unicode_tables;

pub use ast::{
    AssignmentOperator, BinaryOperator, ChildList, ChildNodes, FunctionKind, MetaPropertyKind,
    MethodKind, NodeId, SyntaxNode, SyntaxNodeKind, SyntaxTree, TemplatePart, TemplatePartList,
    UnaryOperator, UpdateOperator, VariableKind,
};
pub use identifier::{
    IdentifierKind, IdentifierToken, ReservedWord, ReservedWordClass, scan_js_identifier,
};
pub use json::{
    JsonArray, JsonDialect, JsonDocument, JsonError, JsonErrorKind, JsonExpected, JsonId,
    JsonLimits, JsonMember, JsonNumber, JsonObject, JsonValue, parse_json,
};
pub use jsx::{JsxStringToken, scan_jsx_identifier, scan_jsx_string, scan_jsx_text};
pub use lexical::{
    Comment, CommentKind, LegacyOctal, LexicalError, LexicalErrorKind, LexicalLimits, SourceGoal,
    StringToken, Trivia, scan_js_string, scan_js_trivia,
};
pub use numeric::{NumericBase, NumericKind, NumericLiteral, scan_js_number};
pub use parser::{
    JsDialect, ParseError, ParseErrorKind, ParseLimits, StrictModeError, parse_js_expression,
    parse_js_program,
};
pub use punctuator::{
    GreaterThanMode, JsLexicalGoal, Punctuator, PunctuatorContext, PunctuatorToken,
    scan_js_punctuator,
};
pub use reference::{
    HtmlCharacterReference, HtmlReferenceContext, HtmlReferenceValue, JsxCharacterReference,
    decode_html_character_reference, decode_jsx_character_reference,
};
pub use regexp::{
    RegExpFlag, RegExpFlags, RegExpLiteral, RegExpToken, scan_js_regexp, validate_js_regexp,
};
pub use regexp_pattern::RegExpPatternError;
pub use template::{
    TemplateContext, TemplateKind, TemplateMode, TemplatePosition, TemplateToken, scan_js_template,
};
pub use typescript::TypeScriptKind;
pub use unicode::{is_js_identifier_part, is_js_identifier_start};
pub use unicode_tables::UNICODE_VERSION;
