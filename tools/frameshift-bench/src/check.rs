//! Reproducible parser and empty-edit contract checks, separate from timings.
//!
//! The public facade supplies every operation. Iterative traversal checks
//! the arena and visitor contract that later analysis will depend on.

use frameshift_engine::{
    CommentKind, EditLimits, Origin, ParseLimits, SourceText, Span, SyntaxNodeKind, SyntaxTree,
    TemplatePart, TextLimits, apply_edits, parse_js_program,
};

use crate::corpus::CorpusFile;

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
        let (nodes, comments) = check_file(file).map_err(|error| {
            format!(
                "file {index} ({:?}, {:?}): {error}",
                file.goal, file.dialect
            )
        })?;
        checked.files += 1;
        checked.bytes += file.text.len();
        checked.nodes += nodes;
        checked.comments += comments;
    }
    Ok(checked)
}

fn check_file(file: &CorpusFile) -> Result<(usize, usize), String> {
    let source = SourceText::new(file.text.clone(), TextLimits::default())
        .map_err(|error| format!("source text: {error:?}"))?;
    let tree = parse_js_program(&source, file.goal, file.dialect, ParseLimits::default())
        .map_err(|error| format!("parse: {error:?}"))?;
    let whole = Span::new(0, source.len()).map_err(|error| format!("source span: {error:?}"))?;
    if tree.consumed_span() != whole
        || !tree.node(tree.root()).is_some_and(|node| {
            node.span() == whole
                && matches!(node.kind(), SyntaxNodeKind::Program { goal, dialect, .. }
                    if goal == file.goal && dialect == file.dialect)
        })
    {
        return Err(
            "program does not cover the source in its declared goal and dialect".to_owned(),
        );
    }
    tree_invariants(&tree)?;

    // Build a fresh index as well, so the comparison does not reuse parser
    // state or rely on one SourceText instance.
    let repeated_source = SourceText::new(file.text.clone(), TextLimits::default())
        .map_err(|error| format!("repeated source text: {error:?}"))?;
    let repeated = parse_js_program(
        &repeated_source,
        file.goal,
        file.dialect,
        ParseLimits::default(),
    )
    .map_err(|error| format!("repeated parse: {error:?}"))?;
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

    let rewritten = apply_edits(&source, &[], EditLimits::default())
        .map_err(|error| format!("empty edits: {error:?}"))?;
    if source.as_str().as_bytes() != file.text.as_bytes()
        || rewritten.as_str().as_bytes() != file.text.as_bytes()
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
    Ok((tree.nodes().len(), tree.comments().len()))
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
        source
            .slice(parent)
            .map_err(|error| format!("node {}: {error:?}", id.index()))?;
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
