//! Framework-independent scopes, bindings, references, imports, and exports
//! of one strict syntax tree.
//!
//! Analysis follows ECMAScript scoping, including sloppy-mode Annex B block
//! functions, and TypeScript's separate value, type, and namespace meanings
//! with declaration merging. It reads only the tree: it never resolves
//! modules, evaluates code, or decides what a binding's value is. A `with`
//! statement or a sloppy direct `eval` makes the references it can capture
//! [`Target::Dynamic`] rather than guessing their binding.

mod binder;

use std::fmt;

use frameshift_syntax::{NodeId, SyntaxTree};

/// Index of a scope in [`Bindings::scopes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeId(u32);

impl ScopeId {
    /// Zero-based index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of a binding in [`Bindings::bindings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingId(u32);

impl BindingId {
    /// Zero-based index.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The construct that introduces a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// A Script's top level.
    Script,
    /// A Module's top level.
    Module,
    /// A function, method, accessor, or arrow function, with its parameters.
    Function {
        /// Whether it is an arrow function, which has no `arguments`.
        arrow: bool,
    },
    /// The scope that binds a named function expression's own name.
    FunctionName,
    /// A class, holding a class expression's name and type parameters.
    Class,
    /// A class static initialization block.
    StaticBlock,
    /// A block, loop head, `switch` case block, or Annex B `if` clause.
    Block,
    /// A `catch` clause parameter.
    Catch,
    /// The body of a `with` statement, whose names may be object properties.
    With,
    /// A TypeScript namespace body.
    Namespace,
    /// A TypeScript ambient module body, `declare module "name" { ... }`.
    AmbientModule,
    /// A TypeScript `declare global { ... }` body.
    GlobalAugmentation,
    /// A TypeScript enum body, where members are visible by name.
    Enum,
    /// Type parameters of an interface, type alias, signature, or mapped
    /// type, or the `infer` declarations of a conditional type.
    Type,
}

/// One scope, in a tree rooted at the program scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// What introduces the scope.
    pub kind: ScopeKind,
    /// The introducing node.
    pub node: NodeId,
    /// The enclosing scope; `None` only for the program scope.
    pub parent: Option<ScopeId>,
    /// Whether the scope's code is strict.
    pub strict: bool,
    /// Whether a sloppy direct `eval` call can add `var` bindings here.
    pub direct_eval: bool,
}

/// Which declaration spaces a binding or reference uses, as TypeScript's
/// value, type, and namespace meanings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Meaning(u8);

impl Meaning {
    /// Values: variables, functions, classes, enums, instantiated namespaces.
    pub const VALUE: Self = Self(1);
    /// Types: classes, interfaces, aliases, enums, type parameters.
    pub const TYPE: Self = Self(2);
    /// Namespaces: namespaces and enums, as qualified-name prefixes.
    pub const NAMESPACE: Self = Self(4);
    /// All meanings, as imports and export specifiers use.
    pub const ALL: Self = Self(7);

    /// Whether the two meanings share a space.
    pub const fn overlaps(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// The union of two meanings.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// How a declaration introduces its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationKind {
    /// `var`.
    Var,
    /// `let`.
    Let,
    /// `const`.
    Const,
    /// `using`.
    Using,
    /// `await using`.
    AwaitUsing,
    /// A function declaration, including TypeScript overload signatures.
    Function,
    /// The function-scoped `var` binding that Annex B adds for a sloppy-mode
    /// block-level function declaration.
    AnnexBFunction,
    /// A class declaration.
    Class,
    /// A function, method, or signature parameter.
    Parameter,
    /// A `catch` clause parameter.
    CatchParameter,
    /// A named function expression's own name.
    FunctionName,
    /// A named class expression's own name.
    ClassName,
    /// An `import` declaration specifier.
    Import,
    /// A TypeScript `import name = ...` declaration.
    ImportEquals,
    /// A TypeScript type alias.
    TypeAlias,
    /// A TypeScript interface.
    Interface,
    /// A TypeScript enum.
    Enum,
    /// A TypeScript enum member.
    EnumMember,
    /// A TypeScript namespace or identifier-named module.
    Namespace,
    /// A TypeScript type parameter, including `infer` and mapped keys.
    TypeParameter,
    /// The implicit `arguments` object of a non-arrow function.
    Arguments,
}

/// One declaration of a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Declaration {
    /// How the name is introduced.
    pub kind: DeclarationKind,
    /// The declaring name node; absent for implicit `arguments`.
    pub name: Option<NodeId>,
    /// The declaring construct: a declarator, function, class, parameter,
    /// specifier, or TypeScript declaration.
    pub node: NodeId,
    /// Whether the declaration is ambient (`declare`, or a declaration file).
    pub ambient: bool,
}

