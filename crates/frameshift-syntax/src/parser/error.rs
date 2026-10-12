use std::{error::Error, fmt};

use frameshift_core::TextError;

use crate::{
    lexical::{LexicalError, LexicalErrorKind},
    punctuator::Punctuator,
};

/// A construct that strict mode code forbids and sloppy code permits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictModeError {
    /// A `with` statement.
    With,
    /// `delete` applied to an unqualified identifier.
    DeleteIdentifier,
    /// A legacy octal or non-octal decimal integer literal such as `017` or `08`.
    LegacyOctalLiteral,
    /// A legacy octal or non-octal decimal string escape such as `"\1"` or `"\8"`.
    LegacyOctalEscape,
    /// `eval` or `arguments` as a binding name or assignment target.
    RestrictedName,
    /// A call expression as the target of `=`, compound assignment, update, or
    /// `for-in` and `for-of`, which sloppy code reports only at runtime.
    CallAssignmentTarget,
}

impl StrictModeError {
    fn message(self) -> &'static str {
        match self {
            Self::With => "a with statement is not allowed in strict mode code",
            Self::DeleteIdentifier => "deleting an identifier is not allowed in strict mode code",
            Self::LegacyOctalLiteral => "a legacy octal literal is not allowed in strict mode code",
            Self::LegacyOctalEscape => "a legacy octal escape is not allowed in strict mode code",
            Self::RestrictedName => {
                "eval or arguments cannot be bound or assigned in strict mode code"
            }
            Self::CallAssignmentTarget => {
                "a call expression is not a valid assignment target in strict mode code"
            }
        }
    }
}

