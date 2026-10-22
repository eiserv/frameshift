//! Diagnostics-only recovery at conservative, explicit program-item boundaries.

use frameshift_core::SourceText;

use super::{JsDialect, ParseError, ParseErrorKind, ParseLimits, Parser};
use crate::{SourceGoal, SyntaxTree, is_js_identifier_part, is_js_identifier_start};

/// Additional per-file bounds for JavaScript diagnostic recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryLimits {
    /// Maximum retained diagnostics, default 256. Zero refuses parsing.
    pub max_diagnostics: usize,
    /// Maximum UTF-8 bytes visited by synchronization scans, default 1 MiB.
    /// Ordinary parsing also shares the file's original node, token, comment,
    /// and nesting bounds across all resumed attempts.
    pub max_work: usize,
}

impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            max_diagnostics: 256,
            max_work: 1_048_576,
        }
    }
}

/// Why diagnostics could not continue through the rest of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStopKind {
    /// No trustworthy boundary exists in the supported synchronization grammar.
    UnsafeBoundary,
    /// The diagnostic count reached its configured bound.
    DiagnosticLimit,
    /// The synchronization byte budget was exhausted.
    WorkLimit,
    /// A shared parse bound, storage failure, or internal parser error stopped work.
    ParseFailure,
}

/// A recovery stop, expressed in original-source coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryStop {
    /// UTF-8 boundary at which work stopped.
    pub offset: usize,
    /// The reason no further diagnostics are available.
    pub kind: RecoveryStopKind,
}

/// A complete validated tree, or diagnostics with no accessible partial tree.
///
/// After the first error, later batches use fresh declaration/export scopes.
/// Cross-batch early errors are not checked, so diagnostics are not exhaustive.
/// Strictness, source goal, dialect, and cumulative parse bounds are preserved.
pub struct JsProgramParse<'source> {
    tree: Option<SyntaxTree<'source>>,
    diagnostics: Vec<ParseError>,
    stop: Option<RecoveryStop>,
}

impl<'source> JsProgramParse<'source> {
    /// The validated complete tree, available only when no error was found.
    pub fn tree(&self) -> Option<&SyntaxTree<'source>> {
        self.tree.as_ref()
    }
    /// Takes the validated complete tree. Erroneous results always return `None`.
    pub fn into_tree(self) -> Option<SyntaxTree<'source>> {
        self.tree
    }
    /// Diagnostics in increasing original-source offset order.
    pub fn diagnostics(&self) -> &[ParseError] {
        &self.diagnostics
    }
    /// Why parsing stopped early, or `None` when it reached EOF.
    pub fn stop(&self) -> Option<RecoveryStop> {
        self.stop
    }
}

