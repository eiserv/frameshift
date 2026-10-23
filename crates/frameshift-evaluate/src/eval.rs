//! Expression, binding, and module-export evaluation.

use std::collections::BTreeMap;

use frameshift_bindings::{
    BindingId, Bindings, DeclarationKind as K, ExportKind, ExportName, ImportedName, Target,
    analyze,
};
use frameshift_core::{SourcePath, SourceText, Span};
use frameshift_input::{FileRead, Snapshot};
use frameshift_modules::{
    FileKind, GraphLimits, Resolution, Resolver, snapshot_resolver, source_kind,
};
use frameshift_syntax::{
    BinaryOperator as B, JsonDialect, JsonDocument, JsonId, JsonValue, MethodKind, NodeId,
    NumericKind, SyntaxNodeKind as N, SyntaxTree, TemplatePart, TypeScriptKind as T,
    UnaryOperator as U, parse_js_program, parse_json,
};

use crate::number;
use crate::{EvaluateError, EvaluateLimits, Number, Object, Reason, Unknown, Value};

/// The deepest value that evaluation builds; recursive helpers rely on it.
const MAX_VALUE_DEPTH: usize = 128;
/// The deepest expression nesting that evaluation follows.
const MAX_EXPRESSION_DEPTH: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Export(ExportName),
    Binding(u32),
}

/// How a reference uses its binding, for mutation analysis.
enum Use {
    Read,
    /// A write, a property write, a method call, or a deletion.
    Mutated,
    /// Passed to code that may mutate it.
    Escaped,
    /// Its value, or a literal containing it, initializes another binding.
    Alias(BindingId),
}

struct Module<'a, 't> {
    path: &'a SourcePath,
    tree: &'a SyntaxTree<'t>,
    bindings: &'a Bindings,
    parents: Vec<Option<NodeId>>,
    values: BTreeMap<BindingId, Value>,
}

impl Module<'_, '_> {
    fn kind(&self, id: NodeId) -> N {
        self.tree.node(id).expect("parser-owned node").kind()
    }

    fn span(&self, id: NodeId) -> Span {
        self.tree.node(id).expect("parser-owned node").span()
    }

    fn text(&self, id: NodeId) -> &str {
        self.tree.source().slice(self.span(id)).unwrap_or("")
    }

    fn children(&self, id: NodeId) -> Vec<NodeId> {
        self.tree
            .child_nodes(id)
            .expect("parser-owned node")
            .collect()
    }

    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents.get(id.index()).copied().flatten()
    }
}

fn parents(tree: &SyntaxTree<'_>) -> Vec<Option<NodeId>> {
    let mut parents = vec![None; tree.nodes().len()];
    let mut pending = vec![tree.root()];
    while let Some(id) = pending.pop() {
        for child in tree.child_nodes(id).expect("parser-owned node") {
            parents[child.index()] = Some(id);
            pending.push(child);
        }
    }
    parents
}

/// Size and depth of a value; values never exceed [`MAX_VALUE_DEPTH`], so
/// the recursion is bounded.
fn measure(value: &Value) -> (usize, usize) {
    match value {
        Value::String(s) | Value::BigInt(s) => (s.len() + 1, 0),
        Value::Array(items) => items.iter().fold((1, 1), |(size, depth), v| {
            let (s, d) = measure(v);
            (size.saturating_add(s), depth.max(d + 1))
        }),
        Value::Object(object) => object.entries.iter().fold((1, 1), |(size, depth), (k, v)| {
            let (s, d) = measure(v);
            (
                size.saturating_add(s).saturating_add(k.len()),
                depth.max(d + 1),
            )
        }),
        Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) | Value::Unknown(_) => {
            (1, 0)
        }
    }
}

fn truthy(value: &Value) -> Option<bool> {
    Some(match value {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(Number(n)) => *n != 0.0 && !n.is_nan(),
        Value::BigInt(digits) => !digits
            .trim_start_matches(['-', '0', 'x', 'X', 'o', 'O', 'b', 'B'])
            .trim_start_matches('0')
            .is_empty(),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
        Value::Unknown(_) => return None,
    })
}

/// `ToString` of values whose conversion runs no user code.
fn to_string(value: &Value) -> Option<String> {
    Some(match value {
        Value::Undefined => "undefined".to_owned(),
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::BigInt(digits) => digits.clone(),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                parts.push(if matches!(item, Value::Undefined | Value::Null) {
                    String::new()
                } else {
                    to_string(item)?
                });
            }
            parts.join(",")
        }
        // A literal object could define `toString`; only known plain
        // objects without such properties convert.
        Value::Object(object)
            if object.complete
                && object.get("toString").is_none()
                && object.get("valueOf").is_none() =>
        {
            "[object Object]".to_owned()
        }
        Value::Object(_) | Value::Unknown(_) => return None,
    })
}

fn to_number(value: &Value) -> Option<f64> {
    Some(match value {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(Number(n)) => *n,
        Value::String(s) => number::from_string(s),
        Value::Array(_) | Value::Object(_) => number::from_string(&to_string(value)?),
        Value::BigInt(_) | Value::Unknown(_) => return None,
    })
}

fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let modulo = n.trunc().rem_euclid(4_294_967_296.0);
    modulo as u32 as i32
}

fn property_key(value: &Value) -> Option<String> {
    if matches!(
        value,
        Value::Array(_) | Value::Object(_) | Value::Unknown(_)
    ) {
        None
    } else {
        to_string(value)
    }
}

fn array_index(key: &str) -> Option<u32> {
    if key.is_empty()
        || key.len() > 1 && key.starts_with('0')
        || !key.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    key.parse::<u64>()
        .ok()
        .filter(|&v| v < u64::from(u32::MAX))
        .map(|v| v as u32)
}

fn insert(entries: &mut Vec<(String, Value)>, key: String, value: Value) {
    if let Some(entry) = entries.iter_mut().find(|(k, _)| *k == key) {
        entry.1 = value;
    } else {
        entries.push((key, value));
    }
}

fn order(mut entries: Vec<(String, Value)>) -> Vec<(String, Value)> {
    entries.sort_by_key(|(key, _)| array_index(key).map_or((1, 0), |i| (0, i)));
    entries
}

fn type_of(value: &Value) -> Option<&'static str> {
    Some(match value {
        Value::Undefined => "undefined",
        Value::Null | Value::Array(_) | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::BigInt(_) => "bigint",
        Value::String(_) => "string",
        Value::Unknown(_) => return None,
    })
}

