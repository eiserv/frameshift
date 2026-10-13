//! Recursive-descent JavaScript parser over a flat syntax-node arena.
//!
//! `expression` holds the operator-precedence grammar, `pattern` the cover
//! grammar reinterpretation, `function` and `class` the function-like forms,
//! `statement` and `module` the source-file grammar, `names` the
//! context-dependent identifier rules, `typescript` the TypeScript grammar
//! of the TypeScript and TSX dialects, and `scope` the transient declaration
//! scopes behind early errors. Context and scopes live only for one parse.

mod class;
mod error;
mod expression;
mod function;
mod jsx;
mod module;
mod names;
mod pattern;
mod scope;
mod statement;
mod typescript;

use std::collections::HashSet;

use frameshift_core::{SourceText, Span};

pub use error::{ParseError, ParseErrorKind, StrictModeError};

use crate::{
    ast::{
        ChildList, NodeId, SyntaxNode, SyntaxNodeKind, SyntaxTree, TemplatePart, TemplatePartList,
    },
    identifier::{IdentifierToken, scan_js_identifier},
    lexical::{Comment, LexicalError, LexicalLimits, SourceGoal, scan_hashbang, scan_js_trivia},
    punctuator::{
        GreaterThanMode, JsLexicalGoal, Punctuator, PunctuatorContext, PunctuatorToken,
        scan_js_punctuator,
    },
    unicode::is_js_identifier_start,
};

use self::{
    class::ClassScope, module::ModuleState, names::Name, scope::Scopes, typescript::ArrowAttempt,
};

/// Explicit resource bounds for one parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    /// Bounds for individual lexical scans.
    pub lexical: LexicalLimits,
    /// Maximum number of syntax nodes retained, default 100,000.
    pub max_nodes: usize,
    /// Maximum consumed lexical tokens, default 200,000.
    pub max_tokens: usize,
    /// Maximum nested expression, statement, function, class, and JSX element
    /// calls, default 256.
    pub max_nesting_depth: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            lexical: LexicalLimits::default(),
            max_nodes: 100_000,
            max_tokens: 200_000,
            max_nesting_depth: 256,
        }
    }
}

/// The JavaScript language variant that source text is parsed as.
///
/// The caller selects the dialect, for example from the file name and the
/// project configuration. The parser never infers it from the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsDialect {
    /// ECMAScript without syntax extensions.
    JavaScript,
    /// ECMAScript with JSX elements and fragments as primary expressions.
    Jsx,
    /// TypeScript, including angle-bracket assertions, without JSX.
    TypeScript,
    /// TypeScript with JSX and the TSX generic-arrow disambiguation rules.
    Tsx,
    /// A TypeScript declaration file, such as `next-env.d.ts`: the TypeScript
    /// grammar without JSX, with the whole file in an ambient context as if
    /// every declaration were marked `declare`, so `const` declarations may
    /// omit their initializers.
    TypeScriptDeclaration,
}

/// Parses one complete JavaScript expression into a span-precise arena.
///
/// The expression is parsed as if it were outside any function, class, or
/// method, with an unknown source goal and unknown strictness. Function and
/// class expressions are parsed with their own bodies. Every construct whose
/// validity depends on the unknown context fails closed with a
/// `...RequiresContext` error: reserved words that only strict code reserves,
/// `await` and `yield` outside async functions and generators,
/// strict-mode-dependent forms, HTML-like comments and `import.meta`,
/// `new.target`, `super`, and private names outside a class. With
/// [`JsDialect::Jsx`], JSX elements and fragments are primary expressions.
/// The TypeScript dialects add type annotations, `as` and `satisfies`
/// expressions, assertions, and instantiation expressions.
/// Leading and trailing trivia is consumed and comments are retained in
/// source order. Any error returns no tree. Parsing is deterministic and
/// bounded by `limits`.
pub fn parse_js_expression(
    source: &SourceText,
    offset: usize,
    dialect: JsDialect,
    limits: ParseLimits,
) -> Result<SyntaxTree<'_>, ParseError> {
    let mut parser = Parser::new(source, Goal::Unknown, dialect, limits);
    parser.skip_trivia_at(offset)?;
    let root = parser.parse_expression(0)?;
    parser.check_expression_only_errors()?;
    parser.skip_trivia()?;
    if parser.cursor < parser.text.len() {
        return Err(parser.error(parser.cursor, ParseErrorKind::TrailingInput));
    }
    parser.finish(offset, root)
}