/// Parses a program, collecting multiple errors only across proven boundaries.
///
/// The first diagnostic is exactly the strict parser's error. Recovery rescans
/// only the failed top-level item and resumes after an explicit semicolon
/// outside balanced delimiters, strings, and comments. It stops at templates,
/// non-comment slashes, angle brackets, escapes outside strings, labels or
/// ternaries, control/declaration keywords with nested statement grammar, and
/// unmatched delimiters. No token is inserted and no source text is rewritten.
/// In modules, static imports, named/star exports, and exported variable
/// declarations can resume at the same explicit semicolon boundaries.
/// Nested lists are not independently recovered. Any error withholds the whole
/// tree, including valid regions. Allocation failure or a zero diagnostic bound
/// returns `Err`; other stops retain all diagnostics collected so far.
pub fn parse_js_program_with_recovery(
    source: &SourceText,
    goal: SourceGoal,
    dialect: JsDialect,
    limits: ParseLimits,
    recovery: RecoveryLimits,
) -> Result<JsProgramParse<'_>, ParseError> {
    if recovery.max_diagnostics == 0 {
        return Err(ParseError {
            offset: 0,
            kind: ParseErrorKind::ResourceLimit,
        });
    }
    let mut output = JsProgramParse {
        tree: None,
        diagnostics: Vec::new(),
        stop: None,
    };
    let mut remaining = limits;
    let mut offset = 0;
    let mut strict = None;
    let mut work = recovery.max_work;
    loop {
        let mut parser = Parser::new(source, goal.into(), dialect, remaining);
        parser.cursor = offset;
        parser.program_item_start = offset;
        if let Some(strict) = strict {
            parser.context.strict = strict;
        }
        let parsed = parser.parse_program(goal, dialect).and_then(|root| {
            parser.check_expression_only_errors()?;
            Ok(root)
        });
        let error = match parsed {
            Ok(root) => {
                if output.diagnostics.is_empty() {
                    output.tree = Some(parser.finish(0, root)?);
                }
                return Ok(output);
            }
            Err(error) => error,
        };
        // Deferred errors can point into an earlier successful statement.
        // Continuing there would repeat work and could reorder diagnostics.
        if output
            .diagnostics
            .last()
            .is_some_and(|last| last.offset >= error.offset)
        {
            output.stop = Some(RecoveryStop {
                offset,
                kind: RecoveryStopKind::UnsafeBoundary,
            });
            return Ok(output);
        }
        output.diagnostics.try_reserve(1).map_err(|_| ParseError {
            offset: error.offset,
            kind: ParseErrorKind::AllocationFailed,
        })?;
        output.diagnostics.push(error.clone());
        let reason = if matches!(
            error.kind,
            ParseErrorKind::ResourceLimit
                | ParseErrorKind::AllocationFailed
                | ParseErrorKind::InternalInvariant(_)
        ) {
            Some(RecoveryStopKind::ParseFailure)
        } else if output.diagnostics.len() == recovery.max_diagnostics {
            Some(RecoveryStopKind::DiagnosticLimit)
        } else if matches!(error.kind, ParseErrorKind::Lexical(_))
            || error.offset < parser.program_item_start
        {
            Some(RecoveryStopKind::UnsafeBoundary)
        } else {
            None
        };
        if let Some(kind) = reason {
            output.stop = Some(RecoveryStop {
                offset: error.offset,
                kind,
            });
            return Ok(output);
        }
        remaining.max_nodes = remaining.max_nodes.saturating_sub(parser.nodes.len());
        remaining.max_tokens = remaining
            .max_tokens
            .saturating_sub(parser.token_count.max(parser.ts_work));
        remaining.lexical.max_comments = remaining
            .lexical
            .max_comments
            .saturating_sub(parser.comments.len());
        strict = Some(parser.program_strict);
        let start = parser.program_item_start;
        drop(parser);
        let mut scan = Synchronizer {
            text: source.as_str(),
            pos: start,
            work: &mut work,
            max_depth: limits.max_nesting_depth,
            module: goal == SourceGoal::Module,
        };
        match scan.boundary(error.offset) {
            Ok(next) if next == source.len() => return Ok(output),
            Ok(next) => {
                offset = next;
            }
            Err(stop) => {
                output.stop = Some(stop);
                return Ok(output);
            }
        }
    }
}

struct Synchronizer<'text, 'budget> {
    text: &'text str,
    pos: usize,
    work: &'budget mut usize,
    max_depth: usize,
    module: bool,
}

#[derive(Clone, Copy)]
enum ModulePrefix {
    Import,
    Export,
}

