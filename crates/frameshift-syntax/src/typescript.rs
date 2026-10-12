//! Structured TypeScript syntax, without type-checking semantics.

/// TypeScript grammar productions in the shared arena.
///
/// Children are direct constituents in source order. Names, literals,
/// expressions, and JavaScript bodies use their existing node kinds. Optional
/// syntax is absent, never represented by a synthetic span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeScriptKind {
    /// A primitive type keyword, with spelling in its source span.
    Keyword,
    /// A type reference: entity name, then optional arguments.
    Reference,
    /// A dotted name: qualifier, then final name.
    QualifiedName,
    /// A `<...>` type argument list.
    TypeArguments,
    /// A `<...>` type parameter list.
    TypeParameters {
        /// Whether a comma follows the last parameter, significant in TSX.
        trailing_comma: bool,
    },
    /// A type parameter: modifiers, name, constraint, then default.
    TypeParameter,
    /// An `extends` constraint with one type.
    Constraint,
    /// A default with one type.
    DefaultType,
    /// A parenthesized type.
    Parenthesized,
    /// A union of types.
    Union,
    /// An intersection of types.
    Intersection,
    /// A conditional: check, constraint, true branch, then false branch.
    Conditional,
    /// `keyof`, `unique`, or `readonly`, with one type.
    Operator,
    /// `infer`, with a type parameter.
    Infer,
    /// An array, with its element type.
    Array,
    /// Indexed access: object, then index type.
    IndexedAccess,
    /// A tuple with its elements.
    Tuple,
    /// A tuple element: optional label, then type.
    TupleElement {
        /// Whether the element is optional.
        optional: bool,
        /// Whether the element is a rest element.
        rest: bool,
    },
    /// A literal type with its literal.
    Literal,
    /// A template type: head, then substitution/segment pairs.
    Template,
    /// A raw template type segment.
    TemplateSegment,
    /// A query: entity name or import type, then arguments.
    Query,
    /// An import type: argument, options, qualifier, then arguments.
    ImportType,
    /// A type literal with its members.
    TypeLiteral,
    /// A mapped type: modifiers, parameter, remapping, type, then members.
    Mapped,
    /// `as` name remapping with one type.
    NameRemapping,
    /// A property: modifiers, name, type, then optional initializer.
    PropertySignature {
        /// Whether the name is optional.
        optional: bool,
    },
    /// A method: modifiers, name, generics, parameters, then return type.
    MethodSignature {
        /// Whether the name is optional.
        optional: bool,
    },
    /// A call signature: generics, parameters, then return type.
    CallSignature,
    /// A construct signature: generics, parameters, then return type.
    ConstructSignature,
    /// An index signature: modifiers, parameters, then result type.
    IndexSignature,
    /// A function type: generics, parameters, then return type.
    FunctionType,
    /// A constructor type: modifiers, generics, parameters, then return type.
    ConstructorType,
    /// A predicate: parameter name or `this`, then optional asserted type.
    Predicate {
        /// Whether the predicate begins with `asserts`.
        asserts: bool,
    },
    /// A colon-delimited annotation with one type or predicate.
    Annotation,
    /// A typed binding: binding, then optional annotation.
    TypedBinding {
        /// Whether the binding is optional.
        optional: bool,
        /// Whether it has a definite assignment marker.
        definite: bool,
    },
    /// A parameter: modifiers, binding, type, then default expression.
    Parameter {
        /// Whether the parameter is optional.
        optional: bool,
        /// Whether it is a rest parameter.
        rest: bool,
    },
    /// A modifier, whose spelling is its source span.
    Modifier,
    /// A decorator with its expression.
    Decorator,
    /// A type alias: name, generics, then type.
    TypeAlias,
    /// An interface: name, generics, heritage, then members.
    Interface,
    /// An `extends` or `implements` list.
    Heritage,
    /// An enum: name, then members.
    Enum,
    /// An enum member: name, then optional initializer.
    EnumMember,
    /// A namespace or ambient module: name, then declaration or body.
    Namespace,
    /// A namespace body with statements.
    NamespaceBody,
    /// An import-equals declaration: modifiers, name, then reference.
    ImportEquals,
    /// A `require("...")` module reference.
    ExternalModuleReference,
    /// A type-only import/export specifier or declaration.
    TypeOnly,
    /// `export =` with one expression.
    ExportAssignment,
    /// `export as namespace` with one name.
    NamespaceExport,
    /// Modifiers or decorators followed by a declaration.
    ModifiedDeclaration,
    /// Function or class constituents together with generic syntax.
    GenericDeclaration,
    /// A signature without an implementation body.
    BodylessFunction,
    /// An expression with type arguments: expression, then arguments.
    Instantiation,
    /// `expression as Type`: expression, then type.
    As,
    /// `expression satisfies Type`: expression, then type.
    Satisfies,
    /// `<Type>expression`: type, then expression.
    Assertion,
    /// A postfix non-null assertion with one expression.
    NonNull,
    /// A JSDoc-compatible type operator accepted by the compiler's TS parser.
    JSDoc,
}