fn json_value(doc: &JsonDocument<'_>, id: JsonId) -> Value {
    match doc.value(id) {
        Some(JsonValue::Null) | None => Value::Null,
        Some(JsonValue::Bool(b)) => Value::Bool(b),
        Some(JsonValue::Number(n)) => Value::Number(Number(n.text().parse().unwrap_or(f64::NAN))),
        Some(JsonValue::String(s)) => Value::String(s.to_owned()),
        Some(JsonValue::Array(a)) => Value::Array(a.iter().map(|i| json_value(doc, i)).collect()),
        Some(JsonValue::Object(o)) => {
            let mut entries = Vec::new();
            for member in o.members() {
                insert(
                    &mut entries,
                    member.key().to_owned(),
                    json_value(doc, member.value()),
                );
            }
            Value::Object(Object {
                entries: order(entries),
                complete: true,
            })
        }
    }
}

/// Evaluates exports of the selected package's modules through one
/// snapshot, resolving local imports as the module graph does.
///
/// Results are cached per module and export. Work, depth, and value limits
/// are shared by every evaluation of one evaluator, so results depend on
/// the order of requests only when a limit is reached.
pub struct Evaluator<'s> {
    // Both are absent for a detached evaluator, which resolves no imports.
    snapshot: Option<&'s Snapshot>,
    resolver: Option<Resolver>,
    graph: GraphLimits,
    limits: EvaluateLimits,
    work: u64,
    exports: BTreeMap<(SourcePath, ExportName), Value>,
    active: Vec<(SourcePath, Key)>,
}

impl<'s> Evaluator<'s> {
    /// An evaluator for the package at `package` (`None` for the run root).
    pub fn new(
        snapshot: &'s Snapshot,
        package: Option<&SourcePath>,
        graph: GraphLimits,
        limits: EvaluateLimits,
    ) -> Result<Self, EvaluateError> {
        Ok(Self {
            snapshot: Some(snapshot),
            resolver: Some(
                snapshot_resolver(snapshot, package, graph).map_err(EvaluateError::Modules)?,
            ),
            graph,
            limits,
            work: limits.max_work,
            exports: BTreeMap::new(),
            active: Vec::new(),
        })
    }

    /// An evaluator without a snapshot: it evaluates only trees that the
    /// caller supplies, and every import is [`Reason::Unresolved`] without a resolver reason.
    pub fn detached(graph: GraphLimits, limits: EvaluateLimits) -> Self {
        Self {
            snapshot: None,
            resolver: None,
            graph,
            limits,
            work: limits.max_work,
            exports: BTreeMap::new(),
            active: Vec::new(),
        }
    }

    /// The value of one export of a module that the caller has parsed and
    /// bound, such as a convention file during graph construction. Imports
    /// resolve through the snapshot unless the evaluator is detached.
    pub fn tree_export(
        &mut self,
        path: &SourcePath,
        tree: &SyntaxTree<'_>,
        bindings: &Bindings,
        name: &ExportName,
    ) -> Result<Value, EvaluateError> {
        let active = (path.clone(), Key::Export(name.clone()));
        if self.active.contains(&active) {
            return Ok(self.unknown_at(path, None, Reason::Cycle));
        }
        self.active.push(active);
        let mut m = Module {
            path,
            tree,
            bindings,
            parents: parents(tree),
            values: BTreeMap::new(),
        };
        let value = self.export_in(&mut m, name);
        self.active.pop();
        value
    }

    /// The value of a configuration module: its default export, else its
    /// TypeScript `export =` or CommonJS `module.exports`.
    pub fn config(&mut self, module: &SourcePath) -> Result<Value, EvaluateError> {
        let value = self.export(module, &ExportName::Default)?;
        if matches!(
            &value,
            Value::Unknown(Unknown {
                reason: Reason::NotExported,
                ..
            })
        ) {
            return self.export(module, &ExportName::Assignment);
        }
        Ok(value)
    }

    /// The value of one export of a module. `ExportName::Assignment` is
    /// `export =` or CommonJS `module.exports`.
    pub fn export(
        &mut self,
        module: &SourcePath,
        name: &ExportName,
    ) -> Result<Value, EvaluateError> {
        let key = (module.clone(), name.clone());
        if let Some(value) = self.exports.get(&key) {
            let value = value.clone();
            return Ok(self.charged(module, None, value));
        }
        let active = (module.clone(), Key::Export(name.clone()));
        if self.active.contains(&active) {
            return Ok(self.unknown_at(module, None, Reason::Cycle));
        }
        if self.active.len() >= self.limits.max_depth {
            return Ok(self.unknown_at(module, None, Reason::DepthLimit));
        }
        self.active.push(active);
        let value = self.load(module, name);
        self.active.pop();
        let value = value?;
        self.exports.insert(key, value.clone());
        Ok(value)
    }

    fn unknown_at(&self, module: &SourcePath, span: Option<Span>, reason: Reason) -> Value {
        Value::Unknown(Unknown {
            reason,
            module: module.clone(),
            span,
        })
    }

    fn unknown(&self, m: &Module<'_, '_>, node: NodeId, reason: Reason) -> Value {
        self.unknown_at(m.path, Some(m.span(node)), reason)
    }

    fn charge(&mut self, units: usize) -> bool {
        let units = units as u64;
        if self.work < units {
            self.work = 0;
            false
        } else {
            self.work -= units;
            true
        }
    }

    /// Charges a value's size and enforces the value limits.
    fn charged(&mut self, module: &SourcePath, span: Option<Span>, value: Value) -> Value {
        let (size, depth) = measure(&value);
        if !self.charge(size) {
            return self.unknown_at(module, span, Reason::WorkLimit);
        }
        if size > self.limits.max_value_size || depth > MAX_VALUE_DEPTH {
            return self.unknown_at(module, span, Reason::ValueLimit);
        }
        value
    }

