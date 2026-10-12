use frameshift_core::{SourceText, Span};

use crate::{
    identifier::{IdentifierKind, ReservedWord},
    numeric::{NumericBase, NumericKind},
    regexp::RegExpFlags,
    template::TemplateToken,
};

/// Stable index into an expression tree's flat syntax-node arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Returns this node's zero-based arena index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn new(index: u32) -> Self {
        Self(index)
    }
}

/// A contiguous range of child node IDs owned by one arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildList {
    start: u32,
    len: u32,
}

impl ChildList {
    /// Returns the number of nodes in this list.
    pub const fn len(self) -> usize {
        self.len as usize
    }

    /// Reports whether this list is empty.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(crate) const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    pub(crate) const fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.start as usize + self.len as usize
    }
}

/// A contiguous range of template parts owned by one expression tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplatePartList {
    start: u32,
    len: u32,
}

impl TemplatePartList {
    /// Returns the number of segments and substitutions in this template.
    pub const fn len(self) -> usize {
        self.len as usize
    }

    /// Reports whether this template contains no segments or substitutions.
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    pub(crate) const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }
}

/// An operator represented by a unary expression node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    /// Logical negation `!`.
    LogicalNot,
    /// Bitwise complement `~`.
    BitwiseNot,
    /// Numeric unary plus `+`.
    Plus,
    /// Numeric unary negation `-`.
    Minus,
    /// The `typeof` operator.
    Typeof,
    /// The `void` operator.
    Void,
    /// The `delete` operator.
    Delete,
}

/// An operator represented by a binary expression node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    /// The comma operator.
    Comma,
    /// Nullish coalescing `??`.
    Nullish,
    /// Logical OR `||`.
    LogicalOr,
    /// Logical AND `&&`.
    LogicalAnd,
    /// Bitwise OR `|`.
    BitwiseOr,
    /// Bitwise XOR `^`.
    BitwiseXor,
    /// Bitwise AND `&`.
    BitwiseAnd,
    /// Equality `==`.
    Equal,
    /// Inequality `!=`.
    NotEqual,
    /// Strict equality `===`.
    StrictEqual,
    /// Strict inequality `!==`.
    StrictNotEqual,
    /// Less than `<`.
    LessThan,
    /// Greater than `>`.
    GreaterThan,
    /// Less than or equal `<=`.
    LessThanEqual,
    /// Greater than or equal `>=`.
    GreaterThanEqual,
    /// The `in` operator.
    In,
    /// The `instanceof` operator.
    Instanceof,
    /// Left shift `<<`.
    ShiftLeft,
    /// Signed right shift `>>`.
    ShiftRight,
    /// Unsigned right shift `>>>`.
    ShiftRightUnsigned,
    /// Addition `+`.
    Add,
    /// Subtraction `-`.
    Subtract,
    /// Multiplication `*`.
    Multiply,
    /// Division `/`.
    Divide,
    /// Remainder `%`.
    Remainder,
    /// Exponentiation `**`.
    Exponent,
}

/// An operator represented by an assignment expression node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentOperator {
    /// Plain assignment `=`.
    Assign,
    /// Addition assignment `+=`.
    AddAssign,
    /// Subtraction assignment `-=`.
    SubtractAssign,
    /// Multiplication assignment `*=`.
    MultiplyAssign,
    /// Division assignment `/=`.
    DivideAssign,
    /// Remainder assignment `%=`.
    RemainderAssign,
    /// Exponentiation assignment `**=`.
    ExponentAssign,
    /// Left shift assignment `<<=`.
    ShiftLeftAssign,
    /// Signed right shift assignment `>>=`.
    ShiftRightAssign,
    /// Unsigned right shift assignment `>>>=`.
    ShiftRightUnsignedAssign,
    /// Bitwise AND assignment `&=`.
    BitwiseAndAssign,
    /// Bitwise OR assignment `|=`.
    BitwiseOrAssign,
    /// Bitwise XOR assignment `^=`.
    BitwiseXorAssign,
    /// Logical AND assignment `&&=`.
    LogicalAndAssign,
    /// Logical OR assignment `||=`.
    LogicalOrAssign,
    /// Nullish coalescing assignment `??=`.
    NullishAssign,
}

/// An operator represented by an update expression node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOperator {
    /// Increment `++`.
    Increment,
    /// Decrement `--`.
    Decrement,
}

