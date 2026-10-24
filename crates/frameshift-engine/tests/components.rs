//! Component graph contracts (`D-096`): render sites resolved through
//! bindings, module edges, and recognized wrappers, with explicit reasons.

use frameshift_engine::bindings::ImportedName;
use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::modules::{GraphLimits, UnresolvedReason};
use frameshift_engine::nextjs::{
    ComponentError, ComponentGraph, Convention, DefinitionKind, Rendered, Resolved, Site,
    SsrOption, UnknownReason, Wrapper, analyze_app_with, link_components, summarize_components,
};
use frameshift_engine::project::{self, Project, ProjectError};
use std::fs;

mod support;

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn open(name: &str, files: &[(&str, &str)], limits: &Limits) -> Snapshot {
    let root = support::temporary().join("component-graph").join(name);
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    for (name, text) in files {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    snapshot(
        &root,
        limits.snapshot(),
        &mut FindingCollector::new(FindingLimits::default()),
    )
    .unwrap()
}

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];

const SOURCE: &[(&str, &str)] = &[
    (
        "app/layout.tsx",
        "import Shell from '../components/shell';\n\
         export default function RootLayout({ children }) {\n\
         return <html><body><Shell>{children}</Shell></body></html>;\n}\n",
    ),
    (
        "app/page.tsx",
        "import { memo } from 'react';\n\
         import Link from 'next/link';\n\
         import { Button, Card } from '../components';\n\
         import * as Ui from '../components/ui';\n\
         import Chart from '../components/chart';\n\
         import { Heavy } from '../components/lazy';\n\
         function Item() { return <li />; }\n\
         function Local() { const Button = () => <i />; return <Button />; }\n\
         function Page() {\n\
         const items = [1];\n\
         return <main><Link href=\"/\">home</Link><Button /><Card /><Ui.Badge /><Chart /><Heavy /><Local />\
         {items.map((i) => <Item key={i} />)}<my-element /></main>;\n}\n\
         export default memo(Page);\n",
    ),
    (
        "app/loading.tsx",
        "export { default } from '../components/card';\n",
    ),
    (
        "app/error.tsx",
        "'use client';\nexport default function Error() { return <p />; }\n",
    ),
    (
        "app/unknown/page.tsx",
        "import { withAuth } from '../../lib/auth';\n\
         import styled from 'fictional-styled';\n\
         import type { Kind } from '../../components/types';\n\
         import { Missing } from '../../components';\n\
         import { Twice } from '../../lib/ambiguous';\n\
         import Absent from '../../lib/absent';\n\
         import Broken from '../../lib/broken';\n\
         import Typed from '../../lib/typed';\n\
         import * as Icons from 'fictional-icons';\n\
         const Guarded = withAuth(() => <p />);\n\
         const Fancy = styled.div`color: red;`;\n\
         const Choice = Math.random() > 0.5 ? Guarded : Fancy;\n\
         let Changing = Guarded;\nChanging = Fancy;\n\
         const { Picked } = Icons;\n\
         const A = B;\nconst B = A;\n\
         class Legacy { Comp = null; render() { return <this.Comp />; } }\n\
         function Slot({ Icon }) { return <Icon />; }\n\
         function Overloaded(): null;\nfunction Overloaded() { return null; }\n\
         export default function Page() {\n\
         return <><Guarded /><Fancy /><Choice /><Changing /><Picked /><A /><Undeclared /><Missing />\
         <Twice /><Absent /><Broken /><Typed /><Icons.Home /><Kind /><Slot Icon={Guarded} /><Legacy />\
         <Overloaded /><Ui.Missing /></>;\n}\n",
    ),
    (
        "components/index.ts",
        "export * from './button';\nexport { default as Card } from './card';\n",
    ),
    (
        "components/button.tsx",
        "import { forwardRef } from 'react';\n\
         export const Button = forwardRef(function Button(props, ref) { return <button ref={ref} />; });\n",
    ),
    ("components/card.tsx", "export default () => <div />;\n"),
    (
        "components/ui.tsx",
        "import React from 'react';\nexport const Badge = React.memo(() => <span />);\n",
    ),
    (
        "components/chart.tsx",
        "import dynamic from 'next/dynamic';\n\
         const Chart = dynamic(() => import('./chart-impl').then((mod) => mod.ChartImpl), { ssr: false });\n\
         export default Chart;\n",
    ),
    (
        "components/chart-impl.tsx",
        "'use client';\nexport function ChartImpl() { return <canvas />; }\n",
    ),
    (
        "components/lazy.tsx",
        "import { lazy } from 'react';\nexport const Heavy = lazy(() => import('./heavy'));\n",
    ),
    (
        "components/heavy.tsx",
        "export default class Heavy { render() { return <section />; } }\n",
    ),
    (
        "components/shell.tsx",
        "export default function Shell({ children }) { return <div>{children}</div>; }\n",
    ),
    ("components/types.ts", "export type Kind = string;\n"),
    (
        "components/svg.jsx",
        "export const Icon = () => <svg:rect />;\n",
    ),
    ("lib/auth.ts", "export function withAuth(c) { return c; }\n"),
    (
        "lib/ambiguous.ts",
        "export * from './one';\nexport * from './two';\n",
    ),
    ("lib/one.tsx", "export const Twice = () => null;\n"),
    ("lib/two.tsx", "export const Twice = () => null;\n"),
    ("lib/broken.ts", "export default = ;\n"),
    (
        "lib/typed.d.ts",
        "declare const Typed: any;\nexport default Typed;\n",
    ),
];

