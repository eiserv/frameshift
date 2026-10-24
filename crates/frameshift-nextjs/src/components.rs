//! The component graph (`D-096`): what each JSX render site renders.
//!
//! Each strict module is summarized once while its tree is alive: render
//! sites, the values of the bindings they reach, and the module's exports.
//! Linking then follows bindings, imports, re-exports, and `export *`
//! through the module graph's own edges, never by name. React's `memo`,
//! `forwardRef`, and `lazy` and the default export of `next/dynamic` are
//! wrappers only when binding provenance proves their import and the
//! import's edge reaches the package; every other call is a higher-order
//! component or factory with an explicit reason.
//! Nothing is evaluated, and no value is guessed from its spelling.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use frameshift_bindings::{
    Binding, BindingId, Bindings, DeclarationKind as K, ExportKind, ExportName, ImportedName,
    Target,
};
use frameshift_core::{SourcePath, Span};
use frameshift_modules::{FileKind, ModuleGraph, Resolution, UnresolvedReason};
use frameshift_syntax::{NodeId, SyntaxNodeKind as N, SyntaxTree, TypeScriptKind as T};

use crate::routes::{AppRoutes, Convention};

/// A wrapper that keeps the wrapped value's identity as rendering evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wrapper {
    /// `memo` from `react`.
    Memo,
    /// `forwardRef` from `react`.
    ForwardRef,
    /// `lazy` from `react`, which renders its loader's default export.
    Lazy,
    /// The default export of `next/dynamic`, with its `ssr` option.
    Dynamic(SsrOption),
}

/// The `ssr` option of a `next/dynamic` call, read only from a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SsrOption {
    /// No options argument, or an object literal without `ssr`, which
    /// Next.js treats as server rendering enabled.
    Absent,
    /// A literal `true` or `false`.
    Literal(bool),
    /// Any other options value, a computed or non-literal `ssr`, or a spread
    /// that may supply it.
    Unknown,
}

/// The syntactic form of a definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DefinitionKind {
    /// A function declaration or expression.
    Function,
    /// An arrow function.
    Arrow,
    /// A class declaration or expression.
    Class,
}

/// A local function, arrow, or class that a render site or an App Router
/// convention renders. A definition is rendering evidence: it is not proof
/// that the value is a valid component or that the callable executes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// The defining module.
    pub path: SourcePath,
    /// The function, arrow, or class node.
    pub span: Span,
    /// The syntactic form.
    pub kind: DefinitionKind,
    /// The `name` that ECMAScript assigns when it is static: the declared
    /// or own name, else the name an anonymous definition takes from the
    /// binding or `export default` that directly holds it.
    pub name: Option<String>,
}

/// Why a render site's value is not established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownReason {
    /// A name without a binding in its module, by its decoded spelling.
    Global(String),
    /// A `with` object or a sloppy direct `eval` may supply the name.
    Dynamic,
    /// A `this` member tag.
    This,
    /// A parameter, such as a component passed in props.
    Parameter,
    /// A binding that is written, or declared more than once.
    Mutable,
    /// A binding introduced by a destructuring pattern.
    Destructured,
    /// An ambient declaration or overload signature without an implementation.
    Ambient,
    /// A call or tagged template that is not a recognized wrapper: a
    /// higher-order component or factory, with its callee's static spelling.
    Call(Option<String>),
    /// A `lazy` or `next/dynamic` loader outside the recognized forms.
    Loader,
    /// A member of a value other than a module namespace, or a namespace
    /// object rendered itself.
    Member,
    /// Any other expression, such as a conditional, literal, or object.
    Value,
    /// An explicitly type-only import.
    TypeOnly,
    /// The target module has no such export, including CommonJS modules.
    NotExported,
    /// `export *` provides the name from several modules, or may provide it
    /// from one whose names are unknown, such as a package.
    AmbiguousStar,
    /// The resolver established no local target.
    Unresolved(UnresolvedReason),
    /// A `node:` built-in module.
    Builtin,
    /// The local target has no strict parse.
    Unparsed,
    /// The local target is a declaration file without an implementation.
    Declaration,
    /// The local target is a stylesheet, JSON, or other asset.
    Asset,
    /// The resolution returns to a binding or export it is already following.
    Cycle,
    /// Another form, such as an enum, namespace, Annex B function,
    /// `import =` alias, `export =` module, or a convention file outside the
    /// module graph.
    Unsupported,
}

/// What a render site or convention renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rendered {
    /// An intrinsic element: a lowercase or hyphenated name, or a namespaced name.
    Intrinsic,
    /// A local definition, by index into [`ComponentGraph::definitions`].
    Definition(usize),
    /// An export of a package, whose contents are never read.
    External {
        /// The decoded import specifier, including any subpath.
        specifier: String,
        /// The package name, as the resolver classified it.
        package: String,
        /// The imported export.
        export: ImportedName,
        /// Member names applied to the export, as in `<Ui.Button>`.
        members: Vec<String>,
    },
    /// Not established.
    Unknown {
        /// Why.
        reason: UnknownReason,
        /// The module where the reason arose.
        path: SourcePath,
        /// The node where it arose, absent for a module-level fact such as a
        /// missing export.
        span: Option<Span>,
    },
}

/// A rendered value with its wrappers, outermost first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// Recognized wrappers, outermost first.
    pub wrappers: Vec<Wrapper>,
    /// The innermost rendered value.
    pub rendered: Rendered,
}

/// One JSX opening tag and what it renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// The module.
    pub path: SourcePath,
    /// The opening-tag span.
    pub span: Span,
    /// The innermost reached definition that lexically contains the tag,
    /// by index into [`ComponentGraph::definitions`]. Tags in module-level
    /// code or in helpers outside any reached definition have none.
    pub owner: Option<usize>,
    /// What the tag renders.
    pub resolved: Resolved,
}

/// The default export of an App Router convention file that Next.js renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The convention file.
    pub path: SourcePath,
    /// Its convention.
    pub convention: Convention,
    /// What its default export renders.
    pub resolved: Resolved,
}