/// Why the parser could not produce a complete tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A lower-level lexical scanner rejected the source.
    Lexical(LexicalErrorKind),
    /// No expression can start at this offset.
    ExpectedExpression,
    /// A required closing parenthesis was not present.
    ExpectedClosingParenthesis,
    /// A required closing bracket was not present.
    ExpectedClosingBracket,
    /// A required closing brace was not present.
    ExpectedClosingBrace,
    /// A required punctuator other than a closing delimiter was not present.
    ExpectedPunctuator(Punctuator),
    /// A statement was not terminated by a semicolon, and automatic semicolon
    /// insertion does not apply.
    ExpectedSemicolon,
    /// A binding or label identifier was required.
    ExpectedIdentifier,
    /// The `while` of a `do ... while` statement was not present.
    ExpectedWhile,
    /// A `try` block was not followed by `catch` or `finally`.
    ExpectedCatchOrFinally,
    /// An import or export clause was not followed by `from`.
    ExpectedFrom,
    /// A namespace import or export was not followed by `as`.
    ExpectedAs,
    /// A module specifier string literal was required.
    ExpectedModuleSpecifier,
    /// A template substitution was not followed by its closing right brace.
    ExpectedTemplateContinuation,
    /// The input contains tokens after the parsed expression.
    TrailingInput,
    /// A reserved word is used where the known context reserves it, such as
    /// `if` as a name, `let` in strict mode code, `yield` in a generator, or
    /// `await` in a module or async function.
    ReservedWord,
    /// An escaped spelling of a reserved word is used where only the
    /// unescaped keyword or a name could appear.
    EscapedReservedWord,
    /// A reserved word cannot be interpreted without strict-mode context.
    ReservedWordRequiresContext,
    /// `await` or `yield` requires function and source-goal context.
    ContextualKeywordRequiresContext,
    /// The construct is valid in sloppy mode and invalid in strict mode code,
    /// so its validity requires source context.
    StrictModeRequiresContext,
    /// The construct's meaning depends on the unknown source goal, such as an
    /// HTML-like comment or `import.meta`.
    SourceGoalRequiresContext,
    /// `new.target` requires function context.
    MetaPropertyRequiresContext,
    /// `super` requires method context.
    SuperRequiresContext,
    /// A private identifier requires class context.
    PrivateIdentifierRequiresContext,
    /// Strict mode code forbids this construct.
    StrictMode(StrictModeError),
    /// Exponentiation cannot have an unparenthesized unary expression on its left.
    InvalidExponentiation,
    /// `??` cannot mix with `&&` or `||` without parentheses.
    MixedNullishAndLogical,
    /// Optional chains cannot be used directly as tagged-template tags.
    OptionalChainTag,
    /// An optional chain cannot be the constructor of a `new` expression.
    OptionalChainInNew,
    /// An object literal member is not a property, method, shorthand property,
    /// or spread element.
    InvalidPropertyDefinition,
    /// The left side of an assignment, an update operand, a `for-in` or
    /// `for-of` target, or a destructuring element is not a valid assignment target.
    InvalidAssignmentTarget,
    /// A rest element is not last, is followed by a comma, has a default value,
    /// or has a form its pattern does not permit.
    InvalidRestElement,
    /// A name or parenthesized list before `=>` is not a valid parameter list.
    InvalidArrowParameters,
    /// A declaration, parameter, or catch binding is not an identifier or pattern.
    InvalidBindingTarget,
    /// A parameter list declares the same name more than once where duplicates
    /// are not permitted.
    DuplicateParameter,
    /// An empty list, rest element, or trailing comma in parentheses is not
    /// followed by `=>`.
    ExpectedArrow,
    /// `=>` appears where an arrow function is not permitted.
    UnexpectedArrow,
    /// A line terminator separates arrow parameters from `=>`, or `async` from
    /// its arrow parameters.
    LineTerminatorBeforeArrow,
    /// A shorthand property with an initializer is used outside a destructuring pattern.
    ShorthandInitializerOutsidePattern,
    /// An object literal defines `__proto__` more than once with `:`.
    DuplicateProto,
    /// `super` is used outside a method that permits it.
    InvalidSuper,
    /// `new.target` is used outside a function.
    InvalidNewTarget,
    /// `new.` or `import.` is followed by an unsupported property name.
    InvalidMetaProperty,
    /// `import.meta` is used outside a module.
    ImportMetaOutsideModule,
    /// `<!--` appears between tokens of module code. The module grammar reads
    /// it as operators, but engines reject it, so the file cannot run.
    HtmlCommentInModule,
    /// A `yield` expression is used in formal parameters.
    YieldInParameters,
    /// An `await` expression, or `await` in an async arrow head, is used in
    /// formal parameters.
    AwaitInParameters,
    /// A function with a `"use strict"` directive has a non-simple parameter list.
    UseStrictWithNonSimpleParameters,
    /// A getter declares parameters, or a setter does not declare exactly one
    /// non-rest parameter.
    InvalidAccessorParameters,
    /// `arguments` is referenced in a class field initializer or static block.
    InvalidArguments,
    /// A declaration appears where only a single statement is permitted, such
    /// as a `let` declaration as a loop body or a function in strict `if`.
    DeclarationInStatementPosition,
    /// A name is declared twice in a scope where that is not permitted.
    DuplicateDeclaration,
    /// `let` is declared as a lexically bound name.
    LetBindingName,
    /// A `const`, `using`, or destructuring declaration lacks an initializer.
    MissingInitializer,
    /// A `for-in` or `for-of` head declares more than one binding, an
    /// initializer, or a `using` binding in `for-in`.
    InvalidForInOfDeclaration,
    /// A `for-of` head starts with `let` or is exactly `async`.
    InvalidForOfLeftHandSide,
    /// `for await` is not a `for-of` statement.
    InvalidForAwait,
    /// A `using` or `await using` declaration appears at the top level of a
    /// script or directly in a `switch` clause, or binds a pattern.
    InvalidUsingDeclaration,
    /// `return` is used outside a function body.
    InvalidReturn,
    /// `break` is used outside a loop or `switch` without a label.
    InvalidBreak,
    /// `continue` is used outside a loop, or its label does not denote a loop.
    InvalidContinue,
    /// `break` or `continue` names a label that does not enclose it.
    UndefinedLabel,
    /// A label is nested inside a statement with the same label.
    DuplicateLabel,
    /// A line terminator follows `throw`.
    LineTerminatorAfterThrow,
    /// A `switch` statement has more than one `default` clause.
    DuplicateDefaultClause,
    /// A class has more than one constructor.
    DuplicateConstructor,
    /// A class constructor is a getter, setter, generator, or async method.
    InvalidConstructor,
    /// A class element is named `#constructor`, a static `prototype`, or a
    /// field `constructor`.
    InvalidClassElementName,
    /// A private name is declared twice in one class body, other than as a
    /// getter and setter pair with the same placement.
    DuplicatePrivateName,
    /// A private name is not declared by an enclosing class.
    UndeclaredPrivateName,
    /// A private name appears outside a member access or `#name in object`.
    UnexpectedPrivateName,
    /// A private member is deleted.
    DeletePrivateName,
    /// An import or export declaration appears outside the top level of a module.
    ModuleDeclarationOutsideModule,
    /// A module export name is exported twice.
    DuplicateExport,
    /// An exported local binding is not declared at the module's top level.
    UndeclaredExport,
    /// A local export name without `from` is a string or a reserved word.
    InvalidLocalExport,
    /// A string module export name contains an unpaired surrogate.
    InvalidModuleExportName,
    /// An import attribute key occurs twice.
    DuplicateImportAttribute,
    /// A JSX tag continues with something other than an attribute, `/>`, or `>`.
    ExpectedJsxTagEnd,
    /// A JSX attribute's `=` is not followed by a string, expression
    /// container, element, or fragment.
    ExpectedJsxAttributeValue,
    /// A JSX attribute value is an empty expression container `{}`.
    EmptyJsxAttributeValue,
    /// A `<` token directly follows a JSX element or fragment expression, as
    /// in adjacent elements without a wrapper.
    AdjacentJsxElements,
    /// A JSX element or fragment reaches EOF before its closing tag. The
    /// offset is the opening `<`.
    UnclosedJsxElement,
    /// A closing tag does not match its opening tag's name or form. The
    /// offset is the closing tag's `<`.
    JsxClosingTagMismatch,
    /// The configured node, token, or nesting bound was reached.
    ResourceLimit,
    /// Parser storage could not be reserved.
    AllocationFailed,
    /// Internal span construction failed despite parser invariants.
    InternalInvariant(TextError),
}