/// What an import binding refers to in its source module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportedName {
    /// The default export, including `{ default as name }`.
    Default,
    /// A named export, by its decoded name.
    Named(String),
    /// The module namespace object, `* as name`.
    Namespace,
    /// TypeScript `import name = require("...")`.
    Require,
}

/// The source of an import binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    /// The string literal node of the module specifier.
    pub source: NodeId,
    /// The decoded specifier, absent when it is not representable.
    pub specifier: Option<String>,
    /// The imported name.
    pub imported: ImportedName,
    /// Whether the import is explicitly type-only.
    pub type_only: bool,
}

/// A name in one scope, with every declaration that it merges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The decoded StringValue of the name.
    pub name: String,
    /// The scope that holds the binding, or for namespace exports and enum
    /// members the scope of the containing declaration's body.
    pub scope: ScopeId,
    /// The union of the declarations' meanings.
    pub meaning: Meaning,
    /// Declarations in source order.
    pub declarations: Vec<Declaration>,
    /// The import source, for import bindings.
    pub import: Option<Import>,
    /// For a namespace export or an enum member, the containing namespace
    /// or enum binding.
    pub container: Option<BindingId>,
}

/// What a reference resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A binding of this tree.
    Binding(BindingId),
    /// No binding of this tree: a global, an ambient, or an undeclared name.
    Global,
    /// A `with` object or a sloppy direct `eval` may supply the name.
    Dynamic,
}

/// One identifier reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// The referencing node: an identifier, identifier name in a type, or
    /// JSX identifier.
    pub node: NodeId,
    /// The decoded name.
    pub name: String,
    /// The meaning the position requires.
    pub meaning: Meaning,
    /// Whether the reference reads the binding.
    pub read: bool,
    /// Whether the reference assigns the binding.
    pub write: bool,
    /// The resolution.
    pub target: Target,
}

/// The exported name of an export.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExportName {
    /// `default`, including `{ x as default }`.
    Default,
    /// A named export, by its decoded name.
    Named(String),
    /// `export * from "..."`, which has no name of its own.
    Star,
    /// TypeScript `export = expression`.
    Assignment,
}

/// What an export provides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportKind {
    /// A local declaration or an export specifier's local reference.
    Local(Target),
    /// An anonymous default export or `export =` expression.
    Expression(NodeId),
    /// A re-export of a name, or with `ImportedName::Namespace` of the
    /// namespace, of another module.
    ReExport {
        /// The string literal node of the module specifier.
        source: NodeId,
        /// The re-exported name.
        imported: ImportedName,
    },
}

/// One export of a module, in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    /// The exported name.
    pub name: ExportName,
    /// What the export provides.
    pub kind: ExportKind,
    /// The export declaration or specifier node.
    pub node: NodeId,
    /// Whether the export is explicitly type-only.
    pub type_only: bool,
}

/// Scopes, bindings, references, and exports of one syntax tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    references: Vec<Reference>,
    exports: Vec<Export>,
    // Sorted by node for binary search.
    declared: Vec<(NodeId, BindingId)>,
}

impl Bindings {
    /// Scopes in creation order; index 0 is the program scope.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// One scope.
    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.index()]
    }

    /// Bindings in creation order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// One binding.
    pub fn binding(&self, id: BindingId) -> &Binding {
        &self.bindings[id.index()]
    }

    /// References sorted by node index; spans give their source order.
    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    /// The reference at a node, if the node is one.
    pub fn reference(&self, node: NodeId) -> Option<&Reference> {
        self.references
            .binary_search_by_key(&node, |r| r.node)
            .ok()
            .map(|i| &self.references[i])
    }

    /// The binding that a declaring name node introduces.
    pub fn declared(&self, node: NodeId) -> Option<BindingId> {
        self.declared
            .binary_search_by_key(&node, |(n, _)| *n)
            .ok()
            .map(|i| self.declared[i].1)
    }

    /// Exports in source order; empty for scripts.
    pub fn exports(&self) -> &[Export] {
        &self.exports
    }

    /// The references that resolve to a binding, in source order.
    pub fn references_to(&self, binding: BindingId) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(move |r| r.target == Target::Binding(binding))
    }
}

/// Why analysis stopped, without partial results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// Storage could not be reserved.
    AllocationFailed,
    /// More scopes, bindings, or references than a 32-bit index holds.
    TooLarge,
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AllocationFailed => "binding analysis allocation failed",
            Self::TooLarge => "binding analysis exceeds its index range",
        })
    }
}

impl std::error::Error for BindingError {}

/// Analyzes one strict syntax tree.
///
/// Work is linear in the tree's nodes, with name lookups along the scope
/// chain, whose depth the parser's nesting bound limits. The tree's own
/// [`frameshift_syntax::ParseLimits`] therefore bound the analysis; no
/// separate limit applies.
pub fn analyze(tree: &SyntaxTree<'_>) -> Result<Bindings, BindingError> {
    binder::Binder::new(tree).run()
}