/// Render sites resolved to definitions, packages, or explicit reasons.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentGraph {
    /// Reached definitions, in path and source order.
    pub definitions: Vec<Definition>,
    /// Every JSX opening tag of every strict module, in path and source order.
    pub sites: Vec<Site>,
    /// Defaults of rendered conventions, in path order.
    pub entries: Vec<Entry>,
    /// Dynamic imports that only the browser loads, in path and source order.
    pub browser_imports: Vec<BrowserImport>,
}

/// The `import()` of a `next/dynamic` loader with a literal `ssr: false`.
/// Next.js neither prerenders the component nor loads the module on the
/// server, so this edge does not carry the prerendering context. The call
/// need not be rendered: the option decides where the import runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserImport {
    /// The module with the call.
    pub path: SourcePath,
    /// The specifier span of the loader's `import()`, which is the module
    /// graph edge's span.
    pub span: Span,
}

impl ComponentGraph {
    /// The sites whose innermost reached definition is `definition`.
    pub fn renders(&self, definition: usize) -> impl Iterator<Item = &Site> {
        self.sites
            .iter()
            .filter(move |site| site.owner == Some(definition))
    }
}

/// Why linking stopped without a graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentError {
    /// Linking exhausted its work budget.
    WorkLimit,
}

impl std::fmt::Display for ComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkLimit => f.write_str("component graph work limit reached"),
        }
    }
}

impl std::error::Error for ComponentError {}

/// The component facts of one strict module, collected while its tree is
/// alive and consumed by [`link_components`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentSummary {
    path: SourcePath,
    sites: Vec<(Span, Leaf)>,
    values: BTreeMap<BindingId, Value>,
    exports: Vec<ExportSummary>,
    // `next/dynamic` import and loader specifier spans of `ssr: false` calls.
    browser_loaders: Vec<(Span, Span)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Value {
    wrappers: Vec<Applied>,
    leaf: Leaf,
}