fn build(name: &str, files: &[(&str, &str)]) -> Project {
    let limits = Limits::default();
    let snap = open(name, files, &limits);
    project::build(&snap, None, &path("app"), EXTENSIONS, &limits).unwrap()
}

fn text<'a>(files: &[(&str, &'a str)], file: &str) -> &'a str {
    files.iter().find(|(name, _)| *name == file).unwrap().1
}

/// The site whose opening tag starts with the first occurrence of `tag`.
fn site<'a>(graph: &'a ComponentGraph, file: &str, tag: &str) -> &'a Site {
    let start = text(SOURCE, file).find(tag).unwrap();
    graph
        .sites
        .iter()
        .find(|site| site.path.as_str() == file && site.span.start() == start)
        .unwrap_or_else(|| panic!("no site {tag} in {file}"))
}

fn definition_name(graph: &ComponentGraph, resolved: &Resolved) -> Option<String> {
    match &resolved.rendered {
        Rendered::Definition(index) => graph.definitions[*index].name.clone(),
        Rendered::Intrinsic | Rendered::External { .. } | Rendered::Unknown { .. } => None,
    }
}

fn definition_path<'a>(graph: &'a ComponentGraph, resolved: &Resolved) -> &'a str {
    match &resolved.rendered {
        Rendered::Definition(index) => graph.definitions[*index].path.as_str(),
        rendered @ (Rendered::Intrinsic | Rendered::External { .. } | Rendered::Unknown { .. }) => {
            panic!("not a definition: {rendered:?}")
        }
    }
}

fn reason(resolved: &Resolved) -> &UnknownReason {
    match &resolved.rendered {
        Rendered::Unknown { reason, .. } => reason,
        rendered @ (Rendered::Intrinsic | Rendered::Definition(_) | Rendered::External { .. }) => {
            panic!("not unknown: {rendered:?}")
        }
    }
}

fn owner(graph: &ComponentGraph, site: &Site) -> Option<String> {
    site.owner
        .and_then(|index| graph.definitions[index].name.clone())
}

