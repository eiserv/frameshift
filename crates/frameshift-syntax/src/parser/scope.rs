//! Transient declaration scopes for the redeclaration early errors.
//!
//! Each scope maps a name's StringValue to flags describing how the scope
//! binds it. A `var` declaration is recorded in every scope between its
//! position and the enclosing function, so a later lexical declaration in any
//! of them can see the conflict. Scopes are discarded after parsing; the
//! semantic layer builds its own scope graph.

use std::collections::HashMap;

use super::{ParseError, ParseErrorKind, Parser, Strictness, names::Name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScopeKind {
    // A script's or function body's top level, where functions are var-scoped.
    Function,
    // A module's top level, where functions are lexically scoped.
    Module,
    // A class static block's top level, which behaves like a function body.
    StaticBlock,
    // A block, `switch` case block, or loop head.
    Block,
    // A catch clause, holding its parameter and the handler block's declarations.
    Catch { simple_parameter: bool },
}

impl ScopeKind {
    fn is_var_scope(self) -> bool {
        match self {
            Self::Function | Self::Module | Self::StaticBlock => true,
            Self::Block | Self::Catch { .. } => false,
        }
    }
}

// How a scope binds one name. Several flags may combine.
#[derive(Clone, Copy, Default)]
struct Binding {
    lexical: bool,
    // A plain function declared in a sloppy block, which Annex B lets other
    // such declarations repeat.
    sloppy_function: bool,
    // A `var` declared here or inside, or a var-scoped function declaration.
    var: bool,
    parameter: bool,
    catch_parameter: bool,
}

impl Binding {
    fn any_lexical(self) -> bool {
        self.lexical || self.sloppy_function
    }
}

struct Scope<'source> {
    kind: ScopeKind,
    names: HashMap<Name<'source>, Binding>,
}

#[derive(Default)]
pub(super) struct Scopes<'source> {
    stack: Vec<Scope<'source>>,
}

/// How a declaration binds its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Declaration {
    // `var`, a `var` loop head, or a for-in `var` head.
    Var,
    // `let`, `const`, `using`, `await using`, `class`, or an import.
    Lexical,
    // A function declaration; `plain` excludes async and generator functions.
    Function { plain: bool },
    // A formal parameter. Duplicates are checked separately.
    Parameter,
    // A catch parameter.
    CatchParameter,
}

impl<'source> Parser<'source> {
    pub(super) fn push_scope(&mut self, kind: ScopeKind) -> Result<(), ParseError> {
        self.scopes
            .stack
            .try_reserve(1)
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.scopes.stack.push(Scope {
            kind,
            names: HashMap::new(),
        });
        Ok(())
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.stack.pop();
    }

    // Reports whether the top-level module scope binds `name`.
    pub(super) fn module_scope_declares(&self, name: &str) -> bool {
        self.scopes.stack.first().is_some_and(|scope| {
            scope
                .names
                .get(name)
                .is_some_and(|binding| binding.lexical || binding.var)
        })
    }

    // Records a declaration of `name` at `offset` in the innermost scope, or
    // for `var` in every scope up to the enclosing function, and reports the
    // redeclaration early errors of the JavaScript dialects.
    pub(super) fn declare(
        &mut self,
        name: Name<'source>,
        offset: usize,
        declaration: Declaration,
    ) -> Result<(), ParseError> {
        // TypeScript declares overloads, merges interfaces, namespaces, and
        // enums, and allows ambient redeclaration, so these dialects report
        // no redeclaration errors and record no bindings: a genuine duplicate
        // is a checker diagnostic, not a grammar error.
        if self.typescript {
            return Ok(());
        }
        let strict = self.context.strict;
        let duplicate = self.error(offset, ParseErrorKind::DuplicateDeclaration);
        let requires_context = self.error(offset, ParseErrorKind::StrictModeRequiresContext);
        let Some(innermost) = self.scopes.stack.len().checked_sub(1) else {
            return Ok(());
        };
        match declaration {
            Declaration::Var => {
                for index in (0..=innermost).rev() {
                    let scope = &mut self.scopes.stack[index];
                    let binding = scope.names.entry(name.clone()).or_default();
                    if binding.any_lexical() {
                        return Err(duplicate);
                    }
                    // Annex B lets `var` redeclare a simple catch parameter.
                    if binding.catch_parameter
                        && scope.kind
                            == (ScopeKind::Catch {
                                simple_parameter: false,
                            })
                    {
                        return Err(duplicate);
                    }
                    binding.var = true;
                    if scope.kind.is_var_scope() {
                        break;
                    }
                }
                Ok(())
            }
            Declaration::Function { plain } => {
                let scope = &mut self.scopes.stack[innermost];
                let binding = scope.names.entry(name).or_default();
                match scope.kind {
                    // Top-level functions of scripts, functions, and static
                    // blocks behave like `var`.
                    ScopeKind::Function | ScopeKind::StaticBlock => {
                        if binding.lexical {
                            return Err(duplicate);
                        }
                        binding.var = true;
                        Ok(())
                    }
                    ScopeKind::Module | ScopeKind::Block | ScopeKind::Catch { .. } => {
                        // Annex B lets sloppy blocks repeat plain function
                        // declarations, if nothing else binds the name.
                        let candidate = plain && scope.kind != ScopeKind::Module;
                        let conflict = binding.any_lexical()
                            || binding.var
                            || binding.parameter
                            || binding.catch_parameter;
                        if conflict {
                            let only_functions = candidate
                                && binding.sloppy_function
                                && !binding.lexical
                                && !binding.var
                                && !binding.parameter
                                && !binding.catch_parameter;
                            if !only_functions {
                                return Err(duplicate);
                            }
                            match strict {
                                Strictness::Sloppy => {}
                                Strictness::Strict => return Err(duplicate),
                                Strictness::Unknown => return Err(requires_context),
                            }
                        }
                        if candidate {
                            binding.sloppy_function = true;
                        } else {
                            binding.lexical = true;
                        }
                        Ok(())
                    }
                }
            }
            Declaration::Lexical => {
                let binding = self.scopes.stack[innermost].names.entry(name).or_default();
                if binding.any_lexical()
                    || binding.var
                    || binding.parameter
                    || binding.catch_parameter
                {
                    return Err(duplicate);
                }
                binding.lexical = true;
                Ok(())
            }
            Declaration::Parameter => {
                let binding = self.scopes.stack[innermost].names.entry(name).or_default();
                binding.parameter = true;
                Ok(())
            }
            Declaration::CatchParameter => {
                let binding = self.scopes.stack[innermost].names.entry(name).or_default();
                if binding.catch_parameter {
                    return Err(duplicate);
                }
                binding.catch_parameter = true;
                Ok(())
            }
        }
    }
}
