//! Sass grammar, arena reachability, spans, repeatability, and limits.

use std::collections::BTreeSet;

use frameshift_engine::syntax::{SassLimits, SassSyntax, parse_sass};

use crate::targets;

pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let source = targets::source(text)?;
    for syntax in [SassSyntax::Scss, SassSyntax::Indented] {
        let result = parse_sass(&source, syntax, SassLimits::default());
        if parse_sass(&source, syntax, SassLimits::default()) != result {
            return Err("Sass parsing is not repeatable".to_owned());
        }
        let doc = match result {
            Ok(doc) => doc,
            Err(error) => {
                source
                    .validate_offset(error.offset)
                    .map_err(|e| format!("Sass error coordinate: {e}"))?;
                continue;
            }
        };
        let mut queue = doc.statements().to_vec();
        let mut seen = BTreeSet::new();
        for comment in doc.comments() {
            source
                .slice(comment.span)
                .map_err(|e| format!("Sass comment span: {e}"))?;
            queue.extend_from_slice(&comment.interpolations);
        }
        while let Some(id) = queue.pop() {
            let node = doc.node(id).ok_or("missing Sass node")?;
            if !seen.insert(id) {
                return Err("Sass node reached twice".to_owned());
            }
            source
                .slice(node.span)
                .map_err(|e| format!("Sass node span: {e}"))?;
            let mut end = node.span.start();
            for &id in &node.children {
                let child = doc.node(id).ok_or("missing Sass child")?;
                if child.span.start() < end || child.span.end() > node.span.end() {
                    return Err(format!("unordered Sass child: {child:?} of {node:?}"));
                }
                end = child.span.end();
            }
            queue.extend_from_slice(&node.children);
        }
        if seen.len() != doc.nodes().len() {
            return Err("unreachable Sass syntax node".to_owned());
        }
        if !doc
            .comments()
            .windows(2)
            .all(|pair| pair[0].span.start() <= pair[1].span.start())
        {
            return Err("Sass comments are out of order".to_owned());
        }
        if doc.nodes().len() + doc.comments().len() > SassLimits::default().max_nodes {
            return Err("Sass arena exceeds its limit".to_owned());
        }
    }
    Ok(())
}