#[test]
fn resolves_local_imported_barrel_and_namespace_components() {
    let model = build("resolution", SOURCE);
    let graph = model.components();
    let page = "app/page.tsx";
    assert_eq!(
        site(graph, page, "<main").resolved.rendered,
        Rendered::Intrinsic
    );
    assert_eq!(
        site(graph, page, "<my-element").resolved.rendered,
        Rendered::Intrinsic
    );
    let item = site(graph, page, "<Item");
    assert_eq!(
        definition_name(graph, &item.resolved).as_deref(),
        Some("Item")
    );
    // The map callback is not a definition; the page creates the element.
    assert_eq!(owner(graph, item).as_deref(), Some("Page"));
    assert_eq!(
        owner(graph, site(graph, page, "<li")).as_deref(),
        Some("Item")
    );
    // A shadowing local declaration wins over the imported name.
    let local = site(graph, page, "<Button />; }");
    assert_eq!(definition_path(graph, &local.resolved), page);
    assert_eq!(owner(graph, local).as_deref(), Some("Local"));
    // `export *` through the barrel, then `forwardRef` around a function.
    let button = site(graph, page, "<Button /><Card");
    assert_eq!(button.resolved.wrappers, [Wrapper::ForwardRef]);
    assert_eq!(
        definition_path(graph, &button.resolved),
        "components/button.tsx"
    );
    assert_eq!(
        definition_name(graph, &button.resolved).as_deref(),
        Some("Button")
    );
    // `export { default as Card }` to an anonymous arrow named `default`.
    let card = site(graph, page, "<Card");
    assert!(card.resolved.wrappers.is_empty());
    assert_eq!(
        definition_path(graph, &card.resolved),
        "components/card.tsx"
    );
    assert_eq!(
        definition_name(graph, &card.resolved).as_deref(),
        Some("default")
    );
    // A namespace member tag and `React.memo` through a default import.
    let badge = site(graph, page, "<Ui.Badge");
    assert_eq!(badge.resolved.wrappers, [Wrapper::Memo]);
    assert_eq!(definition_path(graph, &badge.resolved), "components/ui.tsx");
    let index = graph
        .definitions
        .iter()
        .position(|d| d.path.as_str() == "components/ui.tsx")
        .unwrap();
    assert_eq!(graph.definitions[index].kind, DefinitionKind::Arrow);
    assert_eq!(graph.renders(index).count(), 1);
    // A package component keeps its specifier, package, and export.
    assert_eq!(
        site(graph, page, "<Link").resolved.rendered,
        Rendered::External {
            specifier: "next/link".to_owned(),
            package: "next".to_owned(),
            export: ImportedName::Default,
            members: Vec::new(),
        }
    );
    let unknown = "app/unknown/page.tsx";
    let icons = site(graph, unknown, "<Icons.Home");
    assert_eq!(
        icons.resolved.rendered,
        Rendered::External {
            specifier: "fictional-icons".to_owned(),
            package: "fictional-icons".to_owned(),
            export: ImportedName::Namespace,
            members: vec!["Home".to_owned()],
        }
    );
    assert_eq!(
        site(graph, "components/svg.jsx", "<svg:rect")
            .resolved
            .rendered,
        Rendered::Intrinsic
    );
    // Definitions are sorted and unique.
    assert!(
        graph
            .definitions
            .windows(2)
            .all(|w| (&w[0].path, w[0].span) < (&w[1].path, w[1].span))
    );
}