/// A parse failure with an original-source byte offset and no source snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Original-source byte offset associated with the failure.
    pub offset: usize,
    /// Typed failure reason.
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match &self.kind {
            ParseErrorKind::Lexical(kind) => {
                return write!(
                    f,
                    "{}",
                    LexicalError {
                        offset: self.offset,
                        kind: kind.clone(),
                    }
                );
            }
            ParseErrorKind::ExpectedPunctuator(punctuator) => {
                return write!(
                    f,
                    "expected `{}` at byte offset {}",
                    punctuator.spelling(),
                    self.offset
                );
            }
            ParseErrorKind::StrictMode(error) => error.message(),
            ParseErrorKind::InternalInvariant(error) => {
                return write!(f, "internal parse error at {}: {error}", self.offset);
            }
            ParseErrorKind::ExpectedExpression => "expected an expression",
            ParseErrorKind::ExpectedClosingParenthesis => "expected a closing parenthesis",
            ParseErrorKind::ExpectedClosingBracket => "expected a closing bracket",
            ParseErrorKind::ExpectedClosingBrace => "expected a closing brace",
            ParseErrorKind::ExpectedSemicolon => "expected a semicolon",
            ParseErrorKind::ExpectedIdentifier => "expected an identifier",
            ParseErrorKind::ExpectedWhile => "expected `while` after a do statement body",
            ParseErrorKind::ExpectedCatchOrFinally => "expected `catch` or `finally`",
            ParseErrorKind::ExpectedFrom => "expected `from`",
            ParseErrorKind::ExpectedAs => "expected `as`",
            ParseErrorKind::ExpectedModuleSpecifier => "expected a module specifier string",
            ParseErrorKind::ExpectedTemplateContinuation => {
                "expected the end of a template substitution"
            }
            ParseErrorKind::TrailingInput => "unexpected tokens after the expression",
            ParseErrorKind::ReservedWord => "reserved word cannot be used here",
            ParseErrorKind::EscapedReservedWord => "a keyword cannot contain escapes",
            ParseErrorKind::ReservedWordRequiresContext => "reserved word requires parser context",
            ParseErrorKind::ContextualKeywordRequiresContext => {
                "contextual keyword requires parser context"
            }
            ParseErrorKind::StrictModeRequiresContext => {
                "construct depends on strict mode and requires source context"
            }
            ParseErrorKind::SourceGoalRequiresContext => {
                "construct depends on the source goal and requires source context"
            }
            ParseErrorKind::MetaPropertyRequiresContext => "new.target requires function context",
            ParseErrorKind::SuperRequiresContext => "super requires method context",
            ParseErrorKind::PrivateIdentifierRequiresContext => {
                "private identifier requires class context"
            }
            ParseErrorKind::InvalidExponentiation => {
                "unparenthesized unary expression cannot precede exponentiation"
            }
            ParseErrorKind::MixedNullishAndLogical => {
                "nullish coalescing cannot mix with logical operators without parentheses"
            }
            ParseErrorKind::OptionalChainTag => {
                "an optional chain cannot be used as a tagged-template tag"
            }
            ParseErrorKind::OptionalChainInNew => {
                "an optional chain cannot be used as a constructor in a new expression"
            }
            ParseErrorKind::InvalidPropertyDefinition => "invalid object literal member",
            ParseErrorKind::InvalidAssignmentTarget => "invalid assignment target",
            ParseErrorKind::InvalidRestElement => "invalid rest element",
            ParseErrorKind::InvalidArrowParameters => "invalid arrow function parameters",
            ParseErrorKind::InvalidBindingTarget => "invalid binding target",
            ParseErrorKind::DuplicateParameter => "duplicate parameter name",
            ParseErrorKind::ExpectedArrow => "expected an arrow after the parameter list",
            ParseErrorKind::UnexpectedArrow => "an arrow function is not permitted here",
            ParseErrorKind::LineTerminatorBeforeArrow => {
                "a line terminator cannot precede an arrow"
            }
            ParseErrorKind::ShorthandInitializerOutsidePattern => {
                "a shorthand property initializer is only valid in a destructuring pattern"
            }
            ParseErrorKind::DuplicateProto => "duplicate __proto__ property in an object literal",
            ParseErrorKind::InvalidSuper => "super is not permitted here",
            ParseErrorKind::InvalidNewTarget => "new.target is only permitted in functions",
            ParseErrorKind::InvalidMetaProperty => "invalid meta property",
            ParseErrorKind::ImportMetaOutsideModule => "import.meta is only permitted in modules",
            ParseErrorKind::HtmlCommentInModule => {
                "an HTML-like comment opening is rejected in modules"
            }
            ParseErrorKind::YieldInParameters => {
                "yield expressions are not permitted in parameters"
            }
            ParseErrorKind::AwaitInParameters => "await is not permitted in parameters",
            ParseErrorKind::UseStrictWithNonSimpleParameters => {
                "a use strict directive requires a simple parameter list"
            }
            ParseErrorKind::InvalidAccessorParameters => "invalid accessor parameters",
            ParseErrorKind::InvalidArguments => {
                "arguments is not permitted in class field initializers and static blocks"
            }
            ParseErrorKind::DeclarationInStatementPosition => {
                "a declaration is not permitted in a single-statement position"
            }
            ParseErrorKind::DuplicateDeclaration => "name is already declared in this scope",
            ParseErrorKind::LetBindingName => "let cannot be a lexically bound name",
            ParseErrorKind::MissingInitializer => "declaration requires an initializer",
            ParseErrorKind::InvalidForInOfDeclaration => "invalid for-in or for-of declaration",
            ParseErrorKind::InvalidForOfLeftHandSide => {
                "a for-of target cannot start with let or be async"
            }
            ParseErrorKind::InvalidForAwait => "for await requires a for-of statement",
            ParseErrorKind::InvalidUsingDeclaration => "using declaration is not permitted here",
            ParseErrorKind::InvalidReturn => "return is only permitted in function bodies",
            ParseErrorKind::InvalidBreak => "break requires an enclosing loop or switch",
            ParseErrorKind::InvalidContinue => "continue requires an enclosing loop",
            ParseErrorKind::UndefinedLabel => "label is not defined",
            ParseErrorKind::DuplicateLabel => "label is already defined",
            ParseErrorKind::LineTerminatorAfterThrow => "a line terminator cannot follow throw",
            ParseErrorKind::DuplicateDefaultClause => "switch has more than one default clause",
            ParseErrorKind::DuplicateConstructor => "class has more than one constructor",
            ParseErrorKind::InvalidConstructor => "class constructor cannot be a special method",
            ParseErrorKind::InvalidClassElementName => "invalid class element name",
            ParseErrorKind::DuplicatePrivateName => "private name is already declared",
            ParseErrorKind::UndeclaredPrivateName => "private name is not declared",
            ParseErrorKind::UnexpectedPrivateName => "private name cannot be used here",
            ParseErrorKind::DeletePrivateName => "private members cannot be deleted",
            ParseErrorKind::ModuleDeclarationOutsideModule => {
                "import and export declarations are only permitted at the top level of a module"
            }
            ParseErrorKind::DuplicateExport => "name is already exported",
            ParseErrorKind::UndeclaredExport => "exported name is not declared",
            ParseErrorKind::InvalidLocalExport => "invalid local export name",
            ParseErrorKind::InvalidModuleExportName => {
                "module export name contains an unpaired surrogate"
            }
            ParseErrorKind::DuplicateImportAttribute => "duplicate import attribute key",
            ParseErrorKind::ExpectedJsxTagEnd => "expected a JSX attribute, `/>`, or `>`",
            ParseErrorKind::ExpectedJsxAttributeValue => {
                "expected a JSX string, expression container, element, or fragment"
            }
            ParseErrorKind::EmptyJsxAttributeValue => {
                "a JSX attribute value cannot be an empty expression container"
            }
            ParseErrorKind::AdjacentJsxElements => {
                "adjacent JSX elements must be wrapped in an enclosing element or fragment"
            }
            ParseErrorKind::UnclosedJsxElement => "JSX element has no closing tag",
            ParseErrorKind::JsxClosingTagMismatch => {
                "JSX closing tag does not match its opening tag"
            }
            ParseErrorKind::ResourceLimit => "parse limit exceeded",
            ParseErrorKind::AllocationFailed => "could not allocate syntax tree storage",
        };
        write!(f, "{message} at byte offset {}", self.offset)
    }
}

impl Error for ParseError {}