/// One child part of a template literal, in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplatePart {
    /// A raw template segment, including its delimiters and escape evidence.
    Segment(TemplateToken),
    /// An expression contained between `${` and `}`.
    Expression(NodeId),
}

/// The expression form represented by a syntax node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxNodeKind {
    /// An identifier reference or private identifier reference.
    Identifier {
        /// Plain or private lexical form.
        kind: IdentifierKind,
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
        /// Reserved-word spelling after escape processing, if any.
        reserved_word: Option<ReservedWord>,
    },
    /// An identifier name used as a non-computed member property.
    IdentifierName {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
        /// Reserved-word spelling after escape processing, if any.
        reserved_word: Option<ReservedWord>,
    },
    /// A numeric literal, retaining its spelling category without evaluation.
    NumberLiteral {
        /// Radix indicated by the literal spelling.
        base: NumericBase,
        /// Number or BigInt category.
        kind: NumericKind,
    },
    /// A quoted string literal, represented by the node's original span.
    StringLiteral,
    /// A validated regular-expression literal.
    RegularExpression {
        /// Validated ECMAScript flags.
        flags: RegExpFlags,
        /// Number of capturing groups in the pattern.
        capturing_groups: u32,
        /// Number of source bytes occupied by the flag spelling.
        flags_bytes: u32,
    },
    /// A template literal with source-ordered segments and substitutions.
    TemplateLiteral {
        /// Ranges over [`SyntaxTree::template_parts`].
        parts: TemplatePartList,
    },
    /// A boolean literal.
    BooleanLiteral(bool),
    /// The `null` literal.
    NullLiteral,
    /// The `this` expression.
    This,
    /// A parenthesized expression, retaining the parentheses in its span.
    Parenthesized {
        /// Expression inside the parentheses.
        expression: NodeId,
    },
    /// A prefix unary expression.
    Unary {
        /// Applied operator.
        operator: UnaryOperator,
        /// Operand expression.
        operand: NodeId,
    },
    /// A binary expression.
    Binary {
        /// Applied operator.
        operator: BinaryOperator,
        /// Left operand.
        left: NodeId,
        /// Right operand.
        right: NodeId,
    },
    /// A conditional expression `test ? consequent : alternate`.
    Conditional {
        /// Test expression.
        test: NodeId,
        /// Consequent expression.
        consequent: NodeId,
        /// Alternate expression.
        alternate: NodeId,
    },
    /// A member access, including optional and computed forms.
    Member {
        /// Object expression.
        object: NodeId,
        /// Property name or computed property expression.
        property: NodeId,
        /// Whether the property is inside brackets.
        computed: bool,
        /// Whether the access uses `?.`.
        optional: bool,
    },
    /// A call expression with a contiguous list of arguments.
    Call {
        /// Callee expression.
        callee: NodeId,
        /// Argument node IDs, in source order.
        arguments: ChildList,
        /// Whether the call uses optional chaining.
        optional: bool,
    },
    /// A tagged template expression.
    TaggedTemplate {
        /// Tag expression.
        tag: NodeId,
        /// Template literal expression.
        template: NodeId,
    },
    /// An array literal. Holes are [`SyntaxNodeKind::Elision`] elements.
    ArrayExpression {
        /// Element node IDs, in source order.
        elements: ChildList,
        /// Whether a separating comma follows the last element that is not a hole.
        trailing_comma: bool,
    },
    /// An object literal.
    ObjectExpression {
        /// Property, shorthand, and spread node IDs, in source order.
        properties: ChildList,
        /// Whether a comma follows the last property.
        trailing_comma: bool,
    },
    /// A hole in an array literal or array pattern, with an empty span at its comma.
    Elision,
    /// A spread element `...argument` in an array literal, object literal, or argument list.
    Spread {
        /// Spread expression.
        argument: NodeId,
    },
    /// An object literal property `key: value`.
    Property {
        /// Identifier name, string, or number key, or the computed key expression.
        key: NodeId,
        /// Property value expression.
        value: NodeId,
        /// Whether the key is inside brackets.
        computed: bool,
    },
    /// A shorthand object literal property `{ name }`. The property and its
    /// identifier reference share one span.
    ShorthandProperty {
        /// Identifier reference node.
        identifier: NodeId,
    },
    /// An assignment expression.
    Assignment {
        /// Applied operator.
        operator: AssignmentOperator,
        /// Simple target or, for `=`, an array or object pattern.
        target: NodeId,
        /// Assigned value expression.
        value: NodeId,
    },
    /// A prefix or postfix update expression.
    Update {
        /// Applied operator.
        operator: UpdateOperator,
        /// Whether the operator precedes its operand.
        prefix: bool,
        /// Simple assignment target.
        operand: NodeId,
    },
    /// A `new` expression, with `None` arguments when no argument list follows.
    New {
        /// Constructor expression.
        callee: NodeId,
        /// Argument node IDs, in source order, when an argument list is present.
        arguments: Option<ChildList>,
    },
    /// An arrow function with an expression body.
    ArrowFunction {
        /// Parameter patterns, in source order.
        parameters: ChildList,
        /// Concise body expression.
        body: NodeId,
    },
    /// A name declared by a binding pattern, such as an arrow parameter.
    BindingIdentifier {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
    },
    /// An array destructuring pattern, reinterpreted from an array literal.
    ArrayPattern {
        /// Target, default, rest, and hole node IDs, in source order.
        elements: ChildList,
        /// Whether a separating comma follows the last element that is not a hole.
        trailing_comma: bool,
    },
    /// An object destructuring pattern, reinterpreted from an object literal.
    ObjectPattern {
        /// Pattern property and rest node IDs, in source order.
        properties: ChildList,
        /// Whether a comma follows the last property.
        trailing_comma: bool,
    },
    /// An object pattern property `key: target`.
    PatternProperty {
        /// Identifier name, string, or number key, or the computed key expression.
        key: NodeId,
        /// Target pattern, possibly with a default value.
        value: NodeId,
        /// Whether the key is inside brackets.
        computed: bool,
    },
    /// A shorthand object pattern property `{ name }` or `{ name = default }`.
    ShorthandPatternProperty {
        /// Identifier reference in assignment patterns, binding identifier in parameters.
        target: NodeId,
        /// Default value expression, if present.
        default: Option<NodeId>,
    },
    /// A rest element `...target` in a pattern or parameter list.
    RestElement {
        /// Rest target.
        argument: NodeId,
    },
    /// A target with a default value, `target = default`.
    AssignmentPattern {
        /// Target pattern.
        target: NodeId,
        /// Default value expression.
        default: NodeId,
    },
}