#[test]
fn recognizes_lazy_and_dynamic_loaders_by_import_provenance() {
    let model = build("wrappers", SOURCE);
    let graph = model.components();
    let page = "app/page.tsx";
    let chart = site(graph, page, "<Chart");
    assert_eq!(
        chart.resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Literal(false))]
    );
    assert_eq!(
        definition_path(graph, &chart.resolved),
        "components/chart-impl.tsx"
    );
    assert_eq!(
        definition_name(graph, &chart.resolved).as_deref(),
        Some("ChartImpl")
    );
    let heavy = site(graph, page, "<Heavy");
    assert_eq!(heavy.resolved.wrappers, [Wrapper::Lazy]);
    let index = match heavy.resolved.rendered {
        Rendered::Definition(index) => index,
        ref rendered @ (Rendered::Intrinsic
        | Rendered::External { .. }
        | Rendered::Unknown { .. }) => panic!("{rendered:?}"),
    };
    assert_eq!(graph.definitions[index].kind, DefinitionKind::Class);
    // The method's tag is owned by the class, its innermost definition.
    assert_eq!(
        site(graph, "components/heavy.tsx", "<section").owner,
        Some(index)
    );

    let variants: &[(&str, &str)] = &[
        (
            "app/page.tsx",
            "import dynamic from 'next/dynamic';\n\
             import * as React from 'react';\n\
             import { memo as remember, lazy } from 'react';\n\
             import { lazy as notReact } from './fake';\n\
             const options = { ssr: false };\n\
             const A = dynamic(() => import('./target'));\n\
             const B = dynamic(async () => { return await import('./target'); }, { loading: () => null });\n\
             const C = dynamic(() => import('./target'), { ...options, ssr: true });\n\
             const D = dynamic(() => import('./target'), { ssr: true, ...options });\n\
             const E = dynamic(() => import('./target'), options);\n\
             const F = dynamic(() => import('./target').then((m) => ({ default: m.Named })), { ssr: !0 });\n\
             const G = lazy(() => import('./target').then((m) => m.Named));\n\
             const H = lazy(() => import('./target').then((m) => ({ default: m.Named })));\n\
             const I = dynamic(() => import(`./target`));\n\
             const J = notReact(() => import('./target'));\n\
             const K = React.forwardRef(remember(() => null));\n\
             const L = dynamic(() => import('fictional-widgets').then((m) => m.Widget));\n\
             const M = lazy(load);\nfunction load() { return import('./target'); }\n\
             export default function Page() { return <><A /><B /><C /><D /><E /><F /><G /><H /><I /><J /><K /><L /><M /></>; }\n",
        ),
        (
            "app/target.tsx",
            "export default function Target() { return null; }\nexport function Named() { return null; }\n",
        ),
        ("app/fake.ts", "export const lazy = (f) => f;\n"),
    ];
    let model = build("loader-variants", variants);
    let graph = model.components();
    let source = text(variants, "app/page.tsx");
    let get = |tag: &str| {
        let start = source.find(tag).unwrap();
        graph
            .sites
            .iter()
            .find(|s| s.path.as_str() == "app/page.tsx" && s.span.start() == start)
            .unwrap()
    };
    let named = |tag: &str| definition_name(graph, &get(tag).resolved);
    assert_eq!(
        get("<A ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Absent)]
    );
    assert_eq!(named("<A ").as_deref(), Some("Target"));
    assert_eq!(
        get("<B ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Absent)]
    );
    assert_eq!(named("<B ").as_deref(), Some("Target"));
    assert_eq!(
        get("<C ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Literal(true))]
    );
    assert_eq!(
        get("<D ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Unknown)]
    );
    assert_eq!(
        get("<E ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Unknown)]
    );
    assert_eq!(
        get("<F ").resolved.wrappers,
        [Wrapper::Dynamic(SsrOption::Unknown)]
    );
    assert_eq!(named("<F ").as_deref(), Some("Named"));
    // `lazy` needs a module or a `default` object.
    assert_eq!(reason(&get("<G ").resolved), &UnknownReason::Loader);
    assert_eq!(named("<H ").as_deref(), Some("Named"));
    // A template literal is a computed specifier for the resolver.
    assert_eq!(
        reason(&get("<I ").resolved),
        &UnknownReason::Unresolved(UnresolvedReason::Computed)
    );
    // A local function named `lazy` is a call, not React's wrapper.
    assert_eq!(
        reason(&get("<J ").resolved),
        &UnknownReason::Call(Some("notReact".to_owned()))
    );
    assert_eq!(
        get("<K ").resolved.wrappers,
        [Wrapper::ForwardRef, Wrapper::Memo]
    );
    assert_eq!(
        get("<L ").resolved.rendered,
        Rendered::External {
            specifier: "fictional-widgets".to_owned(),
            package: "fictional-widgets".to_owned(),
            export: ImportedName::Named("Widget".to_owned()),
            members: Vec::new(),
        }
    );
    assert_eq!(reason(&get("<M ").resolved), &UnknownReason::Loader);

    // A `react` specifier that the configuration maps to a local file does
    // not import React's `memo`; the call stays a higher-order component.
    let shimmed: &[(&str, &str)] = &[
        (
            "tsconfig.json",
            "{ \"compilerOptions\": { \"baseUrl\": \".\", \"paths\": { \"react\": [\"./shim/react.ts\"] } } }",
        ),
        ("shim/react.ts", "export const memo = (c) => c;\n"),
        (
            "app/page.tsx",
            "import { memo } from 'react';\nconst Inner = () => null;\nconst Outer = memo(Inner);\n\
             export default function Page() { return <Outer />; }\n",
        ),
    ];
    let model = build("shimmed-react", shimmed);
    let graph = model.components();
    let page = text(shimmed, "app/page.tsx");
    let outer = graph
        .sites
        .iter()
        .find(|s| s.span.start() == page.find("<Outer").unwrap())
        .unwrap();
    assert!(outer.resolved.wrappers.is_empty());
    assert!(matches!(
        &outer.resolved.rendered,
        Rendered::Unknown { reason: UnknownReason::Call(Some(callee)), span: Some(span), .. }
            if callee == "memo" && span.start() == page.find("memo(Inner)").unwrap()
    ));
}

#[test]
fn keeps_every_unestablished_value_explicit() {
    let model = build("reasons", SOURCE);
    let graph = model.components();
    let file = "app/unknown/page.tsx";
    let expect = |tag: &str, expected: UnknownReason| {
        let site = site(graph, file, tag);
        assert_eq!(reason(&site.resolved), &expected, "{tag}");
    };
    expect("<Guarded", UnknownReason::Call(Some("withAuth".to_owned())));
    expect("<Fancy", UnknownReason::Call(Some("styled.div".to_owned())));
    expect("<Choice", UnknownReason::Value);
    expect("<Changing", UnknownReason::Mutable);
    expect("<Picked", UnknownReason::Destructured);
    expect("<A ", UnknownReason::Cycle);
    expect(
        "<Undeclared",
        UnknownReason::Global("Undeclared".to_owned()),
    );
    expect("<Missing", UnknownReason::NotExported);
    expect("<Twice", UnknownReason::AmbiguousStar);
    expect(
        "<Absent",
        UnknownReason::Unresolved(UnresolvedReason::NotFound),
    );
    expect("<Broken", UnknownReason::Unparsed);
    expect("<Typed", UnknownReason::Declaration);
    expect("<Kind", UnknownReason::TypeOnly);
    expect("<Icon", UnknownReason::Parameter);
    expect("<this.Comp", UnknownReason::This);
    expect("<Ui.Missing", UnknownReason::Global("Ui".to_owned()));
    // A class and an overloaded function with one implementation resolve.
    assert_eq!(
        definition_name(graph, &site(graph, file, "<Legacy").resolved).as_deref(),
        Some("Legacy")
    );
    assert_eq!(
        definition_name(graph, &site(graph, file, "<Overloaded").resolved).as_deref(),
        Some("Overloaded")
    );
    // Unknown reasons keep where they arose.
    let broken = site(graph, file, "<Broken");
    assert!(matches!(
        &broken.resolved.rendered,
        Rendered::Unknown { path, span: None, .. } if path.as_str() == "lib/broken.ts"
    ));
    let guarded = site(graph, file, "<Guarded");
    let start = text(SOURCE, file).find("withAuth(() =>").unwrap();
    assert!(matches!(
        &guarded.resolved.rendered,
        Rendered::Unknown { path, span: Some(span), .. }
            if path.as_str() == file && span.start() == start
    ));
}

#[test]
fn lists_rendered_convention_defaults_as_entries() {
    let model = build("entries", SOURCE);
    let graph = model.components();
    let entries: Vec<_> = graph
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                entry.convention,
                entry.resolved.wrappers.clone(),
                definition_name(graph, &entry.resolved),
            )
        })
        .collect();
    assert_eq!(
        entries,
        [
            (
                "app/error.tsx",
                Convention::Error,
                vec![],
                Some("Error".to_owned())
            ),
            (
                "app/layout.tsx",
                Convention::Layout,
                vec![],
                Some("RootLayout".to_owned())
            ),
            (
                "app/loading.tsx",
                Convention::Loading,
                vec![],
                Some("default".to_owned())
            ),
            (
                "app/page.tsx",
                Convention::Page,
                vec![Wrapper::Memo],
                Some("Page".to_owned())
            ),
            (
                "app/unknown/page.tsx",
                Convention::Page,
                vec![],
                Some("Page".to_owned())
            ),
        ]
    );
    // The layout's tags belong to the layout definition.
    let layout = site(graph, "app/layout.tsx", "<Shell");
    assert_eq!(owner(graph, layout).as_deref(), Some("RootLayout"));
    assert_eq!(
        definition_path(graph, &layout.resolved),
        "components/shell.tsx"
    );

    let broken: &[(&str, &str)] = &[
        ("app/page.tsx", "export default = ;"),
        ("app/layout.tsx", "export const layout = 1;"),
    ];
    let model = build("broken-entries", broken);
    let entries = &model.components().entries;
    assert_eq!(reason(&entries[0].resolved), &UnknownReason::NotExported);
    assert_eq!(reason(&entries[1].resolved), &UnknownReason::Unparsed);
}

#[test]
fn star_exports_follow_ecmascript_resolution() {
    let files: &[(&str, &str)] = &[
        (
            "app/page.tsx",
            "import { Deep, Shadowed, Diamond, Packaged } from '../lib/barrel';\n\
             export default function Page() { return <><Deep /><Shadowed /><Diamond /><Packaged /></>; }\n",
        ),
        (
            "lib/barrel.ts",
            "export * from './level';\nexport * from './left';\nexport * from './right';\n\
             export const Shadowed = () => null;\nexport * from './cycle';\n",
        ),
        ("lib/level.ts", "export * from './deep';\n"),
        (
            "lib/deep.tsx",
            "export const Deep = () => null;\nexport const Shadowed = () => null;\nexport default () => null;\n",
        ),
        ("lib/left.ts", "export * from './shared';\n"),
        ("lib/right.ts", "export * from './shared';\n"),
        ("lib/shared.tsx", "export const Diamond = () => null;\n"),
        (
            "lib/cycle.ts",
            "export * from './barrel';\nexport * from 'fictional-kit';\n",
        ),
    ];
    let model = build("stars", files);
    let graph = model.components();
    let text = text(files, "app/page.tsx");
    let get = |tag: &str| {
        let start = text.find(tag).unwrap();
        &graph
            .sites
            .iter()
            .find(|s| s.path.as_str() == "app/page.tsx" && s.span.start() == start)
            .unwrap()
            .resolved
    };
    // A star export into a package makes every name it may provide
    // ambiguous, even with one local provider.
    assert_eq!(reason(get("<Deep")), &UnknownReason::AmbiguousStar);
    // An explicit export wins over star exports.
    assert_eq!(definition_path(graph, get("<Shadowed")), "lib/barrel.ts");
    assert_eq!(reason(get("<Diamond")), &UnknownReason::AmbiguousStar);
    // A name that no local module exports may still come from the package.
    assert_eq!(reason(get("<Packaged")), &UnknownReason::AmbiguousStar);

    let local: Vec<(&str, &str)> = files
        .iter()
        .map(|&(name, text)| {
            if name == "lib/cycle.ts" {
                (name, "export * from './barrel';\n")
            } else {
                (name, text)
            }
        })
        .collect();
    let model = build("stars-local", &local);
    let graph = model.components();
    assert_eq!(
        definition_path(graph, get_in(graph, &local, "<Deep")),
        "lib/deep.tsx"
    );
    // Two paths to one module are one provider.
    assert_eq!(
        definition_path(graph, get_in(graph, &local, "<Diamond")),
        "lib/shared.tsx"
    );
    assert_eq!(
        reason(get_in(graph, &local, "<Packaged")),
        &UnknownReason::NotExported
    );
}

fn get_in<'a>(graph: &'a ComponentGraph, files: &[(&str, &str)], tag: &str) -> &'a Resolved {
    let start = text(files, "app/page.tsx").find(tag).unwrap();
    &graph
        .sites
        .iter()
        .find(|s| s.path.as_str() == "app/page.tsx" && s.span.start() == start)
        .unwrap()
        .resolved
}

