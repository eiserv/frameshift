//! Scope and binding analysis of every tree that parses, and the component
//! and environment summaries that build on it (`D-096`).

use frameshift_engine::bindings::{Target as Resolution, analyze};
use frameshift_engine::foundation::{PathLimits, SourcePath, SourceText, TextLimits};
use frameshift_engine::nextjs::{summarize_components, summarize_environment};
use frameshift_engine::syntax::{ParseLimits, SyntaxNodeKind as N, parse_js_program};

/// Checks repetition, reference and declaration nodes, resolved targets,
/// acyclic scopes in creation order, the declaration index, and repeatable
/// component and environment summaries without panics.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = crate::targets::text(input)? else {
        return Ok(());
    };
    let path = SourcePath::new("fuzz.tsx", PathLimits::default()).map_err(|e| e.to_string())?;
    let source =
        SourceText::new(text.to_owned(), TextLimits::default()).map_err(|e| e.to_string())?;
    for goal in crate::targets::GOALS {
        for dialect in crate::targets::DIALECTS {
            let Ok(tree) = parse_js_program(&source, goal, dialect, ParseLimits::default()) else {
                continue;
            };
            let first = analyze(&tree).map_err(|e| e.to_string())?;
            if first != analyze(&tree).map_err(|e| e.to_string())? {
                return Err("binding analysis changed on repetition".to_owned());
            }
            if summarize_components(&path, &tree, &first)
                != summarize_components(&path, &tree, &first)
            {
                return Err("component summary changed on repetition".to_owned());
            }
            if summarize_environment(&path, &tree, &first)
                != summarize_environment(&path, &tree, &first)
            {
                return Err("environment summary changed on repetition".to_owned());
            }
            for (index, scope) in first.scopes().iter().enumerate() {
                if scope.parent.is_some_and(|p| p.index() >= index)
                    || (index > 0) != scope.parent.is_some()
                {
                    return Err("scopes are not a tree in creation order".to_owned());
                }
            }
            for r in first.references() {
                let kind = tree.node(r.node).map(|n| n.kind());
                if !matches!(
                    kind,
                    Some(N::Identifier { .. } | N::IdentifierName { .. } | N::JsxIdentifier)
                ) {
                    return Err("a reference is not an identifier".to_owned());
                }
                if tree.identifier_value(r.node).as_deref() != Some(r.name.as_str()) {
                    return Err("a reference name differs from its node".to_owned());
                }
                if let Resolution::Binding(id) = r.target {
                    let binding = first.bindings().get(id.index()).ok_or("unknown binding")?;
                    if binding.name != r.name || !binding.meaning.overlaps(r.meaning) {
                        return Err("a reference resolved to another name or meaning".to_owned());
                    }
                }
            }
            if first
                .references()
                .windows(2)
                .any(|w| w[0].node >= w[1].node)
            {
                return Err("references are not sorted by node".to_owned());
            }
            for (index, binding) in first.bindings().iter().enumerate() {
                if binding.scope.index() >= first.scopes().len() || binding.declarations.is_empty()
                {
                    return Err("a binding has no scope or declaration".to_owned());
                }
                for name in binding.declarations.iter().filter_map(|d| d.name) {
                    if first
                        .declared(name)
                        .is_none_or(|b| first.bindings()[b.index()].name != binding.name)
                        && first.declared(name).map(|b| b.index()) != Some(index)
                    {
                        return Err("a declaration name is not indexed".to_owned());
                    }
                }
            }
        }
    }
    Ok(())
}
