//! A partial static evaluator over a safe subset of JavaScript and
//! TypeScript, for configuration files and constant declarations.
//!
//! It evaluates literals, array and object literals with spreads of known
//! values, template literals with known parts, primitive operators,
//! conditionals, member access on known values, `const` bindings and
//! unwritten `let` and `var` bindings without mutation, and the exports of
//! local modules, including imports, re-exports, `export =`, and CommonJS
//! `module.exports`. Everything else evaluates to [`Value::Unknown`] with a
//! [`Reason`]. Nothing is executed: calls, functions, environment variables,
//! and installed packages are reasons, never results.

mod eval;
mod number;

use std::fmt;

use frameshift_core::{SourcePath, Span};

pub use eval::Evaluator;

/// A JavaScript number, compared by its exact bits so that values are
/// deterministic and `NaN` equals itself.
#[derive(Clone, Copy, Debug)]
pub struct Number(pub f64);

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Number {}

impl fmt::Display for Number {
    /// Formats as `Number::prototype.toString` does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&number::to_string(self.0))
    }
}

/// An object's own enumerable string-keyed properties, in JavaScript's
/// own-key order: array-index keys ascending, then insertion order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    /// Properties.
    pub entries: Vec<(String, Value)>,
    /// Whether every property is known: a spread of an unknown value, a
    /// computed key that is not known, or `__proto__` makes it incomplete.
    pub complete: bool,
}

impl Object {
    /// A property by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// An evaluated value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A BigInt, by its literal spelling without the `n` suffix.
    BigInt(String),
    /// A string.
    String(String),
    /// An array; holes are `undefined`.
    Array(Vec<Value>),
    /// An object.
    Object(Object),
    /// A value that the safe subset cannot establish.
    Unknown(Unknown),
}

impl Value {
    /// Whether the value and everything inside it is known.
    pub fn is_known(&self) -> bool {
        match self {
            Self::Unknown(_) => false,
            Self::Array(items) => items.iter().all(Self::is_known),
            Self::Object(object) => {
                object.complete && object.entries.iter().all(|(_, v)| v.is_known())
            }
            Self::Undefined
            | Self::Null
            | Self::Bool(_)
            | Self::Number(_)
            | Self::BigInt(_)
            | Self::String(_) => true,
        }
    }

    /// Every unknown part, in depth-first order.
    pub fn unknowns(&self) -> Vec<&Unknown> {
        let mut out = Vec::new();
        let mut pending = vec![self];
        while let Some(value) = pending.pop() {
            match value {
                Self::Unknown(unknown) => out.push(unknown),
                Self::Array(items) => pending.extend(items.iter().rev()),
                Self::Object(object) => pending.extend(object.entries.iter().rev().map(|(_, v)| v)),
                Self::Undefined
                | Self::Null
                | Self::Bool(_)
                | Self::Number(_)
                | Self::BigInt(_)
                | Self::String(_) => {}
            }
        }
        out
    }
}

/// Why a value is unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// A syntax form outside the safe subset, such as `new` or assignment.
    Unsupported,
    /// A function or method call, with the callee's source spelling when it
    /// is a name or member chain.
    Call(Option<String>),
    /// A function, arrow function, method, getter, or class.
    Function,
    /// A regular-expression literal, which creates a mutable object.
    RegExp,
    /// `process.env`, with the variable name when it is literal.
    Environment(Option<String>),
    /// A global name other than `undefined`, `NaN`, and `Infinity`.
    Global(String),
    /// A name that `with` or a sloppy direct `eval` may supply.
    Dynamic,
    /// A binding that is written, possibly mutated, or passed to code that
    /// may mutate it.
    Mutable,
    /// An import from a package, by package and imported name; installed
    /// contents are never read.
    Package {
        /// The package name.
        package: String,
        /// The imported name, `default`, or `*` for a namespace.
        name: String,
    },
    /// A module reference without an established local module.
    Unresolved,
    /// A module that does not export the name.
    NotExported,
    /// A module that is not UTF-8 text or does not parse strictly.
    Syntax,
    /// A circular dependency between bindings or modules.
    Cycle,
    /// Binding, module, or expression nesting beyond the depth limit.
    DepthLimit,
    /// The work limit was reached.
    WorkLimit,
    /// A value larger or deeper than the value limits.
    ValueLimit,
}

/// An unknown value with the reason and where it arose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unknown {
    /// The reason.
    pub reason: Reason,
    /// The module.
    pub module: SourcePath,
    /// The span of the expression or name, if any.
    pub span: Option<Span>,
}

/// Explicit evaluation limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvaluateLimits {
    /// Charged units: nodes visited and values built. Defaults to 2^24.
    pub max_work: u64,
    /// Nesting of binding and module evaluations. Defaults to 64.
    pub max_depth: usize,
    /// Size of one value: string bytes plus one per element and property.
    /// Defaults to 1 MiB.
    pub max_value_size: usize,
}

impl Default for EvaluateLimits {
    fn default() -> Self {
        Self {
            max_work: 1 << 24,
            max_depth: 64,
            max_value_size: 1 << 20,
        }
    }
}

/// A failure that ends evaluation, without a partial value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluateError {
    /// The resolver could not be built.
    Modules(frameshift_modules::ModuleError),
    /// The snapshot changed or became untrustworthy.
    Snapshot(frameshift_input::SnapshotError),
    /// Storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for EvaluateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Modules(e) => e.fmt(f),
            Self::Snapshot(_) => f.write_str("evaluation snapshot failed"),
            Self::AllocationFailed => f.write_str("evaluation allocation failed"),
        }
    }
}

impl std::error::Error for EvaluateError {}
