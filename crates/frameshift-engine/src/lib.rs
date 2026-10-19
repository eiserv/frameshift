//! Public facade for Frameshift's implemented, in-memory engine operations.
//!
//! The foundation API exposes text, paths, identifiers, findings, SHA-256,
//! run limits, and deterministic collections. The `limits` module bundles
//! every subsystem's resource limits with their relations and canonical list.
//! The text API exposes source positions, lexical primitives, a JavaScript
//! parser for Script and Module source text with explicit JSX, TypeScript,
//! and TSX dialects, regular-expression validation, pinned Unicode identifier
//! classification, character reference decoding with pinned tables, span
//! rewriting, and the Astro component printer with JSX conversion and
//! multi-source provenance of the E2 experiment, and the syntax API also
//! parses JSON and JSONC. The input API discovers a project's files
//! read-only, matches `.gitignore` patterns, and reads admitted files once
//! through the snapshot store; it is the only part that reads the source
//! filesystem. The facade contains no
//! project inspection, migration, or verification pipeline. The M1 help and
//! version CLI consumes this facade, including immutable build identity,
//! rather than lower layers.

pub mod limits;
pub mod version;

/// Framework-independent text, paths, identifiers, findings, SHA-256, limits,
/// and deterministic collections.
pub use frameshift_core as foundation;
/// Astro printing, JSX conversion, and emitted-text provenance.
pub use frameshift_emit as emit;
/// Read-only project discovery, `.gitignore` matching, and the snapshot store.
pub use frameshift_input as input;
/// Language and JSON parsing, lexical scanning, and character reference decoding.
pub use frameshift_syntax as syntax;
/// Validated span edits and rewrite provenance.
pub use frameshift_transform as transform;

// Retain the M0 imports while development tools adopt the layer namespaces.

pub use frameshift_core::{Position, SourceText, Span, TextError, TextLimits};
pub use frameshift_emit::{
    Anchor, Assumption, AssumptionKind, AstroComponent, AttributeItem, AttributeName,
    AttributeValue, BuildError, ClientDirective, ComponentName, ComponentReference,
    ComponentStrategy, ConvertError, ConvertErrorKind, ConvertedMarkup, Element, ElementResolver,
    EmitLimits, EmittedText, Expression, ExpressionPiece, FrontmatterItem, HtmlTagName, ImportItem,
    Indent, JsIdentifier, JsxConversion, LineEnding, NamedImport, OutputBuilder, OutputRegion,
    PrintError, PrintOptions, PropMember, PropType, ProvenanceError, RegionOrigin, SourceId,
    TagName, TemplateNode, convert_jsx, print_astro_component,
};
pub use frameshift_syntax::{
    AssignmentOperator, BinaryOperator, ChildList, ChildNodes, Comment, CommentKind, FunctionKind,
    GreaterThanMode, HtmlCharacterReference, HtmlReferenceContext, HtmlReferenceValue,
    IdentifierKind, IdentifierToken, JsDialect, JsLexicalGoal, JsxCharacterReference,
    JsxStringToken, LegacyOctal, LexicalError, LexicalErrorKind, LexicalLimits, MetaPropertyKind,
    MethodKind, NodeId, NumericBase, NumericKind, NumericLiteral, ParseError, ParseErrorKind,
    ParseLimits, Punctuator, PunctuatorContext, PunctuatorToken, RegExpFlag, RegExpFlags,
    RegExpLiteral, RegExpPatternError, RegExpToken, ReservedWord, ReservedWordClass, SourceGoal,
    StrictModeError, StringToken, SyntaxNode, SyntaxNodeKind, SyntaxTree, TemplateContext,
    TemplateKind, TemplateMode, TemplatePart, TemplatePartList, TemplatePosition, TemplateToken,
    Trivia, TypeScriptKind, UNICODE_VERSION, UnaryOperator, UpdateOperator, VariableKind,
    decode_html_character_reference, decode_jsx_character_reference, is_js_identifier_part,
    is_js_identifier_start, parse_js_expression, parse_js_program, scan_js_identifier,
    scan_js_number, scan_js_punctuator, scan_js_regexp, scan_js_string, scan_js_template,
    scan_js_trivia, scan_jsx_identifier, scan_jsx_string, scan_jsx_text, validate_js_regexp,
};
pub use frameshift_transform::{
    AppliedEdit, Edit, EditLimits, EditOrder, Origin, ProvenanceRegion, RewriteError,
    RewrittenText, apply_edits,
};
