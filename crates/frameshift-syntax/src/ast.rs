use std::num::NonZeroU32;

use frameshift_core::{SourceText, Span};

use crate::{
    identifier::ReservedWord,
    lexical::SourceGoal,
    numeric::{NumericBase, NumericKind},
    parser::JsDialect,
    regexp::RegExpFlags,
    template::TemplateToken,
};

/// Stable index into a syntax tree's flat node arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// Returns this node's zero-based arena index.
    pub const fn index(self) -> usize {
        self.0.get() as usize - 1
    }

    // The arena never holds `u32::MAX` nodes, so `index + 1` cannot overflow.
    // The non-zero representation lets `Option<NodeId>` stay four bytes.
    pub(crate) const fn new(index: u32) -> Self {
        match NonZeroU32::new(index.wrapping_add(1)) {
            Some(value) => Self(value),
            None => panic!("node index out of range"),
        }
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

/// A contiguous range of template parts owned by one syntax tree.
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
    /// The `in` operator, including `#name in object`.
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

/// How a function evaluates, as spelled by `async` and `*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    /// An ordinary function.
    Normal,
    /// A generator function `function*`.
    Generator,
    /// An async function.
    Async,
    /// An async generator function.
    AsyncGenerator,
}

impl FunctionKind {
    /// Reports whether the function is async.
    pub const fn is_async(self) -> bool {
        matches!(self, Self::Async | Self::AsyncGenerator)
    }

    /// Reports whether the function is a generator.
    pub const fn is_generator(self) -> bool {
        matches!(self, Self::Generator | Self::AsyncGenerator)
    }
}

/// The syntactic form of an object literal or class method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    /// An ordinary method `name() {}`.
    Method,
    /// A generator method `*name() {}`.
    Generator,
    /// An async method `async name() {}`.
    Async,
    /// An async generator method `async *name() {}`.
    AsyncGenerator,
    /// A getter `get name() {}`.
    Getter,
    /// A setter `set name(value) {}`.
    Setter,
    /// The non-static `constructor` method of a class.
    Constructor,
}

impl MethodKind {
    /// Returns how the method's body evaluates. Accessors and constructors
    /// are ordinary functions.
    pub const fn function_kind(self) -> FunctionKind {
        match self {
            Self::Method | Self::Getter | Self::Setter | Self::Constructor => FunctionKind::Normal,
            Self::Generator => FunctionKind::Generator,
            Self::Async => FunctionKind::Async,
            Self::AsyncGenerator => FunctionKind::AsyncGenerator,
        }
    }
}

/// The keyword that introduces a variable declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    /// `var`, scoped to the enclosing function or source file.
    Var,
    /// `let`.
    Let,
    /// `const`.
    Const,
    /// `using`, an explicit resource management declaration.
    Using,
    /// `await using`.
    AwaitUsing,
}

/// A meta property expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaPropertyKind {
    /// `new.target`.
    NewTarget,
    /// `import.meta`, valid only in modules.
    ImportMeta,
}

/// One child part of a template literal, in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplatePart {
    /// A raw template segment, including its delimiters and escape evidence.
    Segment(TemplateToken),
    /// An expression contained between `${` and `}`.
    Expression(NodeId),
}

