//! Reproducible parser and empty-edit contract checks, separate from timings.
//!
//! The public facade supplies every operation. Iterative traversal checks
//! the arena and visitor contract that later analysis will depend on. The
//! measurement harness applies the checks to generated files, which must
//! parse. The fuzzer applies them to arbitrary text, where a rejection is a
//! valid outcome with contracts of its own.

use frameshift_engine::{
    CommentKind, EditLimits, JsDialect, Origin, ParseError, ParseLimits, SourceGoal, SourceText,
    Span, SyntaxNodeKind, SyntaxTree, TemplatePart, TextLimits, apply_edits, parse_js_expression,
    parse_js_program,
};

use crate::CorpusFile;

/// The outcome of a parse whose contracts held.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// The parser returned a tree.
    Accepted {
        /// Reachable syntax nodes.
        nodes: usize,
        /// Retained comments.
        comments: usize,
    },
    /// The parser rejected the text with this error.
    Rejected(ParseError),
}

/// Counts of successfully checked files and their syntax.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Checked {
    /// Files checked with their declared goal and dialect and default limits.
    pub files: usize,
    /// Source bytes, including original line endings and encoding signatures.
    pub bytes: usize,
    /// Reachable syntax nodes across all files.
    pub nodes: usize,
    /// Retained comments across all files.
    pub comments: usize,
}

/// Checks every file in input order. A parse or contract failure stops the
/// check with its file index, goal, and dialect, without a success result.
pub fn corpus(files: &[CorpusFile]) -> Result<Checked, String> {
    let mut checked = Checked::default();
    for (index, file) in files.iter().enumerate() {
        let context = || format!("file {index} ({:?}, {:?})", file.goal, file.dialect);
        let outcome = program(&file.text, file.goal, file.dialect, ParseLimits::default())
            .map_err(|error| format!("{}: {error}", context()))?;
        let (nodes, comments) = match outcome {
            Parsed::Accepted { nodes, comments } => (nodes, comments),
            Parsed::Rejected(error) => return Err(format!("{}: parse: {error:?}", context())),
        };
        checked.files += 1;
        checked.bytes += file.text.len();
        checked.nodes += nodes;
        checked.comments += comments;
    }
    Ok(checked)
}

/// Parses `text` as complete source text and checks the contracts of the
/// outcome.
///
/// An accepted program must span the whole text as a `Program` node of its
/// goal and dialect, form a tree whose nodes are reachable exactly once with
/// nested, source-ordered spans and valid template and comment spans, parse
/// identically from a fresh source text, and survive an empty edit set byte
/// for byte with complete provenance. A rejection must point to a character
/// boundary of the text and repeat identically. `Err` describes the first
/// violated contract.
pub fn program(
    text: &str,
    goal: SourceGoal,
    dialect: JsDialect,
    limits: ParseLimits,
) -> Result<Parsed, String> {
    let source = source_text(text)?;
    // A fresh index for the repeated parse keeps the comparison from reusing
    // parser state or relying on one SourceText instance.
    let repeated_source = source_text(text)?;
    let tree = match (
        parse_js_program(&source, goal, dialect, limits),
        parse_js_program(&repeated_source, goal, dialect, limits),
    ) {
        (Ok(tree), Ok(repeated)) => {
            same_trees(&tree, &repeated)?;
            tree
        }
        (Err(error), Err(repeated)) => return rejected(text, 0, error, repeated),
        (Ok(_), Err(error)) | (Err(error), Ok(_)) => {
            return Err(format!("a repeated parse changed the outcome: {error:?}"));
        }
    };
    let whole = Span::new(0, source.len()).map_err(|error| format!("source span: {error:?}"))?;
    if tree.consumed_span() != whole
        || !tree.node(tree.root()).is_some_and(|node| {
            node.span() == whole
                && matches!(node.kind(), SyntaxNodeKind::Program { goal: parsed_goal, dialect: parsed_dialect, .. }
                    if parsed_goal == goal && parsed_dialect == dialect)
        })
    {
        return Err(
            "program does not cover the source in its declared goal and dialect".to_owned(),
        );
    }
    tree_invariants(&tree)?;
    empty_edits(&source, text)?;
    Ok(Parsed::Accepted {
        nodes: tree.nodes().len(),
        comments: tree.comments().len(),
    })
}