/// A wrapper call whose callee binding imports a recognized name. Linking
/// confirms that the import's edge reaches the expected package; otherwise
/// the call is a higher-order component like any other.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Applied {
    wrapper: Wrapper,
    import: Span,
    call: Span,
    callee: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Leaf {
    Intrinsic,
    Definition {
        kind: DefinitionKind,
        span: Span,
        name: Option<String>,
    },
    Binding {
        binding: BindingId,
        members: Vec<String>,
    },
    Import {
        source: Span,
        imported: ImportedName,
    },
    /// A loader's dynamic import and the export it renders.
    Loaded {
        source: Span,
        export: ExportName,
    },
    Unknown {
        reason: UnknownReason,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExportSummary {
    name: ExportName,
    type_only: bool,
    target: ExportTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExportTarget {
    Value(Value),
    ReExport {
        source: Span,
        imported: ImportedName,
    },
}

impl ComponentSummary {
    /// The summarized module.
    pub fn path(&self) -> &SourcePath {
        &self.path
    }
}

impl Value {
    fn leaf(leaf: Leaf) -> Self {
        Self {
            wrappers: Vec::new(),
            leaf,
        }
    }
}

/// Summarizes one strict module. Work is linear in its nodes and bindings.
pub fn summarize_components(
    path: &SourcePath,
    tree: &SyntaxTree<'_>,
    bindings: &Bindings,
) -> ComponentSummary {
    let mut summarizer = Summarizer {
        tree,
        bindings,
        written: bindings
            .references()
            .iter()
            .filter(|r| r.write)
            .filter_map(|r| match r.target {
                Target::Binding(b) => Some(b),
                Target::Global | Target::Dynamic => None,
            })
            .collect(),
        pending: Vec::new(),
    };
    let mut sites = Vec::new();
    let mut browser_loaders = Vec::new();
    let mut nodes = vec![tree.root()];
    while let Some(id) = nodes.pop() {
        let kind = summarizer.kind(id);
        if let N::JsxOpeningElement { name, .. } = kind {
            let leaf = summarizer.tag(name);
            sites.push((summarizer.span(id), leaf));
        } else if let N::Call {
            callee,
            arguments,
            optional: false,
        } = kind
            && let Some((Wrapper::Dynamic(_), import)) = summarizer.wrapper(callee)
            && let [loader, options, ..] = summarizer.list(arguments)
            && summarizer.ssr(Some(*options)) == SsrOption::Literal(false)
            && let Leaf::Loaded { source, .. } = summarizer.loader(*loader, false)
        {
            browser_loaders.push((import, source));
        }
        nodes.extend(tree.child_nodes(id).expect("parser-owned node"));
    }
    sites.sort_by_key(|(span, _)| *span);
    browser_loaders.sort();
    let mut exports = Vec::new();
    for export in bindings.exports() {
        let target = match &export.kind {
            ExportKind::Local(Target::Binding(binding)) => {
                ExportTarget::Value(summarizer.binding_leaf(*binding, Vec::new()))
            }
            ExportKind::Local(Target::Global) => {
                let name = match &export.name {
                    ExportName::Named(name) => name.clone(),
                    ExportName::Default | ExportName::Star | ExportName::Assignment => {
                        String::new()
                    }
                };
                ExportTarget::Value(summarizer.unknown(UnknownReason::Global(name), export.node))
            }
            ExportKind::Local(Target::Dynamic) => {
                ExportTarget::Value(summarizer.unknown(UnknownReason::Dynamic, export.node))
            }
            ExportKind::Expression(node) => {
                let inferred = (export.name == ExportName::Default).then(|| "default".to_owned());
                ExportTarget::Value(summarizer.expression(*node, inferred))
            }
            ExportKind::ReExport { source, imported } => ExportTarget::ReExport {
                source: summarizer.span(*source),
                imported: imported.clone(),
            },
        };
        exports.push(ExportSummary {
            name: export.name.clone(),
            type_only: export.type_only,
            target,
        });
    }
    let mut values = BTreeMap::new();
    while let Some(binding) = summarizer.pending.pop() {
        if values.contains_key(&binding) {
            continue;
        }
        let value = summarizer.binding(binding);
        values.insert(binding, value);
    }
    ComponentSummary {
        path: path.clone(),
        sites,
        values,
        exports,
        browser_loaders,
    }
}

struct Summarizer<'a, 't> {
    tree: &'a SyntaxTree<'t>,
    bindings: &'a Bindings,
    written: BTreeSet<BindingId>,
    // Bindings that a summarized value reaches, summarized afterwards.
    pending: Vec<BindingId>,
}

impl Summarizer<'_, '_> {
    fn kind(&self, id: NodeId) -> N {
        self.tree.node(id).expect("parser-owned node").kind()
    }

    fn span(&self, id: NodeId) -> Span {
        self.tree.node(id).expect("parser-owned node").span()
    }

    fn children(&self, id: NodeId) -> &[NodeId] {
        if let N::TypeScript { children, .. } = self.kind(id) {
            self.tree.children(children).unwrap_or_default()
        } else {
            &[]
        }
    }

    fn list(&self, list: frameshift_syntax::ChildList) -> &[NodeId] {
        self.tree.children(list).unwrap_or_default()
    }

    fn unknown(&self, reason: UnknownReason, node: NodeId) -> Value {
        Value::leaf(self.unknown_leaf(reason, node))
    }

    fn unknown_leaf(&self, reason: UnknownReason, node: NodeId) -> Leaf {
        Leaf::Unknown {
            reason,
            span: self.span(node),
        }
    }

    fn binding_leaf(&mut self, binding: BindingId, members: Vec<String>) -> Value {
        self.pending.push(binding);
        Value::leaf(Leaf::Binding { binding, members })
    }

    /// A reference's target with member names applied to it.
    fn reference(&mut self, node: NodeId, members: Vec<String>) -> Leaf {
        let Some(reference) = self.bindings.reference(node) else {
            return self.unknown_leaf(UnknownReason::Value, node);
        };
        match reference.target {
            Target::Binding(binding) => self.binding_leaf(binding, members).leaf,
            Target::Global => {
                let name = reference.name.clone();
                self.unknown_leaf(UnknownReason::Global(name), node)
            }
            Target::Dynamic => self.unknown_leaf(UnknownReason::Dynamic, node),
        }
    }

    /// A JSX tag name. Intrinsic and namespaced names have no reference;
    /// member tags reference their leftmost name.
    fn tag(&mut self, mut name: NodeId) -> Leaf {
        if let N::TypeScript {
            kind: T::Instantiation,
            ..
        } = self.kind(name)
            && let Some(&first) = self.children(name).first()
        {
            name = first;
        }
        let mut members = Vec::new();
        let mut root = name;
        while let N::JsxMemberExpression { object, property } = self.kind(root) {
            members.push(self.tree.identifier_value(property).unwrap_or_default());
            root = object;
        }
        members.reverse();
        if self.bindings.reference(root).is_some() {
            self.reference(root, members)
        } else if members.is_empty() {
            Leaf::Intrinsic
        } else {
            self.unknown_leaf(UnknownReason::This, root)
        }
    }

    /// The value of a binding, from its import or single value declaration.
    fn binding(&mut self, id: BindingId) -> Value {
        let binding = self.bindings.binding(id);
        if let Some(import) = &binding.import {
            let node = import.source;
            return if import.type_only {
                self.unknown(UnknownReason::TypeOnly, node)
            } else {
                Value::leaf(Leaf::Import {
                    source: self.span(node),
                    imported: import.imported.clone(),
                })
            };
        }
        // Interfaces, aliases, and namespace merges add types or members;
        // they do not change which value the binding holds.
        let declarations: Vec<_> = binding
            .declarations
            .iter()
            .filter(|d| {
                !matches!(
                    d.kind,
                    K::Interface | K::TypeAlias | K::TypeParameter | K::Namespace
                )
            })
            .copied()
            .collect();
        let Some(first) = declarations.first().copied() else {
            return self.unknown(UnknownReason::Unsupported, binding_node(binding));
        };
        if self.written.contains(&id) {
            return self.unknown(UnknownReason::Mutable, first.node);
        }
        let name = Some(binding.name.clone());
        if declarations.iter().all(|d| d.kind == K::Function) {
            if declarations.iter().any(|d| d.ambient) {
                return self.unknown(UnknownReason::Ambient, first.node);
            }
            let implementations: Vec<_> = declarations
                .iter()
                .filter(|d| self.has_body(d.node))
                .collect();
            return match implementations.as_slice() {
                [only] => Value::leaf(Leaf::Definition {
                    kind: DefinitionKind::Function,
                    span: self.span(only.node),
                    name,
                }),
                [] => self.unknown(UnknownReason::Ambient, first.node),
                [_, _, ..] => self.unknown(UnknownReason::Mutable, first.node),
            };
        }
        let [declaration] = declarations.as_slice() else {
            return self.unknown(UnknownReason::Mutable, first.node);
        };
        if declaration.ambient {
            return self.unknown(UnknownReason::Ambient, declaration.node);
        }
        let node = declaration.node;
        let definition = |kind| {
            Value::leaf(Leaf::Definition {
                kind,
                span: self.span(node),
                name: name.clone(),
            })
        };
        match declaration.kind {
            K::FunctionName => definition(DefinitionKind::Function),
            K::Class | K::ClassName => definition(DefinitionKind::Class),
            K::Const | K::Let | K::Var => {
                let N::VariableDeclarator { target, init } = self.kind(node) else {
                    return self.unknown(UnknownReason::Unsupported, node);
                };
                let typed = matches!(
                    self.kind(target),
                    N::TypeScript {
                        kind: T::TypedBinding { .. },
                        ..
                    }
                );
                let direct = Some(target) == declaration.name
                    || typed && self.children(target).first().copied() == declaration.name;
                if !direct {
                    return self.unknown(UnknownReason::Destructured, node);
                }
                match init {
                    Some(init) => self.expression(init, name),
                    None => self.unknown(UnknownReason::Value, node),
                }
            }
            K::Parameter => self.unknown(UnknownReason::Parameter, node),
            K::Using | K::AwaitUsing | K::CatchParameter | K::Arguments | K::Enum => {
                self.unknown(UnknownReason::Value, node)
            }
            K::Function
            | K::AnnexBFunction
            | K::Import
            | K::ImportEquals
            | K::TypeAlias
            | K::Interface
            | K::EnumMember
            | K::Namespace
            | K::TypeParameter => self.unknown(UnknownReason::Unsupported, node),
        }
    }

    /// Whether a function declaration has an implementation body rather
    /// than an overload signature.
    fn has_body(&self, function: NodeId) -> bool {
        let N::FunctionDeclaration { body, .. } = self.kind(function) else {
            return false;
        };
        let mut node = body;
        loop {
            if let N::FunctionBody { .. } = self.kind(node) {
                return true;
            }
            let generic = matches!(
                self.kind(node),
                N::TypeScript {
                    kind: T::GenericDeclaration,
                    ..
                }
            );
            match self.children(node).last() {
                Some(&last) if generic => node = last,
                Some(_) | None => return false,
            }
        }
    }

    /// Strips parentheses, erased TypeScript syntax, and declaration modifiers.
    fn unwrap(&self, mut node: NodeId) -> NodeId {
        loop {
            let kind = self.kind(node);
            let next = if let N::Parenthesized { expression } = kind {
                Some(expression)
            } else if let N::TypeScript { kind, .. } = kind {
                let children = self.children(node);
                if matches!(kind, T::NonNull | T::As | T::Satisfies | T::Instantiation) {
                    children.first().copied()
                } else if matches!(kind, T::Assertion) {
                    children.get(1).copied()
                } else if matches!(kind, T::ModifiedDeclaration) {
                    children.last().copied()
                } else {
                    None
                }
            } else {
                None
            };
            match next {
                Some(inner) => node = inner,
                None => return node,
            }
        }
    }

    /// The value of an expression as a chain of wrappers and one leaf.
    /// `inferred` is the name an anonymous definition takes when it is the
    /// expression itself, as ECMAScript's NamedEvaluation assigns it.
    fn expression(&mut self, node: NodeId, mut inferred: Option<String>) -> Value {
        let mut wrappers = Vec::new();
        let mut node = node;
        let leaf = loop {
            node = self.unwrap(node);
            let kind = self.kind(node);
            let own = |name: Option<NodeId>| name.and_then(|n| self.tree.identifier_value(n));
            let definition = |kind, name: Option<String>| Leaf::Definition {
                kind,
                span: self.span(node),
                name,
            };
            if let N::ArrowFunction { .. } = kind {
                break definition(DefinitionKind::Arrow, inferred);
            } else if let N::FunctionExpression { name, .. } | N::FunctionDeclaration { name, .. } =
                kind
            {
                break definition(DefinitionKind::Function, own(name).or(inferred));
            } else if let N::ClassExpression { name, .. } | N::ClassDeclaration { name, .. } = kind
            {
                break definition(DefinitionKind::Class, own(name).or(inferred));
            } else if let N::Identifier { .. } = kind {
                break self.reference(node, Vec::new());
            } else if let N::Member { .. } = kind {
                break self.member(node);
            } else if let N::TaggedTemplate { tag, .. } = kind {
                let tag = self.spelling(tag);
                break self.unknown_leaf(UnknownReason::Call(tag), node);
            } else if let N::Call {
                callee,
                arguments,
                optional: false,
            } = kind
            {
                let arguments = self.list(arguments).to_vec();
                let first = arguments
                    .first()
                    .copied()
                    .filter(|&a| !matches!(self.kind(a), N::Spread { .. }));
                let spelling = self.spelling(callee);
                let Some((wrapper, import)) = self.wrapper(callee) else {
                    break self.unknown_leaf(UnknownReason::Call(spelling), node);
                };
                let Some(first) = first else {
                    break self.unknown_leaf(UnknownReason::Value, node);
                };
                let mut applied = Applied {
                    wrapper,
                    import,
                    call: self.span(node),
                    callee: spelling,
                };
                match wrapper {
                    Wrapper::Memo | Wrapper::ForwardRef => {
                        wrappers.push(applied);
                        inferred = None;
                        node = first;
                    }
                    Wrapper::Lazy => {
                        wrappers.push(applied);
                        break self.loader(first, true);
                    }
                    Wrapper::Dynamic(_) => {
                        applied.wrapper = Wrapper::Dynamic(self.ssr(arguments.get(1).copied()));
                        wrappers.push(applied);
                        break self.loader(first, false);
                    }
                }
            } else {
                break self.unknown_leaf(UnknownReason::Value, node);
            }
        };
        Value { wrappers, leaf }
    }

    /// A static member chain on an identifier, such as `Ui.Button`.
    fn member(&mut self, node: NodeId) -> Leaf {
        let mut members = Vec::new();
        let mut root = node;
        while let N::Member {
            object,
            property,
            computed,
            optional: false,
        } = self.kind(root)
        {
            let name = if computed {
                self.tree.string_literal_value(self.unwrap(property))
            } else {
                self.tree.identifier_value(property)
            };
            let Some(name) = name else {
                return self.unknown_leaf(UnknownReason::Member, node);
            };
            members.push(name);
            root = self.unwrap(object);
        }
        members.reverse();
        if let N::Identifier { .. } = self.kind(root) {
            self.reference(root, members)
        } else {
            self.unknown_leaf(UnknownReason::Member, node)
        }
    }

    /// A recognized wrapper callee, by the binding provenance of its import,
    /// with the import's specifier span for linking to confirm.
    fn wrapper(&self, callee: NodeId) -> Option<(Wrapper, Span)> {
        let callee = self.unwrap(callee);
        let (binding, name) = if let N::Identifier { .. } = self.kind(callee) {
            (self.import_binding(callee)?, None)
        } else if let N::Member {
            object,
            property,
            computed: false,
            optional: false,
        } = self.kind(callee)
        {
            let object = self.unwrap(object);
            if !matches!(self.kind(object), N::Identifier { .. }) {
                return None;
            }
            (
                self.import_binding(object)?,
                Some(self.tree.identifier_value(property)?),
            )
        } else {
            return None;
        };
        let import = self.bindings.binding(binding).import.as_ref()?;
        if import.type_only {
            return None;
        }
        let react = |name: &str| match name {
            "memo" => Some(Wrapper::Memo),
            "forwardRef" => Some(Wrapper::ForwardRef),
            "lazy" => Some(Wrapper::Lazy),
            _ => None,
        };
        let wrapper = match (import.specifier.as_deref()?, &import.imported, name) {
            ("react", ImportedName::Named(imported), None) => react(imported),
            ("react", ImportedName::Default | ImportedName::Namespace, Some(name)) => react(&name),
            ("next/dynamic", ImportedName::Default, None) => {
                Some(Wrapper::Dynamic(SsrOption::Absent))
            }
            _ => None,
        }?;
        Some((wrapper, self.span(import.source)))
    }

    fn import_binding(&self, identifier: NodeId) -> Option<BindingId> {
        match self.bindings.reference(identifier)?.target {
            Target::Binding(b) if self.bindings.binding(b).import.is_some() => Some(b),
            Target::Binding(_) | Target::Global | Target::Dynamic => None,
        }
    }

    /// A static callee spelling: an identifier or a member chain on one.
    fn spelling(&self, node: NodeId) -> Option<String> {
        let mut parts = Vec::new();
        let mut node = self.unwrap(node);
        loop {
            let kind = self.kind(node);
            if let N::Identifier { .. } = kind {
                parts.push(self.tree.identifier_value(node)?);
                break;
            } else if let N::Member {
                object,
                property,
                computed: false,
                ..
            } = kind
            {
                parts.push(self.tree.identifier_value(property)?);
                node = self.unwrap(object);
            } else {
                return None;
            }
        }
        parts.reverse();
        Some(parts.join("."))
    }

    /// The decoded name of a non-computed property key.
    fn key(&self, key: NodeId) -> Option<String> {
        self.tree
            .identifier_value(key)
            .or_else(|| self.tree.string_literal_value(key))
    }

    /// The `ssr` property of a `next/dynamic` options argument. Later
    /// properties override earlier ones, so a spread or computed key after
    /// a literal makes the option unknown again.
    fn ssr(&self, options: Option<NodeId>) -> SsrOption {
        let Some(options) = options else {
            return SsrOption::Absent;
        };
        let N::ObjectExpression { properties, .. } = self.kind(self.unwrap(options)) else {
            return SsrOption::Unknown;
        };
        let mut ssr = SsrOption::Absent;
        for &property in self.list(properties) {
            let kind = self.kind(property);
            if let N::Property {
                key,
                value,
                computed: false,
            } = kind
            {
                if self.key(key).as_deref() == Some("ssr") {
                    ssr = if let N::BooleanLiteral(value) = self.kind(self.unwrap(value)) {
                        SsrOption::Literal(value)
                    } else {
                        SsrOption::Unknown
                    };
                }
            } else if let N::ShorthandProperty { identifier } = kind {
                if self.tree.identifier_value(identifier).as_deref() == Some("ssr") {
                    ssr = SsrOption::Unknown;
                }
            } else if let N::MethodDefinition {
                key,
                computed: false,
                ..
            } = kind
            {
                if self.key(key).as_deref() == Some("ssr") {
                    ssr = SsrOption::Unknown;
                }
            } else {
                ssr = SsrOption::Unknown;
            }
        }
        ssr
    }

    /// A loader: `() => import("...")`, optionally followed by
    /// `.then((m) => m.Name)` or `.then((m) => ({ default: m.Name }))`.
    /// `lazy` needs a module or a `default` object; a bare member result
    /// renders nothing there and stays a loader reason.
    fn loader(&self, loader: NodeId, lazy: bool) -> Leaf {
        let unknown = || self.unknown_leaf(UnknownReason::Loader, loader);
        let Some(body) = self.arrow_result(loader) else {
            return unknown();
        };
        let body = self.unwrap_await(body);
        let kind = self.kind(body);
        if let N::ImportCall { source, .. } = kind {
            return Leaf::Loaded {
                source: self.span(source),
                export: ExportName::Default,
            };
        }
        let N::Call {
            callee, arguments, ..
        } = kind
        else {
            return unknown();
        };
        let N::Member {
            object,
            property,
            computed: false,
            ..
        } = self.kind(self.unwrap(callee))
        else {
            return unknown();
        };
        let N::ImportCall { source, .. } = self.kind(self.unwrap(object)) else {
            return unknown();
        };
        let [callback] = self.list(arguments) else {
            return unknown();
        };
        if self.tree.identifier_value(property).as_deref() != Some("then") {
            return unknown();
        }
        match self.then_export(*callback, lazy) {
            Some(export) => Leaf::Loaded {
                source: self.span(source),
                export,
            },
            None => unknown(),
        }
    }

    fn unwrap_await(&self, mut node: NodeId) -> NodeId {
        loop {
            node = self.unwrap(node);
            let N::Await { argument } = self.kind(node) else {
                return node;
            };
            node = argument;
        }
    }

    /// The returned expression of an arrow with an expression body or a
    /// body of one `return` statement.
    fn arrow_result(&self, node: NodeId) -> Option<NodeId> {
        let N::ArrowFunction { body, .. } = self.kind(self.unwrap(node)) else {
            return None;
        };
        let N::FunctionBody {
            body: statements, ..
        } = self.kind(body)
        else {
            return Some(body);
        };
        let [statement] = self.list(statements) else {
            return None;
        };
        let N::ReturnStatement { argument } = self.kind(*statement) else {
            return None;
        };
        argument
    }

    /// The export that a `then` callback selects from the module namespace.
    fn then_export(&self, callback: NodeId, lazy: bool) -> Option<ExportName> {
        let N::ArrowFunction { parameters, .. } = self.kind(self.unwrap(callback)) else {
            return None;
        };
        let N::FormalParameters { parameters } = self.kind(parameters) else {
            return None;
        };
        let &[mut parameter] = self.list(parameters) else {
            return None;
        };
        if matches!(
            self.kind(parameter),
            N::TypeScript {
                kind: T::TypedBinding { .. } | T::Parameter { .. },
                ..
            }
        ) {
            parameter = *self.children(parameter).first()?;
        }
        let module = self.bindings.declared(parameter)?;
        let result = self.unwrap(self.arrow_result(callback)?);
        let selected = |node: NodeId| -> Option<ExportName> {
            let N::Member {
                object,
                property,
                computed: false,
                ..
            } = self.kind(self.unwrap(node))
            else {
                return None;
            };
            if self.bindings.reference(self.unwrap(object))?.target != Target::Binding(module) {
                return None;
            }
            let name = self.tree.identifier_value(property)?;
            Some(if name == "default" {
                ExportName::Default
            } else {
                ExportName::Named(name)
            })
        };
        let kind = self.kind(result);
        if let N::Member { .. } = kind {
            return if lazy { None } else { selected(result) };
        }
        let N::ObjectExpression { properties, .. } = kind else {
            return None;
        };
        let [property] = self.list(properties) else {
            return None;
        };
        let N::Property {
            key,
            value,
            computed: false,
        } = self.kind(*property)
        else {
            return None;
        };
        if self.key(key)? == "default" {
            selected(value)
        } else {
            None
        }
    }
}

fn binding_node(binding: &Binding) -> NodeId {
    binding
        .declarations
        .first()
        .map(|d| d.node)
        .expect("every binding has a declaration")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Binding(usize, BindingId),
    Export(usize, ExportName),
}

#[derive(Clone, Debug)]
enum Step {
    Leaf(Leaf),
    Export(ExportName),
}

/// A rendered value before definition indices are assigned.
#[derive(Clone, Debug)]
enum Linked {
    Final(Rendered),
    Definition(usize, Span),
}

/// A site's module, opening-tag span, wrappers, and linked value.
type LinkedSite = (usize, Span, (Vec<Wrapper>, Linked));

/// Where an edge leads.
enum EdgeTarget {
    /// A strict local module, by summary index.
    Module(usize),
    /// A package's specifier and name.
    Package(String, String),
    /// Resolution ends here.
    End(Linked),
}

struct Linker<'a> {
    graph: &'a ModuleGraph,
    summaries: &'a [ComponentSummary],
    work: u64,
    cache: BTreeMap<Key, (Vec<Wrapper>, Linked)>,
    definitions: BTreeMap<(usize, Span), Definition>,
}

/// Links the summaries of one package's strict modules, which must be in
/// path order, into a component graph. Each step, `export *` module, and
/// star export charges one unit of `max_work`; exhaustion returns no
/// partial graph. Results are cached per binding and export, so work is
/// linear in the summarized facts apart from `export *` searches.
pub fn link_components(
    graph: &ModuleGraph,
    routes: &AppRoutes,
    summaries: &[ComponentSummary],
    max_work: u64,
) -> Result<ComponentGraph, ComponentError> {
    let mut linker = Linker {
        graph,
        summaries,
        work: max_work,
        cache: BTreeMap::new(),
        definitions: BTreeMap::new(),
    };
    let mut sites = Vec::new();
    for (module, summary) in summaries.iter().enumerate() {
        for (span, leaf) in &summary.sites {
            let resolved = linker.resolve(module, Step::Leaf(leaf.clone()))?;
            sites.push((module, *span, resolved));
        }
    }
    let mut browser_imports = Vec::new();
    for (module, summary) in summaries.iter().enumerate() {
        for &(import, source) in &summary.browser_loaders {
            linker.charge()?;
            if matches!(
                linker.edge(module, import),
                EdgeTarget::Package(s, p) if s == "next/dynamic" && p == "next"
            ) {
                browser_imports.push(BrowserImport {
                    path: summary.path.clone(),
                    span: source,
                });
            }
        }
    }
    let mut entries = Vec::new();
    for file in &routes.files {
        if !renders(file.convention) {
            continue;
        }
        let resolved = match linker.module(&file.path) {
            Some(module) => linker.resolve(module, Step::Export(ExportName::Default))?,
            None => (
                Vec::new(),
                Linked::Final(Rendered::Unknown {
                    reason: if graph.node(&file.path).is_some() {
                        UnknownReason::Unparsed
                    } else {
                        UnknownReason::Unsupported
                    },
                    path: file.path.clone(),
                    span: None,
                }),
            ),
        };
        entries.push((file.path.clone(), file.convention, resolved));
    }
    // Outer definitions first when several start together.
    let mut keys: Vec<(usize, Span)> = linker.definitions.keys().copied().collect();
    keys.sort_by_key(|(module, span)| (*module, span.start(), std::cmp::Reverse(span.end())));
    let index: BTreeMap<(usize, Span), usize> = linker
        .definitions
        .keys()
        .enumerate()
        .map(|(i, key)| (*key, i))
        .collect();
    let owners = owners(&keys, &sites);
    let finish = |(wrappers, linked): (Vec<Wrapper>, Linked)| Resolved {
        wrappers,
        rendered: match linked {
            Linked::Final(rendered) => rendered,
            Linked::Definition(module, span) => Rendered::Definition(index[&(module, span)]),
        },
    };
    Ok(ComponentGraph {
        browser_imports,
        sites: sites
            .into_iter()
            .zip(owners)
            .map(|((module, span, resolved), owner)| Site {
                path: summaries[module].path.clone(),
                span,
                owner: owner.map(|key| index[&key]),
                resolved: finish(resolved),
            })
            .collect(),
        entries: entries
            .into_iter()
            .map(|(path, convention, resolved)| Entry {
                path,
                convention,
                resolved: finish(resolved),
            })
            .collect(),
        definitions: linker.definitions.into_values().collect(),
    })
}

/// The innermost definition containing each site. Definitions of one tree
/// nest or are disjoint, so one sweep over both lists, in module and start
/// order with outer definitions first, keeps the open ones on a stack.
fn owners(definitions: &[(usize, Span)], sites: &[LinkedSite]) -> Vec<Option<(usize, Span)>> {
    let contains = |outer: &(usize, Span), module: usize, inner: Span| {
        outer.0 == module && outer.1.start() <= inner.start() && inner.end() <= outer.1.end()
    };
    let mut owners = Vec::with_capacity(sites.len());
    let mut stack: Vec<(usize, Span)> = Vec::new();
    let mut next = definitions.iter().peekable();
    for &(module, span, _) in sites {
        while let Some(&&definition) = next.peek()
            && (definition.0, definition.1.start()) <= (module, span.start())
        {
            next.next();
            while stack
                .last()
                .is_some_and(|open| !contains(open, definition.0, definition.1))
            {
                stack.pop();
            }
            stack.push(definition);
        }
        // A definition that does not contain this site ends before it, and
        // before every later site.
        while stack
            .last()
            .is_some_and(|open| !contains(open, module, span))
        {
            stack.pop();
        }
        owners.push(stack.last().copied());
    }
    owners
}

/// Conventions whose default export Next.js renders as a component.
fn renders(convention: Convention) -> bool {
    match convention {
        Convention::Page
        | Convention::Layout
        | Convention::Template
        | Convention::Loading
        | Convention::Error
        | Convention::NotFound
        | Convention::Forbidden
        | Convention::Unauthorized
        | Convention::Default
        | Convention::GlobalError
        | Convention::GlobalNotFound => true,
        Convention::Route
        | Convention::Metadata(_)
        | Convention::Middleware
        | Convention::Proxy
        | Convention::Instrumentation => false,
    }
}

impl Linker<'_> {
    fn module(&self, path: &SourcePath) -> Option<usize> {
        self.summaries
            .binary_search_by(|summary| summary.path.cmp(path))
            .ok()
    }

    fn charge(&mut self) -> Result<(), ComponentError> {
        self.work = self.work.checked_sub(1).ok_or(ComponentError::WorkLimit)?;
        Ok(())
    }

    fn unknown(&self, module: usize, reason: UnknownReason, span: Option<Span>) -> Linked {
        Linked::Final(Rendered::Unknown {
            reason,
            path: self.summaries[module].path.clone(),
            span,
        })
    }

    /// Confirms each wrapper's import edge, outermost first, and appends the
    /// confirmed ones. The first unconfirmed wrapper ends resolution as the
    /// call it is: its import does not reach the expected package.
    fn apply(
        &self,
        module: usize,
        applied: &[Applied],
        wrappers: &mut Vec<Wrapper>,
    ) -> Option<Linked> {
        for a in applied {
            let (specifier, expected) = match a.wrapper {
                Wrapper::Memo | Wrapper::ForwardRef | Wrapper::Lazy => ("react", "react"),
                Wrapper::Dynamic(_) => ("next/dynamic", "next"),
            };
            let confirmed = matches!(
                self.edge(module, a.import),
                EdgeTarget::Package(s, p) if s == specifier && p == expected
            );
            if !confirmed {
                return Some(self.unknown(
                    module,
                    UnknownReason::Call(a.callee.clone()),
                    Some(a.call),
                ));
            }
            wrappers.push(a.wrapper);
        }
        None
    }

    /// Follows one value to its rendered target iteratively. Member names
    /// wait in `members` until a module namespace consumes them.
    fn resolve(
        &mut self,
        mut module: usize,
        mut step: Step,
    ) -> Result<(Vec<Wrapper>, Linked), ComponentError> {
        let mut wrappers = Vec::new();
        let mut members: VecDeque<String> = VecDeque::new();
        // Cacheable states with the number of wrappers outside them.
        let mut chain: Vec<(Key, usize)> = Vec::new();
        // A state that recurs with as many pending members made no progress.
        let mut active: BTreeSet<(Key, usize)> = BTreeSet::new();
        let linked = loop {
            self.charge()?;
            let state = if let Step::Leaf(Leaf::Binding {
                binding,
                members: own,
            }) = &step
            {
                Some((Key::Binding(module, *binding), own.len()))
            } else if let Step::Export(name) = &step {
                Some((Key::Export(module, name.clone()), 0))
            } else {
                None
            };
            if let Some((key, own)) = state {
                let pending = members.len() + own;
                if !active.insert((key.clone(), pending)) {
                    break self.unknown(module, UnknownReason::Cycle, None);
                }
                if pending == 0 {
                    if let Some((more, linked)) = self.cache.get(&key) {
                        wrappers.extend(more.iter().copied());
                        break linked.clone();
                    }
                    chain.push((key, wrappers.len()));
                }
            }
            match step {
                Step::Leaf(Leaf::Intrinsic) => break Linked::Final(Rendered::Intrinsic),
                Step::Leaf(Leaf::Unknown { reason, span }) => {
                    break self.unknown(module, reason, Some(span));
                }
                Step::Leaf(Leaf::Definition { kind, span, name }) => {
                    if !members.is_empty() {
                        break self.unknown(module, UnknownReason::Member, Some(span));
                    }
                    let path = &self.summaries[module].path;
                    self.definitions
                        .entry((module, span))
                        .or_insert_with(|| Definition {
                            path: path.clone(),
                            span,
                            kind,
                            name,
                        });
                    break Linked::Definition(module, span);
                }
                Step::Leaf(Leaf::Binding {
                    binding,
                    members: own,
                }) => {
                    for member in own.into_iter().rev() {
                        members.push_front(member);
                    }
                    let Some(value) = self.summaries[module].values.get(&binding) else {
                        break self.unknown(module, UnknownReason::Unsupported, None);
                    };
                    if !members.is_empty() && !value.wrappers.is_empty() {
                        break self.unknown(module, UnknownReason::Member, None);
                    }
                    if let Some(linked) = self.apply(module, &value.wrappers, &mut wrappers) {
                        break linked;
                    }
                    step = Step::Leaf(value.leaf.clone());
                }
                Step::Leaf(Leaf::Import { source, imported }) => match self.edge(module, source) {
                    EdgeTarget::End(linked) => break linked,
                    EdgeTarget::Package(specifier, package) => {
                        break Linked::Final(Rendered::External {
                            specifier,
                            package,
                            export: imported,
                            members: members.into_iter().collect(),
                        });
                    }
                    EdgeTarget::Module(target) => {
                        module = target;
                        step = match imported {
                            ImportedName::Default => Step::Export(ExportName::Default),
                            ImportedName::Named(name) => Step::Export(ExportName::Named(name)),
                            ImportedName::Namespace => match members.pop_front() {
                                Some(name) => Step::Export(ExportName::Named(name)),
                                None => {
                                    break self.unknown(module, UnknownReason::Member, None);
                                }
                            },
                            ImportedName::Require => {
                                break self.unknown(module, UnknownReason::Unsupported, None);
                            }
                        };
                    }
                },
                Step::Leaf(Leaf::Loaded { source, export }) => {
                    if !members.is_empty() {
                        break self.unknown(module, UnknownReason::Member, Some(source));
                    }
                    match self.edge(module, source) {
                        EdgeTarget::End(linked) => break linked,
                        EdgeTarget::Package(specifier, package) => {
                            let export = match export {
                                ExportName::Named(name) => ImportedName::Named(name),
                                ExportName::Default | ExportName::Star | ExportName::Assignment => {
                                    ImportedName::Default
                                }
                            };
                            break Linked::Final(Rendered::External {
                                specifier,
                                package,
                                export,
                                members: Vec::new(),
                            });
                        }
                        EdgeTarget::Module(target) => {
                            module = target;
                            step = Step::Export(export);
                        }
                    }
                }
                Step::Export(name) => match self.export(module, &name)? {
                    Ok((target, export)) => {
                        module = target;
                        match &self.summaries[module].exports[export].target {
                            ExportTarget::Value(value) => {
                                if !members.is_empty() && !value.wrappers.is_empty() {
                                    break self.unknown(module, UnknownReason::Member, None);
                                }
                                if let Some(linked) =
                                    self.apply(module, &value.wrappers, &mut wrappers)
                                {
                                    break linked;
                                }
                                step = Step::Leaf(value.leaf.clone());
                            }
                            ExportTarget::ReExport { source, imported } => {
                                step = Step::Leaf(Leaf::Import {
                                    source: *source,
                                    imported: imported.clone(),
                                });
                            }
                        }
                    }
                    Err(reason) => break self.unknown(module, reason, None),
                },
            }
        };
        for (key, outside) in chain {
            self.cache
                .insert(key, (wrappers[outside..].to_vec(), linked.clone()));
        }
        Ok((wrappers, linked))
    }

    /// The runtime edge whose specifier span is `source`.
    fn edge(&self, module: usize, source: Span) -> EdgeTarget {
        let path = &self.summaries[module].path;
        // Edges are in source order, so a binary search finds the span.
        let edge = self.graph.node(path).and_then(|node| {
            let first = node
                .edges
                .partition_point(|e| e.span.start() < source.start());
            node.edges[first..]
                .iter()
                .take_while(|e| e.span.start() == source.start())
                .find(|edge| edge.span == source && !edge.type_only)
        });
        let Some(edge) = edge else {
            return EdgeTarget::End(self.unknown(module, UnknownReason::Unsupported, Some(source)));
        };
        let at = |reason, path: &SourcePath| {
            EdgeTarget::End(Linked::Final(Rendered::Unknown {
                reason,
                path: path.clone(),
                span: None,
            }))
        };
        match &edge.resolution {
            Resolution::Local { path, kind } => match kind {
                FileKind::Module => match self.module(path) {
                    Some(target) => EdgeTarget::Module(target),
                    None => at(UnknownReason::Unparsed, path),
                },
                FileKind::Declaration => at(UnknownReason::Declaration, path),
                FileKind::Stylesheet | FileKind::Json | FileKind::Asset => {
                    at(UnknownReason::Asset, path)
                }
            },
            Resolution::External { package } => {
                EdgeTarget::Package(edge.specifier.clone().unwrap_or_default(), package.clone())
            }
            Resolution::NodeBuiltin => {
                EdgeTarget::End(self.unknown(module, UnknownReason::Builtin, Some(source)))
            }
            Resolution::Unresolved(reason) => EdgeTarget::End(self.unknown(
                module,
                UnknownReason::Unresolved(*reason),
                Some(source),
            )),
        }
    }

    /// The module and export index that provide a value export, following
    /// `export *` as ECMAScript's ResolveExport does: an explicit export
    /// wins, and star exports never provide `default`. More than one
    /// providing module, or a star export into a package or an unanalyzed
    /// module, is ambiguous here, because the other module's names are unknown.
    fn export(
        &mut self,
        module: usize,
        name: &ExportName,
    ) -> Result<Result<(usize, usize), UnknownReason>, ComponentError> {
        if let Some(index) = self.explicit(module, name) {
            return Ok(Ok((module, index)));
        }
        if self.summaries[module]
            .exports
            .iter()
            .any(|e| e.name == ExportName::Assignment)
        {
            return Ok(Err(UnknownReason::Unsupported));
        }
        if !matches!(name, ExportName::Named(_)) {
            return Ok(Err(UnknownReason::NotExported));
        }
        let mut providers = Vec::new();
        let mut opaque = false;
        let mut seen = BTreeSet::from([module]);
        let mut queue = VecDeque::from([module]);
        while let Some(current) = queue.pop_front() {
            self.charge()?;
            let stars: Vec<Span> = self.summaries[current]
                .exports
                .iter()
                .filter(|e| e.name == ExportName::Star && !e.type_only)
                .filter_map(|e| match e.target {
                    ExportTarget::ReExport { source, .. } => Some(source),
                    ExportTarget::Value(_) => None,
                })
                .collect();
            for source in stars {
                self.charge()?;
                let EdgeTarget::Module(target) = self.edge(current, source) else {
                    opaque = true;
                    continue;
                };
                if let Some(index) = self.explicit(target, name) {
                    if !providers.contains(&(target, index)) {
                        providers.push((target, index));
                    }
                } else if seen.insert(target) {
                    queue.push_back(target);
                }
            }
        }
        Ok(match providers.as_slice() {
            [only] if !opaque => Ok(*only),
            [] if !opaque => Err(UnknownReason::NotExported),
            [..] => Err(UnknownReason::AmbiguousStar),
        })
    }

    fn explicit(&self, module: usize, name: &ExportName) -> Option<usize> {
        self.summaries[module]
            .exports
            .iter()
            .position(|e| e.name == *name && !e.type_only)
    }
}