#[test]
fn linking_is_repeatable_and_its_work_boundary_inclusive() {
    let first = build("repeat-a", SOURCE);
    let second = build("repeat-b", SOURCE);
    assert_eq!(first.components(), second.components());

    let limits = Limits::default();
    let snap = open("budget", SOURCE, &limits);
    let mut summaries = Vec::new();
    let app = analyze_app_with(
        &snap,
        None,
        &path("app"),
        EXTENSIONS,
        GraphLimits {
            modules: limits.modules,
            text: limits.text,
            parse: limits.parse,
            json: limits.json,
            path: limits.path,
        },
        &mut |path, tree, bindings| {
            summaries.push(summarize_components(path, tree, bindings));
            Ok(())
        },
    )
    .unwrap();
    assert!(summaries.windows(2).all(|w| w[0].path() < w[1].path()));
    let link = |work| link_components(&app.graph, &app.routes, &summaries, work);
    let complete = link(u64::MAX).unwrap();
    assert_eq!(&complete, first.components());
    // The smallest sufficient budget, found by bisection.
    let (mut low, mut high) = (0u64, 1u64 << 20);
    assert!(link(high).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        if link(middle).is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(low > 0);
    assert_eq!(link(low).unwrap(), complete);
    assert_eq!(link(low - 1), Err(ComponentError::WorkLimit));

    let mut tight = Limits::default();
    tight.modules.max_work = 1;
    let snap = open("project-budget", SOURCE, &tight);
    assert!(matches!(
        project::build(&snap, None, &path("app"), EXTENSIONS, &tight),
        Err(ProjectError::App(_) | ProjectError::WorkLimit)
    ));
}

fn graph_snapshot() -> String {
    use std::fmt::Write;
    let model = build("golden", SOURCE);
    let graph = model.components();
    let slice = |file: &str, start: usize, end: usize| -> String {
        let text = text(SOURCE, file);
        let spelling = &text[start..end.min(start + 40)];
        spelling.replace('\n', " ")
    };
    let mut out = String::new();
    for (index, definition) in graph.definitions.iter().enumerate() {
        writeln!(
            out,
            "definition {index} {:?} {:?} {:?} {:?}",
            definition.path.as_str(),
            definition.kind,
            definition.name,
            slice(
                definition.path.as_str(),
                definition.span.start(),
                definition.span.end()
            )
        )
        .unwrap();
    }
    for site in &graph.sites {
        writeln!(
            out,
            "site {:?} {:?} owner={:?} wrappers={:?} {:?}",
            site.path.as_str(),
            slice(site.path.as_str(), site.span.start(), site.span.end()),
            site.owner,
            site.resolved.wrappers,
            site.resolved.rendered
        )
        .unwrap();
    }
    for entry in &graph.entries {
        writeln!(
            out,
            "entry {:?} {:?} wrappers={:?} {:?}",
            entry.path.as_str(),
            entry.convention,
            entry.resolved.wrappers,
            entry.resolved.rendered
        )
        .unwrap();
    }
    out
}

#[test]
fn component_graph_golden() {
    assert_eq!(graph_snapshot(), include_str!("golden/components.txt"));
}

#[test]
#[ignore = "explicit golden update"]
fn update_components_golden() {
    fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/components.txt"),
        graph_snapshot(),
    )
    .unwrap();
}