/// One expression node in a syntax tree arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxNode {
    pub(crate) kind: SyntaxNodeKind,
    pub(crate) span: Span,
}

impl SyntaxNode {
    /// Returns the expression or name kind represented by this node.
    pub const fn kind(self) -> SyntaxNodeKind {
        self.kind
    }

    /// Returns the exact half-open source span of this node.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// An expression tree backed by flat node and child-list arenas.
///
/// Array elements, object properties, arrow parameters, and argument lists
/// share the child arena. Patterns are reinterpreted from their cover
/// expressions in place, so every node of a returned tree is reachable from
/// the root.
pub struct SyntaxTree<'source> {
    pub(crate) source: &'source SourceText,
    pub(crate) nodes: Vec<SyntaxNode>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) template_parts: Vec<TemplatePart>,
    pub(crate) comments: Vec<crate::Comment>,
    pub(crate) root: NodeId,
    pub(crate) consumed_span: Span,
}

impl<'source> SyntaxTree<'source> {
    /// Returns the immutable source text from which all spans are derived.
    pub const fn source(&self) -> &'source SourceText {
        self.source
    }

    /// Returns the root expression's arena ID.
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Returns a node by its arena index, or `None` when the index is out of bounds.
    pub fn node(&self, id: NodeId) -> Option<&SyntaxNode> {
        self.nodes.get(id.index())
    }

    /// Returns all nodes in arena insertion order.
    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    /// Returns the node IDs of a child list, or `None` for an invalid list.
    pub fn children(&self, list: ChildList) -> Option<&[NodeId]> {
        slice_range(&self.children, list.start, list.len)
    }

    /// Returns parts for a template literal, or `None` for an invalid list.
    pub fn template_parts(&self, list: TemplatePartList) -> Option<&[TemplatePart]> {
        slice_range(&self.template_parts, list.start, list.len)
    }

    /// Returns comments encountered while parsing, in source order.
    pub fn comments(&self) -> &[crate::Comment] {
        &self.comments
    }

    /// Returns the range consumed by this parse, including surrounding trivia.
    pub const fn consumed_span(&self) -> Span {
        self.consumed_span
    }
}

fn slice_range<T>(items: &[T], start: u32, len: u32) -> Option<&[T]> {
    let start = start as usize;
    let end = start.checked_add(len as usize)?;
    items.get(start..end)
}