/// The syntactic form represented by a syntax node.
///
/// Spans of statements include their semicolon when one is present in the
/// source. Children are listed in source order by [`SyntaxTree::child_nodes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxNodeKind {
    /// A structured TypeScript production with constituents in source order.
    TypeScript {
        /// The grammar production and its syntactic flags.
        kind: crate::TypeScriptKind,
        /// Direct constituents in source order.
        children: ChildList,
    },
    /// A complete Script or Module.
    Program {
        /// The goal symbol the source was parsed with.
        goal: SourceGoal,
        /// The language variant the source was parsed as.
        dialect: JsDialect,
        /// Directives, statements, declarations, and module items, in source order.
        body: ChildList,
        /// Whether the top-level code is strict: always for modules, and for
        /// scripts with a `"use strict"` directive.
        strict: bool,
    },
    /// A directive prologue entry such as `"use strict";`.
    Directive {
        /// The string literal.
        expression: NodeId,
    },
    /// An expression statement.
    ExpressionStatement {
        /// The expression.
        expression: NodeId,
    },
    /// A block `{ ... }`.
    BlockStatement {
        /// Statements and declarations, in source order.
        body: ChildList,
    },
    /// An empty statement `;`.
    EmptyStatement,
    /// A `debugger` statement.
    DebuggerStatement,
    /// An `if` statement.
    IfStatement {
        /// Condition.
        test: NodeId,
        /// Statement executed when the condition holds.
        consequent: NodeId,
        /// Statement after `else`, if present.
        alternate: Option<NodeId>,
    },
    /// A `for (init; test; update)` statement.
    ForStatement {
        /// Variable declaration or expression before the first semicolon.
        init: Option<NodeId>,
        /// Condition.
        test: Option<NodeId>,
        /// Expression after the second semicolon.
        update: Option<NodeId>,
        /// Loop body.
        body: NodeId,
    },
    /// A `for (left in right)` statement.
    ForInStatement {
        /// Variable declaration or assignment target.
        left: NodeId,
        /// Object expression.
        right: NodeId,
        /// Loop body.
        body: NodeId,
    },
    /// A `for (left of right)` or `for await (left of right)` statement.
    ForOfStatement {
        /// Variable declaration or assignment target.
        left: NodeId,
        /// Iterable expression.
        right: NodeId,
        /// Loop body.
        body: NodeId,
        /// Whether the loop is `for await`.
        is_await: bool,
    },
    /// A `while` statement.
    WhileStatement {
        /// Condition.
        test: NodeId,
        /// Loop body.
        body: NodeId,
    },
    /// A `do ... while` statement.
    DoWhileStatement {
        /// Loop body.
        body: NodeId,
        /// Condition.
        test: NodeId,
    },
    /// A `continue` statement.
    ContinueStatement {
        /// Target label, if present.
        label: Option<NodeId>,
    },
    /// A `break` statement.
    BreakStatement {
        /// Target label, if present.
        label: Option<NodeId>,
    },
    /// A `return` statement.
    ReturnStatement {
        /// Returned expression, if present.
        argument: Option<NodeId>,
    },
    /// A `with` statement, valid only in sloppy code.
    WithStatement {
        /// Object expression.
        object: NodeId,
        /// Body statement.
        body: NodeId,
    },
    /// A `switch` statement.
    SwitchStatement {
        /// Discriminant expression.
        discriminant: NodeId,
        /// Case and default clauses, in source order.
        cases: ChildList,
    },
    /// A `case` clause, or the `default` clause when `test` is `None`.
    SwitchCase {
        /// Case expression.
        test: Option<NodeId>,
        /// Statements and declarations after the colon.
        consequent: ChildList,
    },
    /// A labelled statement.
    LabeledStatement {
        /// Label definition.
        label: NodeId,
        /// Labelled statement or sloppy-mode function declaration.
        body: NodeId,
    },
    /// A label name in a labelled statement, `break`, or `continue`.
    LabelIdentifier {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
    },
    /// A `throw` statement.
    ThrowStatement {
        /// Thrown expression.
        argument: NodeId,
    },
    /// A `try` statement.
    TryStatement {
        /// Protected block.
        block: NodeId,
        /// Catch clause, if present.
        handler: Option<NodeId>,
        /// Finally block, if present.
        finalizer: Option<NodeId>,
    },
    /// A `catch` clause.
    CatchClause {
        /// Catch parameter, if present.
        parameter: Option<NodeId>,
        /// Handler block.
        body: NodeId,
    },
    /// A `var`, `let`, `const`, `using`, or `await using` declaration.
    VariableDeclaration {
        /// Declaration keyword.
        kind: VariableKind,
        /// Declarators, in source order.
        declarators: ChildList,
    },
    /// One binding of a variable declaration.
    VariableDeclarator {
        /// Binding identifier or pattern.
        target: NodeId,
        /// Initializer, if present.
        init: Option<NodeId>,
    },
    /// A function declaration. Only `export default` declarations may omit the name.
    FunctionDeclaration {
        /// Binding identifier, if present.
        name: Option<NodeId>,
        /// The [`SyntaxNodeKind::FormalParameters`] node.
        parameters: NodeId,
        /// The [`SyntaxNodeKind::FunctionBody`] node.
        body: NodeId,
        /// Ordinary, generator, async, or async generator.
        kind: FunctionKind,
    },
    /// A function expression.
    FunctionExpression {
        /// Binding identifier, if present.
        name: Option<NodeId>,
        /// The [`SyntaxNodeKind::FormalParameters`] node.
        parameters: NodeId,
        /// The [`SyntaxNodeKind::FunctionBody`] node.
        body: NodeId,
        /// Ordinary, generator, async, or async generator.
        kind: FunctionKind,
    },
    /// A parameter list. Its span includes the parentheses, or is the single
    /// parameter of an unparenthesized arrow function.
    FormalParameters {
        /// Binding identifiers, patterns, defaults, and a final rest element.
        parameters: ChildList,
    },
    /// A function body `{ ... }`.
    FunctionBody {
        /// Directives, statements, and declarations, in source order.
        body: ChildList,
        /// Whether the body is strict code, by directive or by inheritance.
        strict: bool,
    },
    /// An arrow function.
    ArrowFunction {
        /// The [`SyntaxNodeKind::FormalParameters`] node.
        parameters: NodeId,
        /// Concise body expression or a [`SyntaxNodeKind::FunctionBody`].
        body: NodeId,
        /// Whether the arrow function is async.
        is_async: bool,
    },
    /// A class declaration. Only `export default` declarations may omit the name.
    ClassDeclaration {
        /// Binding identifier, if present.
        name: Option<NodeId>,
        /// Expression after `extends`, if present.
        super_class: Option<NodeId>,
        /// The [`SyntaxNodeKind::ClassBody`] node.
        body: NodeId,
    },
    /// A class expression.
    ClassExpression {
        /// Binding identifier, if present.
        name: Option<NodeId>,
        /// Expression after `extends`, if present.
        super_class: Option<NodeId>,
        /// The [`SyntaxNodeKind::ClassBody`] node.
        body: NodeId,
    },
    /// A class body `{ ... }`.
    ClassBody {
        /// Methods, fields, and static blocks, in source order.
        elements: ChildList,
    },
    /// An object literal or class method, getter, setter, or constructor.
    MethodDefinition {
        /// Identifier name, string, number, private name, or computed key expression.
        key: NodeId,
        /// The [`SyntaxNodeKind::FormalParameters`] node.
        parameters: NodeId,
        /// The [`SyntaxNodeKind::FunctionBody`] node.
        body: NodeId,
        /// Method form.
        kind: MethodKind,
        /// Whether the class method is `static`.
        is_static: bool,
        /// Whether the key is inside brackets.
        computed: bool,
    },
    /// A class field.
    FieldDefinition {
        /// Identifier name, string, number, private name, or computed key expression.
        key: NodeId,
        /// Initializer, if present.
        value: Option<NodeId>,
        /// Whether the field is `static`.
        is_static: bool,
        /// Whether the key is inside brackets.
        computed: bool,
    },
    /// A class static initialization block `static { ... }`.
    StaticBlock {
        /// Statements and declarations, in source order.
        body: ChildList,
    },
    /// An identifier reference.
    Identifier {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
        /// Reserved-word spelling after escape processing, if any, such as a
        /// sloppy-mode `let` or `yield` used as a name.
        reserved_word: Option<ReservedWord>,
    },
    /// An identifier name used as a property name, member property, or module
    /// export name.
    IdentifierName {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
        /// Reserved-word spelling after escape processing, if any.
        reserved_word: Option<ReservedWord>,
    },
    /// A private name `#name` used as a class element name, a member property,
    /// or the left operand of `in`.
    PrivateIdentifier {
        /// Whether any part of the name used a Unicode escape.
        has_escape: bool,
    },
    /// A numeric literal, retaining its spelling category without evaluation.
    NumberLiteral {
        /// Radix indicated by the literal spelling.
        base: NumericBase,
        /// Number or BigInt category.
        kind: NumericKind,
        /// Whether the literal is a sloppy-mode legacy octal-like form.
        legacy_octal: bool,
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
    /// The `super` keyword of a super property access or super call.
    Super,
    /// `new.target` or `import.meta`.
    MetaProperty {
        /// Which meta property.
        kind: MetaPropertyKind,
    },
    /// A dynamic `import(specifier)` or `import(specifier, options)` call.
    ImportCall {
        /// Specifier expression.
        source: NodeId,
        /// Options expression, if present.
        options: Option<NodeId>,
    },
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
    /// An `await` expression.
    Await {
        /// Awaited expression.
        argument: NodeId,
    },
    /// A `yield` or `yield*` expression.
    Yield {
        /// Yielded expression, if present.
        argument: Option<NodeId>,
        /// Whether the expression is `yield*`.
        delegate: bool,
    },
    /// A binary expression.
    Binary {
        /// Applied operator.
        operator: BinaryOperator,
        /// Left operand, a private name only for `#name in object`.
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
    /// A member access, including optional, computed, and private forms.
    Member {
        /// Object expression or `super`.
        object: NodeId,
        /// Property name, private name, or computed property expression.
        property: NodeId,
        /// Whether the property is inside brackets.
        computed: bool,
        /// Whether the access uses `?.`.
        optional: bool,
    },
    /// A call expression with a contiguous list of arguments.
    Call {
        /// Callee expression or `super`.
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
        /// Property, shorthand, method, and spread node IDs, in source order.
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
    /// A name declared by a binding pattern, parameter, function, class, or import.
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
        /// Identifier reference in assignment patterns, binding identifier in bindings.
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
    /// An `import` declaration.
    ImportDeclaration {
        /// Default, namespace, and named import specifiers, in source order.
        specifiers: ChildList,
        /// Module specifier string literal.
        source: NodeId,
        /// The [`SyntaxNodeKind::ImportAttributes`] node, if present.
        attributes: Option<NodeId>,
    },
    /// A default import binding `import name from ...`.
    ImportDefaultSpecifier {
        /// Binding identifier.
        local: NodeId,
    },
    /// A namespace import `* as name`.
    ImportNamespaceSpecifier {
        /// Binding identifier.
        local: NodeId,
    },
    /// A named import `name` or `exported as name`.
    ImportSpecifier {
        /// Imported identifier name or string, absent when it equals `local`.
        imported: Option<NodeId>,
        /// Binding identifier.
        local: NodeId,
    },
    /// An import attribute clause `with { key: "value" }`.
    ImportAttributes {
        /// Attributes, in source order.
        attributes: ChildList,
    },
    /// One import attribute `key: "value"`.
    ImportAttribute {
        /// Identifier name or string key.
        key: NodeId,
        /// String value.
        value: NodeId,
    },
    /// `export` followed by a variable, function, or class declaration.
    ExportDeclaration {
        /// Exported declaration.
        declaration: NodeId,
    },
    /// `export { ... }`, optionally re-exported `from` another module.
    ExportNamed {
        /// Export specifiers, in source order.
        specifiers: ChildList,
        /// Module specifier string literal, if present.
        source: Option<NodeId>,
        /// The [`SyntaxNodeKind::ImportAttributes`] node, if present.
        attributes: Option<NodeId>,
    },
    /// An export specifier `local` or `local as exported`.
    ExportSpecifier {
        /// Local identifier reference, or with `from` an identifier name or string.
        local: NodeId,
        /// Exported identifier name or string, absent when it equals `local`.
        exported: Option<NodeId>,
    },
    /// `export * from ...` or `export * as name from ...`.
    ExportAll {
        /// Exported namespace name, if present.
        exported: Option<NodeId>,
        /// Module specifier string literal.
        source: NodeId,
        /// The [`SyntaxNodeKind::ImportAttributes`] node, if present.
        attributes: Option<NodeId>,
    },
    /// `export default` followed by a function or class declaration or an expression.
    ExportDefault {
        /// Declaration or expression.
        declaration: NodeId,
    },
    /// A JSX element `<name ...>children</name>` or `<name ... />`.
    JsxElement {
        /// The [`SyntaxNodeKind::JsxOpeningElement`] node.
        opening: NodeId,
        /// Text, element, fragment, expression container, and spread child
        /// nodes, in source order. Empty for a self-closing element.
        children: ChildList,
        /// The [`SyntaxNodeKind::JsxClosingElement`] node, absent for a
        /// self-closing element.
        closing: Option<NodeId>,
    },
    /// The opening tag `<name ...>` or `<name ... />` of a JSX element.
    JsxOpeningElement {
        /// A JSX identifier, namespaced name, or member expression.
        name: NodeId,
        /// Attributes and spread attributes, in source order.
        attributes: ChildList,
        /// Whether the tag ends with `/>`.
        self_closing: bool,
    },
    /// The closing tag `</name>` of a JSX element.
    JsxClosingElement {
        /// A JSX identifier, namespaced name, or member expression with the
        /// same structure and spellings as the opening tag's name.
        name: NodeId,
    },
    /// A JSX fragment `<>children</>`.
    JsxFragment {
        /// The [`SyntaxNodeKind::JsxOpeningFragment`] node.
        opening: NodeId,
        /// Text, element, fragment, expression container, and spread child
        /// nodes, in source order.
        children: ChildList,
        /// The [`SyntaxNodeKind::JsxClosingFragment`] node.
        closing: NodeId,
    },
    /// The `<>` tag of a JSX fragment.
    JsxOpeningFragment,
    /// The `</>` tag of a JSX fragment.
    JsxClosingFragment,
    /// A JSX element or attribute name part, such as `div` or `aria-label`.
    /// Its span is its exact spelling; JSX names have no escapes.
    JsxIdentifier,
    /// A namespaced JSX name `namespace:name`.
    JsxNamespacedName {
        /// The [`SyntaxNodeKind::JsxIdentifier`] before the colon.
        namespace: NodeId,
        /// The [`SyntaxNodeKind::JsxIdentifier`] after the colon.
        name: NodeId,
    },
    /// A JSX element name `object.property`, such as `Menu.Item`.
    JsxMemberExpression {
        /// A JSX identifier or member expression.
        object: NodeId,
        /// The [`SyntaxNodeKind::JsxIdentifier`] after the dot.
        property: NodeId,
    },
    /// A JSX attribute `name`, `name="value"`, `name={expression}`, or
    /// `name=<element />`.
    JsxAttribute {
        /// A JSX identifier or namespaced name.
        name: NodeId,
        /// A JSX string, expression container, element, or fragment, if present.
        value: Option<NodeId>,
    },
    /// A JSX spread attribute `{...expression}`.
    JsxSpreadAttribute {
        /// Spread expression.
        argument: NodeId,
    },
    /// A JSX expression container `{expression}`, or a child container `{}`
    /// that holds only trivia.
    JsxExpressionContainer {
        /// Contained expression, absent for an empty child container.
        expression: Option<NodeId>,
    },
    /// A JSX spread child `{...expression}`.
    JsxSpreadChild {
        /// Spread expression.
        expression: NodeId,
    },
    /// JSX text between tags and containers, represented by its original
    /// span without decoding character references or trimming whitespace.
    JsxText,
    /// A quoted JSX attribute string, represented by its original span. JSX
    /// strings have no escapes; character references are not decoded.
    JsxStringLiteral,
}

/// One node in a syntax tree arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxNode {
    pub(crate) kind: SyntaxNodeKind,
    pub(crate) span: Span,
}

// Nodes stay compact, because the E1 memory budget counts them per source byte.
const _: () = assert!(std::mem::size_of::<SyntaxNode>() <= 40);

impl SyntaxNode {
    /// Returns the syntactic form represented by this node.
    pub const fn kind(self) -> SyntaxNodeKind {
        self.kind
    }

    /// Returns the exact half-open source span of this node.
    pub const fn span(self) -> Span {
        self.span
    }
}

/// A syntax tree backed by flat node, child-list, and template-part arenas.
///
/// Lists of statements, elements, properties, parameters, arguments, and
/// specifiers share the child arena. Patterns are reinterpreted from their
/// cover expressions in place, so every node of a returned tree is reachable
/// from the root.
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

    /// Returns the root node's arena ID.
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

    /// Returns the direct children of a node in source order, or `None` for
    /// an ID outside this tree. Template segments are not nodes and are
    /// skipped; their substitutions are included. Iteration allocates nothing.
    pub fn child_nodes(&self, id: NodeId) -> Option<ChildNodes<'_>> {
        let kind = self.node(id)?.kind;
        let list = |list: ChildList| self.children(list).unwrap_or_default();
        let mut children = ChildNodes::default();
        match kind {
            SyntaxNodeKind::TypeScript {
                children: items, ..
            } => children.list = list(items),
            SyntaxNodeKind::EmptyStatement
            | SyntaxNodeKind::DebuggerStatement
            | SyntaxNodeKind::LabelIdentifier { .. }
            | SyntaxNodeKind::Identifier { .. }
            | SyntaxNodeKind::IdentifierName { .. }
            | SyntaxNodeKind::PrivateIdentifier { .. }
            | SyntaxNodeKind::NumberLiteral { .. }
            | SyntaxNodeKind::StringLiteral
            | SyntaxNodeKind::RegularExpression { .. }
            | SyntaxNodeKind::BooleanLiteral(_)
            | SyntaxNodeKind::NullLiteral
            | SyntaxNodeKind::This
            | SyntaxNodeKind::Super
            | SyntaxNodeKind::MetaProperty { .. }
            | SyntaxNodeKind::Elision
            | SyntaxNodeKind::BindingIdentifier { .. }
            | SyntaxNodeKind::JsxOpeningFragment
            | SyntaxNodeKind::JsxClosingFragment
            | SyntaxNodeKind::JsxIdentifier
            | SyntaxNodeKind::JsxText
            | SyntaxNodeKind::JsxStringLiteral => {}
            SyntaxNodeKind::Program { body, .. }
            | SyntaxNodeKind::BlockStatement { body }
            | SyntaxNodeKind::FunctionBody { body, .. }
            | SyntaxNodeKind::StaticBlock { body } => children.list = list(body),
            SyntaxNodeKind::FormalParameters { parameters } => children.list = list(parameters),
            SyntaxNodeKind::ClassBody { elements }
            | SyntaxNodeKind::ArrayExpression { elements, .. }
            | SyntaxNodeKind::ArrayPattern { elements, .. } => children.list = list(elements),
            SyntaxNodeKind::ObjectExpression { properties, .. }
            | SyntaxNodeKind::ObjectPattern { properties, .. } => children.list = list(properties),
            SyntaxNodeKind::ImportAttributes { attributes } => children.list = list(attributes),
            SyntaxNodeKind::VariableDeclaration { declarators, .. } => {
                children.list = list(declarators);
            }
            SyntaxNodeKind::Directive { expression }
            | SyntaxNodeKind::ExpressionStatement { expression }
            | SyntaxNodeKind::Parenthesized { expression } => children.prefix[0] = Some(expression),
            SyntaxNodeKind::ThrowStatement { argument }
            | SyntaxNodeKind::Await { argument }
            | SyntaxNodeKind::Spread { argument }
            | SyntaxNodeKind::RestElement { argument } => children.prefix[0] = Some(argument),
            SyntaxNodeKind::ReturnStatement { argument }
            | SyntaxNodeKind::Yield { argument, .. } => children.prefix[0] = argument,
            SyntaxNodeKind::ContinueStatement { label }
            | SyntaxNodeKind::BreakStatement { label } => children.prefix[0] = label,
            SyntaxNodeKind::ExportDeclaration { declaration }
            | SyntaxNodeKind::ExportDefault { declaration } => {
                children.prefix[0] = Some(declaration)
            }
            SyntaxNodeKind::ImportDefaultSpecifier { local }
            | SyntaxNodeKind::ImportNamespaceSpecifier { local } => {
                children.prefix[0] = Some(local)
            }
            SyntaxNodeKind::Unary { operand, .. } | SyntaxNodeKind::Update { operand, .. } => {
                children.prefix[0] = Some(operand);
            }
            SyntaxNodeKind::ShorthandProperty { identifier } => {
                children.prefix[0] = Some(identifier);
            }
            SyntaxNodeKind::IfStatement {
                test,
                consequent,
                alternate,
            } => children.prefix = [Some(test), Some(consequent), alternate, None],
            SyntaxNodeKind::ForStatement {
                init,
                test,
                update,
                body,
            } => children.prefix = [init, test, update, Some(body)],
            SyntaxNodeKind::ForInStatement { left, right, body }
            | SyntaxNodeKind::ForOfStatement {
                left, right, body, ..
            } => children.prefix = [Some(left), Some(right), Some(body), None],
            SyntaxNodeKind::WhileStatement { test, body } => {
                children.prefix = [Some(test), Some(body), None, None];
            }
            SyntaxNodeKind::DoWhileStatement { body, test } => {
                children.prefix = [Some(body), Some(test), None, None];
            }
            SyntaxNodeKind::WithStatement { object, body } => {
                children.prefix = [Some(object), Some(body), None, None];
            }
            SyntaxNodeKind::SwitchStatement {
                discriminant,
                cases,
            } => {
                children.prefix[0] = Some(discriminant);
                children.list = list(cases);
            }
            SyntaxNodeKind::SwitchCase { test, consequent } => {
                children.prefix[0] = test;
                children.list = list(consequent);
            }
            SyntaxNodeKind::LabeledStatement { label, body } => {
                children.prefix = [Some(label), Some(body), None, None];
            }
            SyntaxNodeKind::TryStatement {
                block,
                handler,
                finalizer,
            } => children.prefix = [Some(block), handler, finalizer, None],
            SyntaxNodeKind::CatchClause { parameter, body } => {
                children.prefix = [parameter, Some(body), None, None];
            }
            SyntaxNodeKind::VariableDeclarator { target, init } => {
                children.prefix = [Some(target), init, None, None];
            }
            SyntaxNodeKind::FunctionDeclaration {
                name,
                parameters,
                body,
                ..
            }
            | SyntaxNodeKind::FunctionExpression {
                name,
                parameters,
                body,
                ..
            } => children.prefix = [name, Some(parameters), Some(body), None],
            SyntaxNodeKind::ArrowFunction {
                parameters, body, ..
            } => children.prefix = [Some(parameters), Some(body), None, None],
            SyntaxNodeKind::ClassDeclaration {
                name,
                super_class,
                body,
            }
            | SyntaxNodeKind::ClassExpression {
                name,
                super_class,
                body,
            } => children.prefix = [name, super_class, Some(body), None],
            SyntaxNodeKind::MethodDefinition {
                key,
                parameters,
                body,
                ..
            } => children.prefix = [Some(key), Some(parameters), Some(body), None],
            SyntaxNodeKind::FieldDefinition { key, value, .. } => {
                children.prefix = [Some(key), value, None, None];
            }
            SyntaxNodeKind::TemplateLiteral { parts } => {
                children.parts = self.template_parts(parts).unwrap_or_default();
            }
            SyntaxNodeKind::ImportCall { source, options } => {
                children.prefix = [Some(source), options, None, None];
            }
            SyntaxNodeKind::Binary { left, right, .. }
            | SyntaxNodeKind::Assignment {
                target: left,
                value: right,
                ..
            } => children.prefix = [Some(left), Some(right), None, None],
            SyntaxNodeKind::Conditional {
                test,
                consequent,
                alternate,
            } => children.prefix = [Some(test), Some(consequent), Some(alternate), None],
            SyntaxNodeKind::Member {
                object, property, ..
            } => children.prefix = [Some(object), Some(property), None, None],
            SyntaxNodeKind::Call {
                callee, arguments, ..
            } => {
                children.prefix[0] = Some(callee);
                children.list = list(arguments);
            }
            SyntaxNodeKind::New { callee, arguments } => {
                children.prefix[0] = Some(callee);
                children.list = arguments.map(list).unwrap_or_default();
            }
            SyntaxNodeKind::TaggedTemplate { tag, template } => {
                children.prefix = [Some(tag), Some(template), None, None];
            }
            SyntaxNodeKind::Property { key, value, .. }
            | SyntaxNodeKind::PatternProperty { key, value, .. } => {
                children.prefix = [Some(key), Some(value), None, None];
            }
            SyntaxNodeKind::ShorthandPatternProperty { target, default } => {
                children.prefix = [Some(target), default, None, None];
            }
            SyntaxNodeKind::AssignmentPattern { target, default } => {
                children.prefix = [Some(target), Some(default), None, None];
            }
            SyntaxNodeKind::ImportDeclaration {
                specifiers,
                source,
                attributes,
            } => {
                children.list = list(specifiers);
                children.suffix = [Some(source), attributes];
            }
            SyntaxNodeKind::ImportSpecifier { imported, local } => {
                children.prefix = [imported, Some(local), None, None];
            }
            SyntaxNodeKind::ImportAttribute { key, value } => {
                children.prefix = [Some(key), Some(value), None, None];
            }
            SyntaxNodeKind::ExportNamed {
                specifiers,
                source,
                attributes,
            } => {
                children.list = list(specifiers);
                children.suffix = [source, attributes];
            }
            SyntaxNodeKind::ExportSpecifier { local, exported } => {
                children.prefix = [Some(local), exported, None, None];
            }
            SyntaxNodeKind::ExportAll {
                exported,
                source,
                attributes,
            } => children.prefix = [exported, Some(source), attributes, None],
            SyntaxNodeKind::JsxElement {
                opening,
                children: elements,
                closing,
            } => {
                children.prefix[0] = Some(opening);
                children.list = list(elements);
                children.suffix[0] = closing;
            }
            SyntaxNodeKind::JsxFragment {
                opening,
                children: elements,
                closing,
            } => {
                children.prefix[0] = Some(opening);
                children.list = list(elements);
                children.suffix[0] = Some(closing);
            }
            SyntaxNodeKind::JsxOpeningElement {
                name, attributes, ..
            } => {
                children.prefix[0] = Some(name);
                children.list = list(attributes);
            }
            SyntaxNodeKind::JsxClosingElement { name } => children.prefix[0] = Some(name),
            SyntaxNodeKind::JsxNamespacedName { namespace, name } => {
                children.prefix = [Some(namespace), Some(name), None, None];
            }
            SyntaxNodeKind::JsxMemberExpression { object, property } => {
                children.prefix = [Some(object), Some(property), None, None];
            }
            SyntaxNodeKind::JsxAttribute { name, value } => {
                children.prefix = [Some(name), value, None, None];
            }
            SyntaxNodeKind::JsxSpreadAttribute {
                argument: expression,
            }
            | SyntaxNodeKind::JsxSpreadChild { expression } => {
                children.prefix[0] = Some(expression);
            }
            SyntaxNodeKind::JsxExpressionContainer { expression } => {
                children.prefix[0] = expression;
            }
        }
        Some(children)
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

/// Iterator over a node's direct children in source order, returned by
/// [`SyntaxTree::child_nodes`].
#[derive(Clone, Debug, Default)]
pub struct ChildNodes<'tree> {
    prefix: [Option<NodeId>; 4],
    prefix_index: usize,
    list: &'tree [NodeId],
    parts: &'tree [TemplatePart],
    suffix: [Option<NodeId>; 2],
    suffix_index: usize,
}

impl Iterator for ChildNodes<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        while let Some(slot) = self.prefix.get(self.prefix_index) {
            self.prefix_index += 1;
            if let Some(id) = slot {
                return Some(*id);
            }
        }
        if let Some((&first, rest)) = self.list.split_first() {
            self.list = rest;
            return Some(first);
        }
        while let Some((first, rest)) = self.parts.split_first() {
            self.parts = rest;
            match first {
                TemplatePart::Expression(id) => return Some(*id),
                TemplatePart::Segment(_) => {}
            }
        }
        while let Some(slot) = self.suffix.get(self.suffix_index) {
            self.suffix_index += 1;
            if let Some(id) = slot {
                return Some(*id);
            }
        }
        None
    }
}

fn slice_range<T>(items: &[T], start: u32, len: u32) -> Option<&[T]> {
    let start = start as usize;
    let end = start.checked_add(len as usize)?;
    items.get(start..end)
}
