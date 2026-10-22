//! Speculative readings of the TypeScript grammar.
//!
//! The TypeScript dialects try some readings before others and roll a failed
//! reading back. `snapshot` classifies every parser field once: what decides
//! the outcome of a speculation, what a rollback restores, and why the rest
//! needs no restoring. The record key and the rollback both derive from it.

use super::{Context, CoverMarks, ParseError, ParseErrorKind, Parser};

// A speculative reading that the TypeScript grammar tries before another
// one: an arrow function head, the lookahead for a function type, type
// arguments in an expression, or a constraint after `infer`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Speculation {
    ArrowHead { start: usize, is_async: bool },
    FunctionTypeStart,
    Instantiation,
    InferConstraint,
}

// The parser state that, together with the source, decides the outcome of a
// speculation.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Grammar {
    cursor: usize,
    context: Context,
    no_in: bool,
    template_expression_depth: usize,
    disallow_conditional: bool,
}

// A speculation and the state it starts in. The outcome repeats in the same
// state, so the parser speculates once per attempt and records failures and
// lookahead answers. Without the record, nested constructs that each start a
// speculation, such as `([([a])])` or `f<([[f<([[a]])>]])>`, are parsed once
// inside every failed or discarded reading and again in the next one, which
// at least doubles the work per level (the compiler keeps a set of positions
// that are not arrow functions for the same reason).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Attempt {
    speculation: Speculation,
    grammar: Grammar,
}

// Everything a failed speculation restores.
struct Snapshot {
    grammar: Grammar,
    last_end: usize,
    line_break_before: bool,
    token_count: usize,
    cover: CoverMarks,
    nodes: usize,
    children: usize,
    template_parts: usize,
    comments: usize,
    bound_names: usize,
    expression_only_errors: usize,
    // A class that a speculation enters ends inside it and moves its
    // unresolved references to the enclosing class, so only the innermost
    // class at the snapshot gains references.
    class_references: usize,
    // Checked, not restored: the constructs that change them restore them on
    // every path, including errors.
    classes: usize,
    scopes: usize,
}

impl Parser<'_> {
    // The pattern names every field without `..`, so a new parser field does
    // not compile until it is classified here.
    fn snapshot(&self) -> Snapshot {
        let Self {
            // Observations of the outer program list, unchanged by speculation.
            program_item_start: _,
            program_strict: _,
            // Fixed for the whole parse.
            source: _,
            text: _,
            limits: _,
            goal: _,
            jsx: _,
            typescript: _,
            ambient: _,
            // Decide the outcome; restored.
            cursor,
            context,
            no_in,
            template_expression_depth,
            ts_disallow_conditional,
            // Restored.
            last_end,
            line_break_before,
            token_count,
            cover,
            nodes,
            children,
            template_parts,
            comments,
            bound_names,
            expression_only_errors,
            classes,
            // Balanced on every path: `enter` and `leave` bound the depth,
            // function frames replace the jump targets, scopes are pushed and
            // popped in pairs, and namespace bodies swap the module state.
            // Speculation declares no bindings, because the TypeScript
            // dialects record none.
            depth: _,
            jumps: _,
            scopes,
            module: _,
            // Not restored: failed work stays counted, so the token limit
            // bounds all speculation.
            ts_work: _,
            // Caches of pure results keyed by offset, and empty buffers.
            trivia_end: _,
            peeked_identifier: _,
            peeked_punctuator: _,
            list_buffers: _,
            // The record of outcomes.
            speculations: _,
        } = self;
        Snapshot {
            grammar: Grammar {
                cursor: *cursor,
                context: *context,
                no_in: *no_in,
                template_expression_depth: *template_expression_depth,
                disallow_conditional: *ts_disallow_conditional,
            },
            last_end: *last_end,
            line_break_before: *line_break_before,
            token_count: *token_count,
            cover: *cover,
            nodes: nodes.len(),
            children: children.len(),
            template_parts: template_parts.len(),
            comments: comments.len(),
            bound_names: bound_names.len(),
            expression_only_errors: expression_only_errors.len(),
            class_references: classes.last().map_or(0, |class| class.references.len()),
            classes: classes.len(),
            scopes: scopes.depth(),
        }
    }

    fn rollback(&mut self, snapshot: Snapshot) {
        debug_assert_eq!(self.classes.len(), snapshot.classes);
        debug_assert_eq!(self.scopes.depth(), snapshot.scopes);
        let Grammar {
            cursor,
            context,
            no_in,
            template_expression_depth,
            disallow_conditional,
        } = snapshot.grammar;
        self.cursor = cursor;
        self.context = context;
        self.no_in = no_in;
        self.template_expression_depth = template_expression_depth;
        self.ts_disallow_conditional = disallow_conditional;
        self.last_end = snapshot.last_end;
        self.line_break_before = snapshot.line_break_before;
        self.token_count = snapshot.token_count;
        self.cover = snapshot.cover;
        self.nodes.truncate(snapshot.nodes);
        self.children.truncate(snapshot.children);
        self.template_parts.truncate(snapshot.template_parts);
        self.comments.truncate(snapshot.comments);
        self.bound_names.truncate(snapshot.bound_names);
        self.expression_only_errors
            .truncate(snapshot.expression_only_errors);
        if let Some(class) = self.classes.last_mut() {
            class.references.truncate(snapshot.class_references);
        }
    }

    pub(super) fn ts_attempt(&self, speculation: Speculation) -> Attempt {
        Attempt {
            speculation,
            grammar: self.snapshot().grammar,
        }
    }

    pub(super) fn ts_record(&mut self, attempt: Attempt, outcome: bool) -> Result<(), ParseError> {
        self.speculations
            .try_reserve(1)
            .map_err(|_| self.error(attempt.grammar.cursor, ParseErrorKind::AllocationFailed))?;
        self.speculations.insert(attempt, outcome);
        Ok(())
    }

    // Speculates unless the same attempt failed before, and records a
    // failure.
    pub(super) fn ts_try_once<T>(
        &mut self,
        speculation: Speculation,
        parse: impl FnOnce(&mut Self) -> Result<Option<T>, ParseError>,
    ) -> Result<Option<T>, ParseError> {
        let attempt = self.ts_attempt(speculation);
        if self.speculations.get(&attempt) == Some(&false) {
            return Ok(None);
        }
        let result = self.ts_try(parse)?;
        if result.is_none() {
            self.ts_record(attempt, false)?;
        }
        Ok(result)
    }

    // Speculation is restricted to type syntax and function heads. Bodies are
    // parsed only after a head commits. Failed work still consumes `ts_work`.
    pub(super) fn ts_try<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<Option<T>, ParseError>,
    ) -> Result<Option<T>, ParseError> {
        let snapshot = self.snapshot();
        let error = match parse(self) {
            Ok(Some(value)) => return Ok(Some(value)),
            Ok(None) => None,
            Err(error) => Some(error),
        };
        self.rollback(snapshot);
        // A syntax error only rejects this reading; exhausted limits and
        // internal errors end the parse.
        match error {
            Some(error)
                if matches!(
                    error.kind,
                    ParseErrorKind::ResourceLimit
                        | ParseErrorKind::AllocationFailed
                        | ParseErrorKind::InternalInvariant(_)
                ) =>
            {
                Err(error)
            }
            Some(_) | None => Ok(None),
        }
    }
}