    fn load(&mut self, path: &SourcePath, name: &ExportName) -> Result<Value, EvaluateError> {
        let Some(snapshot) = self.snapshot else {
            return Ok(self.unknown_at(path, None, Reason::Unresolved(None)));
        };
        let Some(id) = snapshot.inventory().files().find(path) else {
            return Ok(self.unknown_at(path, None, Reason::Unresolved(None)));
        };
        let file = match snapshot.read(id).map_err(EvaluateError::Snapshot)? {
            FileRead::Content(file) => file,
            FileRead::Unreadable(_) => return Ok(self.unknown_at(path, None, Reason::Syntax)),
        };
        if !self.charge(file.bytes().len()) {
            return Ok(self.unknown_at(path, None, Reason::WorkLimit));
        }
        let Ok(source) = SourceText::from_bytes(file.bytes().to_vec(), self.graph.text) else {
            return Ok(self.unknown_at(path, None, Reason::Syntax));
        };
        if FileKind::of(path) == FileKind::Json {
            let Ok(doc) = parse_json(&source, JsonDialect::Json, self.graph.json) else {
                return Ok(self.unknown_at(path, None, Reason::Syntax));
            };
            return Ok(if *name == ExportName::Default {
                let value = json_value(&doc, doc.root());
                self.charged(path, None, value)
            } else {
                self.unknown_at(path, None, Reason::NotExported)
            });
        }
        let Some((goal, dialect)) = source_kind(path) else {
            return Ok(self.unknown_at(path, None, Reason::Unsupported));
        };
        let Ok(tree) = parse_js_program(&source, goal, dialect, self.graph.parse) else {
            return Ok(self.unknown_at(path, None, Reason::Syntax));
        };
        let bindings = analyze(&tree).map_err(|_| EvaluateError::AllocationFailed)?;
        if !self.charge(tree.nodes().len()) {
            return Ok(self.unknown_at(path, None, Reason::WorkLimit));
        }
        let mut m = Module {
            path,
            tree: &tree,
            bindings: &bindings,
            parents: parents(&tree),
            values: BTreeMap::new(),
        };
        self.export_in(&mut m, name)
    }

    fn resolve(&mut self, m: &Module<'_, '_>, source: NodeId) -> Result<Option<SourcePath>, Value> {
        let Some(specifier) = m.tree.string_literal_value(source) else {
            return Err(self.unknown(m, source, Reason::Unresolved(None)));
        };
        let Some(resolver) = self.resolver.as_mut() else {
            return Err(self.unknown(m, source, Reason::Unresolved(None)));
        };
        match resolver.resolve(m.path, &specifier) {
            Ok(Resolution::Local {
                path,
                kind: FileKind::Module | FileKind::Declaration | FileKind::Json,
            }) => Ok(Some(path)),
            Ok(Resolution::External { package }) => Err(self.unknown(
                m,
                source,
                Reason::Package {
                    package,
                    name: String::new(),
                },
            )),
            Ok(Resolution::NodeBuiltin) => Err(self.unknown(
                m,
                source,
                Reason::Package {
                    package: specifier,
                    name: String::new(),
                },
            )),
            Ok(Resolution::Unresolved(reason)) => {
                Err(self.unknown(m, source, Reason::Unresolved(Some(reason))))
            }
            Ok(Resolution::Local { .. }) | Err(_) => {
                Err(self.unknown(m, source, Reason::Unresolved(None)))
            }
        }
    }

    fn reexport(
        &mut self,
        m: &Module<'_, '_>,
        source: NodeId,
        imported: &ImportedName,
    ) -> Result<Value, EvaluateError> {
        let target = match self.resolve(m, source) {
            Ok(Some(target)) => target,
            Ok(None) => return Ok(self.unknown(m, source, Reason::Unresolved(None))),
            Err(mut unknown) => {
                if let Value::Unknown(Unknown {
                    reason: Reason::Package { name, .. },
                    ..
                }) = &mut unknown
                {
                    *name = match imported {
                        ImportedName::Default => "default".to_owned(),
                        ImportedName::Named(n) => n.clone(),
                        ImportedName::Namespace => "*".to_owned(),
                        ImportedName::Require => "=".to_owned(),
                    };
                }
                return Ok(unknown);
            }
        };
        match imported {
            ImportedName::Default => self.export(&target, &ExportName::Default),
            ImportedName::Named(name) => self.export(&target, &ExportName::Named(name.clone())),
            ImportedName::Require => self.export(&target, &ExportName::Assignment),
            ImportedName::Namespace => Ok(self.unknown(m, source, Reason::Unsupported)),
        }
    }

    fn export_in(
        &mut self,
        m: &mut Module<'_, '_>,
        name: &ExportName,
    ) -> Result<Value, EvaluateError> {
        let exports = m.bindings.exports().to_vec();
        if let Some(export) = exports.iter().find(|e| e.name == *name) {
            return match &export.kind {
                ExportKind::Local(Target::Binding(b)) => self.binding(m, *b),
                ExportKind::Local(Target::Global) => {
                    Ok(self.unknown(m, export.node, Reason::Global(String::new())))
                }
                ExportKind::Local(Target::Dynamic) => {
                    Ok(self.unknown(m, export.node, Reason::Dynamic))
                }
                ExportKind::Expression(node) => self.expr(m, *node, 0),
                ExportKind::ReExport { source, imported } => self.reexport(m, *source, imported),
            };
        }
        if *name == ExportName::Assignment {
            return self.common_js(m);
        }
        if let ExportName::Named(_) = name {
            for export in exports.iter().filter(|e| e.name == ExportName::Star) {
                if let ExportKind::ReExport { source, .. } = &export.kind {
                    let ExportName::Named(n) = name else { continue };
                    let value = self.reexport(m, *source, &ImportedName::Named(n.clone()))?;
                    if !matches!(
                        &value,
                        Value::Unknown(Unknown {
                            reason: Reason::NotExported,
                            ..
                        })
                    ) {
                        return Ok(value);
                    }
                }
            }
        }
        Ok(self.unknown_at(m.path, None, Reason::NotExported))
    }