impl Synchronizer<'_, '_> {
    fn stop(&self, kind: RecoveryStopKind) -> RecoveryStop {
        RecoveryStop {
            offset: self.pos,
            kind,
        }
    }
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }
    fn bump(&mut self) -> Result<(), RecoveryStop> {
        let size = self.peek().unwrap().len_utf8();
        if *self.work < size {
            return Err(self.stop(RecoveryStopKind::WorkLimit));
        }
        *self.work -= size;
        self.pos += size;
        Ok(())
    }
    fn starts(&self, text: &str) -> bool {
        self.text[self.pos..].starts_with(text)
    }

    fn boundary(&mut self, error: usize) -> Result<usize, RecoveryStop> {
        let mut stack = Vec::new();
        let mut first = true;
        let mut prefix = None;
        while let Some(ch) = self.peek() {
            if self.starts("//") {
                while self.peek().is_some_and(|c| !line_end(c)) {
                    self.bump()?;
                }
            } else if self.starts("/*") {
                self.bump()?;
                self.bump()?;
                while !self.starts("*/") {
                    if self.peek().is_none() {
                        return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                    }
                    self.bump()?;
                }
                self.bump()?;
                self.bump()?;
            } else if ch.is_whitespace() || ch == '\u{feff}' {
                self.bump()?;
            } else if let Some(prefix) = prefix.take() {
                let allowed = match prefix {
                    ModulePrefix::Import => {
                        matches!(ch, '\'' | '"' | '{' | '*') || is_js_identifier_start(ch)
                    }
                    ModulePrefix::Export => {
                        matches!(ch, '{' | '*')
                            || ["const", "let", "var"].iter().any(|word| {
                                self.starts(word)
                                    && !self.text[self.pos + word.len()..]
                                        .chars()
                                        .next()
                                        .is_some_and(|c| c == '\\' || is_js_identifier_part(c))
                            })
                    }
                };
                if !allowed {
                    return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                }
            } else if matches!(ch, '\'' | '"') {
                if first {
                    return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                }
                self.quoted(ch)?;
            } else if matches!(ch, '/' | '`' | '<' | '\\')
                || self.starts("-->")
                || (ch == ':' && stack.is_empty())
            {
                return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
            } else if is_js_identifier_start(ch) {
                let start = self.pos;
                while self.peek().is_some_and(is_js_identifier_part) {
                    self.bump()?;
                }
                let word = &self.text[start..self.pos];
                if first && self.module && matches!(word, "import" | "export") {
                    prefix = Some(if word == "import" {
                        ModulePrefix::Import
                    } else {
                        ModulePrefix::Export
                    });
                } else if stack.is_empty()
                    && matches!(
                        word,
                        "if" | "else"
                            | "for"
                            | "while"
                            | "do"
                            | "try"
                            | "catch"
                            | "finally"
                            | "with"
                            | "switch"
                            | "case"
                            | "default"
                            | "function"
                            | "class"
                            | "export"
                            | "import"
                            | "return"
                            | "throw"
                            | "break"
                            | "continue"
                            | "interface"
                            | "type"
                            | "namespace"
                            | "module"
                            | "declare"
                            | "enum"
                    )
                {
                    return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                }
                first = false;
            } else {
                if first && matches!(ch, '{' | '@' | '#' | '}') {
                    return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                }
                first = false;
                match ch {
                    '(' | '[' | '{' => {
                        if stack.len() >= self.max_depth {
                            return Err(self.stop(RecoveryStopKind::ParseFailure));
                        }
                        stack
                            .try_reserve(1)
                            .map_err(|_| self.stop(RecoveryStopKind::ParseFailure))?;
                        stack.push(match ch {
                            '(' => ')',
                            '[' => ']',
                            _ => '}',
                        });
                    }
                    ')' | ']' | '}' if stack.pop() != Some(ch) => {
                        return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                    }
                    ';' if stack.is_empty() && self.pos >= error => {
                        self.bump()?;
                        return Ok(self.pos);
                    }
                    _ => {}
                }
                self.bump()?;
            }
        }
        if stack.is_empty() && prefix.is_none() {
            Ok(self.pos)
        } else {
            Err(self.stop(RecoveryStopKind::UnsafeBoundary))
        }
    }

    fn quoted(&mut self, quote: char) -> Result<(), RecoveryStop> {
        self.bump()?;
        loop {
            match self.peek() {
                Some(c) if c == quote => {
                    self.bump()?;
                    return Ok(());
                }
                Some('\\') => {
                    self.bump()?;
                    if self.peek().is_none() {
                        return Err(self.stop(RecoveryStopKind::UnsafeBoundary));
                    }
                    let cr = self.peek() == Some('\r');
                    self.bump()?;
                    if cr && self.peek() == Some('\n') {
                        self.bump()?;
                    }
                }
                Some(c) if !line_end(c) => self.bump()?,
                Some(_) | None => return Err(self.stop(RecoveryStopKind::UnsafeBoundary)),
            }
        }
    }
}

fn line_end(ch: char) -> bool {
    matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}')
}
