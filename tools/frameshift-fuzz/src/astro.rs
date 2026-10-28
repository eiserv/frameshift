//! The `astro` target: JSX conversion and Astro printing (`D-044`, `D-048`).
//!
//! The input is parsed as a JSX and as a TSX module. Up to a fixed number of
//! JSX elements and fragments, in source order, are converted with a resolver
//! that accepts every component, alternately as an island, and every result
//! is printed with two formatting options. Up to the same number of
//! functions are converted as components and printed the same way. A
//! rejection is a valid outcome if its anchor lies in the source and it
//! repeats identically.

use frameshift_engine::{
    AstroComponent, ClientDirective, ComponentConversion, ComponentName, ComponentReference,
    ComponentStrategy, ConvertedMarkup, ElementResolver, EmitLimits, Indent, JsDialect,
    JsxConversion, LineEnding, NodeId, ParseLimits, PrintOptions, SourceGoal, SourceId, SourceText,
    SyntaxNodeKind, SyntaxTree, convert_component, convert_jsx, parse_js_program,
    print_astro_component,
};

use crate::targets::{source, text};

/// Converted roots per dialect, which bounds the work of one execution.
const MAX_ROOTS: usize = 32;

/// Runs one input.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = text(input)? else {
        return Ok(());
    };
    let source = source(text)?;
    for dialect in [JsDialect::Jsx, JsDialect::Tsx] {
        let Ok(tree) =
            parse_js_program(&source, SourceGoal::Module, dialect, ParseLimits::default())
        else {
            continue;
        };
        for root in jsx_roots(&tree)? {
            check_root(&source, &tree, root).map_err(|error| format!("{dialect:?}: {error}"))?;
        }
        for function in functions(&tree)? {
            check_function(&source, &tree, function)
                .map_err(|error| format!("{dialect:?}: {error}"))?;
        }
    }
    Ok(())
}

// JSX elements and fragments in source order, found without recursion.
fn jsx_roots(tree: &SyntaxTree<'_>) -> Result<Vec<NodeId>, String> {
    let mut roots = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        let node = tree.node(id).ok_or("node outside the tree")?;
        if matches!(
            node.kind(),
            SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
        ) {
            roots.push(id);
            if roots.len() == MAX_ROOTS {
                break;
            }
        }
        let children: Vec<NodeId> = tree.child_nodes(id).ok_or("invalid node")?.collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(roots)
}

// Function declarations, expressions, and arrows in source order.
fn functions(tree: &SyntaxTree<'_>) -> Result<Vec<NodeId>, String> {
    let mut functions = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        let node = tree.node(id).ok_or("node outside the tree")?;
        if matches!(
            node.kind(),
            SyntaxNodeKind::FunctionDeclaration { .. }
                | SyntaxNodeKind::FunctionExpression { .. }
                | SyntaxNodeKind::ArrowFunction { .. }
        ) {
            functions.push(id);
            if functions.len() == MAX_ROOTS {
                break;
            }
        }
        let children: Vec<NodeId> = tree.child_nodes(id).ok_or("invalid node")?.collect();
        stack.extend(children.into_iter().rev());
    }
    Ok(functions)
}

struct Accepting;

impl ElementResolver for Accepting {
    fn resolve(&mut self, reference: ComponentReference<'_>) -> Option<ComponentStrategy> {
        let name =
            ComponentName::new(reference.spelling).or_else(|| ComponentName::new("Fallback"))?;
        let directive = (reference.spelling.len() % 2 == 1).then_some(ClientDirective::Load);
        Some(ComponentStrategy::Component { name, directive })
    }
}

fn convert(
    tree: &SyntaxTree<'_>,
    root: NodeId,
) -> Result<ConvertedMarkup, frameshift_engine::ConvertError> {
    let conversion = JsxConversion {
        children_binding: Some("children"),
        ..JsxConversion::default()
    };
    convert_jsx(tree, SourceId(0), root, &conversion, &mut Accepting)
}