    /// `module.exports = value`, when it is the module's only use of the
    /// global `module` and `exports`.
    fn common_js(&mut self, m: &mut Module<'_, '_>) -> Result<Value, EvaluateError> {
        let N::Program { body, .. } = m.kind(m.tree.root()) else {
            return Ok(self.unknown_at(m.path, None, Reason::NotExported));
        };
        let mut found = None;
        for &statement in m.tree.children(body).unwrap_or_default() {
            let N::ExpressionStatement { expression } = m.kind(statement) else {
                continue;
            };
            let N::Assignment {
                operator: frameshift_syntax::AssignmentOperator::Assign,
                target,
                value,
            } = m.kind(expression)
            else {
                continue;
            };
            let N::Member {
                object,
                property,
                computed: false,
                ..
            } = m.kind(target)
            else {
                continue;
            };
            if m.text(object) == "module" && m.text(property) == "exports" {
                found = Some((object, value));
            }
        }
        let Some((object, value)) = found else {
            return Ok(self.unknown_at(m.path, None, Reason::NotExported));
        };
        let other = m.bindings.references().iter().any(|r| {
            r.target == Target::Global
                && (r.name == "module" || r.name == "exports")
                && r.node != object
        });
        if other {
            return Ok(self.unknown(m, object, Reason::Mutable));
        }
        self.expr(m, value, 0)
    }

    fn binding(&mut self, m: &mut Module<'_, '_>, b: BindingId) -> Result<Value, EvaluateError> {
        if let Some(value) = m.values.get(&b) {
            let value = value.clone();
            return Ok(self.charged(m.path, None, value));
        }
        let active = (m.path.clone(), Key::Binding(b.index() as u32));
        let span = m.bindings.binding(b).declarations[0]
            .name
            .map(|n| m.span(n));
        if self.active.contains(&active) {
            return Ok(self.unknown_at(m.path, span, Reason::Cycle));
        }
        if self.active.len() >= self.limits.max_depth {
            return Ok(self.unknown_at(m.path, span, Reason::DepthLimit));
        }
        self.active.push(active);
        let value = self.binding_value(m, b, span);
        self.active.pop();
        let value = value?;
        m.values.insert(b, value.clone());
        Ok(value)
    }

    fn binding_value(
        &mut self,
        m: &mut Module<'_, '_>,
        b: BindingId,
        span: Option<Span>,
    ) -> Result<Value, EvaluateError> {
        if !self.mutation_free(m, b, 0) {
            return Ok(self.unknown_at(m.path, span, Reason::Mutable));
        }
        let binding = m.bindings.binding(b).clone();
        if let Some(import) = &binding.import {
            return self.reexport(m, import.source, &import.imported);
        }
        let [declaration] = binding.declarations.as_slice() else {
            return Ok(self.unknown_at(m.path, span, Reason::Unsupported));
        };
        match declaration.kind {
            K::Const | K::Let | K::Var => {
                let N::VariableDeclarator { target, init } = m.kind(declaration.node) else {
                    return Ok(self.unknown_at(m.path, span, Reason::Unsupported));
                };
                // Destructuring reads properties of the initializer; it is
                // outside the subset.
                let name = declaration.name;
                let direct = Some(target) == name
                    || m.children(target).first().copied() == name
                        && matches!(
                            m.kind(target),
                            N::TypeScript {
                                kind: T::TypedBinding { .. },
                                ..
                            }
                        );
                if !direct {
                    return match (init, name) {
                        (Some(init), Some(name)) => self.destructured(m, target, init, name, span),
                        _ => Ok(self.unknown_at(m.path, span, Reason::Unsupported)),
                    };
                }
                match init {
                    Some(init) => self.expr(m, init, 0),
                    None => Ok(Value::Undefined),
                }
            }
            K::Function | K::AnnexBFunction | K::Class | K::FunctionName | K::ClassName => {
                Ok(self.unknown_at(m.path, span, Reason::Function))
            }
            K::Using
            | K::AwaitUsing
            | K::Parameter
            | K::CatchParameter
            | K::Import
            | K::ImportEquals
            | K::TypeAlias
            | K::Interface
            | K::Enum
            | K::EnumMember
            | K::Namespace
            | K::TypeParameter
            | K::Arguments => Ok(self.unknown_at(m.path, span, Reason::Unsupported)),
        }
    }

    /// `const { key: name = fallback } = init`: one property of a known
    /// object, with its default for `undefined`. Nested and rest patterns
    /// are outside the subset.
    fn destructured(
        &mut self,
        m: &mut Module<'_, '_>,
        mut target: NodeId,
        init: NodeId,
        name: NodeId,
        span: Option<Span>,
    ) -> Result<Value, EvaluateError> {
        if matches!(
            m.kind(target),
            N::TypeScript {
                kind: T::TypedBinding { .. },
                ..
            }
        ) && let Some(&first) = m.children(target).first()
        {
            target = first;
        }
        let N::ObjectPattern { properties, .. } = m.kind(target) else {
            return Ok(self.unknown_at(m.path, span, Reason::Unsupported));
        };
        let mut found = None;
        for &p in m.tree.children(properties).unwrap_or_default() {
            let kind = m.kind(p);
            if let N::ShorthandPatternProperty { target, default } = kind
                && target == name
            {
                found = m.tree.identifier_value(name).map(|key| (key, default));
            } else if let N::PatternProperty {
                key,
                value,
                computed: false,
            } = kind
            {
                let (bound, default) =
                    if let N::AssignmentPattern { target, default } = m.kind(value) {
                        (target, Some(default))
                    } else {
                        (value, None)
                    };
                if bound == name {
                    found = self.static_key(m, key).map(|key| (key, default));
                }
            }
        }
        let Some((key, default)) = found else {
            return Ok(self.unknown_at(m.path, span, Reason::Unsupported));
        };
        let base = self.expr(m, init, 0)?;
        let value = match &base {
            Value::Unknown(_) => return Ok(base),
            Value::Object(object) => match object.get(&key) {
                Some(value) => value.clone(),
                None if object.complete => Value::Undefined,
                None => return Ok(self.unknown_at(m.path, span, Reason::Unsupported)),
            },
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::BigInt(_)
            | Value::String(_)
            | Value::Array(_) => return Ok(self.unknown_at(m.path, span, Reason::Unsupported)),
        };
        match (value, default) {
            (Value::Undefined, Some(default)) => self.expr(m, default, 0),
            (value, _) => Ok(value),
        }
    }

