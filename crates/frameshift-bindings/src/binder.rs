//! One iterative declaration pass over the tree, then Annex B hoisting,
//! direct-`eval` marking, and reference resolution. Declarations are known
//! before any reference resolves, so hoisting needs no second traversal.

use std::collections::BTreeMap;

use frameshift_syntax::{
    AssignmentOperator, JsDialect, NodeId, SyntaxNodeKind as N, SyntaxTree, TypeScriptKind as T,
    VariableKind,
};

use crate::{
    Binding, BindingError, BindingId, Bindings, Declaration, DeclarationKind as K, Export,
    ExportKind, ExportName, Import, ImportedName, Meaning, Reference, Scope, ScopeId, ScopeKind,
    Target,
};

/// What an identifier in the current position means.
#[derive(Clone, Copy)]
enum Mode {
    /// An expression: identifiers are value reads.
    Value,
    /// An assignment target: identifiers are writes, and also reads for
    /// compound and update operators.
    Write { read: bool },
    /// A type: identifiers are type references.
    Type,
    /// A binding position.
    Declare(Decl),
}

/// How an export marks the declaration it wraps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exported {
    No,
    Named,
    Default,
}

#[derive(Clone, Copy)]
enum ImportSpec {
    No,
    Default(NodeId),
    Namespace(NodeId),
    Named { source: NodeId, imported: NodeId },
    Require(NodeId),
}

#[derive(Clone, Copy)]
struct Decl {
    kind: K,
    scope: ScopeId,
    node: NodeId,
    exported: Exported,
    import: ImportSpec,
    /// Declares into this container's member table instead of `scope`.
    container: Option<BindingId>,
}

#[derive(Clone, Copy)]
struct Ctx {
    scope: ScopeId,
    var_scope: ScopeId,
    strict: bool,
    ambient: bool,
    type_only: bool,
    exported: Exported,
    mode: Mode,
    /// The conditional-type scope that receives `infer` declarations.
    infer: Option<ScopeId>,
    /// The class scope of an enclosing static member, whose type
    /// parameters static code cannot reference.
    static_class: Option<ScopeId>,
    /// The function scope whose parameter list or return type encloses the
    /// position, where only parameters and type parameters are visible.
    signature: Option<ScopeId>,
}

struct ScopeData {
    scope: Scope,
    names: BTreeMap<String, BindingId>,
    /// The namespace or enum whose members are visible in this body, or
    /// the interface or class whose merged type parameters it shares.
    container: Option<BindingId>,
    /// Type parameters that this declaration itself declares; a merged
    /// declaration sees only its own.
    own: Vec<BindingId>,
}

struct Pending {
    node: NodeId,
    name: String,
    meaning: Meaning,
    read: bool,
    write: bool,
    scope: ScopeId,
    static_class: Option<ScopeId>,
    signature: Option<ScopeId>,
}

enum PendingExport {
    Done(Export),
    /// An export specifier's local reference, by pending index.
    Specifier {
        name: ExportName,
        reference: usize,
        node: NodeId,
        type_only: bool,
    },
}

struct AnnexB {
    name: String,
    block: ScopeId,
    var_scope: ScopeId,
    declaration: Declaration,
    name_node: NodeId,
}

pub(crate) struct Binder<'t, 's> {
    tree: &'t SyntaxTree<'s>,
    scopes: Vec<ScopeData>,
    bindings: Vec<Binding>,
    members: BTreeMap<BindingId, BTreeMap<String, BindingId>>,
    pending: Vec<Pending>,
    exports: Vec<PendingExport>,
    declared: Vec<(NodeId, BindingId)>,
    annex_b: Vec<AnnexB>,
    /// Callee references of possible direct `eval` calls in sloppy code,
    /// with the variable scope they can extend.
    evals: Vec<(usize, ScopeId)>,
    stack: Vec<(NodeId, Ctx)>,
}

fn reserve<T>(vec: &mut Vec<T>) -> Result<(), BindingError> {
    if vec.len() >= u32::MAX as usize {
        return Err(BindingError::TooLarge);
    }
    vec.try_reserve(1)
        .map_err(|_| BindingError::AllocationFailed)
}

fn meaning(kind: K) -> Meaning {
    match kind {
        K::Var
        | K::Let
        | K::Const
        | K::Using
        | K::AwaitUsing
        | K::Function
        | K::AnnexBFunction
        | K::Parameter
        | K::CatchParameter
        | K::FunctionName
        | K::Arguments => Meaning::VALUE,
        K::Class | K::ClassName | K::EnumMember => Meaning::VALUE.union(Meaning::TYPE),
        K::Enum => Meaning::ALL,
        K::Import | K::ImportEquals => Meaning::ALL,
        K::TypeAlias | K::Interface | K::TypeParameter => Meaning::TYPE,
        // Instantiation adds the value meaning where the namespace is declared.
        K::Namespace => Meaning::NAMESPACE,
    }
}

fn lexical(kind: K) -> bool {
    matches!(
        kind,
        K::Let | K::Const | K::Using | K::AwaitUsing | K::Class | K::CatchParameter
    )
}

impl<'t, 's> Binder<'t, 's> {
    pub(crate) fn new(tree: &'t SyntaxTree<'s>) -> Self {
        Self {
            tree,
            scopes: Vec::new(),
            bindings: Vec::new(),
            members: BTreeMap::new(),
            pending: Vec::new(),
            exports: Vec::new(),
            declared: Vec::new(),
            annex_b: Vec::new(),
            evals: Vec::new(),
            stack: Vec::new(),
        }
    }

    fn kind(&self, id: NodeId) -> N {
        self.tree.node(id).expect("parser-owned node").kind()
    }

    fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.tree
            .child_nodes(id)
            .expect("parser-owned node")
            .collect()
    }

    fn text(&self, id: NodeId) -> &'s str {
        let span = self.tree.node(id).expect("parser-owned node").span();
        self.tree.source().slice(span).unwrap_or("")
    }

    fn scope(
        &mut self,
        kind: ScopeKind,
        node: NodeId,
        parent: ScopeId,
        strict: bool,
    ) -> Result<ScopeId, BindingError> {
        reserve(&mut self.scopes)?;
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(ScopeData {
            scope: Scope {
                kind,
                node,
                parent: Some(parent),
                strict,
                direct_eval: false,
            },
            names: BTreeMap::new(),
            container: None,
            own: Vec::new(),
        });
        Ok(id)
    }

    /// Pushes children so that they are visited in source order.
    fn push(&mut self, items: &[(NodeId, Ctx)]) -> Result<(), BindingError> {
        self.stack
            .try_reserve(items.len())
            .map_err(|_| BindingError::AllocationFailed)?;
        self.stack.extend(items.iter().rev().copied());
        Ok(())
    }

    fn push_all(&mut self, id: NodeId, ctx: Ctx) -> Result<(), BindingError> {
        let items: Vec<_> = self.children(id).into_iter().map(|c| (c, ctx)).collect();
        self.push(&items)
    }

    fn reference(
        &mut self,
        node: NodeId,
        meaning: Meaning,
        read: bool,
        write: bool,
        ctx: Ctx,
    ) -> Result<usize, BindingError> {
        let Some(name) = self.tree.identifier_value(node) else {
            return Ok(usize::MAX);
        };
        reserve(&mut self.pending)?;
        self.pending.push(Pending {
            node,
            name,
            meaning,
            read,
            write,
            scope: ctx.scope,
            static_class: ctx.static_class,
            signature: ctx.signature,
        });
        Ok(self.pending.len() - 1)
    }

    fn declare(
        &mut self,
        name_node: NodeId,
        decl: Decl,
        ambient: bool,
        type_only: bool,
    ) -> Result<BindingId, BindingError> {
        let name = self
            .tree
            .identifier_value(name_node)
            .or_else(|| self.tree.string_literal_value(name_node))
            .unwrap_or_default();
        let declaration = Declaration {
            kind: decl.kind,
            name: Some(name_node),
            node: decl.node,
            ambient,
        };
        let mut meaning = meaning(decl.kind);
        if decl.kind == K::Namespace && self.instantiated(decl.node) {
            meaning = meaning.union(Meaning::VALUE);
        }
        let import = match decl.import {
            ImportSpec::No => None,
            ImportSpec::Default(source) => Some((source, ImportedName::Default)),
            ImportSpec::Namespace(source) => Some((source, ImportedName::Namespace)),
            ImportSpec::Require(source) => Some((source, ImportedName::Require)),
            ImportSpec::Named { source, imported } => {
                let imported = self
                    .tree
                    .identifier_value(imported)
                    .or_else(|| self.tree.string_literal_value(imported))
                    .unwrap_or_default();
                Some((
                    source,
                    if imported == "default" {
                        ImportedName::Default
                    } else {
                        ImportedName::Named(imported)
                    },
                ))
            }
        }
        .map(|(source, imported)| Import {
            source,
            specifier: self.tree.string_literal_value(source),
            imported,
            type_only,
        });
        let existing = match decl.container {
            Some(container) => self
                .members
                .get(&container)
                .and_then(|m| m.get(&name))
                .copied(),
            None => self.scopes[decl.scope.index()].names.get(&name).copied(),
        };
        let id = if let Some(id) = existing {
            let binding = &mut self.bindings[id.index()];
            binding
                .declarations
                .try_reserve(1)
                .map_err(|_| BindingError::AllocationFailed)?;
            binding.declarations.push(declaration);
            binding.meaning = binding.meaning.union(meaning);
            if binding.import.is_none() {
                binding.import = import;
            }
            id
        } else {
            reserve(&mut self.bindings)?;
            let id = BindingId(self.bindings.len() as u32);
            self.bindings.push(Binding {
                name: name.clone(),
                scope: decl.scope,
                meaning,
                declarations: vec![declaration],
                import,
                container: decl.container,
            });
            match decl.container {
                Some(container) => {
                    self.members.entry(container).or_default().insert(name, id);
                }
                None => {
                    self.scopes[decl.scope.index()].names.insert(name, id);
                }
            }
            id
        };
        if decl.kind == K::TypeParameter && decl.container.is_some() {
            self.scopes[decl.scope.index()].own.push(id);
        }
        reserve(&mut self.declared)?;
        self.declared.push((name_node, id));
        let module_scope = decl.container.is_none()
            && decl.scope == ScopeId(0)
            && self.scopes[0].scope.kind == ScopeKind::Module;
        if module_scope && decl.exported != Exported::No {
            let name = if decl.exported == Exported::Default {
                ExportName::Default
            } else {
                ExportName::Named(self.bindings[id.index()].name.clone())
            };
            let duplicate = self.exports.iter().any(|e| {
                matches!(e, PendingExport::Done(Export { name: n, kind: ExportKind::Local(Target::Binding(b)), .. }) if *n == name && *b == id)
            });
            if !duplicate {
                reserve(&mut self.exports)?;
                self.exports.push(PendingExport::Done(Export {
                    name,
                    kind: ExportKind::Local(Target::Binding(id)),
                    node: decl.node,
                    type_only,
                }));
            }
        }
        Ok(id)
    }

    /// Whether a namespace declaration has a value side, as TypeScript's
    /// module instance state: anything but types, interfaces, type-only
    /// syntax, and uninstantiated namespaces instantiates it.
    fn instantiated(&self, namespace: NodeId) -> bool {
        let mut pending = vec![namespace];
        while let Some(id) = pending.pop() {
            for child in self.children(id) {
                if let N::TypeScript {
                    kind: T::NamespaceBody,
                    ..
                } = self.kind(child)
                {
                    for statement in self.children(child) {
                        if self.statement_instantiates(statement, &mut pending) {
                            return true;
                        }
                    }
                } else if let N::TypeScript {
                    kind: T::Namespace, ..
                } = self.kind(child)
                {
                    pending.push(child);
                }
            }
        }
        false
    }

    fn statement_instantiates(&self, statement: NodeId, nested: &mut Vec<NodeId>) -> bool {
        let mut node = statement;
        loop {
            let kind = self.kind(node);
            if let N::ExportDeclaration { declaration } = kind {
                node = declaration;
            } else if let N::TypeScript {
                kind: T::ModifiedDeclaration,
                ..
            } = kind
            {
                match self.children(node).last() {
                    Some(&last) => node = last,
                    None => return false,
                }
            } else if matches!(
                kind,
                N::TypeScript {
                    kind: T::Interface | T::TypeAlias | T::TypeOnly,
                    ..
                } | N::EmptyStatement
            ) {
                return false;
            } else if matches!(
                kind,
                N::TypeScript {
                    kind: T::Namespace,
                    ..
                }
            ) {
                nested.push(node);
                return false;
            } else {
                return true;
            }
        }
    }

    pub(crate) fn run(mut self) -> Result<Bindings, BindingError> {
        let tree = self.tree;
        let root = tree.root();
        let N::Program {
            goal,
            dialect,
            strict,
            ..
        } = self.kind(root)
        else {
            return Err(BindingError::TooLarge);
        };
        let kind = if goal == frameshift_syntax::SourceGoal::Module {
            ScopeKind::Module
        } else {
            ScopeKind::Script
        };
        self.scopes.push(ScopeData {
            scope: Scope {
                kind,
                node: root,
                parent: None,
                strict,
                direct_eval: false,
            },
            names: BTreeMap::new(),
            container: None,
            own: Vec::new(),
        });
        let ctx = Ctx {
            scope: ScopeId(0),
            var_scope: ScopeId(0),
            strict,
            ambient: dialect == JsDialect::TypeScriptDeclaration,
            type_only: false,
            exported: Exported::No,
            mode: Mode::Value,
            infer: None,
            static_class: None,
            signature: None,
        };
        self.push_all(root, ctx)?;
        while let Some((id, ctx)) = self.stack.pop() {
            self.visit(id, ctx)?;
        }
        self.hoist_annex_b()?;
        self.mark_evals();
        self.finish()
    }

    fn hoist_annex_b(&mut self) -> Result<(), BindingError> {
        for candidate in std::mem::take(&mut self.annex_b) {
            // Replacing the declaration with `var` must not conflict with a
            // lexical name between the block and the variable scope, and a
            // parameter of the same name takes precedence (B.3.2.1).
            let mut scope = self.scopes[candidate.block.index()].scope.parent;
            let mut blocked = false;
            while let Some(id) = scope {
                if let Some(&b) = self.scopes[id.index()].names.get(&candidate.name) {
                    let binding = &self.bindings[b.index()];
                    blocked |= binding.declarations.iter().any(|d| {
                        lexical(d.kind)
                            || d.kind == K::Parameter
                            || d.kind == K::Function && id != candidate.var_scope
                    });
                }
                if id == candidate.var_scope || blocked {
                    break;
                }
                scope = self.scopes[id.index()].scope.parent;
            }
            if blocked {
                continue;
            }
            let decl = Decl {
                kind: K::AnnexBFunction,
                scope: candidate.var_scope,
                node: candidate.declaration.node,
                exported: Exported::No,
                import: ImportSpec::No,
                container: None,
            };
            // The function's name node already maps to its block binding.
            let declared = self.declared.len();
            self.declare(
                candidate.name_node,
                decl,
                candidate.declaration.ambient,
                false,
            )?;
            self.declared.truncate(declared);
        }
        Ok(())
    }

    fn mark_evals(&mut self) {
        for (reference, var_scope) in std::mem::take(&mut self.evals) {
            let pending = &self.pending[reference];
            if self.lookup(pending.scope, "eval", Meaning::VALUE).is_none() {
                self.scopes[var_scope.index()].scope.direct_eval = true;
            }
        }
    }

    /// The binding a name resolves to, without dynamic capture.
    fn lookup(&self, scope: ScopeId, name: &str, meaning: Meaning) -> Option<BindingId> {
        let mut scope = Some(scope);
        while let Some(id) = scope {
            let data = &self.scopes[id.index()];
            if let Some(&b) = data.names.get(name)
                && self.bindings[b.index()].meaning.overlaps(meaning)
            {
                return Some(b);
            }
            if let Some(container) = data.container
                && let Some(&b) = self.members.get(&container).and_then(|m| m.get(name))
                && self.bindings[b.index()].meaning.overlaps(meaning)
            {
                return Some(b);
            }
            scope = data.scope.parent;
        }
        None
    }

    fn resolve(
        &mut self,
        scope: ScopeId,
        name: &str,
        meaning: Meaning,
        static_class: Option<ScopeId>,
        signature: Option<ScopeId>,
    ) -> Result<Target, BindingError> {
        let mut current = Some(scope);
        let mut dynamic = false;
        while let Some(id) = current {
            let data = &self.scopes[id.index()];
            let visible = |b: &BindingId| {
                let binding = &self.bindings[b.index()];
                let only =
                    |kinds: &[K]| binding.declarations.iter().all(|d| kinds.contains(&d.kind));
                // Static members cannot see class type parameters, and a
                // signature sees only its own parameters, as TypeScript and
                // the parameter scope of ECMAScript decide.
                let has =
                    |kinds: &[K]| binding.declarations.iter().any(|d| kinds.contains(&d.kind));
                let signature_visible = if meaning.overlaps(Meaning::VALUE) {
                    has(&[K::Parameter, K::Arguments])
                } else {
                    has(&[K::TypeParameter])
                };
                binding.meaning.overlaps(meaning)
                    && !(static_class == Some(id) && only(&[K::TypeParameter]))
                    && (signature != Some(id) || signature_visible)
            };
            let found = data.names.get(name).copied().filter(visible).or_else(|| {
                data.container
                    .and_then(|c| self.members.get(&c))
                    .and_then(|m| m.get(name))
                    .copied()
                    .filter(visible)
                    .filter(|b| {
                        let parameter = self.bindings[b.index()]
                            .declarations
                            .iter()
                            .all(|d| d.kind == K::TypeParameter);
                        !parameter || data.own.contains(b)
                    })
            });
            if let Some(b) = found {
                return Ok(if dynamic {
                    Target::Dynamic
                } else {
                    Target::Binding(b)
                });
            }
            let value = meaning.overlaps(Meaning::VALUE);
            if value
                && name == "arguments"
                && data.scope.kind == (ScopeKind::Function { arrow: false })
            {
                let node = data.scope.node;
                reserve(&mut self.bindings)?;
                let b = BindingId(self.bindings.len() as u32);
                self.bindings.push(Binding {
                    name: name.to_owned(),
                    scope: id,
                    meaning: Meaning::VALUE,
                    declarations: vec![Declaration {
                        kind: K::Arguments,
                        name: None,
                        node,
                        ambient: false,
                    }],
                    import: None,
                    container: None,
                });
                self.scopes[id.index()].names.insert(name.to_owned(), b);
                return Ok(if dynamic {
                    Target::Dynamic
                } else {
                    Target::Binding(b)
                });
            }
            if value && (data.scope.kind == ScopeKind::With || data.scope.direct_eval) {
                dynamic = true;
            }
            current = data.scope.parent;
        }
        // `declare global` declarations are visible throughout the program.
        for data in &self.scopes {
            if data.scope.kind == ScopeKind::GlobalAugmentation
                && let Some(&b) = data.names.get(name)
                && self.bindings[b.index()].meaning.overlaps(meaning)
            {
                return Ok(if dynamic {
                    Target::Dynamic
                } else {
                    Target::Binding(b)
                });
            }
        }
        Ok(if dynamic {
            Target::Dynamic
        } else {
            Target::Global
        })
    }

    fn finish(mut self) -> Result<Bindings, BindingError> {
        let pending = std::mem::take(&mut self.pending);
        let mut references = Vec::new();
        references
            .try_reserve(pending.len())
            .map_err(|_| BindingError::AllocationFailed)?;
        for p in &pending {
            let target = self.resolve(p.scope, &p.name, p.meaning, p.static_class, p.signature)?;
            references.push(Reference {
                node: p.node,
                name: p.name.clone(),
                meaning: p.meaning,
                read: p.read,
                write: p.write,
                target,
            });
        }
        let mut exports = Vec::new();
        exports
            .try_reserve(self.exports.len())
            .map_err(|_| BindingError::AllocationFailed)?;
        for export in std::mem::take(&mut self.exports) {
            exports.push(match export {
                PendingExport::Done(export) => export,
                PendingExport::Specifier {
                    name,
                    reference,
                    node,
                    type_only,
                } => Export {
                    name,
                    kind: ExportKind::Local(
                        references
                            .get(reference)
                            .map_or(Target::Global, |r| r.target),
                    ),
                    node,
                    type_only,
                },
            });
        }
        references.sort_by_key(|r| r.node);
        self.declared.sort_by_key(|(n, _)| *n);
        Ok(Bindings {
            scopes: self.scopes.into_iter().map(|s| s.scope).collect(),
            bindings: self.bindings,
            references,
            exports,
            declared: self.declared,
        })
    }

    /// Static class members cannot reference the class's type parameters.
    fn member(ctx: Ctx, is_static: bool) -> Ctx {
        Ctx {
            static_class: if is_static {
                Some(ctx.scope)
            } else {
                ctx.static_class
            },
            ..ctx
        }
    }

    fn value(ctx: Ctx) -> Ctx {
        Ctx {
            mode: Mode::Value,
            exported: Exported::No,
            ..ctx
        }
    }

    fn typed(ctx: Ctx) -> Ctx {
        Ctx {
            mode: Mode::Type,
            exported: Exported::No,
            ..ctx
        }
    }

    fn declare_ctx(ctx: Ctx, kind: K, scope: ScopeId, node: NodeId) -> Ctx {
        Ctx {
            mode: Mode::Declare(Decl {
                kind,
                scope,
                node,
                exported: ctx.exported,
                import: ImportSpec::No,
                container: None,
            }),
            exported: Exported::No,
            ..ctx
        }
    }

    /// The container whose member table receives an exported declaration
    /// in a namespace body.
    fn container(&self, ctx: Ctx) -> Option<BindingId> {
        let data = &self.scopes[ctx.scope.index()];
        (ctx.exported != Exported::No && data.scope.kind == ScopeKind::Namespace)
            .then_some(data.container)
            .flatten()
    }

    fn body_strict(&self, body: NodeId, strict: bool) -> bool {
        let mut node = body;
        loop {
            let kind = self.kind(node);
            if let N::FunctionBody { strict, .. } = kind {
                return strict;
            }
            let generic = matches!(
                kind,
                N::TypeScript {
                    kind: T::GenericDeclaration,
                    ..
                }
            );
            match self.children(node).last() {
                Some(&last) if generic => node = last,
                Some(_) | None => return strict,
            }
        }
    }

    fn function(
        &mut self,
        id: NodeId,
        parameters: NodeId,
        body: NodeId,
        arrow: bool,
        parent: ScopeId,
        ctx: Ctx,
    ) -> Result<(), BindingError> {
        let strict = self.body_strict(body, ctx.strict);
        let scope = self.scope(ScopeKind::Function { arrow }, id, parent, strict)?;
        let inner = Ctx {
            scope,
            var_scope: scope,
            strict,
            mode: Mode::Value,
            exported: Exported::No,
            type_only: false,
            infer: None,
            signature: Some(scope),
            ..ctx
        };
        // A body wrapper also holds the return type; a body itself does not.
        let wrapper = matches!(
            self.kind(body),
            N::TypeScript {
                kind: T::GenericDeclaration | T::BodylessFunction,
                ..
            }
        );
        let body_ctx = Ctx {
            signature: if wrapper { Some(scope) } else { None },
            ..inner
        };
        self.push(&[(parameters, inner), (body, body_ctx)])
    }

    fn visit(&mut self, id: NodeId, ctx: Ctx) -> Result<(), BindingError> {
        let kind = self.kind(id);
        match kind {
            N::BindingIdentifier { .. } => {
                if let Mode::Declare(decl) = ctx.mode {
                    let binding = self.declare(id, decl, ctx.ambient, ctx.type_only)?;
                    // Merged class declarations share their type parameters.
                    let data = &mut self.scopes[ctx.scope.index()];
                    if decl.kind == K::Class && data.scope.kind == ScopeKind::Class {
                        data.container = Some(binding);
                    }
                }
            }
            N::Identifier { .. } => match ctx.mode {
                Mode::Value | Mode::Declare(_) => {
                    self.reference(id, Meaning::VALUE, true, false, ctx)?;
                }
                Mode::Write { read } => {
                    self.reference(id, Meaning::VALUE, read, true, ctx)?;
                }
                Mode::Type => {
                    self.reference(id, Meaning::TYPE, true, false, ctx)?;
                }
            },
            N::ArrayPattern { .. } | N::ObjectPattern { .. } | N::RestElement { .. } => {
                self.push_all(id, ctx)?;
            }
            N::PatternProperty {
                key,
                value,
                computed,
            } => {
                let mut items = vec![(value, ctx)];
                if computed {
                    items.insert(0, (key, Self::value(ctx)));
                }
                self.push(&items)?;
            }
            N::ShorthandPatternProperty { target, default } => {
                let mut items = vec![(target, ctx)];
                if let Some(default) = default {
                    items.push((default, Self::value(ctx)));
                }
                self.push(&items)?;
            }
            N::AssignmentPattern { target, default } => {
                self.push(&[(target, ctx), (default, Self::value(ctx))])?;
            }
            N::VariableDeclaration { kind, declarators } => {
                let (kind, scope) = match kind {
                    VariableKind::Var => (K::Var, ctx.var_scope),
                    VariableKind::Let => (K::Let, ctx.scope),
                    VariableKind::Const => (K::Const, ctx.scope),
                    VariableKind::Using => (K::Using, ctx.scope),
                    VariableKind::AwaitUsing => (K::AwaitUsing, ctx.scope),
                };
                let mut items = Vec::new();
                for &declarator in self.tree.children(declarators).unwrap_or_default() {
                    let mut inner = Self::declare_ctx(ctx, kind, scope, declarator);
                    if let Mode::Declare(decl) = &mut inner.mode {
                        decl.container = self.container(ctx);
                    }
                    items.push((declarator, inner));
                }
                self.push(&items)?;
            }
            N::VariableDeclarator { target, init } => {
                let mut items = vec![(target, ctx)];
                if let Some(init) = init {
                    items.push((init, Self::value(ctx)));
                }
                self.push(&items)?;
            }
            N::FunctionDeclaration {
                name,
                parameters,
                body,
                ..
            } => {
                if let Some(name) = name {
                    let scope_kind = self.scopes[ctx.scope.index()].scope.kind;
                    let block = ctx.scope != ctx.var_scope
                        && matches!(scope_kind, ScopeKind::Block | ScopeKind::With);
                    let scope = if block { ctx.scope } else { ctx.var_scope };
                    let mut decl = Decl {
                        kind: K::Function,
                        scope,
                        node: id,
                        exported: ctx.exported,
                        import: ImportSpec::No,
                        container: self.container(ctx),
                    };
                    if decl.container.is_some() {
                        decl.scope = ctx.scope;
                    }
                    self.declare(name, decl, ctx.ambient, ctx.type_only)?;
                    if block && !ctx.strict {
                        reserve(&mut self.annex_b)?;
                        self.annex_b.push(AnnexB {
                            name: self.tree.identifier_value(name).unwrap_or_default(),
                            block: ctx.scope,
                            var_scope: ctx.var_scope,
                            declaration: Declaration {
                                kind: K::AnnexBFunction,
                                name: Some(name),
                                node: id,
                                ambient: ctx.ambient,
                            },
                            name_node: name,
                        });
                    }
                }
                self.function(id, parameters, body, false, ctx.scope, ctx)?;
            }
            N::FunctionExpression {
                name,
                parameters,
                body,
                ..
            } => {
                let mut parent = ctx.scope;
                if let Some(name) = name {
                    parent = self.scope(ScopeKind::FunctionName, id, ctx.scope, ctx.strict)?;
                    let decl = Decl {
                        kind: K::FunctionName,
                        scope: parent,
                        node: id,
                        exported: Exported::No,
                        import: ImportSpec::No,
                        container: None,
                    };
                    self.declare(name, decl, ctx.ambient, false)?;
                }
                self.function(id, parameters, body, false, parent, ctx)?;
            }
            N::ArrowFunction {
                parameters, body, ..
            } => {
                self.function(id, parameters, body, true, ctx.scope, ctx)?;
            }
            N::FormalParameters { parameters } => {
                let items: Vec<_> = self
                    .tree
                    .children(parameters)
                    .unwrap_or_default()
                    .iter()
                    .map(|&p| (p, Self::declare_ctx(ctx, K::Parameter, ctx.scope, p)))
                    .collect();
                self.push(&items)?;
            }
            N::MethodDefinition {
                key,
                parameters,
                body,
                computed,
                is_static,
                ..
            } => {
                if computed {
                    self.push(&[(key, Self::value(ctx))])?;
                }
                let inner = Self::member(ctx, is_static);
                self.function(id, parameters, body, false, ctx.scope, inner)?;
            }
            N::FieldDefinition { is_static, .. } => {
                self.push_all(id, Self::member(Self::value(ctx), is_static))?;
            }
            N::StaticBlock { .. } => {
                let scope = self.scope(ScopeKind::StaticBlock, id, ctx.scope, true)?;
                let inner = Ctx {
                    scope,
                    var_scope: scope,
                    strict: true,
                    ..Self::member(Self::value(ctx), true)
                };
                self.push_all(id, inner)?;
            }
            N::ClassDeclaration {
                name,
                super_class,
                body,
            }
            | N::ClassExpression {
                name,
                super_class,
                body,
            } => {
                let expression = matches!(kind, N::ClassExpression { .. });
                let scope = self.scope(ScopeKind::Class, id, ctx.scope, true)?;
                let inner = Ctx {
                    scope,
                    strict: true,
                    ..Self::value(ctx)
                };
                let mut items = Vec::new();
                if let Some(name) = name {
                    let (kind, target) = if expression {
                        (K::ClassName, scope)
                    } else {
                        (K::Class, ctx.scope)
                    };
                    let mut decl = Self::declare_ctx(ctx, kind, target, id);
                    decl.scope = scope;
                    decl.strict = true;
                    if let Mode::Declare(d) = &mut decl.mode {
                        d.container = if expression {
                            None
                        } else {
                            self.container(ctx)
                        };
                    }
                    items.push((name, decl));
                }
                if let Some(super_class) = super_class {
                    items.push((super_class, inner));
                }
                items.push((body, inner));
                self.push(&items)?;
            }
            N::BlockStatement { .. } => {
                let scope = self.scope(ScopeKind::Block, id, ctx.scope, ctx.strict)?;
                self.push_all(
                    id,
                    Ctx {
                        scope,
                        ..Self::value(ctx)
                    },
                )?;
            }
            N::ForStatement { .. } => {
                let scope = self.scope(ScopeKind::Block, id, ctx.scope, ctx.strict)?;
                self.push_all(
                    id,
                    Ctx {
                        scope,
                        ..Self::value(ctx)
                    },
                )?;
            }
            N::ForInStatement { left, right, body }
            | N::ForOfStatement {
                left, right, body, ..
            } => {
                let scope = self.scope(ScopeKind::Block, id, ctx.scope, ctx.strict)?;
                let inner = Ctx {
                    scope,
                    ..Self::value(ctx)
                };
                let left_ctx = if matches!(self.kind(left), N::VariableDeclaration { .. }) {
                    inner
                } else {
                    Ctx {
                        mode: Mode::Write { read: false },
                        ..inner
                    }
                };
                self.push(&[(left, left_ctx), (right, inner), (body, inner)])?;
            }
            N::SwitchStatement {
                discriminant,
                cases,
            } => {
                let scope = self.scope(ScopeKind::Block, id, ctx.scope, ctx.strict)?;
                let inner = Ctx {
                    scope,
                    ..Self::value(ctx)
                };
                let mut items = vec![(discriminant, Self::value(ctx))];
                for &case in self.tree.children(cases).unwrap_or_default() {
                    items.push((case, inner));
                }
                self.push(&items)?;
            }
            N::CatchClause { parameter, body } => {
                let scope = self.scope(ScopeKind::Catch, id, ctx.scope, ctx.strict)?;
                let inner = Ctx {
                    scope,
                    ..Self::value(ctx)
                };
                let mut items = Vec::new();
                if let Some(parameter) = parameter {
                    items.push((
                        parameter,
                        Self::declare_ctx(ctx, K::CatchParameter, scope, id),
                    ));
                    if let Some((_, item)) = items.last_mut() {
                        item.scope = scope;
                    }
                }
                items.push((body, inner));
                self.push(&items)?;
            }
            N::WithStatement { object, body } => {
                let scope = self.scope(ScopeKind::With, id, ctx.scope, ctx.strict)?;
                self.push(&[
                    (object, Self::value(ctx)),
                    (
                        body,
                        Ctx {
                            scope,
                            ..Self::value(ctx)
                        },
                    ),
                ])?;
            }
            N::IfStatement {
                test,
                consequent,
                alternate,
            } => {
                let mut items = vec![(test, Self::value(ctx))];
                for clause in std::iter::once(consequent).chain(alternate) {
                    // Annex B.3.3: a function declaration as an `if` clause
                    // behaves as if it were inside a block.
                    let clause_ctx = if matches!(self.kind(clause), N::FunctionDeclaration { .. }) {
                        let scope = self.scope(ScopeKind::Block, clause, ctx.scope, ctx.strict)?;
                        Ctx {
                            scope,
                            ..Self::value(ctx)
                        }
                    } else {
                        Self::value(ctx)
                    };
                    items.push((clause, clause_ctx));
                }
                self.push(&items)?;
            }
            N::LabeledStatement { body, .. } => self.push(&[(body, Self::value(ctx))])?,
            N::ImportDeclaration {
                specifiers, source, ..
            } => {
                let mut items = Vec::new();
                for &specifier in self.tree.children(specifiers).unwrap_or_default() {
                    items.push((
                        specifier,
                        Ctx {
                            mode: Mode::Declare(Decl {
                                kind: K::Import,
                                scope: ctx.scope,
                                node: specifier,
                                exported: Exported::No,
                                import: ImportSpec::Named {
                                    source,
                                    imported: specifier,
                                },
                                container: None,
                            }),
                            ..ctx
                        },
                    ));
                }
                self.push(&items)?;
            }
            N::ImportDefaultSpecifier { local }
            | N::ImportNamespaceSpecifier { local }
            | N::ImportSpecifier { local, .. } => {
                if let Mode::Declare(mut decl) = ctx.mode {
                    let source = match decl.import {
                        ImportSpec::Named { source, .. } => source,
                        ImportSpec::No
                        | ImportSpec::Default(_)
                        | ImportSpec::Namespace(_)
                        | ImportSpec::Require(_) => return Ok(()),
                    };
                    decl.node = id;
                    decl.import = if let N::ImportSpecifier { imported, .. } = kind {
                        ImportSpec::Named {
                            source,
                            imported: imported.unwrap_or(local),
                        }
                    } else if matches!(kind, N::ImportDefaultSpecifier { .. }) {
                        ImportSpec::Default(source)
                    } else {
                        ImportSpec::Namespace(source)
                    };
                    self.declare(local, decl, ctx.ambient, ctx.type_only)?;
                }
            }
            N::ExportDeclaration { declaration } => {
                self.push(&[(
                    declaration,
                    Ctx {
                        exported: Exported::Named,
                        ..ctx
                    },
                )])?;
            }
            N::ExportDefault { declaration } => {
                let mut inner = declaration;
                while let N::TypeScript {
                    kind: T::ModifiedDeclaration,
                    ..
                } = self.kind(inner)
                {
                    match self.children(inner).last() {
                        Some(&last) => inner = last,
                        None => break,
                    }
                }
                let named = matches!(
                    self.kind(inner),
                    N::FunctionDeclaration { name: Some(_), .. }
                        | N::ClassDeclaration { name: Some(_), .. }
                        | N::TypeScript {
                            kind: T::Interface,
                            ..
                        }
                );
                if named {
                    self.push(&[(
                        declaration,
                        Ctx {
                            exported: Exported::Default,
                            ..ctx
                        },
                    )])?;
                } else {
                    reserve(&mut self.exports)?;
                    self.exports.push(PendingExport::Done(Export {
                        name: ExportName::Default,
                        kind: ExportKind::Expression(declaration),
                        node: id,
                        type_only: false,
                    }));
                    self.push(&[(declaration, Self::value(ctx))])?;
                }
            }
            N::ExportNamed {
                specifiers, source, ..
            } => {
                for specifier in self.tree.children(specifiers).unwrap_or_default().to_vec() {
                    self.export_specifier(specifier, source, ctx)?;
                }
            }
            N::ExportAll {
                exported, source, ..
            } => {
                let name = match exported {
                    Some(name) => self.export_name(name),
                    None => ExportName::Star,
                };
                reserve(&mut self.exports)?;
                self.exports.push(PendingExport::Done(Export {
                    name,
                    kind: ExportKind::ReExport {
                        source,
                        imported: ImportedName::Namespace,
                    },
                    node: id,
                    type_only: ctx.type_only,
                }));
            }
            N::Assignment {
                operator,
                target,
                value,
            } => {
                let read = operator != AssignmentOperator::Assign;
                self.push(&[
                    (
                        target,
                        Ctx {
                            mode: Mode::Write { read },
                            ..Self::value(ctx)
                        },
                    ),
                    (value, Self::value(ctx)),
                ])?;
            }
            N::Update { operand, .. } => {
                self.push(&[(
                    operand,
                    Ctx {
                        mode: Mode::Write { read: true },
                        ..Self::value(ctx)
                    },
                )])?;
            }
            N::Parenthesized { .. }
            | N::TypeScript {
                kind: T::NonNull, ..
            } => match ctx.mode {
                Mode::Write { .. } => self.push_all(id, ctx)?,
                Mode::Value | Mode::Type | Mode::Declare(_) => {
                    self.push_all(id, Self::value(ctx))?
                }
            },
            N::TypeScript {
                kind: kind @ (T::As | T::Satisfies | T::Assertion),
                ..
            } => {
                let target = match ctx.mode {
                    Mode::Write { .. } => ctx,
                    Mode::Value | Mode::Type | Mode::Declare(_) => Self::value(ctx),
                };
                let children = self.children(id);
                let items: Vec<_> = children
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| {
                        let expression = if kind == T::Assertion { i == 1 } else { i == 0 };
                        (c, if expression { target } else { Self::typed(ctx) })
                    })
                    .collect();
                self.push(&items)?;
            }
            N::Member {
                object,
                property,
                computed,
                ..
            } => {
                let mut items = vec![(object, Self::value(ctx))];
                if computed {
                    items.push((property, Self::value(ctx)));
                }
                self.push(&items)?;
            }
            N::Call {
                callee,
                arguments,
                optional,
            } => {
                let mut target = callee;
                while let N::Parenthesized { expression } = self.kind(target) {
                    target = expression;
                }
                if !optional
                    && !ctx.strict
                    && matches!(self.kind(target), N::Identifier { .. })
                    && self.tree.identifier_value(target).as_deref() == Some("eval")
                {
                    // Resolved after declarations: only an unbound `eval` is
                    // direct. Parentheses around it hold no other reference.
                    let reference = self.reference(target, Meaning::VALUE, true, false, ctx)?;
                    reserve(&mut self.evals)?;
                    self.evals.push((reference, ctx.var_scope));
                    let items: Vec<_> = self
                        .tree
                        .children(arguments)
                        .unwrap_or_default()
                        .iter()
                        .map(|&a| (a, Self::value(ctx)))
                        .collect();
                    self.push(&items)?;
                } else {
                    self.push_all(id, Self::value(ctx))?;
                }
            }
            N::JsxOpeningElement { name, .. } | N::JsxClosingElement { name } => {
                self.jsx_name(name, ctx)?;
                let children: Vec<_> = self
                    .children(id)
                    .into_iter()
                    .filter(|&c| c != name)
                    .map(|c| (c, Self::value(ctx)))
                    .collect();
                self.push(&children)?;
            }
            N::TypeScript { kind, children } => self.typescript(id, kind, children, ctx)?,
            N::Program { .. }
            | N::Directive { .. }
            | N::ExpressionStatement { .. }
            | N::EmptyStatement
            | N::DebuggerStatement
            | N::WhileStatement { .. }
            | N::DoWhileStatement { .. }
            | N::ContinueStatement { .. }
            | N::BreakStatement { .. }
            | N::ReturnStatement { .. }
            | N::SwitchCase { .. }
            | N::LabelIdentifier { .. }
            | N::ThrowStatement { .. }
            | N::TryStatement { .. }
            | N::FunctionBody { .. }
            | N::ClassBody { .. }
            | N::IdentifierName { .. }
            | N::PrivateIdentifier { .. }
            | N::NumberLiteral { .. }
            | N::StringLiteral
            | N::RegularExpression { .. }
            | N::TemplateLiteral { .. }
            | N::BooleanLiteral(_)
            | N::NullLiteral
            | N::This
            | N::Super
            | N::MetaProperty { .. }
            | N::ImportCall { .. }
            | N::Unary { .. }
            | N::Await { .. }
            | N::Yield { .. }
            | N::Binary { .. }
            | N::Conditional { .. }
            | N::TaggedTemplate { .. }
            | N::ArrayExpression { .. }
            | N::ObjectExpression { .. }
            | N::Elision
            | N::Spread { .. }
            | N::Property { .. }
            | N::ShorthandProperty { .. }
            | N::New { .. }
            | N::ImportAttributes { .. }
            | N::ImportAttribute { .. }
            | N::ExportSpecifier { .. }
            | N::JsxElement { .. }
            | N::JsxFragment { .. }
            | N::JsxOpeningFragment
            | N::JsxClosingFragment
            | N::JsxIdentifier
            | N::JsxNamespacedName { .. }
            | N::JsxMemberExpression { .. }
            | N::JsxAttribute { .. }
            | N::JsxSpreadAttribute { .. }
            | N::JsxExpressionContainer { .. }
            | N::JsxSpreadChild { .. }
            | N::JsxText
            | N::JsxStringLiteral => {
                let inner = match ctx.mode {
                    Mode::Type => Self::typed(ctx),
                    Mode::Value | Mode::Write { .. } | Mode::Declare(_) => Self::value(ctx),
                };
                self.push_all(id, inner)?;
            }
        }
        Ok(())
    }

    fn export_name(&self, node: NodeId) -> ExportName {
        let name = self
            .tree
            .identifier_value(node)
            .or_else(|| self.tree.string_literal_value(node))
            .unwrap_or_default();
        if name == "default" {
            ExportName::Default
        } else {
            ExportName::Named(name)
        }
    }

    fn export_specifier(
        &mut self,
        mut specifier: NodeId,
        source: Option<NodeId>,
        mut ctx: Ctx,
    ) -> Result<(), BindingError> {
        if let N::TypeScript {
            kind: T::TypeOnly, ..
        } = self.kind(specifier)
        {
            ctx.type_only = true;
            match self.children(specifier).first() {
                Some(&inner) => specifier = inner,
                None => return Ok(()),
            }
        }
        let N::ExportSpecifier { local, exported } = self.kind(specifier) else {
            return Ok(());
        };
        let name = self.export_name(exported.unwrap_or(local));
        if let Some(source) = source {
            let imported = match self.export_name(local) {
                ExportName::Default => ImportedName::Default,
                ExportName::Named(name) => ImportedName::Named(name),
                ExportName::Star | ExportName::Assignment => ImportedName::Namespace,
            };
            reserve(&mut self.exports)?;
            self.exports.push(PendingExport::Done(Export {
                name,
                kind: ExportKind::ReExport { source, imported },
                node: specifier,
                type_only: ctx.type_only,
            }));
            return Ok(());
        }
        let reference = self.reference(local, Meaning::ALL, true, false, ctx)?;
        reserve(&mut self.exports)?;
        self.exports.push(PendingExport::Specifier {
            name,
            reference,
            node: specifier,
            type_only: ctx.type_only,
        });
        Ok(())
    }

    fn jsx_name(&mut self, name: NodeId, ctx: Ctx) -> Result<(), BindingError> {
        let mut object = name;
        // `<Name<T> ...>` wraps the tag name and its type arguments.
        if let N::TypeScript {
            kind: T::Instantiation,
            children,
        } = self.kind(name)
        {
            let parts: Vec<_> = self.tree.children(children).unwrap_or_default().to_vec();
            let Some((&first, rest)) = parts.split_first() else {
                return Ok(());
            };
            let rest: Vec<_> = rest.iter().map(|&c| (c, Self::typed(ctx))).collect();
            self.push(&rest)?;
            object = first;
        }
        let mut member = false;
        while let N::JsxMemberExpression { object: inner, .. } = self.kind(object) {
            object = inner;
            member = true;
        }
        if !matches!(self.kind(object), N::JsxIdentifier) {
            return Ok(());
        }
        let text = self.text(object);
        // Intrinsic elements start with a lowercase ASCII letter or contain
        // a hyphen, as TypeScript and the React transform decide.
        let intrinsic =
            !member && (text.starts_with(|c: char| c.is_ascii_lowercase()) || text.contains('-'));
        if !intrinsic && text != "this" {
            self.reference(object, Meaning::VALUE, true, false, ctx)?;
        }
        Ok(())
    }

    /// The leftmost name of a qualified name, member chain, or single name.
    fn leftmost(&self, mut node: NodeId) -> NodeId {
        loop {
            let kind = self.kind(node);
            if let N::Member { object, .. } = kind {
                node = object;
            } else if matches!(
                kind,
                N::TypeScript {
                    kind: T::QualifiedName,
                    ..
                }
            ) && let Some(&first) = self.children(node).first()
            {
                node = first;
            } else {
                return node;
            }
        }
    }

    fn entity(
        &mut self,
        node: NodeId,
        single: Meaning,
        qualified: Meaning,
        ctx: Ctx,
    ) -> Result<(), BindingError> {
        let left = self.leftmost(node);
        if !matches!(
            self.kind(left),
            N::IdentifierName { .. } | N::Identifier { .. }
        ) || self.text(left) == "this"
        {
            return Ok(());
        }
        let meaning = if left == node { single } else { qualified };
        self.reference(left, meaning, true, false, ctx)?;
        Ok(())
    }

    fn typescript(
        &mut self,
        id: NodeId,
        kind: T,
        list: frameshift_syntax::ChildList,
        ctx: Ctx,
    ) -> Result<(), BindingError> {
        let children: Vec<NodeId> = self.tree.children(list).unwrap_or_default().to_vec();
        let typed = Self::typed(ctx);
        match kind {
            T::Reference => {
                if let Some(&entity) = children.first() {
                    self.entity(entity, Meaning::TYPE, Meaning::NAMESPACE, ctx)?;
                }
                let rest: Vec<_> = children.iter().skip(1).map(|&c| (c, typed)).collect();
                self.push(&rest)?;
            }
            T::Query => {
                let mut rest = Vec::new();
                for (i, &child) in children.iter().enumerate() {
                    let entity = matches!(
                        self.kind(child),
                        N::IdentifierName { .. }
                            | N::TypeScript {
                                kind: T::QualifiedName,
                                ..
                            }
                    );
                    if i == 0 && entity {
                        self.entity(child, Meaning::VALUE, Meaning::VALUE, ctx)?;
                    } else {
                        rest.push((child, typed));
                    }
                }
                self.push(&rest)?;
            }
            T::ImportType => {
                let rest: Vec<_> = children
                    .iter()
                    .filter(|&&c| {
                        matches!(
                            self.kind(c),
                            N::TypeScript {
                                kind: T::TypeArguments,
                                ..
                            }
                        )
                    })
                    .map(|&c| (c, typed))
                    .collect();
                self.push(&rest)?;
            }
            T::Predicate { .. } => {
                let mut rest = Vec::new();
                for (i, &child) in children.iter().enumerate() {
                    if i == 0 && matches!(self.kind(child), N::IdentifierName { .. }) {
                        self.entity(child, Meaning::VALUE, Meaning::VALUE, ctx)?;
                    } else {
                        rest.push((child, typed));
                    }
                }
                self.push(&rest)?;
            }
            T::Heritage => {
                for child in children {
                    let kind = self.kind(child);
                    if matches!(kind, N::Identifier { .. } | N::Member { .. }) {
                        self.entity(child, Meaning::TYPE, Meaning::NAMESPACE, ctx)?;
                    } else if let N::TypeScript {
                        kind: T::Instantiation,
                        children: inner,
                    } = kind
                    {
                        let inner: Vec<_> = self.tree.children(inner).unwrap_or_default().to_vec();
                        if let Some(&expression) = inner.first() {
                            self.entity(expression, Meaning::TYPE, Meaning::NAMESPACE, ctx)?;
                        }
                        let rest: Vec<_> = inner.iter().skip(1).map(|&c| (c, typed)).collect();
                        self.push(&rest)?;
                    } else {
                        self.push(&[(child, Self::value(ctx))])?;
                    }
                }
            }
            T::Instantiation => {
                let items: Vec<_> = children
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| (c, if i == 0 { Self::value(ctx) } else { typed }))
                    .collect();
                self.push(&items)?;
            }
            T::TypedBinding { .. } => {
                let items: Vec<_> = children
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| (c, if i == 0 { ctx } else { typed }))
                    .collect();
                self.push(&items)?;
            }
            T::Parameter { .. } => {
                let mut items = Vec::new();
                let mut binding_seen = false;
                for child in children {
                    let child_kind = self.kind(child);
                    let item = if matches!(
                        child_kind,
                        N::TypeScript {
                            kind: T::Modifier,
                            ..
                        }
                    ) {
                        continue;
                    } else if matches!(
                        child_kind,
                        N::TypeScript {
                            kind: T::Decorator,
                            ..
                        }
                    ) {
                        Self::value(ctx)
                    } else if matches!(
                        child_kind,
                        N::TypeScript {
                            kind: T::Annotation,
                            ..
                        }
                    ) {
                        typed
                    } else if !binding_seen {
                        binding_seen = true;
                        ctx
                    } else {
                        Self::value(ctx)
                    };
                    items.push((child, item));
                }
                self.push(&items)?;
            }
            T::Modifier
            | T::Keyword
            | T::Literal
            | T::TemplateSegment
            | T::ExternalModuleReference
            | T::NamespaceExport
            | T::QualifiedName => {}
            T::TypeParameters { .. } => {
                let target = ctx.scope;
                let data = &self.scopes[target.index()];
                let container = matches!(data.scope.kind, ScopeKind::Type | ScopeKind::Class)
                    .then_some(data.container)
                    .flatten();
                let items: Vec<_> = children
                    .iter()
                    .map(|&c| {
                        let mut inner = Self::declare_ctx(ctx, K::TypeParameter, target, c);
                        if let Mode::Declare(decl) = &mut inner.mode {
                            decl.container = container;
                        }
                        (c, inner)
                    })
                    .collect();
                self.push(&items)?;
            }
            T::TypeParameter => {
                let decl = match ctx.mode {
                    Mode::Declare(decl) => decl,
                    Mode::Value | Mode::Write { .. } | Mode::Type => Decl {
                        kind: K::TypeParameter,
                        scope: ctx.scope,
                        node: id,
                        exported: Exported::No,
                        import: ImportSpec::No,
                        container: None,
                    },
                };
                let mut items = Vec::new();
                for child in children {
                    if matches!(self.kind(child), N::BindingIdentifier { .. }) {
                        self.declare(child, decl, ctx.ambient, false)?;
                    } else if !matches!(
                        self.kind(child),
                        N::TypeScript {
                            kind: T::Modifier,
                            ..
                        }
                    ) {
                        items.push((child, typed));
                    }
                }
                self.push(&items)?;
            }
            T::Infer => {
                let scope = ctx.infer.unwrap_or(ctx.scope);
                let items: Vec<_> = children
                    .iter()
                    .map(|&c| (c, Self::declare_ctx(ctx, K::TypeParameter, scope, c)))
                    .collect();
                self.push(&items)?;
            }
            T::Conditional => {
                let scope = self.scope(ScopeKind::Type, id, ctx.scope, ctx.strict)?;
                let inner = Ctx {
                    scope,
                    infer: None,
                    ..typed
                };
                let items: Vec<_> = children
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| {
                        (
                            c,
                            match i {
                                1 => Ctx {
                                    infer: Some(scope),
                                    ..inner
                                },
                                2 => inner,
                                _ => typed,
                            },
                        )
                    })
                    .collect();
                self.push(&items)?;
            }
            T::Mapped
            | T::FunctionType
            | T::ConstructorType
            | T::CallSignature
            | T::ConstructSignature
            | T::IndexSignature => {
                let scope = self.scope(ScopeKind::Type, id, ctx.scope, ctx.strict)?;
                self.push_all(id, Ctx { scope, ..typed })?;
            }
            T::PropertySignature { .. } | T::MethodSignature { .. } => {
                let method = matches!(kind, T::MethodSignature { .. });
                let scope = if method {
                    self.scope(ScopeKind::Type, id, ctx.scope, ctx.strict)?
                } else {
                    ctx.scope
                };
                let mut items = Vec::new();
                let mut name_seen = false;
                for child in children {
                    let child_kind = self.kind(child);
                    if matches!(
                        child_kind,
                        N::TypeScript {
                            kind: T::Modifier,
                            ..
                        }
                    ) {
                        continue;
                    }
                    if !name_seen {
                        name_seen = true;
                        // A computed name is a value expression in the outer scope.
                        if !matches!(
                            child_kind,
                            N::IdentifierName { .. }
                                | N::StringLiteral
                                | N::NumberLiteral { .. }
                                | N::PrivateIdentifier { .. }
                        ) {
                            items.push((child, Self::value(ctx)));
                        }
                        continue;
                    }
                    items.push((child, Ctx { scope, ..typed }));
                }
                self.push(&items)?;
            }
            T::Interface | T::TypeAlias => {
                let scope = self.scope(ScopeKind::Type, id, ctx.scope, ctx.strict)?;
                let decl_kind = if kind == T::Interface {
                    K::Interface
                } else {
                    K::TypeAlias
                };
                let mut items = Vec::new();
                for (i, &child) in children.iter().enumerate() {
                    if i == 0 && matches!(self.kind(child), N::BindingIdentifier { .. }) {
                        let mut decl = Decl {
                            kind: decl_kind,
                            scope: ctx.scope,
                            node: id,
                            exported: ctx.exported,
                            import: ImportSpec::No,
                            container: self.container(ctx),
                        };
                        if decl.container.is_some() {
                            decl.scope = ctx.scope;
                        }
                        let binding = self.declare(child, decl, ctx.ambient, ctx.type_only)?;
                        // Merged interfaces share their type parameters.
                        if kind == T::Interface {
                            self.scopes[scope.index()].container = Some(binding);
                        }
                    } else {
                        items.push((child, Ctx { scope, ..typed }));
                    }
                }
                self.push(&items)?;
            }
            T::Enum => {
                let mut binding = None;
                let mut members = Vec::new();
                for child in children {
                    if binding.is_none() && matches!(self.kind(child), N::BindingIdentifier { .. })
                    {
                        let decl = Decl {
                            kind: K::Enum,
                            scope: ctx.scope,
                            node: id,
                            exported: ctx.exported,
                            import: ImportSpec::No,
                            container: self.container(ctx),
                        };
                        binding = Some(self.declare(child, decl, ctx.ambient, false)?);
                    } else {
                        members.push(child);
                    }
                }
                let scope = self.scope(ScopeKind::Enum, id, ctx.scope, ctx.strict)?;
                self.scopes[scope.index()].container = binding;
                let inner = Ctx {
                    scope,
                    ..Self::value(ctx)
                };
                let mut items = Vec::new();
                for member in members {
                    let parts = self.children(member);
                    if let (Some(&name), Some(container)) = (parts.first(), binding) {
                        let decl = Decl {
                            kind: K::EnumMember,
                            scope,
                            node: member,
                            exported: Exported::No,
                            import: ImportSpec::No,
                            container: Some(container),
                        };
                        self.declare(name, decl, ctx.ambient, false)?;
                    }
                    for &part in parts.iter().skip(1) {
                        items.push((part, inner));
                    }
                }
                self.push(&items)?;
            }
            T::Namespace => {
                let Some(&name) = children.first() else {
                    return Ok(());
                };
                let name_kind = self.kind(name);
                let (scope_kind, binding) = if matches!(name_kind, N::BindingIdentifier { .. }) {
                    let decl = Decl {
                        kind: K::Namespace,
                        scope: ctx.scope,
                        node: id,
                        exported: ctx.exported,
                        import: ImportSpec::No,
                        container: self.container(ctx),
                    };
                    (
                        ScopeKind::Namespace,
                        Some(self.declare(name, decl, ctx.ambient, ctx.type_only)?),
                    )
                } else if matches!(name_kind, N::StringLiteral) {
                    (ScopeKind::AmbientModule, None)
                } else {
                    (ScopeKind::GlobalAugmentation, None)
                };
                let scope = self.scope(scope_kind, id, ctx.scope, ctx.strict)?;
                self.scopes[scope.index()].container = binding;
                let inner = Ctx {
                    scope,
                    var_scope: scope,
                    ambient: ctx.ambient,
                    infer: None,
                    ..Self::value(ctx)
                };
                let mut items = Vec::new();
                for &child in children.iter().skip(1) {
                    // A dotted inner namespace is an exported member.
                    let nested = matches!(
                        self.kind(child),
                        N::TypeScript {
                            kind: T::Namespace,
                            ..
                        }
                    );
                    items.push((
                        child,
                        if nested {
                            Ctx {
                                exported: Exported::Named,
                                ..inner
                            }
                        } else {
                            inner
                        },
                    ));
                }
                self.push(&items)?;
            }
            T::NamespaceBody => self.push_all(id, Self::value(ctx))?,
            T::ImportEquals => {
                let mut source = None;
                let mut entity = None;
                let mut name = None;
                for &child in &children {
                    let kind = self.kind(child);
                    if name.is_none() && matches!(kind, N::BindingIdentifier { .. }) {
                        name = Some(child);
                    } else if matches!(
                        kind,
                        N::TypeScript {
                            kind: T::ExternalModuleReference,
                            ..
                        }
                    ) {
                        source = self.children(child).first().copied();
                    } else if !matches!(
                        kind,
                        N::TypeScript {
                            kind: T::Modifier,
                            ..
                        }
                    ) {
                        entity = Some(child);
                    }
                }
                if let Some(name) = name {
                    let decl = Decl {
                        kind: K::ImportEquals,
                        scope: ctx.scope,
                        node: id,
                        exported: ctx.exported,
                        import: source.map_or(ImportSpec::No, ImportSpec::Require),
                        container: self.container(ctx),
                    };
                    self.declare(name, decl, ctx.ambient, ctx.type_only)?;
                }
                if let Some(entity) = entity {
                    self.entity(entity, Meaning::ALL, Meaning::NAMESPACE, ctx)?;
                }
            }
            T::ExportAssignment => {
                if let Some(&expression) = children.first() {
                    reserve(&mut self.exports)?;
                    self.exports.push(PendingExport::Done(Export {
                        name: ExportName::Assignment,
                        kind: ExportKind::Expression(expression),
                        node: id,
                        type_only: false,
                    }));
                }
                self.push_all(id, Self::value(ctx))?;
            }
            T::ModifiedDeclaration => {
                let ambient = ctx.ambient
                    || children.iter().any(|&c| {
                        matches!(
                            self.kind(c),
                            N::TypeScript {
                                kind: T::Modifier,
                                ..
                            }
                        ) && self.text(c) == "declare"
                    });
                let items: Vec<_> = children
                    .iter()
                    .map(|&c| {
                        let decorator = matches!(
                            self.kind(c),
                            N::TypeScript {
                                kind: T::Decorator,
                                ..
                            }
                        );
                        (
                            c,
                            if decorator {
                                Self::value(ctx)
                            } else {
                                Ctx { ambient, ..ctx }
                            },
                        )
                    })
                    .collect();
                self.push(&items)?;
            }
            T::TypeOnly => {
                self.push_all(
                    id,
                    Ctx {
                        type_only: true,
                        ..ctx
                    },
                )?;
            }
            T::GenericDeclaration | T::BodylessFunction => {
                let items: Vec<_> = children
                    .iter()
                    .map(|&c| {
                        let signature = matches!(
                            self.kind(c),
                            N::TypeScript {
                                kind: T::Annotation | T::TypeParameters { .. },
                                ..
                            } | N::FormalParameters { .. }
                        );
                        (
                            c,
                            if signature {
                                ctx
                            } else {
                                Ctx {
                                    signature: None,
                                    ..ctx
                                }
                            },
                        )
                    })
                    .collect();
                self.push(&items)?;
            }
            T::Decorator | T::EnumMember => self.push_all(id, Self::value(ctx))?,
            T::NonNull | T::As | T::Satisfies | T::Assertion => {
                unreachable!("handled with expressions")
            }
            T::TypeArguments
            | T::Constraint
            | T::DefaultType
            | T::Parenthesized
            | T::Union
            | T::Intersection
            | T::Operator
            | T::Array
            | T::IndexedAccess
            | T::Tuple
            | T::TupleElement { .. }
            | T::Template
            | T::TypeLiteral
            | T::NameRemapping
            | T::Annotation
            | T::JSDoc => self.push_all(id, typed)?,
        }
        Ok(())
    }
}