/// Parses complete Script or Module source text into a span-precise arena.
///
/// The root is a [`SyntaxNodeKind::Program`] node. The complete ECMAScript
/// syntactic grammar is supported, including sloppy mode and the Annex B
/// web-compatibility grammar in scripts, and the ECMAScript early errors,
/// which are checked with transient scopes. [`JsDialect::Jsx`] adds the JSX
/// grammar in either goal; [`JsDialect::TypeScript`], [`JsDialect::Tsx`], and
/// [`JsDialect::TypeScriptDeclaration`] add the TypeScript type, declaration,
/// and expression grammar without type checking and report no redeclaration
/// early errors, because TypeScript declares overloads and merges
/// declarations while a genuine duplicate is a checker diagnostic. A leading hashbang comment is retained, also after
/// a byte order mark. Any error returns no tree, so a file with a syntax error
/// is never partially transformed. Parsing is deterministic and bounded by
/// `limits`.
pub fn parse_js_program(
    source: &SourceText,
    goal: SourceGoal,
    dialect: JsDialect,
    limits: ParseLimits,
) -> Result<SyntaxTree<'_>, ParseError> {
    let mut parser = Parser::new(source, Goal::from(goal), dialect, limits);
    let root = parser.parse_program(goal, dialect)?;
    parser.check_expression_only_errors()?;
    parser.finish(0, root)
}

// The internal source goal, which is unknown for context-free expressions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Goal {
    Script,
    Module,
    Unknown,
}

impl From<SourceGoal> for Goal {
    fn from(goal: SourceGoal) -> Self {
        match goal {
            SourceGoal::Script => Self::Script,
            SourceGoal::Module => Self::Module,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Strictness {
    Sloppy,
    Strict,
    Unknown,
}

// Whether a construct that needs an enclosing function or method is permitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Permission {
    Allowed,
    Denied,
    Unknown,
}

// Grammar parameters and placement facts of the code being parsed. Function
// and class boundaries save and replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Context {
    strict: Strictness,
    // `return` is permitted.
    in_function: bool,
    // `yield` is a keyword: generator bodies and parameters.
    yield_keyword: bool,
    // `await` is a keyword: async bodies and parameters, and module top level.
    await_keyword: bool,
    // `await` is reserved without being an expression: class static blocks.
    await_reserved: bool,
    // Yield and await expressions are errors: formal parameter lists.
    in_parameters: bool,
    new_target: Permission,
    super_property: Permission,
    super_call: Permission,
    // `arguments` is an error: class field initializers and static blocks.
    arguments_forbidden: bool,
}

// Label and loop bookkeeping, reset at every function and static block.
#[derive(Default)]
struct JumpTargets<'source> {
    labels: Vec<Label<'source>>,
    breakable_depth: u32,
    iteration_depth: u32,
}

struct Label<'source> {
    name: Name<'source>,
    // Offset of the labelled item; consecutive labels share their item.
    item_start: usize,
    is_loop: bool,
}

// First offsets of constructs that a cover grammar must reject once it
// becomes arrow parameters.
#[derive(Clone, Copy, Default)]
struct CoverMarks {
    // A yield or await expression.
    yield_await: Option<usize>,
    // `await` used as an identifier, invalid in async arrow heads.
    await_identifier: Option<usize>,
}

impl CoverMarks {
    fn merge(self, other: Self) -> Self {
        Self {
            yield_await: earliest(self.yield_await, other.yield_await),
            await_identifier: earliest(self.await_identifier, other.await_identifier),
        }
    }
}