    /// Whether no reference writes or may mutate the binding's value,
    /// following aliases into other bindings.
    fn mutation_free(&mut self, m: &Module<'_, '_>, b: BindingId, depth: usize) -> bool {
        let references: Vec<(NodeId, bool)> = m
            .bindings
            .references_to(b)
            .map(|r| (r.node, r.write))
            .collect();
        for (node, write) in references {
            if !self.charge(1) {
                return false;
            }
            if write {
                return false;
            }
            match self.use_of(m, node) {
                Use::Read => {}
                Use::Mutated | Use::Escaped => return false,
                Use::Alias(other) => {
                    if other == b || depth >= 8 || !self.mutation_free(m, other, depth + 1) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn use_of(&self, m: &Module<'_, '_>, node: NodeId) -> Use {
        let mut current = node;
        let mut member = false;
        loop {
            let Some(parent) = m.parent(current) else {
                return Use::Read;
            };
            let kind = m.kind(parent);
            if let N::Member { object, .. } = kind {
                if object != current {
                    return Use::Read;
                }
                member = true;
            } else if let N::Assignment { target, .. } = kind {
                if target == current {
                    return Use::Mutated;
                }
                // `module.exports = value` exports the value; it is a read.
                if let N::Member {
                    object,
                    property,
                    computed: false,
                    ..
                } = m.kind(target)
                    && m.text(object) == "module"
                    && m.text(property) == "exports"
                    && m.bindings
                        .reference(object)
                        .is_some_and(|r| r.target == Target::Global)
                {
                    return Use::Read;
                }
                let target_reference = m.bindings.reference(target);
                return match target_reference.map(|r| r.target) {
                    Some(Target::Binding(other)) => Use::Alias(other),
                    Some(Target::Global | Target::Dynamic) | None => Use::Escaped,
                };
            } else if matches!(kind, N::Update { .. })
                || matches!(
                    kind,
                    N::Unary {
                        operator: U::Delete,
                        ..
                    }
                )
            {
                return Use::Mutated;
            } else if let N::Call { callee, .. } = kind {
                return if callee == current {
                    if member { Use::Mutated } else { Use::Read }
                } else {
                    Use::Escaped
                };
            } else if let N::TaggedTemplate { tag, .. } = kind {
                return if tag == current && member {
                    Use::Mutated
                } else {
                    Use::Read
                };
            } else if matches!(
                kind,
                N::New { .. }
                    | N::JsxSpreadAttribute { .. }
                    | N::JsxExpressionContainer { .. }
                    | N::JsxAttribute { .. }
            ) {
                return Use::Escaped;
            } else if let N::VariableDeclarator { target, .. } = kind {
                let name = if matches!(m.kind(target), N::BindingIdentifier { .. }) {
                    Some(target)
                } else {
                    m.children(target)
                        .first()
                        .copied()
                        .filter(|&c| matches!(m.kind(c), N::BindingIdentifier { .. }))
                };
                return match name.and_then(|n| m.bindings.declared(n)) {
                    Some(other) => Use::Alias(other),
                    None => Use::Escaped,
                };
            } else if let N::Property {
                key,
                computed: true,
                ..
            } = kind
                && key == current
            {
                return Use::Read;
            } else if matches!(
                kind,
                N::Parenthesized { .. }
                    | N::ArrayExpression { .. }
                    | N::ObjectExpression { .. }
                    | N::Property { .. }
                    | N::ShorthandProperty { .. }
                    | N::Spread { .. }
                    | N::Conditional { .. }
                    | N::Binary {
                        operator: B::LogicalAnd | B::LogicalOr | B::Nullish | B::Comma,
                        ..
                    }
                    | N::TypeScript {
                        kind: T::NonNull | T::As | T::Satisfies | T::Assertion,
                        ..
                    }
            ) {
                // The value flows on, inside a literal or as the result.
            } else {
                return Use::Read;
            }
            current = parent;
        }
    }

    fn callee(&self, m: &Module<'_, '_>, mut node: NodeId) -> Option<String> {
        let start = node;
        loop {
            let kind = m.kind(node);
            if matches!(kind, N::Identifier { .. }) {
                break;
            }
            let N::Member {
                object,
                computed: false,
                ..
            } = kind
            else {
                return None;
            };
            node = object;
        }
        let text = m.text(start);
        (text.len() <= 128).then(|| text.to_owned())
    }

    fn expr(
        &mut self,
        m: &mut Module<'_, '_>,
        node: NodeId,
        depth: usize,
    ) -> Result<Value, EvaluateError> {
        if depth > MAX_EXPRESSION_DEPTH {
            return Ok(self.unknown(m, node, Reason::DepthLimit));
        }
        if !self.charge(1) {
            return Ok(self.unknown(m, node, Reason::WorkLimit));
        }
        let kind = m.kind(node);
        let value = match kind {
            N::NumberLiteral {
                base,
                kind: NumericKind::Number,
                legacy_octal,
            } => match number::literal(m.text(node), base, legacy_octal) {
                Some(n) => Value::Number(Number(n)),
                None => self.unknown(m, node, Reason::Unsupported),
            },
            N::NumberLiteral {
                kind: NumericKind::BigInt,
                ..
            } => {
                let text: String = m
                    .text(node)
                    .chars()
                    .filter(|&c| c != '_' && c != 'n')
                    .collect();
                Value::BigInt(text)
            }
            N::StringLiteral => match m.tree.string_literal_value(node) {
                Some(s) => Value::String(s),
                None => self.unknown(m, node, Reason::Unsupported),
            },
            N::BooleanLiteral(b) => Value::Bool(b),
            N::NullLiteral => Value::Null,
            N::RegularExpression { .. } => self.unknown(m, node, Reason::RegExp),
            N::TemplateLiteral { parts } => {
                let parts: Vec<TemplatePart> =
                    m.tree.template_parts(parts).unwrap_or_default().to_vec();
                let mut out = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Segment(token) => match m.tree.template_cooked(&token) {
                            Some(s) => out.push_str(&s),
                            None => return Ok(self.unknown(m, node, Reason::Unsupported)),
                        },
                        TemplatePart::Expression(e) => {
                            let v = self.expr(m, e, depth + 1)?;
                            if let Value::Unknown(_) = v {
                                return Ok(v);
                            }
                            match to_string(&v) {
                                Some(s) => out.push_str(&s),
                                None => return Ok(self.unknown(m, e, Reason::Unsupported)),
                            }
                        }
                    }
                    if out.len() > self.limits.max_value_size {
                        return Ok(self.unknown(m, node, Reason::ValueLimit));
                    }
                }
                Value::String(out)
            }
            N::Identifier { .. } => self.identifier(m, node)?,
            N::Parenthesized { expression } => self.expr(m, expression, depth + 1)?,
            N::TypeScript {
                kind: T::NonNull | T::As | T::Satisfies,
                ..
            } => {
                let inner = m.children(node)[0];
                self.expr(m, inner, depth + 1)?
            }
            N::TypeScript {
                kind: T::Assertion, ..
            } => {
                let inner = m.children(node)[1];
                self.expr(m, inner, depth + 1)?
            }
            N::ArrayExpression { elements, .. } => {
                let elements: Vec<NodeId> = m.tree.children(elements).unwrap_or_default().to_vec();
                let mut items = Vec::new();
                for e in elements {
                    let kind = m.kind(e);
                    if matches!(kind, N::Elision) {
                        items.push(Value::Undefined);
                    } else if let N::Spread { argument } = kind {
                        match self.expr(m, argument, depth + 1)? {
                            Value::Array(more) => items.extend(more),
                            Value::String(s) => {
                                items.extend(s.chars().map(|c| Value::String(c.to_string())))
                            }
                            unknown @ Value::Unknown(_) => return Ok(unknown),
                            Value::Undefined
                            | Value::Null
                            | Value::Bool(_)
                            | Value::Number(_)
                            | Value::BigInt(_)
                            | Value::Object(_) => {
                                return Ok(self.unknown(m, e, Reason::Unsupported));
                            }
                        }
                    } else {
                        items.push(self.expr(m, e, depth + 1)?);
                    }
                }
                Value::Array(items)
            }
            N::ObjectExpression { properties, .. } => {
                let properties: Vec<NodeId> =
                    m.tree.children(properties).unwrap_or_default().to_vec();
                self.object(m, &properties, depth)?
            }
            N::Unary { operator, operand } => {
                let v = self.expr(m, operand, depth + 1)?;
                if let Value::Unknown(_) = v {
                    return Ok(v);
                }
                let number = |v: &Value| to_number(v).map(|n| Value::Number(Number(n)));
                let result = match operator {
                    U::LogicalNot => truthy(&v).map(|t| Value::Bool(!t)),
                    U::Typeof => type_of(&v).map(|t| Value::String(t.to_owned())),
                    U::Void => Some(Value::Undefined),
                    U::Plus => number(&v),
                    U::Minus => match &v {
                        Value::BigInt(d) => Some(Value::BigInt(match d.strip_prefix('-') {
                            Some(positive) => positive.to_owned(),
                            None => format!("-{d}"),
                        })),
                        Value::Undefined
                        | Value::Null
                        | Value::Bool(_)
                        | Value::Number(_)
                        | Value::String(_)
                        | Value::Array(_)
                        | Value::Object(_)
                        | Value::Unknown(_) => to_number(&v).map(|n| Value::Number(Number(-n))),
                    },
                    U::BitwiseNot => {
                        to_number(&v).map(|n| Value::Number(Number(f64::from(!to_int32(n)))))
                    }
                    U::Delete => None,
                };
                result.unwrap_or_else(|| self.unknown(m, node, Reason::Unsupported))
            }
            N::Binary {
                operator,
                left,
                right,
            } => self.binary(m, node, operator, left, right, depth)?,
            N::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let t = self.expr(m, test, depth + 1)?;
                match truthy(&t) {
                    Some(true) => self.expr(m, consequent, depth + 1)?,
                    Some(false) => self.expr(m, alternate, depth + 1)?,
                    None => t,
                }
            }
            N::Member {
                object,
                property,
                computed,
                optional,
            } => self.member(m, node, object, property, computed, optional, depth)?,
            N::Call { callee, .. } => {
                let name = self.callee(m, callee);
                self.unknown(m, node, Reason::Call(name))
            }
            N::TaggedTemplate { tag, .. } => {
                let name = self.callee(m, tag);
                self.unknown(m, node, Reason::Call(name))
            }
            N::FunctionExpression { .. } | N::ArrowFunction { .. } | N::ClassExpression { .. } => {
                self.unknown(m, node, Reason::Function)
            }
            N::TypeScript { .. }
            | N::Program { .. }
            | N::Directive { .. }
            | N::ExpressionStatement { .. }
            | N::BlockStatement { .. }
            | N::EmptyStatement
            | N::DebuggerStatement
            | N::IfStatement { .. }
            | N::ForStatement { .. }
            | N::ForInStatement { .. }
            | N::ForOfStatement { .. }
            | N::WhileStatement { .. }
            | N::DoWhileStatement { .. }
            | N::ContinueStatement { .. }
            | N::BreakStatement { .. }
            | N::ReturnStatement { .. }
            | N::WithStatement { .. }
            | N::SwitchStatement { .. }
            | N::SwitchCase { .. }
            | N::LabeledStatement { .. }
            | N::LabelIdentifier { .. }
            | N::ThrowStatement { .. }
            | N::TryStatement { .. }
            | N::CatchClause { .. }
            | N::VariableDeclaration { .. }
            | N::VariableDeclarator { .. }
            | N::FunctionDeclaration { .. }
            | N::FormalParameters { .. }
            | N::FunctionBody { .. }
            | N::ClassDeclaration { .. }
            | N::ClassBody { .. }
            | N::MethodDefinition { .. }
            | N::FieldDefinition { .. }
            | N::StaticBlock { .. }
            | N::IdentifierName { .. }
            | N::PrivateIdentifier { .. }
            | N::This
            | N::Super
            | N::MetaProperty { .. }
            | N::ImportCall { .. }
            | N::Await { .. }
            | N::Yield { .. }
            | N::Elision
            | N::Spread { .. }
            | N::Property { .. }
            | N::ShorthandProperty { .. }
            | N::Assignment { .. }
            | N::Update { .. }
            | N::New { .. }
            | N::BindingIdentifier { .. }
            | N::ArrayPattern { .. }
            | N::ObjectPattern { .. }
            | N::PatternProperty { .. }
            | N::ShorthandPatternProperty { .. }
            | N::RestElement { .. }
            | N::AssignmentPattern { .. }
            | N::ImportDeclaration { .. }
            | N::ImportDefaultSpecifier { .. }
            | N::ImportNamespaceSpecifier { .. }
            | N::ImportSpecifier { .. }
            | N::ImportAttributes { .. }
            | N::ImportAttribute { .. }
            | N::ExportDeclaration { .. }
            | N::ExportNamed { .. }
            | N::ExportSpecifier { .. }
            | N::ExportAll { .. }
            | N::ExportDefault { .. }
            | N::JsxElement { .. }
            | N::JsxOpeningElement { .. }
            | N::JsxClosingElement { .. }
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
            | N::JsxStringLiteral => self.unknown(m, node, Reason::Unsupported),
        };
        let span = Some(m.span(node));
        Ok(match value {
            Value::Array(_) | Value::Object(_) | Value::String(_) => {
                let (size, depth) = measure(&value);
                if size > self.limits.max_value_size || depth > MAX_VALUE_DEPTH {
                    self.unknown_at(m.path, span, Reason::ValueLimit)
                } else {
                    value
                }
            }
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::BigInt(_)
            | Value::Unknown(_) => value,
        })
    }

    fn identifier(&mut self, m: &mut Module<'_, '_>, node: NodeId) -> Result<Value, EvaluateError> {
        let Some(reference) = m.bindings.reference(node) else {
            return Ok(self.unknown(m, node, Reason::Unsupported));
        };
        let (target, name) = (reference.target, reference.name.clone());
        Ok(match target {
            Target::Binding(b) => return self.binding(m, b),
            Target::Dynamic => self.unknown(m, node, Reason::Dynamic),
            Target::Global => match name.as_str() {
                "undefined" => Value::Undefined,
                "NaN" => Value::Number(Number(f64::NAN)),
                "Infinity" => Value::Number(Number(f64::INFINITY)),
                _ => self.unknown(m, node, Reason::Global(name)),
            },
        })
    }

    fn object(
        &mut self,
        m: &mut Module<'_, '_>,
        properties: &[NodeId],
        depth: usize,
    ) -> Result<Value, EvaluateError> {
        let mut entries = Vec::new();
        let mut complete = true;
        for &p in properties {
            let kind = m.kind(p);
            if let N::Property {
                key,
                value,
                computed,
            } = kind
            {
                let key = if computed {
                    property_key(&self.expr(m, key, depth + 1)?)
                } else {
                    self.static_key(m, key)
                };
                // A literal `__proto__` key sets the prototype instead.
                let Some(key) = key.filter(|k| computed || k != "__proto__") else {
                    complete = false;
                    continue;
                };
                let value = self.expr(m, value, depth + 1)?;
                insert(&mut entries, key, value);
            } else if let N::ShorthandProperty { identifier } = kind {
                let key = m.tree.identifier_value(identifier).unwrap_or_default();
                let value = self.expr(m, identifier, depth + 1)?;
                insert(&mut entries, key, value);
            } else if let N::MethodDefinition {
                key,
                computed,
                kind: method,
                ..
            } = kind
            {
                let key = if computed {
                    property_key(&self.expr(m, key, depth + 1)?)
                } else {
                    self.static_key(m, key)
                };
                let Some(key) = key else {
                    complete = false;
                    continue;
                };
                let reason = if matches!(method, MethodKind::Getter | MethodKind::Setter) {
                    Reason::Unsupported
                } else {
                    Reason::Function
                };
                let value = self.unknown(m, p, reason);
                insert(&mut entries, key, value);
            } else if let N::Spread { argument } = kind {
                match self.expr(m, argument, depth + 1)? {
                    Value::Object(other) => {
                        complete &= other.complete;
                        for (k, v) in other.entries {
                            insert(&mut entries, k, v);
                        }
                    }
                    Value::Array(items) => {
                        for (i, v) in items.into_iter().enumerate() {
                            insert(&mut entries, i.to_string(), v);
                        }
                    }
                    Value::String(s) => {
                        for (i, c) in s.chars().enumerate() {
                            insert(&mut entries, i.to_string(), Value::String(c.to_string()));
                        }
                    }
                    Value::Null
                    | Value::Undefined
                    | Value::Bool(_)
                    | Value::Number(_)
                    | Value::BigInt(_) => {}
                    Value::Unknown(_) => complete = false,
                }
            } else {
                complete = false;
            }
        }
        Ok(Value::Object(Object {
            entries: order(entries),
            complete,
        }))
    }

    fn static_key(&self, m: &Module<'_, '_>, key: NodeId) -> Option<String> {
        let kind = m.kind(key);
        if matches!(kind, N::IdentifierName { .. }) {
            m.tree.identifier_value(key)
        } else if matches!(kind, N::StringLiteral) {
            m.tree.string_literal_value(key)
        } else if let N::NumberLiteral {
            base,
            kind: NumericKind::Number,
            legacy_octal,
        } = kind
        {
            number::literal(m.text(key), base, legacy_octal).map(number::to_string)
        } else if matches!(
            kind,
            N::TypeScript {
                kind: T::TypedBinding { .. },
                ..
            }
        ) {
            m.children(key).first().and_then(|&c| self.static_key(m, c))
        } else {
            None
        }
    }

    #[expect(clippy::too_many_arguments, reason = "member expression fields")]
    fn member(
        &mut self,
        m: &mut Module<'_, '_>,
        node: NodeId,
        object: NodeId,
        property: NodeId,
        computed: bool,
        optional: bool,
        depth: usize,
    ) -> Result<Value, EvaluateError> {
        // `process.env` and its properties are run-time environment inputs.
        if let N::Member {
            object: root,
            property: env,
            computed: false,
            ..
        } = m.kind(object)
            && m.text(root) == "process"
            && m.text(env) == "env"
            && m.bindings
                .reference(root)
                .is_some_and(|r| r.target == Target::Global)
        {
            let name = if computed {
                m.tree.string_literal_value(property)
            } else {
                m.tree.identifier_value(property)
            };
            return Ok(self.unknown(m, node, Reason::Environment(name)));
        }
        if !computed
            && m.text(object) == "process"
            && m.text(property) == "env"
            && m.bindings
                .reference(object)
                .is_some_and(|r| r.target == Target::Global)
        {
            return Ok(self.unknown(m, node, Reason::Environment(None)));
        }
        let base = self.expr(m, object, depth + 1)?;
        if let Value::Unknown(_) = base {
            return Ok(base);
        }
        if optional && matches!(base, Value::Null | Value::Undefined) {
            return Ok(Value::Undefined);
        }
        let key = if computed {
            let key = self.expr(m, property, depth + 1)?;
            if let Value::Unknown(_) = key {
                return Ok(key);
            }
            property_key(&key)
        } else if matches!(m.kind(property), N::IdentifierName { .. }) {
            m.tree.identifier_value(property)
        } else {
            None
        };
        let Some(key) = key else {
            return Ok(self.unknown(m, node, Reason::Unsupported));
        };
        Ok(match &base {
            Value::Object(o) => match o.get(&key) {
                Some(v) => v.clone(),
                None if o.complete
                    && !matches!(
                        key.as_str(),
                        "constructor" | "toString" | "valueOf" | "hasOwnProperty" | "__proto__"
                    ) =>
                {
                    Value::Undefined
                }
                None => self.unknown(m, node, Reason::Unsupported),
            },
            Value::Array(items) => {
                if key == "length" {
                    Value::Number(Number(items.len() as f64))
                } else if let Some(i) = array_index(&key) {
                    items.get(i as usize).cloned().unwrap_or(Value::Undefined)
                } else {
                    self.unknown(m, node, Reason::Unsupported)
                }
            }
            Value::String(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                if key == "length" {
                    Value::Number(Number(units.len() as f64))
                } else if let Some(i) = array_index(&key) {
                    match units.get(i as usize) {
                        None => Value::Undefined,
                        Some(&u) => match String::from_utf16(&[u]) {
                            Ok(c) => Value::String(c),
                            Err(_) => self.unknown(m, node, Reason::Unsupported),
                        },
                    }
                } else {
                    self.unknown(m, node, Reason::Unsupported)
                }
            }
            Value::Undefined
            | Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::BigInt(_)
            | Value::Unknown(_) => self.unknown(m, node, Reason::Unsupported),
        })
    }

    fn binary(
        &mut self,
        m: &mut Module<'_, '_>,
        node: NodeId,
        operator: B,
        left: NodeId,
        right: NodeId,
        depth: usize,
    ) -> Result<Value, EvaluateError> {
        if matches!(operator, B::LogicalAnd | B::LogicalOr | B::Nullish) {
            let l = self.expr(m, left, depth + 1)?;
            let take_left = if operator == B::LogicalAnd {
                truthy(&l).map(|t| !t)
            } else if operator == B::LogicalOr {
                truthy(&l)
            } else if matches!(l, Value::Unknown(_)) {
                None
            } else {
                Some(!matches!(l, Value::Null | Value::Undefined))
            };
            return match take_left {
                None => Ok(l),
                Some(true) => Ok(l),
                Some(false) => self.expr(m, right, depth + 1),
            };
        }
        if matches!(operator, B::Comma | B::In | B::Instanceof) {
            return Ok(self.unknown(m, node, Reason::Unsupported));
        }
        let l = self.expr(m, left, depth + 1)?;
        if let Value::Unknown(_) = l {
            return Ok(l);
        }
        let r = self.expr(m, right, depth + 1)?;
        if let Value::Unknown(_) = r {
            return Ok(r);
        }
        let primitive = |v: &Value| !matches!(v, Value::Array(_) | Value::Object(_));
        if !primitive(&l)
            || !primitive(&r)
            || matches!(l, Value::BigInt(_))
            || matches!(r, Value::BigInt(_))
        {
            return Ok(self.unknown(m, node, Reason::Unsupported));
        }
        let num = |v: &Value| to_number(v).unwrap_or(f64::NAN);
        let n = |x: f64| Value::Number(Number(x));
        Ok(match operator {
            B::Add => {
                if matches!(l, Value::String(_)) || matches!(r, Value::String(_)) {
                    match (to_string(&l), to_string(&r)) {
                        (Some(a), Some(b)) if a.len() + b.len() <= self.limits.max_value_size => {
                            Value::String(a + &b)
                        }
                        _ => self.unknown(m, node, Reason::ValueLimit),
                    }
                } else {
                    n(num(&l) + num(&r))
                }
            }
            B::Subtract => n(num(&l) - num(&r)),
            B::Multiply => n(num(&l) * num(&r)),
            B::Divide => n(num(&l) / num(&r)),
            B::Remainder => n(num(&l) % num(&r)),
            B::Exponent => n(num(&l).powf(num(&r))),
            B::BitwiseAnd => n(f64::from(to_int32(num(&l)) & to_int32(num(&r)))),
            B::BitwiseOr => n(f64::from(to_int32(num(&l)) | to_int32(num(&r)))),
            B::BitwiseXor => n(f64::from(to_int32(num(&l)) ^ to_int32(num(&r)))),
            B::ShiftLeft => n(f64::from(
                to_int32(num(&l)).wrapping_shl(to_int32(num(&r)) as u32 & 31),
            )),
            B::ShiftRight => n(f64::from(
                to_int32(num(&l)) >> (to_int32(num(&r)) as u32 & 31),
            )),
            B::ShiftRightUnsigned => n(f64::from(
                (to_int32(num(&l)) as u32) >> (to_int32(num(&r)) as u32 & 31),
            )),
            B::StrictEqual => Value::Bool(strict_equal(&l, &r)),
            B::StrictNotEqual => Value::Bool(!strict_equal(&l, &r)),
            B::Equal | B::NotEqual => {
                let equal = match (&l, &r) {
                    (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
                    (Value::Null | Value::Undefined, _) | (_, Value::Null | Value::Undefined) => {
                        false
                    }
                    _ if std::mem::discriminant(&l) == std::mem::discriminant(&r) => {
                        strict_equal(&l, &r)
                    }
                    _ => num(&l) == num(&r),
                };
                Value::Bool(equal == (operator == B::Equal))
            }
            B::LessThan | B::GreaterThan | B::LessThanEqual | B::GreaterThanEqual => {
                let ordering = match (&l, &r) {
                    (Value::String(a), Value::String(b)) => {
                        Some(a.encode_utf16().cmp(b.encode_utf16()))
                    }
                    _ => num(&l).partial_cmp(&num(&r)),
                };
                Value::Bool(match (operator, ordering) {
                    (_, None) => false,
                    (B::LessThan, Some(o)) => o.is_lt(),
                    (B::GreaterThan, Some(o)) => o.is_gt(),
                    (B::LessThanEqual, Some(o)) => o.is_le(),
                    (_, Some(o)) => o.is_ge(),
                })
            }
            B::Comma | B::In | B::Instanceof | B::LogicalAnd | B::LogicalOr | B::Nullish => {
                self.unknown(m, node, Reason::Unsupported)
            }
        })
    }
}

fn strict_equal(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::Number(Number(a)), Value::Number(Number(b))) => a == b,
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}