/// Parses `text` as one expression from the character boundary `offset` and
/// checks the contracts of the outcome.
///
/// An accepted expression must consume the text from `offset` to its end,
/// form a tree with the invariants of [`program`], and parse identically from
/// a fresh source text. A rejection must point to a character boundary at or
/// after `offset` and repeat identically. `Err` describes the first violated
/// contract.
pub fn expression(
    text: &str,
    offset: usize,
    dialect: JsDialect,
    limits: ParseLimits,
) -> Result<Parsed, String> {
    let source = source_text(text)?;
    let repeated_source = source_text(text)?;
    let tree = match (
        parse_js_expression(&source, offset, dialect, limits),
        parse_js_expression(&repeated_source, offset, dialect, limits),
    ) {
        (Ok(tree), Ok(repeated)) => {
            same_trees(&tree, &repeated)?;
            tree
        }
        (Err(error), Err(repeated)) => return rejected(text, offset, error, repeated),
        (Ok(_), Err(error)) | (Err(error), Ok(_)) => {
            return Err(format!("a repeated parse changed the outcome: {error:?}"));
        }
    };
    let consumed =
        Span::new(offset, source.len()).map_err(|error| format!("expression span: {error:?}"))?;
    if tree.consumed_span() != consumed {
        return Err(format!(
            "expression consumed {:?} instead of {consumed:?}",
            tree.consumed_span()
        ));
    }
    tree_invariants(&tree)?;
    Ok(Parsed::Accepted {
        nodes: tree.nodes().len(),
        comments: tree.comments().len(),
    })
}

fn source_text(text: &str) -> Result<SourceText, String> {
    SourceText::new(text.to_owned(), TextLimits::default())
        .map_err(|error| format!("source text: {error:?}"))
}

fn rejected(
    text: &str,
    start: usize,
    error: ParseError,
    repeated: ParseError,
) -> Result<Parsed, String> {
    if error != repeated {
        return Err(format!(
            "a repeated parse changed the error from {error:?} to {repeated:?}"
        ));
    }
    if error.offset < start || !text.is_char_boundary(error.offset) {
        return Err(format!(
            "error offset {} is not a character boundary in {start}..={}: {error:?}",
            error.offset,
            text.len()
        ));
    }
    Ok(Parsed::Rejected(error))
}

fn same_trees(tree: &SyntaxTree<'_>, repeated: &SyntaxTree<'_>) -> Result<(), String> {
    if tree.root() != repeated.root()
        || tree.consumed_span() != repeated.consumed_span()
        || tree.nodes() != repeated.nodes()
        || tree.comments() != repeated.comments()
    {
        return Err("repeated parse changed the arena, comments, or consumed span".to_owned());
    }
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        let children = tree.child_nodes(id).ok_or("missing child iterator")?;
        let repeated_children = repeated
            .child_nodes(id)
            .ok_or("missing repeated child iterator")?;
        if !children.clone().eq(repeated_children) {
            return Err(format!(
                "repeated parse changed the children of node {}",
                id.index()
            ));
        }
        stack.extend(children);
        if let SyntaxNodeKind::TemplateLiteral { parts } =
            tree.node(id).ok_or("missing node")?.kind()
            && tree.template_parts(parts) != repeated.template_parts(parts)
        {
            return Err(format!(
                "repeated parse changed template parts at node {}",
                id.index()
            ));
        }
    }
    Ok(())
}