fn earliest(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

struct Parser<'source> {
    source: &'source SourceText,
    text: &'source str,
    limits: ParseLimits,
    goal: Goal,
    // Whether `<` at the start of a primary expression begins JSX.
    jsx: bool,
    typescript: bool,
    ambient: bool,
    ts_work: usize,
    ts_disallow_conditional: bool,
    cursor: usize,
    // End of the last consumed token, before any trivia that a peek skipped.
    last_end: usize,
    depth: usize,
    token_count: usize,
    template_expression_depth: usize,
    // Whether the trivia before the next token contains a line terminator.
    line_break_before: bool,
    // Lookahead caches. The grammar peeks at the next token several times
    // before consuming it, and scans are pure functions of the offset and the
    // requested context, so a repeated peek reuses the last result. A skip
    // stops before a non-trivia character, so skipping again at the offset
    // where the last successful skip stopped changes nothing.
    trivia_end: Option<usize>,
    peeked_identifier: Option<(usize, IdentifierToken)>,
    peeked_punctuator: Option<(usize, JsLexicalGoal, Option<PunctuatorToken>)>,
    // The `[~In]` grammar parameter: `in` is not a binary operator here.
    no_in: bool,
    context: Context,
    jumps: JumpTargets<'source>,
    cover: CoverMarks,
    scopes: Scopes<'source>,
    classes: Vec<ClassScope<'source>>,
    module: ModuleState<'source>,
    nodes: Vec<SyntaxNode>,
    children: Vec<NodeId>,
    template_parts: Vec<TemplatePart>,
    comments: Vec<Comment>,
    // Emptied buffers for child lists. A list is collected in a buffer before
    // it is appended to the child arena; reusing the buffers saves an
    // allocation per list.
    list_buffers: Vec<Vec<NodeId>>,
    // Names declared by the parameter list or pattern being converted.
    bound_names: Vec<Span>,
    // Errors that apply only if the object literal stays an expression.
    expression_only_errors: Vec<(NodeId, ParseError)>,
    // TypeScript arrow function heads that failed, with the state their
    // outcome depends on (`typescript.rs`).
    failed_arrow_heads: HashSet<ArrowAttempt>,
}

// Saved state for a speculative look at later tokens.
struct Checkpoint {
    cursor: usize,
    last_end: usize,
    line_break_before: bool,
    token_count: usize,
    comments: usize,
}

impl<'source> Parser<'source> {
    fn new(
        source: &'source SourceText,
        goal: Goal,
        dialect: JsDialect,
        limits: ParseLimits,
    ) -> Self {
        let unknown = goal == Goal::Unknown;
        let context = Context {
            strict: match goal {
                Goal::Script => Strictness::Sloppy,
                Goal::Module => Strictness::Strict,
                Goal::Unknown => Strictness::Unknown,
            },
            in_function: false,
            yield_keyword: false,
            await_keyword: goal == Goal::Module,
            await_reserved: false,
            in_parameters: false,
            new_target: if unknown {
                Permission::Unknown
            } else {
                Permission::Denied
            },
            super_property: if unknown {
                Permission::Unknown
            } else {
                Permission::Denied
            },
            super_call: if unknown {
                Permission::Unknown
            } else {
                Permission::Denied
            },
            arguments_forbidden: false,
        };
        Self {
            source,
            text: source.as_str(),
            limits,
            goal,
            jsx: match dialect {
                JsDialect::JavaScript
                | JsDialect::TypeScript
                | JsDialect::TypeScriptDeclaration => false,
                JsDialect::Jsx | JsDialect::Tsx => true,
            },
            typescript: matches!(
                dialect,
                JsDialect::TypeScript | JsDialect::Tsx | JsDialect::TypeScriptDeclaration
            ),
            ambient: dialect == JsDialect::TypeScriptDeclaration,
            ts_work: 0,
            ts_disallow_conditional: false,
            cursor: 0,
            last_end: 0,
            depth: 0,
            token_count: 0,
            template_expression_depth: 0,
            line_break_before: false,
            trivia_end: None,
            peeked_identifier: None,
            peeked_punctuator: None,
            no_in: false,
            context,
            jumps: JumpTargets::default(),
            cover: CoverMarks::default(),
            scopes: Scopes::default(),
            classes: Vec::new(),
            module: ModuleState::default(),
            nodes: Vec::new(),
            children: Vec::new(),
            template_parts: Vec::new(),
            comments: Vec::new(),
            list_buffers: Vec::new(),
            bound_names: Vec::new(),
            expression_only_errors: Vec::new(),
            failed_arrow_heads: HashSet::new(),
        }
    }

