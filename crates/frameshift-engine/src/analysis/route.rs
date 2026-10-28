//! Strict-tree evidence for independent static parameter enumeration.

use super::assess::Syntax;
use frameshift_bindings::{Bindings, DeclarationKind, ExportKind, ExportName, Target};
use frameshift_nextjs::{Convention, SegmentKind};
use frameshift_syntax::{NodeId, SyntaxNodeKind as N, SyntaxTree};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub(crate) struct Summary {
    generator: Option<bool>,
    parameters: Option<Parameters>,
    default_function: Option<NodeId>,
}
#[derive(Clone, Debug, Default)]
struct Parameters {
    direct: BTreeSet<String>,
    destructured: BTreeSet<String>,
    // `params` in `params.x` and `await params` in its destructuring; both
    // read `Astro.params` in an Astro page.
    substitutions: Vec<frameshift_core::Span>,
}
impl Summary {
    /// The default export's function, with the expressions that read its
    /// page parameters when every use is recognized.
    pub(crate) fn page(&self) -> Option<(NodeId, Option<&[frameshift_core::Span]>)> {
        Some((
            self.default_function?,
            self.parameters.as_ref().map(|p| p.substitutions.as_slice()),
        ))
    }
    /// Parameter names that the default export reads, if every use is recognized.
    pub(crate) fn parameter_keys(&self) -> Option<impl Iterator<Item = &String>> {
        self.parameters
            .as_ref()
            .map(|p| p.direct.iter().chain(&p.destructured))
    }
}
fn contains(outer: frameshift_core::Span, inner: frameshift_core::Span) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}
fn function(tree: &SyntaxTree<'_>, bindings: &Bindings, name: ExportName) -> Option<NodeId> {
    let export = bindings
        .exports()
        .iter()
        .find(|e| e.name == name && !e.type_only)?;
    let mut node = match export.kind {
        ExportKind::Local(Target::Binding(id)) => {
            if bindings
                .references()
                .iter()
                .any(|r| r.target == Target::Binding(id) && r.write)
            {
                return None;
            }
            let [declaration] = bindings.binding(id).declarations.as_slice() else {
                return None;
            };
            declaration.node
        }
        ExportKind::Expression(node) => node,
        ExportKind::Local(Target::Global | Target::Dynamic) | ExportKind::ReExport { .. } => {
            return None;
        }
    };
    if let N::VariableDeclarator {
        init: Some(init), ..
    } = tree.node(node)?.kind()
    {
        node = super::assess::unwrap(tree, init);
    }
    matches!(
        tree.node(node)?.kind(),
        N::FunctionDeclaration { .. } | N::FunctionExpression { .. } | N::ArrowFunction { .. }
    )
    .then_some(node)
}
fn parameters(tree: &SyntaxTree<'_>, node: NodeId) -> Option<NodeId> {
    if let N::FunctionDeclaration {
        parameters, kind, ..
    }
    | N::FunctionExpression {
        parameters, kind, ..
    } = tree.node(node)?.kind()
    {
        (!kind.is_generator()).then_some(parameters)
    } else if let N::ArrowFunction { parameters, .. } = tree.node(node)?.kind() {
        Some(parameters)
    } else {
        None
    }
}
pub(crate) fn summarize(tree: &SyntaxTree<'_>, bindings: &Bindings) -> Summary {
    let generator = function(
        tree,
        bindings,
        ExportName::Named("generateStaticParams".into()),
    )
    .map(|node| {
        let span = tree.node(node).unwrap().span();
        let Some(params) = parameters(tree, node) else {
            return false;
        };
        if tree
            .child_nodes(params)
            .is_none_or(|mut p| p.next().is_some())
        {
            return false;
        }
        if tree.nodes().iter().any(|n| {
            contains(span, n.span()) && matches!(n.kind(), N::This | N::MetaProperty { .. })
        }) {
            return false;
        }
        bindings
            .references()
            .iter()
            .filter(|r| contains(span, tree.node(r.node).unwrap().span()))
            .all(|r| match r.target {
                Target::Binding(id) => {
                    let binding = bindings.binding(id);
                    binding.import.as_ref().is_some_and(|i| !i.type_only)
                        || binding.declarations.iter().all(|d| {
                            d.kind != DeclarationKind::Arguments
                                && d.node != node
                                && contains(span, tree.node(d.node).unwrap().span())
                        })
                }
                Target::Global => r.name != "eval" && r.name != "arguments",
                Target::Dynamic => false,
            })
    });
    let default_function = function(tree, bindings, ExportName::Default);
    let parameters = default_function.and_then(|node| page_parameters(tree, bindings, node));
    Summary {
        generator,
        parameters,
        default_function,
    }
}
fn page_parameters(tree: &SyntaxTree<'_>, bindings: &Bindings, node: NodeId) -> Option<Parameters> {
    let mut result = Parameters::default();
    let span = tree.node(node)?.span();
    if bindings.references().iter().any(|r| {
        contains(span, tree.node(r.node).unwrap().span())
            && match r.target {
                Target::Binding(id) => bindings
                    .binding(id)
                    .declarations
                    .iter()
                    .any(|d| d.kind == DeclarationKind::Arguments),
                Target::Dynamic => true,
                Target::Global => r.name == "eval" || r.name == "arguments",
            }
    }) {
        return None;
    }
    let params: Vec<_> = tree.child_nodes(parameters(tree, node)?)?.collect();
    if params.is_empty() {
        return Some(result);
    }
    let [param] = params.as_slice() else {
        return None;
    };
    let param = super::assess::unwrap(tree, *param);
    let N::ObjectPattern { properties, .. } = tree.node(param)?.kind() else {
        return None;
    };
    let mut binding = None;
    for &property in tree.children(properties)? {
        let (name, target) = if let N::ShorthandPatternProperty {
            target,
            default: None,
        } = tree.node(property)?.kind()
        {
            (tree.identifier_value(target)?, target)
        } else if let N::PatternProperty {
            key,
            value,
            computed: false,
            ..
        } = tree.node(property)?.kind()
        {
            (
                tree.identifier_value(key)
                    .or_else(|| tree.string_literal_value(key))?,
                super::assess::unwrap(tree, value),
            )
        } else {
            return None;
        };
        if name == "params" {
            if binding.is_some() {
                return None;
            }
            binding = Some(bindings.declared(target)?);
        }
    }
    let Some(binding) = binding else {
        return Some(result);
    };
    let mut accepted = BTreeSet::new();
    for n in tree.nodes().iter().filter(|n| contains(span, n.span())) {
        if let N::Member {
            object,
            property,
            computed: false,
            optional: false,
        } = n.kind()
        {
            let object = super::assess::unwrap(tree, object);
            if bindings
                .reference(object)
                .is_some_and(|r| r.target == Target::Binding(binding))
            {
                accepted.insert(object);
                result.direct.insert(tree.identifier_value(property)?);
                result.substitutions.push(tree.node(object)?.span());
            }
        } else if let N::VariableDeclarator {
            target,
            init: Some(init),
        } = n.kind()
        {
            let init = super::assess::unwrap(tree, init);
            let N::Await { argument } = tree.node(init)?.kind() else {
                continue;
            };
            let argument = super::assess::unwrap(tree, argument);
            if !bindings
                .reference(argument)
                .is_some_and(|r| r.target == Target::Binding(binding))
            {
                continue;
            }
            let N::ObjectPattern { properties, .. } = tree.node(target)?.kind() else {
                return None;
            };
            for &p in tree.children(properties)? {
                let key = if let N::ShorthandPatternProperty {
                    target,
                    default: None,
                } = tree.node(p)?.kind()
                {
                    tree.identifier_value(target)?
                } else if let N::PatternProperty {
                    key,
                    value,
                    computed: false,
                    ..
                } = tree.node(p)?.kind()
                {
                    bindings.declared(super::assess::unwrap(tree, value))?;
                    tree.identifier_value(key)
                        .or_else(|| tree.string_literal_value(key))?
                } else {
                    return None;
                };
                result.destructured.insert(key);
            }
            accepted.insert(argument);
            result.substitutions.push(tree.node(init)?.span());
        }
    }
    bindings
        .references()
        .iter()
        .filter(|r| r.target == Target::Binding(binding))
        .all(|r| !r.write && accepted.contains(&r.node))
        .then_some(result)
}
pub(crate) fn automatic(route: &frameshift_nextjs::AppRoute, syntax: &Syntax) -> bool {
    let dynamic: Vec<_> = route
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            matches!(
                s.kind,
                SegmentKind::Dynamic(_)
                    | SegmentKind::CatchAll(_)
                    | SegmentKind::OptionalCatchAll(_)
            )
        })
        .collect();
    let [(index, segment)] = dynamic.as_slice() else {
        return false;
    };
    let (parameter, catch_all) = match &segment.kind {
        SegmentKind::Dynamic(p) => (p, false),
        SegmentKind::CatchAll(p) => (p, true),
        SegmentKind::Static
        | SegmentKind::Group
        | SegmentKind::Parallel
        | SegmentKind::OptionalCatchAll(_) => return false,
    };
    let paths: Vec<_> = std::iter::once((&route.file, true))
        .chain(
            route
                .chain
                .iter()
                .filter(|f| f.convention == Convention::Layout)
                .map(|f| (&f.path, f.segments.len() > *index)),
        )
        .collect();
    let mut generators = 0;
    for (path, can_generate) in paths {
        let Some(module) = syntax.modules.get(path) else {
            return false;
        };
        if can_generate && let Some(safe) = module.route.generator {
            if !safe {
                return false;
            }
            generators += 1;
        }
        let Some(params) = &module.route.parameters else {
            return false;
        };
        if params
            .direct
            .iter()
            .chain(&params.destructured)
            .any(|p| p != parameter)
            || (catch_all && !params.direct.is_empty())
        {
            return false;
        }
    }
    generators == 1
}