fn empty_edits(source: &SourceText, text: &str) -> Result<(), String> {
    let whole = Span::new(0, source.len()).map_err(|error| format!("source span: {error:?}"))?;
    let rewritten = apply_edits(source, &[], EditLimits::default())
        .map_err(|error| format!("empty edits: {error:?}"))?;
    if source.as_str().as_bytes() != text.as_bytes()
        || rewritten.as_str().as_bytes() != text.as_bytes()
        || !rewritten.applied_edits().is_empty()
    {
        return Err("empty edits changed the source bytes or recorded an edit".to_owned());
    }
    let provenance = rewritten.provenance();
    let preserved = if source.is_empty() {
        provenance.is_empty()
    } else {
        provenance.len() == 1
            && provenance[0].output == whole
            && provenance[0].origin == Origin::Source(whole)
    };
    if !preserved {
        return Err("empty edits did not account for every original source byte".to_owned());
    }
    Ok(())
}

fn tree_invariants(tree: &SyntaxTree<'_>) -> Result<(), String> {
    let source = tree.source();
    let mut reached = vec![false; tree.nodes().len()];
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        let seen = reached
            .get_mut(id.index())
            .ok_or("node index outside arena")?;
        if *seen {
            return Err(format!("node {} is shared or cyclic", id.index()));
        }
        *seen = true;
        let node = tree.node(id).ok_or("missing node")?;
        let parent = node.span();
        let spelling = source
            .slice(parent)
            .map_err(|error| format!("node {}: {error:?}", id.index()))?;
        if !matches!(
            node.kind(),
            SyntaxNodeKind::Program { .. } | SyntaxNodeKind::JsxText
        ) && !spelling.is_empty()
            && !at_token_boundaries(tree, parent, spelling)
        {
            return Err(format!(
                "node {} at {parent:?} starts or ends with trivia",
                id.index()
            ));
        }
        let mut previous = parent.start();
        for child in tree.child_nodes(id).ok_or("missing child iterator")? {
            let span = tree.node(child).ok_or("child outside arena")?.span();
            ordered_span(span, parent, &mut previous)?;
            stack.push(child);
        }
        if let SyntaxNodeKind::TemplateLiteral { parts } = node.kind() {
            let mut previous = parent.start();
            for part in tree
                .template_parts(parts)
                .ok_or("template parts outside arena")?
            {
                let span = match part {
                    TemplatePart::Segment(token) => {
                        source
                            .slice(token.span)
                            .map_err(|error| format!("template span: {error:?}"))?;
                        source
                            .slice(token.content_span)
                            .map_err(|error| format!("template content: {error:?}"))?;
                        let mut content_start = token.span.start();
                        ordered_span(token.content_span, token.span, &mut content_start)?;
                        if let Some(offset) = token.invalid_escape
                            && (offset < token.content_span.start()
                                || offset >= token.content_span.end()
                                || !source
                                    .as_str()
                                    .get(offset..)
                                    .is_some_and(|text| text.starts_with('\\')))
                        {
                            return Err("template escape evidence outside its content".to_owned());
                        }
                        token.span
                    }
                    TemplatePart::Expression(id) => {
                        tree.node(*id).ok_or("missing template expression")?.span()
                    }
                };
                ordered_span(span, parent, &mut previous)?;
            }
        }
    }
    if reached.iter().any(|&seen| !seen) {
        return Err("unreachable node in arena".to_owned());
    }
    let whole = tree.consumed_span();
    let mut previous = whole.start();
    for comment in tree.comments() {
        ordered_span(comment.span, whole, &mut previous)?;
        let spelling = source
            .slice(comment.span)
            .map_err(|error| format!("comment span: {error:?}"))?;
        let valid = match comment.kind {
            CommentKind::Line => spelling.starts_with("//"),
            CommentKind::Block => spelling.starts_with("/*") && spelling.ends_with("*/"),
            CommentKind::HtmlOpen => spelling.starts_with("<!--"),
            CommentKind::HtmlClose => spelling.starts_with("-->"),
            CommentKind::Hashbang => spelling.starts_with("#!"),
        };
        if !valid {
            return Err("comment kind does not match its source spelling".to_owned());
        }
    }
    Ok(())
}

