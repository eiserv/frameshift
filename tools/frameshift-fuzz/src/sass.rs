//! Sass grammar, arena reachability, spans, repeatability, and limits.

use std::collections::BTreeSet;

use frameshift_engine::syntax::{
    SassAtRule, SassDocument, SassLimits, SassNode, SassNodeKind, SassSyntax, SassUnchecked,
    SassUncheckedKind, parse_sass,
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
            if node.kind == SassNodeKind::IfExpression
                && (node.children.len() < 2
                    || doc.node(node.children[0]).unwrap().kind != SassNodeKind::Identifier
                    || node.children[1..].iter().any(|&id| {
                        !matches!(doc.node(id).unwrap().kind, SassNodeKind::IfBranch { .. })
                    }))
            {
                return Err("invalid modern if expression structure".to_owned());
            }
            if matches!(node.kind, SassNodeKind::IfBranch { .. }) && node.children.len() != 2 {
                return Err("invalid modern if branch arity".to_owned());
            }
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
            if node.kind == SassNodeKind::ImportModifiers {
                let kinds: Vec<_> = node
                    .children
                    .iter()
                    .map(|&id| doc.node(id).map(|child| &child.kind))
                    .collect::<Option<_>>()
                    .ok_or("missing import modifier")?;
                let list = kinds
                    .iter()
                    .position(|kind| **kind == SassNodeKind::MediaQueryList);
                if kinds.is_empty()
                    || list.is_some_and(|index| index + 1 != kinds.len())
                    || kinds.iter().any(|kind| {
                        !matches!(
                            kind,
                            SassNodeKind::Identifier
                                | SassNodeKind::InterpolatedText
                                | SassNodeKind::Interpolation
                                | SassNodeKind::ImportFunction
                                | SassNodeKind::MediaQueryList
                        )
                    })
                {
                    return Err("invalid import modifier structure".to_owned());
                }
            }
            if matches!(
                node.kind,
                SassNodeKind::ImportFunction
                    | SassNodeKind::SupportsFunction
                    | SassNodeKind::SupportsDeclaration
                    | SassNodeKind::SupportsAnything
                    | SassNodeKind::SupportsNegation
            ) && node.children.len() != 2
            {
                return Err(format!("invalid condition arity: {node:?}"));
            }
            if node.kind == SassNodeKind::MediaQueryList
                && (node.children.is_empty()
                    || node.children.iter().any(|&id| {
                        doc.node(id)
                            .is_none_or(|child| child.kind != SassNodeKind::MediaQuery)
                    }))
            {
                return Err("invalid media query list".to_owned());
            }
            if node.kind == SassNodeKind::AtRule(SassAtRule::Media) {
                let list = node
                    .children
                    .get(1)
                    .filter(|&&id| {
                        doc.node(id)
                            .is_some_and(|child| child.kind == SassNodeKind::MediaQueryList)
                    })
                    .ok_or("media rule without query list")?;
                // Only value-dependent query text is parsed again.
                let mut stack = vec![*list];
                let mut dependent = false;
                while let Some(id) = stack.pop() {
                    let child = doc.node(id).ok_or("missing media node")?;
                    dependent |= matches!(
                        child.kind,
                        SassNodeKind::Interpolation
                            | SassNodeKind::Variable
                            | SassNodeKind::Call
                            | SassNodeKind::SpecialFunction
                            | SassNodeKind::String { quoted: true }
                            | SassNodeKind::ParentSelector
                            | SassNodeKind::Null
                            | SassNodeKind::Map
                    );
                    stack.extend_from_slice(&child.children);
                }
                let span = doc.node(*list).ok_or("missing media query list")?.span;
                let evidence = unchecked
                    .iter()
                    .any(|entry| entry.kind == SassUncheckedKind::MediaQuery && entry.span == span);
                if dependent != evidence {
                    return Err("media query evidence does not match its values".to_owned());
                }
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
            if node.kind == SassNodeKind::AtRule(SassAtRule::Extend) {
                check_extend(&doc, node, &unchecked)?;
            }
            if node.kind == SassNodeKind::AtRootQuery {
                check_at_root_query(&doc, node, &unchecked)?;
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

/// An established target is a list of single simple selectors without `&`;
/// any other target is raw text with selector evidence (`D-084`).
fn check_extend(
    doc: &SassDocument<'_>,
    node: &SassNode,
    unchecked: &[SassUnchecked],
) -> Result<(), String> {
    let target = node
        .children
        .get(1)
        .and_then(|&id| doc.node(id))
        .ok_or("extension without target")?;
    let evidence = unchecked
        .iter()
        .any(|entry| entry.kind == SassUncheckedKind::Selector && entry.span == target.span);
    let flag = node.children.get(2).and_then(|&id| doc.node(id));
    if node.children.len() > 3 || flag.is_some_and(|flag| flag.kind != SassNodeKind::Flag) {
        return Err("invalid extension children".to_owned());
    }
    if target.kind == SassNodeKind::InterpolatedText {
        return if evidence {
            Ok(())
        } else {
            Err("raw extension target without evidence".to_owned())
        };
    }
    if target.kind != SassNodeKind::SelectorList || evidence {
        return Err(format!("unexpected extension target: {target:?}"));
    }
    for &complex in &target.children {
        let complex = doc.node(complex).ok_or("missing complex selector")?;
        let [compound] = complex.children[..] else {
            return Err("complex extension target".to_owned());
        };
        let compound = doc.node(compound).ok_or("missing compound selector")?;
        if compound.kind != SassNodeKind::CompoundSelector || compound.children.len() != 1 {
            return Err("compound extension target".to_owned());
        }
    }
    let mut stack = target.children.clone();
    while let Some(id) = stack.pop() {
        let child = doc.node(id).ok_or("missing extension node")?;
        if child.kind == SassNodeKind::NestingSelector {
            return Err("parent selector in an extension target".to_owned());
        }
        stack.extend_from_slice(&child.children);
    }
    Ok(())
}

/// A query without evidence spells `kind: rules` with unquoted plain names.
fn check_at_root_query(
    doc: &SassDocument<'_>,
    node: &SassNode,
    unchecked: &[SassUnchecked],
) -> Result<(), String> {
    if !(1..=2).contains(&node.children.len()) {
        return Err("invalid at-root query arity".to_owned());
    }
    let evidence = unchecked
        .iter()
        .any(|entry| entry.kind == SassUncheckedKind::AtRootPrelude && entry.span == node.span);
    if evidence {
        return Ok(());
    }
    let plain = |id| {
        doc.node(id).is_some_and(|child| {
            child.kind == (SassNodeKind::String { quoted: false }) && child.children.is_empty()
        })
    };
    let [kind, rules] = node.children[..] else {
        return Err("at-root query without rules lacks evidence".to_owned());
    };
    let rules = doc.node(rules).ok_or("missing at-root rules")?;
    let plain_rules = if matches!(rules.kind, SassNodeKind::List { .. }) {
        rules.children.iter().all(|&id| plain(id))
    } else {
        rules.kind == (SassNodeKind::String { quoted: false }) && rules.children.is_empty()
    };
    if !plain(kind) || !plain_rules {
        return Err("value-dependent at-root query lacks evidence".to_owned());
    }
    Ok(())
}