fn check_root(source: &SourceText, tree: &SyntaxTree<'_>, root: NodeId) -> Result<(), String> {
    let first = convert(tree, root);
    if first != convert(tree, root) {
        return Err("conversion is not deterministic".to_owned());
    }
    let markup = match first {
        Ok(markup) => markup,
        Err(error) => {
            return if error.anchor.source == SourceId(0) && source.slice(error.anchor.span).is_ok()
            {
                Ok(())
            } else {
                Err(format!("rejection anchor outside the source: {error}"))
            };
        }
    };
    for span in &markup.copied {
        source
            .slice(*span)
            .map_err(|_| format!("copied span {span:?} is not a source range"))?;
    }
    for assumption in &markup.assumptions {
        source
            .slice(assumption.anchor.span)
            .map_err(|_| format!("assumption anchor {:?} is not a source range", assumption))?;
    }
    let component = AstroComponent {
        frontmatter: Vec::new(),
        template: markup.nodes,
    };
    for options in [
        PrintOptions::default(),
        PrintOptions {
            indent: Indent::Tab,
            line_ending: LineEnding::CrLf,
        },
    ] {
        let printed = print_astro_component(&component, &[source], options, EmitLimits::default());
        let again = print_astro_component(&component, &[source], options, EmitLimits::default());
        if printed != again {
            return Err("printing is not deterministic".to_owned());
        }
        // Converted markup has only valid names and anchors, so printing can
        // fail only at the depth limit.
        match printed {
            Ok(emitted) => emitted
                .verify(&[source])
                .map_err(|error| format!("printed provenance: {error}"))?,
            Err(frameshift_engine::PrintError::TooDeep(_)) => {}
            Err(error) => return Err(format!("printing converted markup failed: {error}")),
        }
    }
    Ok(())
}

fn check_function(
    source: &SourceText,
    tree: &SyntaxTree<'_>,
    function: NodeId,
) -> Result<(), String> {
    let convert = || {
        convert_component(
            tree,
            SourceId(0),
            function,
            &ComponentConversion::default(),
            &mut Accepting,
        )
    };
    let first = convert();
    if first != convert() {
        return Err("component conversion is not deterministic".to_owned());
    }
    let converted = match first {
        Ok(converted) => converted,
        Err(error) => {
            return if error.anchor.source == SourceId(0) && source.slice(error.anchor.span).is_ok()
            {
                Ok(())
            } else {
                Err(format!("rejection anchor outside the source: {error}"))
            };
        }
    };
    for span in &converted.copied {
        source
            .slice(*span)
            .map_err(|_| format!("copied span {span:?} is not a source range"))?;
    }
    let span = tree.node(function).ok_or("invalid function")?.span();
    for comment in tree.comments() {
        let inside = span.start() <= comment.span.start() && comment.span.end() <= span.end();
        let copied = converted
            .copied
            .iter()
            .any(|c| c.start() <= comment.span.start() && comment.span.end() <= c.end());
        if inside && !copied {
            return Err(format!("comment {:?} is dropped", comment.span));
        }
    }
    for options in [
        PrintOptions::default(),
        PrintOptions {
            indent: Indent::Tab,
            line_ending: LineEnding::CrLf,
        },
    ] {
        let printed = print_astro_component(
            &converted.component,
            &[source],
            options,
            EmitLimits::default(),
        );
        if printed
            != print_astro_component(
                &converted.component,
                &[source],
                options,
                EmitLimits::default(),
            )
        {
            return Err("component printing is not deterministic".to_owned());
        }
        // Copied statements may contain a line that starts with `---`.
        match printed {
            Ok(emitted) => emitted
                .verify(&[source])
                .map_err(|error| format!("printed provenance: {error}"))?,
            Err(
                frameshift_engine::PrintError::TooDeep(_)
                | frameshift_engine::PrintError::FrontmatterFence(_),
            ) => {}
            Err(error) => return Err(format!("printing a converted component failed: {error}")),
        }
    }
    Ok(())
}