// Whether a node's span starts at a token and ends with one: it neither
// starts nor ends with whitespace or a line terminator, and neither boundary
// lies inside a comment. Only programs and JSX text include trivia.
fn at_token_boundaries(tree: &SyntaxTree<'_>, span: Span, spelling: &str) -> bool {
    let trivia = |ch: char| {
        matches!(
            ch,
            '\t' | '\u{b}' | '\u{c}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
                    | '\n'
                    | '\r'
                    | '\u{2028}'
                    | '\u{2029}'
        )
    };
    let comments = tree.comments();
    let inside_comment = |offset: usize| {
        let index = comments.partition_point(|comment| comment.span.end() <= offset);
        comments
            .get(index)
            .is_some_and(|comment| comment.span.start() <= offset)
    };
    !spelling.starts_with(trivia)
        && !spelling.ends_with(trivia)
        && !inside_comment(span.start())
        && !inside_comment(span.end() - 1)
}

fn ordered_span(span: Span, parent: Span, previous: &mut usize) -> Result<(), String> {
    if span.start() < *previous || span.end() > parent.end() {
        return Err(format!(
            "span {span:?} is not nested in source order inside {parent:?}"
        ));
    }
    *previous = span.end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_engine::{JsDialect, SourceGoal};

    #[test]
    fn checks_empty_unicode_and_template_sources_in_every_dialect() {
        for dialect in [
            JsDialect::JavaScript,
            JsDialect::Jsx,
            JsDialect::TypeScript,
            JsDialect::Tsx,
            JsDialect::TypeScriptDeclaration,
        ] {
            let empty = CorpusFile {
                goal: SourceGoal::Module,
                dialect,
                text: String::new(),
            };
            assert_eq!(
                corpus(&[empty]).unwrap(),
                Checked {
                    files: 1,
                    nodes: 1,
                    ..Checked::default()
                }
            );
        }
        for (dialect, text) in [
            (
                JsDialect::JavaScript,
                "\u{feff}/* é🦀 */\r\nconst café = tag`é${'🦀'}\\u{}`; // tail\r\n",
            ),
            (
                JsDialect::Tsx,
                "// head\r\nconst Box = <T,>(value: T) => <p title={`é${value}`}>{value}</p>;",
            ),
            (
                JsDialect::TypeScript,
                "type Label<T extends string> = `é${T}-${{ value: true }}`;",
            ),
            (
                JsDialect::TypeScriptDeclaration,
                "export interface Box<T> { value: T }\r\nexport const box: Box<string>;",
            ),
        ] {
            let file = CorpusFile {
                goal: SourceGoal::Module,
                dialect,
                text: text.to_owned(),
            };
            let checked = corpus(&[file]).unwrap();
            assert_eq!(checked.bytes, text.len());
            assert!(checked.nodes > 1);
        }
    }

    #[test]
    fn rejections_and_expressions_have_checked_outcomes() {
        let limits = ParseLimits::default();
        for goal in [SourceGoal::Script, SourceGoal::Module] {
            let outcome = program("const é = ;", goal, JsDialect::JavaScript, limits).unwrap();
            assert!(
                matches!(outcome, Parsed::Rejected(ParseError { offset: 11, .. })),
                "{outcome:?}"
            );
        }
        for dialect in [
            JsDialect::JavaScript,
            JsDialect::Jsx,
            JsDialect::TypeScript,
            JsDialect::Tsx,
            JsDialect::TypeScriptDeclaration,
        ] {
            assert_eq!(
                expression("x; /* é */ a?.b ?? c ", 3, dialect, limits).unwrap(),
                Parsed::Accepted {
                    nodes: 5,
                    comments: 1
                }
            );
            let outcome = expression("a +", 0, dialect, limits).unwrap();
            assert!(
                matches!(outcome, Parsed::Rejected(ParseError { offset: 3, .. })),
                "{outcome:?}"
            );
        }
    }

    #[test]
    fn syntax_failure_identifies_the_file_without_a_success_result() {
        let files = ["export {};", "const missing = ;"].map(|text| CorpusFile {
            goal: SourceGoal::Module,
            dialect: JsDialect::TypeScript,
            text: text.to_owned(),
        });
        let error = corpus(&files).unwrap_err();
        assert!(
            error.starts_with("file 1 (Module, TypeScript): parse:"),
            "{error}"
        );
    }
}
