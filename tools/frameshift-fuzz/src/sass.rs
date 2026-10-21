//! Sass grammar, arena reachability, spans, repeatability, and limits.

use std::collections::BTreeSet;

use frameshift_engine::syntax::{
    SassAtRule, SassLimits, SassNodeKind, SassSyntax, SassUncheckedKind, parse_sass,
};

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
        let unchecked: Vec<_> = doc.unchecked_syntax().collect();
        for entry in &unchecked {
            source
                .slice(entry.span)
                .map_err(|e| format!("Sass unchecked span: {e}"))?;
        }
        for node in doc.nodes() {
            if let SassNodeKind::Import { css } = node.kind {
                let url = node
                    .children
                    .first()
                    .and_then(|&id| doc.node(id))
                    .ok_or("import without URL")?;
                if !matches!(url.kind, SassNodeKind::String { .. } | SassNodeKind::Call)
                    || matches!(url.kind, SassNodeKind::String { .. }) && !url.children.is_empty()
                    || !(1..=2).contains(&node.children.len())
                {
                    return Err("invalid import URL structure".to_owned());
                }
                if let Some(&modifiers) = node.children.get(1) {
                    let modifiers = doc.node(modifiers).ok_or("missing import modifiers")?;
                    if !css || modifiers.kind != SassNodeKind::ImportModifiers {
                        return Err("modified import must be CSS".to_owned());
                    }
                }
            }
            if node.kind == SassNodeKind::ImportModifiers
                && !unchecked.iter().any(|entry| {
                    entry.kind == SassUncheckedKind::ImportPrelude && entry.span == node.span
                })
            {
                return Err("import modifiers without unchecked evidence".to_owned());
            }
            if matches!(
                node.kind,
                SassNodeKind::AtRule(SassAtRule::Use | SassAtRule::Forward)
            ) {
                let url = node
                    .children
                    .get(1)
                    .and_then(|&id| doc.node(id))
                    .ok_or("module without URL")?;
                if url.kind != (SassNodeKind::String { quoted: true }) || !url.children.is_empty() {
                    return Err("module URL must be a non-interpolating string".to_owned());
                }
                if unchecked.iter().any(|entry| entry.span == node.span) {
                    return Err("module rule has unchecked grammar".to_owned());
                }
            }
            let name_count = if matches!(node.kind, SassNodeKind::ModuleNamespace { global: true })
            {
                Some(0)
            } else if matches!(
                node.kind,
                SassNodeKind::ModuleNamespace { global: false } | SassNodeKind::ForwardPrefix
            ) {
                Some(1)
            } else {
                None
            };
            if let Some(count) = name_count
                && (node.children.len() != count
                    || node.children.iter().any(|&id| {
                        doc.node(id)
                            .is_none_or(|n| n.kind != SassNodeKind::Identifier)
                    }))
            {
                return Err("module clause has invalid children".to_owned());
            }
        }
        for rule in doc
            .nodes()
            .iter()
            .filter(|node| node.kind == SassNodeKind::StyleRule)
        {
            let selector = rule
                .children
                .first()
                .and_then(|&id| doc.node(id))
                .ok_or("Sass style rule without selector")?;
            let raw = selector.kind == SassNodeKind::InterpolatedText;
            if !raw
                && !matches!(
                    selector.kind,
                    SassNodeKind::SelectorList | SassNodeKind::KeyframeSelectorList
                )
            {
                return Err(format!("unexpected Sass selector: {selector:?}"));
            }
            // A selector retained as raw text never validates the file.
            if raw
                && !unchecked.iter().any(|entry| {
                    entry.kind == SassUncheckedKind::Selector && entry.span == selector.span
                })
            {
                return Err("raw Sass selector without unchecked evidence".to_owned());
            }
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