    fn finish(self, offset: usize, root: NodeId) -> Result<SyntaxTree<'source>, ParseError> {
        let consumed_span = self.source_span(offset, self.cursor)?;
        Ok(SyntaxTree {
            source: self.source,
            nodes: self.nodes,
            children: self.children,
            template_parts: self.template_parts,
            comments: self.comments,
            root,
            consumed_span,
        })
    }

    // A leading byte order mark is an encoding signature rather than source
    // text, so a hashbang may follow it.
    fn skip_hashbang(&mut self) -> Result<(), ParseError> {
        let start = if self.text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        if let Some(comment) = scan_hashbang(self.text, start) {
            if self.comments.len() >= self.limits.lexical.max_comments {
                return Err(self.error(
                    start,
                    ParseErrorKind::Lexical(crate::lexical::LexicalErrorKind::CommentLimit),
                ));
            }
            self.push_comment(comment)?;
            self.cursor = comment.span.end();
        }
        Ok(())
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        if self.depth >= self.limits.max_nesting_depth {
            return Err(self.error(self.cursor, ParseErrorKind::ResourceLimit));
        }
        self.depth += 1;
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    // Runs `parse` with the `[~In]` flag set to `no_in`: brackets and
    // parentheses restore the `in` operator, `for` heads exclude it.
    fn with_in<T>(
        &mut self,
        no_in: bool,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let saved = std::mem::replace(&mut self.no_in, no_in);
        let result = parse(self);
        self.no_in = saved;
        result
    }

    // Inside braces, a right brace closes the construct, not a template substitution.
    fn within_braces<T>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let saved_template = std::mem::replace(&mut self.template_expression_depth, 0);
        let saved_in = std::mem::replace(&mut self.no_in, false);
        let result = parse(self);
        self.template_expression_depth = saved_template;
        self.no_in = saved_in;
        result
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            cursor: self.cursor,
            last_end: self.last_end,
            line_break_before: self.line_break_before,
            token_count: self.token_count,
            comments: self.comments.len(),
        }
    }

    fn restore(&mut self, checkpoint: Checkpoint) {
        self.cursor = checkpoint.cursor;
        self.last_end = checkpoint.last_end;
        self.line_break_before = checkpoint.line_break_before;
        self.token_count = checkpoint.token_count;
        self.comments.truncate(checkpoint.comments);
    }

    // Inspects later tokens without consuming them. `inspect` must not add nodes.
    fn lookahead<T>(
        &mut self,
        inspect: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let checkpoint = self.checkpoint();
        let nodes = self.nodes.len();
        let result = inspect(self);
        debug_assert_eq!(nodes, self.nodes.len());
        self.restore(checkpoint);
        result
    }

    fn add_node(&mut self, kind: SyntaxNodeKind, span: Span) -> Result<NodeId, ParseError> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(self.error(span.start(), ParseErrorKind::ResourceLimit));
        }
        let index = u32::try_from(self.nodes.len())
            .ok()
            .filter(|&index| index < u32::MAX)
            .ok_or_else(|| self.error(span.start(), ParseErrorKind::ResourceLimit))?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| self.error(span.start(), ParseErrorKind::AllocationFailed))?;
        self.nodes.push(SyntaxNode { kind, span });
        Ok(NodeId::new(index))
    }

    // Reinterprets a cover node in place, keeping its ID and span.
    fn set_kind(&mut self, id: NodeId, kind: SyntaxNodeKind) {
        self.nodes[id.index()].kind = kind;
    }

    fn push_child(&self, items: &mut Vec<NodeId>, item: NodeId) -> Result<(), ParseError> {
        items
            .try_reserve(1)
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        items.push(item);
        Ok(())
    }

    // An empty buffer for collecting a child list.
    fn take_list(&mut self) -> Vec<NodeId> {
        self.list_buffers.pop().unwrap_or_default()
    }

    fn recycle_list(&mut self, mut items: Vec<NodeId>) {
        items.clear();
        if self.list_buffers.try_reserve(1).is_ok() {
            self.list_buffers.push(items);
        }
    }

    // Appends a collected list to the child arena and recycles its buffer.
    fn finish_list(&mut self, items: Vec<NodeId>) -> Result<ChildList, ParseError> {
        let list = self.append_children(&items);
        self.recycle_list(items);
        list
    }

    fn append_children(&mut self, items: &[NodeId]) -> Result<ChildList, ParseError> {
        let start = self.children.len();
        let end = start
            .checked_add(items.len())
            .filter(|&end| end <= self.limits.max_nodes && end <= u32::MAX as usize)
            .ok_or_else(|| self.error(self.cursor, ParseErrorKind::ResourceLimit))?;
        self.children
            .try_reserve(items.len())
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.children.extend_from_slice(items);
        Ok(ChildList::new(start as u32, (end - start) as u32))
    }

    fn append_template_parts(
        &mut self,
        items: &[TemplatePart],
    ) -> Result<TemplatePartList, ParseError> {
        let start = self.template_parts.len();
        let end = start
            .checked_add(items.len())
            .filter(|&end| end <= self.limits.max_tokens && end <= u32::MAX as usize)
            .ok_or_else(|| self.error(self.cursor, ParseErrorKind::ResourceLimit))?;
        self.template_parts
            .try_reserve(items.len())
            .map_err(|_| self.error(self.cursor, ParseErrorKind::AllocationFailed))?;
        self.template_parts.extend_from_slice(items);
        Ok(TemplatePartList::new(start as u32, (end - start) as u32))
    }

    fn consume_token(&mut self, span: Span) -> Result<(), ParseError> {
        if self.typescript {
            if self.ts_work >= self.limits.max_tokens {
                return Err(self.error(span.start(), ParseErrorKind::ResourceLimit));
            }
            self.ts_work += 1;
        }
        if self.token_count >= self.limits.max_tokens {
            return Err(self.error(span.start(), ParseErrorKind::ResourceLimit));
        }
        self.token_count += 1;
        self.cursor = span.end();
        self.last_end = span.end();
        self.line_break_before = false;
        Ok(())
    }

    fn push_comment(&mut self, comment: Comment) -> Result<(), ParseError> {
        self.comments
            .try_reserve(1)
            .map_err(|_| self.error(comment.span.start(), ParseErrorKind::AllocationFailed))?;
        self.comments.push(comment);
        Ok(())
    }

    fn skip_trivia_at(&mut self, offset: usize) -> Result<(), ParseError> {
        self.cursor = offset;
        self.skip_trivia()
    }

    // Most calls find the trivia already skipped, so only that check is inlined.
    #[inline]
    fn skip_trivia(&mut self) -> Result<(), ParseError> {
        if self.trivia_end == Some(self.cursor) {
            return Ok(());
        }
        self.scan_trivia()
    }

    #[inline(never)]
    fn scan_trivia(&mut self) -> Result<(), ParseError> {
        let remaining = self
            .limits
            .lexical
            .max_comments
            .saturating_sub(self.comments.len());
        let goal = match self.goal {
            Goal::Script => SourceGoal::Script,
            Goal::Module | Goal::Unknown => SourceGoal::Module,
        };
        let trivia = scan_js_trivia(
            self.source,
            self.cursor,
            goal,
            LexicalLimits {
                max_comments: remaining,
                ..self.limits.lexical
            },
        )
        .map_err(parse_lexical_error)?;
        if !trivia.span.is_empty() {
            self.comments
                .try_reserve(trivia.comments.len())
                .map_err(|_| self.error(trivia.span.start(), ParseErrorKind::AllocationFailed))?;
            self.comments.extend(trivia.comments);
            self.cursor = trivia.span.end();
            self.line_break_before = trivia.has_line_terminator;
        }
        // Scripts read these spellings as comments and modules as operators.
        // Engines reject `<!--` in modules although the grammar reads `<`, `!`,
        // and `--`; such a file cannot run, so it is rejected as well.
        let rest = &self.text[self.cursor..];
        match self.goal {
            Goal::Script => {}
            Goal::Module => {
                if rest.starts_with("<!--") {
                    return Err(self.error(self.cursor, ParseErrorKind::HtmlCommentInModule));
                }
            }
            Goal::Unknown => {
                if rest.starts_with("<!--") || (self.line_break_before && rest.starts_with("-->")) {
                    return Err(self.error(self.cursor, ParseErrorKind::SourceGoalRequiresContext));
                }
            }
        }
        self.trivia_end = Some(self.cursor);
        Ok(())
    }

    fn at_end(&mut self) -> Result<bool, ParseError> {
        self.skip_trivia()?;
        Ok(self.cursor >= self.text.len())
    }

    fn peek_char(&mut self) -> Result<Option<char>, ParseError> {
        self.skip_trivia()?;
        Ok(self.text[self.cursor..].chars().next())
    }

    fn at_char(&mut self, ch: char) -> Result<bool, ParseError> {
        Ok(self.peek_char()? == Some(ch))
    }

    fn identifier_can_start_here(&mut self) -> Result<bool, ParseError> {
        Ok(self
            .peek_char()?
            .is_some_and(|ch| is_js_identifier_start(ch) || matches!(ch, '#' | '\\')))
    }

    fn peek_identifier(&mut self) -> Result<IdentifierToken, ParseError> {
        self.skip_trivia()?;
        if let Some((offset, token)) = self.peeked_identifier
            && offset == self.cursor
        {
            return Ok(token);
        }
        let token = scan_js_identifier(self.source, self.cursor, self.limits.lexical)
            .map_err(parse_lexical_error)?;
        self.peeked_identifier = Some((self.cursor, token));
        Ok(token)
    }

    // Returns the next token if it is an identifier name, without consuming it.
    fn peek_name(&mut self) -> Result<Option<IdentifierToken>, ParseError> {
        if self.identifier_can_start_here()? {
            return self.peek_identifier().map(Some);
        }
        Ok(None)
    }

    fn scan_identifier(&mut self) -> Result<IdentifierToken, ParseError> {
        let token = self.peek_identifier()?;
        self.consume_token(token.span)?;
        Ok(token)
    }

    // Reports whether the next token is the unescaped word `word`. Keywords
    // and contextual keywords never match escaped spellings.
    fn at_word(&mut self, word: &str) -> Result<bool, ParseError> {
        Ok(self
            .peek_name()?
            .is_some_and(|token| self.is_unescaped(token, word)))
    }

    fn consume_word(&mut self, word: &str) -> Result<Option<IdentifierToken>, ParseError> {
        match self.peek_name()? {
            Some(token) if self.is_unescaped(token, word) => {
                self.consume_token(token.span)?;
                Ok(Some(token))
            }
            Some(_) | None => Ok(None),
        }
    }

    fn is_unescaped(&self, token: IdentifierToken, word: &str) -> bool {
        !token.has_escape && self.slice(token.span) == word
    }

    fn peek_punctuator(
        &mut self,
        goal: JsLexicalGoal,
    ) -> Result<Option<PunctuatorToken>, ParseError> {
        self.skip_trivia()?;
        if let Some((offset, cached_goal, token)) = self.peeked_punctuator
            && offset == self.cursor
            && cached_goal == goal
        {
            return Ok(token);
        }
        let token = scan_js_punctuator(
            self.source,
            self.cursor,
            PunctuatorContext {
                goal,
                greater_than: GreaterThanMode::LongestMatch,
            },
        )
        .map_err(parse_lexical_error)?;
        self.peeked_punctuator = Some((self.cursor, goal, token));
        Ok(token)
    }

    fn at_punctuator(&mut self, expected: Punctuator) -> Result<bool, ParseError> {
        Ok(self
            .peek_punctuator(self.goal_after_expression())?
            .is_some_and(|token| token.kind == expected))
    }

    fn consume_punctuator(
        &mut self,
        expected: Punctuator,
        goal: JsLexicalGoal,
    ) -> Result<Option<PunctuatorToken>, ParseError> {
        let Some(token) = self.peek_punctuator(goal)? else {
            return Ok(None);
        };
        if token.kind != expected {
            return Ok(None);
        }
        self.consume_token(token.span)?;
        Ok(Some(token))
    }

    fn expect_punctuator(
        &mut self,
        expected: Punctuator,
        goal: JsLexicalGoal,
    ) -> Result<PunctuatorToken, ParseError> {
        self.consume_punctuator(expected, goal)?
            .ok_or_else(|| self.error(self.cursor, expected_punctuator_error(expected)))
    }

    // Automatic semicolon insertion: a missing semicolon is inserted before
    // `}`, at the end of the input, or after a line terminator.
    fn consume_semicolon(&mut self) -> Result<(), ParseError> {
        if let Some(token) = self.peek_punctuator(self.goal_after_expression())? {
            if token.kind == Punctuator::Semicolon {
                return self.consume_token(token.span);
            }
            if token.kind == Punctuator::RightBrace {
                return Ok(());
            }
        }
        if self.cursor >= self.text.len() || self.line_break_before {
            return Ok(());
        }
        Err(self.error(self.cursor, ParseErrorKind::ExpectedSemicolon))
    }

    // Whether a statement ends here without consuming a semicolon, as after
    // `return`, `break`, and `continue`.
    fn statement_ends_here(&mut self) -> Result<bool, ParseError> {
        if self.at_end()? || self.line_break_before {
            return Ok(true);
        }
        Ok(self
            .peek_punctuator(self.goal_after_expression())?
            .is_some_and(|token| {
                matches!(token.kind, Punctuator::Semicolon | Punctuator::RightBrace)
            }))
    }

    fn goal_for_expression_start(&self) -> JsLexicalGoal {
        if self.template_expression_depth == 0 {
            JsLexicalGoal::RegExp
        } else {
            JsLexicalGoal::RegExpOrTemplateTail
        }
    }

    fn goal_after_expression(&self) -> JsLexicalGoal {
        if self.template_expression_depth == 0 {
            JsLexicalGoal::Div
        } else {
            JsLexicalGoal::TemplateTail
        }
    }

    fn slice(&self, span: Span) -> &'source str {
        &self.text[span.start()..span.end()]
    }

    fn node_kind(&self, id: NodeId) -> SyntaxNodeKind {
        self.nodes[id.index()].kind()
    }

    fn node_span(&self, id: NodeId) -> Span {
        self.nodes[id.index()].span()
    }

    fn source_span(&self, start: usize, end: usize) -> Result<Span, ParseError> {
        Span::new(start, end)
            .map_err(|error| self.error(start, ParseErrorKind::InternalInvariant(error)))
    }

    // A node span from `start` to the end of the last consumed token.
    fn span_from(&self, start: usize) -> Result<Span, ParseError> {
        self.source_span(start, self.last_end)
    }

    fn error(&self, offset: usize, kind: ParseErrorKind) -> ParseError {
        ParseError { offset, kind }
    }

    // Shorthand initializers and duplicate `__proto__` properties are errors
    // only in object literals that did not become patterns.
    fn check_expression_only_errors(&self) -> Result<(), ParseError> {
        let earliest = self
            .expression_only_errors
            .iter()
            .filter(|(object, _)| {
                matches!(
                    self.node_kind(*object),
                    SyntaxNodeKind::ObjectExpression { .. }
                )
            })
            .map(|(_, error)| error)
            .min_by_key(|error| error.offset);
        match earliest {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn parse_lexical_error(error: LexicalError) -> ParseError {
    ParseError {
        offset: error.offset,
        kind: ParseErrorKind::Lexical(error.kind),
    }
}

fn expected_punctuator_error(kind: Punctuator) -> ParseErrorKind {
    match kind {
        Punctuator::RightParen => ParseErrorKind::ExpectedClosingParenthesis,
        Punctuator::RightBracket => ParseErrorKind::ExpectedClosingBracket,
        Punctuator::RightBrace => ParseErrorKind::ExpectedClosingBrace,
        Punctuator::LeftBrace
        | Punctuator::LeftParen
        | Punctuator::LeftBracket
        | Punctuator::Dot
        | Punctuator::Ellipsis
        | Punctuator::Semicolon
        | Punctuator::Comma
        | Punctuator::LessThan
        | Punctuator::GreaterThan
        | Punctuator::LessThanEqual
        | Punctuator::GreaterThanEqual
        | Punctuator::Equal
        | Punctuator::NotEqual
        | Punctuator::StrictEqual
        | Punctuator::StrictNotEqual
        | Punctuator::Plus
        | Punctuator::Minus
        | Punctuator::Star
        | Punctuator::Percent
        | Punctuator::Exponent
        | Punctuator::Increment
        | Punctuator::Decrement
        | Punctuator::ShiftLeft
        | Punctuator::ShiftRight
        | Punctuator::ShiftRightUnsigned
        | Punctuator::BitAnd
        | Punctuator::BitOr
        | Punctuator::BitXor
        | Punctuator::Bang
        | Punctuator::Tilde
        | Punctuator::LogicalAnd
        | Punctuator::LogicalOr
        | Punctuator::Nullish
        | Punctuator::Question
        | Punctuator::Colon
        | Punctuator::Assign
        | Punctuator::PlusAssign
        | Punctuator::MinusAssign
        | Punctuator::StarAssign
        | Punctuator::PercentAssign
        | Punctuator::ExponentAssign
        | Punctuator::ShiftLeftAssign
        | Punctuator::ShiftRightAssign
        | Punctuator::ShiftRightUnsignedAssign
        | Punctuator::BitAndAssign
        | Punctuator::BitOrAssign
        | Punctuator::BitXorAssign
        | Punctuator::LogicalAndAssign
        | Punctuator::LogicalOrAssign
        | Punctuator::NullishAssign
        | Punctuator::Arrow
        | Punctuator::OptionalChain
        | Punctuator::Slash
        | Punctuator::SlashAssign => ParseErrorKind::ExpectedPunctuator(kind),
    }
}
