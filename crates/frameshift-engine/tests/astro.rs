//! Astro printing and JSX conversion contracts through the public facade,
//! using fictional components.

use frameshift_engine::{
    Anchor, AssumptionKind, AstroComponent, ClientDirective, ComponentConversion, ComponentName,
    ComponentReference, ComponentStrategy, ConvertErrorKind, ConvertedMarkup, ElementResolver,
    EmitLimits, ExpressionPiece, FrontmatterItem, HtmlTagName, ImportItem, Indent, JsDialect,
    JsIdentifier, JsxConversion, LineEnding, NamedImport, NodeId, ParseLimits, PrintError,
    PrintOptions, PropMember, PropType, PropsSource, RegionOrigin, SourceGoal, SourceId,
    SourceText, Span, SyntaxNodeKind, SyntaxTree, TemplateNode, TextLimits, convert_component,
    convert_jsx, parse_js_program, print_astro_component,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

struct Resolver;

impl ElementResolver for Resolver {
    fn resolve(&mut self, reference: ComponentReference<'_>) -> Option<ComponentStrategy> {
        match reference.spelling {
            "Link" => Some(ComponentStrategy::Html(HtmlTagName::new("a")?)),
            "Counter" => Some(ComponentStrategy::Component {
                name: ComponentName::new("Counter")?,
                directive: Some(ClientDirective::Load),
            }),
            "Card" | "Menu.Item" => Some(ComponentStrategy::Component {
                name: ComponentName::new(reference.spelling)?,
                directive: None,
            }),
            _ => None,
        }
    }
}

// Finds the expression of the first `return` statement in source order,
// iteratively, so that deep trees cannot overflow the test's stack.
fn returned(tree: &SyntaxTree<'_>) -> NodeId {
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        if let SyntaxNodeKind::ReturnStatement { argument } = tree.node(id).unwrap().kind() {
            return argument.expect("a returned expression");
        }
        let children: Vec<NodeId> = tree.child_nodes(id).unwrap().collect();
        stack.extend(children.into_iter().rev());
    }
    panic!("a return statement")
}

fn parse(text: &SourceText) -> SyntaxTree<'_> {
    parse_js_program(
        text,
        SourceGoal::Module,
        JsDialect::Tsx,
        ParseLimits::default(),
    )
    .unwrap()
}

fn convert(text: &SourceText, children: Option<&str>) -> Result<ConvertedMarkup, ConvertErrorKind> {
    let tree = parse(text);
    let conversion = JsxConversion {
        children_binding: children,
        ..JsxConversion::default()
    };
    convert_jsx(
        &tree,
        SourceId(0),
        returned(&tree),
        &conversion,
        &mut Resolver,
    )
    .map_err(|error| error.kind)
}

fn component(body: &str) -> SourceText {
    source(&format!(
        "export default function Example({{ children, items, count }}) {{\n  return (\n{body}\n  );\n}}\n"
    ))
}

fn print(text: &SourceText, nodes: Vec<TemplateNode>) -> String {
    let component = AstroComponent {
        frontmatter: Vec::new(),
        template: nodes,
    };
    let emitted = print_astro_component(
        &component,
        &[text],
        PrintOptions::default(),
        EmitLimits::default(),
    )
    .unwrap();
    emitted.verify(&[text]).unwrap();
    emitted.as_str().to_owned()
}

fn converted_text(body: &str) -> String {
    let text = component(body);
    let markup = convert(&text, Some("children")).unwrap();
    print(&text, markup.nodes)
}

#[test]
fn html_attributes_follow_react_rendering() {
    let output = converted_text(
        r#"    <section className="prose" key="intro" data-ready aria-hidden hidden>
      <label htmlFor="name" tabIndex={0}>Name</label>
      <input id="name" disabled readOnly />
      <div dangerouslySetInnerHTML={{ __html: items }} />
    </section>"#,
    );
    assert_eq!(
        output,
        concat!(
            "<section class=\"prose\" data-ready=\"true\" aria-hidden=\"true\" hidden>\n",
            "  <label for=\"name\" tabindex={0}>Name</label>\n",
            "  <input id=\"name\" disabled readonly />\n",
            "  <div set:html={items}></div>\n",
            "</section>\n"
        )
    );
}

#[test]
fn text_keeps_react_whitespace_and_references() {
    let output = converted_text(
        "    <p>\n      Clicked {count} times &amp; &copy b,\n      then <b>stopped</b>.\n    </p>",
    );
    assert_eq!(
        output,
        "<p>Clicked {count} times &amp; &amp;copy b, then <b>stopped</b>.</p>\n"
    );
    let output = converted_text("    <pre>\n      kept\n      lines\n    </pre>");
    assert_eq!(output, "<pre>kept lines</pre>\n");
    let output = converted_text("    <p>&#123;braces&#125;&nbsp;&lt;</p>");
    assert_eq!(output, "<p>&#123;braces&#125;&nbsp;&lt;</p>\n");
}

#[test]
fn components_links_islands_and_slots_are_resolved() {
    let text = component(
        r#"    <Card title="Featured &amp; new" featured>
      <Link href="/contact" className="underline">Contact</Link>
      <Counter client initial={3} step={count} />
      <Menu.Item label='x' {...items} />
      {children}
    </Card>"#,
    );
    let markup = convert(&text, Some("children")).unwrap();
    let kinds: Vec<AssumptionKind> = markup.assumptions.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [AssumptionKind::SerializableIslandProp]);
    assert_eq!(
        print(&text, markup.nodes),
        concat!(
            "<Card title=\"Featured &amp; new\" featured={true}>\n",
            "  <a href=\"/contact\" class=\"underline\">Contact</a>\n",
            "  <Counter client:load client={true} initial={3} step={count} />\n",
            "  <Menu.Item label=\"x\" {...items} />\n",
            "  <slot />\n",
            "</Card>\n"
        )
    );
}

#[test]
fn expressions_are_copied_with_nested_markup_converted() {
    let text = component(
        "    <ul>\n      {/* keep this note */}\n      {items.map((item) => <li key={item.id} className=\"row\">{item.name}</li>)}\n      {count > 0 && <>none</>}\n    </ul>",
    );
    let markup = convert(&text, None).unwrap();
    let kinds: Vec<AssumptionKind> = markup.assumptions.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [AssumptionKind::ChildNotTrue]);
    let output = print(&text, markup.nodes);
    assert_eq!(
        output,
        concat!(
            "<ul>\n",
            "  {/* keep this note */}\n",
            "  {items.map((item) => <li class=\"row\">{item.name}</li>)}\n",
            "  {count > 0 && <>none</>}\n",
            "</ul>\n"
        )
    );
}

#[test]
fn unsupported_markup_fails_closed() {
    let cases = [
        (
            "<button onClick={items}>x</button>",
            ConvertErrorKind::UnsupportedAttribute,
        ),
        (
            "<div ref={items} />",
            ConvertErrorKind::UnsupportedAttribute,
        ),
        (
            "<rect strokeWidth=\"2\" />",
            ConvertErrorKind::UnsupportedAttribute,
        ),
        (
            "<svg viewBox=\"0 0 1 1\" />",
            ConvertErrorKind::ForeignContent,
        ),
        ("<div title />", ConvertErrorKind::ValuelessAttribute),
        (
            "<a title=\"one\ntwo\" />",
            ConvertErrorKind::MultilineAttributeString,
        ),
        ("<div>{...items}</div>", ConvertErrorKind::SpreadChild),
        ("<Unknown />", ConvertErrorKind::UnresolvedComponent),
        ("<svg:rect />", ConvertErrorKind::NamespacedName),
        ("<myElement />", ConvertErrorKind::UnsupportedElementName),
        (
            "<img src=\"a.png\">x</img>",
            ConvertErrorKind::VoidElementChildren,
        ),
        ("<style>{items}</style>", ConvertErrorKind::RawTextChildren),
        (
            "<div dangerouslySetInnerHTML={{ __html: items }}>x</div>",
            ConvertErrorKind::UnsupportedInnerHtml,
        ),
        (
            "<div style={{ marginTop: 4 }} />",
            ConvertErrorKind::UnsupportedStyle,
        ),
        (
            "<div style={{ msTransform: 'none' }} />",
            ConvertErrorKind::UnsupportedStyle,
        ),
        ("<p>a&#32;b</p>", ConvertErrorKind::WhitespaceReference),
        ("<p>a\tb</p>", ConvertErrorKind::UnsupportedTextCharacter),
        (
            "<a /* note */ href=\"/\">x</a>",
            ConvertErrorKind::CommentInMarkup,
        ),
        (
            "<Card icon={<b />} />",
            ConvertErrorKind::MarkupAttributeValue,
        ),
        (
            "<a {...[<b />]}>x</a>",
            ConvertErrorKind::MarkupAttributeValue,
        ),
        (
            "<p title={children}>x</p>",
            ConvertErrorKind::ChildrenOutsideSlot,
        ),
        (
            "<ul>{items.map((children) => <li>{children}</li>)}</ul>",
            ConvertErrorKind::ChildrenOutsideSlot,
        ),
        (
            "<p>{/* a */ children}</p>",
            ConvertErrorKind::CommentInMarkup,
        ),
    ];
    for (body, kind) in cases {
        let text = component(&format!("    {body}"));
        assert_eq!(
            convert(&text, Some("children")).map(|_| ()),
            Err(kind),
            "{body}"
        );
    }
}

#[test]
fn styles_with_string_values_are_copied() {
    let output = converted_text(
        "    <div style={{ backgroundColor: 'red', '--accent': \"blue\", WebkitTransition: 'none' }} />",
    );
    assert_eq!(
        output,
        "<div style={{ backgroundColor: 'red', '--accent': \"blue\", WebkitTransition: 'none' }}></div>\n"
    );
}

#[test]
fn spreads_and_dynamic_values_record_assumptions() {
    let text = component(
        "    <a {...items} href={count} key={items.next()} title={`t`}>x {count ? 'a' : 'b'}</a>",
    );
    let markup = convert(&text, None).unwrap();
    let kinds: Vec<AssumptionKind> = markup.assumptions.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            AssumptionKind::SpreadHtmlNames,
            AssumptionKind::AttributeValueSerialization,
            AssumptionKind::DiscardedKey,
        ]
    );
}

fn anchor(start: usize, end: usize) -> Anchor {
    Anchor {
        source: SourceId(0),
        span: Span::new(start, end).unwrap(),
    }
}

fn identifier(name: &str) -> JsIdentifier {
    JsIdentifier::new(name).unwrap()
}

#[test]
fn frontmatter_prints_imports_copies_and_props_with_options() {
    let text = source("// A note.\nconst year = 2026;\n");
    let mut component = AstroComponent {
        frontmatter: vec![
            FrontmatterItem::Import(ImportItem {
                default: Some(identifier("Layout")),
                named: Vec::new(),
                specifier: "../layouts/Layout.astro".to_owned(),
                anchor: anchor(0, 0),
            }),
            FrontmatterItem::Import(ImportItem {
                default: None,
                named: vec![NamedImport {
                    imported: identifier("format"),
                    local: Some(identifier("formatDate")),
                }],
                specifier: "./it's\n".to_owned(),
                anchor: anchor(0, 0),
            }),
            FrontmatterItem::Import(ImportItem {
                default: None,
                named: Vec::new(),
                specifier: "../styles/site.css".to_owned(),
                anchor: anchor(0, 0),
            }),
            FrontmatterItem::PropsInterface {
                members: vec![PropMember {
                    name: identifier("title"),
                    ty: PropType::String,
                    optional: true,
                }],
                anchor: anchor(0, 0),
            },
            FrontmatterItem::PropsBinding {
                names: vec![identifier("title")],
                anchor: anchor(0, 0),
            },
            FrontmatterItem::Copied(anchor(0, 29)),
        ],
        template: vec![TemplateNode::Text {
            value: "Year".to_owned(),
            anchor: anchor(11, 29),
        }],
    };
    let options = PrintOptions {
        indent: Indent::Tab,
        line_ending: LineEnding::CrLf,
    };
    let emitted =
        print_astro_component(&component, &[&text], options, EmitLimits::default()).unwrap();
    emitted.verify(&[&text]).unwrap();
    assert_eq!(
        emitted.as_str(),
        concat!(
            "---\r\n",
            "import Layout from '../layouts/Layout.astro';\r\n",
            "import { format as formatDate } from './it\\'s\\n';\r\n",
            "import '../styles/site.css';\r\n",
            "\r\n",
            "interface Props {\r\n",
            "\ttitle?: string;\r\n",
            "}\r\n",
            "const { title } = Astro.props;\r\n",
            "// A note.\nconst year = 2026;\r\n",
            "---\r\n",
            "Year\r\n"
        )
    );
    let copied = emitted
        .regions()
        .iter()
        .find(|region| matches!(region.origin, RegionOrigin::Copied(_)))
        .unwrap();
    assert_eq!(copied.origin, RegionOrigin::Copied(anchor(0, 29)));

    let fence = source("const a = `\n---\n`;");
    component.frontmatter = vec![FrontmatterItem::Copied(anchor(0, 18))];
    assert_eq!(
        print_astro_component(&component, &[&fence], options, EmitLimits::default()),
        Err(PrintError::FrontmatterFence(anchor(0, 18)))
    );
    let invalid = PrintOptions {
        indent: Indent::Spaces(0),
        line_ending: LineEnding::Lf,
    };
    assert_eq!(
        print_astro_component(&component, &[&text], invalid, EmitLimits::default()),
        Err(PrintError::InvalidIndent)
    );
}

#[test]
fn expression_pieces_print_generated_identifiers() {
    let text = source("x");
    let component = AstroComponent {
        frontmatter: Vec::new(),
        template: vec![TemplateNode::Expression(frameshift_engine::Expression {
            pieces: vec![
                ExpressionPiece::Identifier(identifier("title")),
                ExpressionPiece::Source(Span::new(0, 1).unwrap()),
            ],
            anchor: anchor(0, 1),
        })],
    };
    let emitted = print_astro_component(
        &component,
        &[&text],
        PrintOptions::default(),
        EmitLimits::default(),
    )
    .unwrap();
    assert_eq!(emitted.as_str(), "{titlex}\n");
    emitted.verify(&[&text]).unwrap();
}

#[test]
fn long_member_chains_neither_recurse_nor_overflow() {
    // The parser builds member chains without nesting, so a chain is limited
    // only by the node limit. Every walk of the conversion is iterative, so a
    // small stack suffices even in debug builds.
    let chain = format!("items{}", ".next".repeat(20_000));
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let key = component(&format!("    <ul><li key={{{chain}}}>x</li></ul>"));
            let markup = convert(&key, None).unwrap();
            assert!(markup.assumptions.is_empty());
            let child = component(&format!("    <p>{{{chain}}}</p>"));
            let markup = convert(&child, Some("children")).unwrap();
            let kinds: Vec<AssumptionKind> = markup.assumptions.iter().map(|a| a.kind).collect();
            assert_eq!(kinds, [AssumptionKind::ChildNotTrue]);
        })
        .unwrap();
    handle.join().unwrap();
}

// The first function in source order, found iteratively.
fn first_function(tree: &SyntaxTree<'_>) -> NodeId {
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        if matches!(
            tree.node(id).unwrap().kind(),
            SyntaxNodeKind::FunctionDeclaration { .. } | SyntaxNodeKind::ArrowFunction { .. }
        ) {
            return id;
        }
        let children: Vec<NodeId> = tree.child_nodes(id).unwrap().collect();
        stack.extend(children.into_iter().rev());
    }
    panic!("a function")
}

// The spans of every occurrence of `needle`, as a caller's route facts would
// name them.
fn spans_of(text: &str, needle: &str) -> Vec<Span> {
    text.match_indices(needle)
        .map(|(start, _)| Span::new(start, start + needle.len()).unwrap())
        .collect()
}

// The leading `params` of every `params.<property>` occurrence.
fn member_objects(text: &str, member: &str) -> Vec<Span> {
    spans_of(text, member)
        .iter()
        .map(|span| Span::new(span.start(), span.start() + "params".len()).unwrap())
        .collect()
}

fn convert_function(
    text: &SourceText,
    props: PropsSource,
    astro_params: &[Span],
) -> Result<String, ConvertErrorKind> {
    let tree = parse(text);
    let converted = convert_component(
        &tree,
        SourceId(0),
        first_function(&tree),
        &ComponentConversion {
            props,
            astro_params,
            ..ComponentConversion::default()
        },
        &mut Resolver,
    )
    .map_err(|error| error.kind)?;
    let emitted = print_astro_component(
        &converted.component,
        &[text],
        PrintOptions::default(),
        EmitLimits::default(),
    )
    .unwrap();
    emitted.verify(&[text]).unwrap();
    // Every comment of the function is copied.
    for comment in tree.comments() {
        assert!(
            converted.copied.iter().any(
                |span| span.start() <= comment.span.start() && comment.span.end() <= span.end()
            ),
            "{comment:?}"
        );
    }
    Ok(emitted.as_str().to_owned())
}

#[test]
fn async_pages_become_frontmatter_with_astro_params() {
    let text = source(concat!(
        "export default async function Post({ params }: { params: Promise<{ slug: string }> }) {\n",
        "  // Route parameters.\n",
        "  const { slug } = await params;\n",
        "  const post = posts.find((p) => p.slug === slug); /* by slug */\n",
        "  return (\n",
        "    <article>\n",
        "      <h1>{post.title}</h1>\n",
        "      <p>{params.slug}</p>\n",
        "    </article>\n",
        "  );\n",
        "}\n",
    ));
    let mut spans = spans_of(text.as_str(), "await params");
    spans.extend(member_objects(text.as_str(), "params.slug"));
    let output = convert_function(&text, PropsSource::Dropped, &spans).unwrap();
    assert_eq!(
        output,
        concat!(
            "---\n",
            "// Route parameters.\n",
            "const { slug } = Astro.params;\n",
            "const post = posts.find((p) => p.slug === slug);\n",
            "/* by slug */\n",
            "---\n",
            "<article>\n",
            "  <h1>\n",
            "    {post.title}\n",
            "  </h1>\n",
            "  <p>\n",
            "    {Astro.params.slug}\n",
            "  </p>\n",
            "</article>\n",
        )
    );
}

#[test]
fn component_props_and_statements_become_frontmatter() {
    let text = source(
        "const Card = ({ title, children }) => <section><h2>{title}</h2>{children}</section>;\n",
    );
    assert_eq!(
        convert_function(&text, PropsSource::AstroProps, &[]).unwrap(),
        concat!(
            "---\n",
            "const { title, children } = Astro.props;\n",
            "---\n",
            "<section>\n",
            "  <h2>\n",
            "    {title}\n",
            "  </h2>\n",
            "  <slot />\n",
            "</section>\n",
        )
    );
    let text = source(
        "function List({ items = [] }) {\n  let total = items.length;\n  total += 1;\n  return <p>{`${total}`}</p>;\n}\n",
    );
    assert_eq!(
        convert_function(&text, PropsSource::AstroProps, &[]).unwrap(),
        concat!(
            "---\n",
            "const { items = [] } = Astro.props;\n",
            "let total = items.length;\n",
            "total += 1;\n",
            "---\n",
            "<p>\n",
            "  {`${total}`}\n",
            "</p>\n",
        )
    );
}

#[test]
fn unsupported_component_bodies_fail_closed() {
    let cases = [
        (
            "async function P({ x }) { const a = await x; if (!a) return null; return <p>{a}</p>; }",
            ConvertErrorKind::UnsupportedBody,
        ),
        (
            "function P({ x }) { if (x) return <a />; return <p />; }",
            ConvertErrorKind::UnsupportedBody,
        ),
        (
            "function P({ x }) { return x ? <a /> : <p />; }",
            ConvertErrorKind::NotJsx,
        ),
        (
            "function P({ x }) { const a = <b />; return <p>{a}</p>; }",
            ConvertErrorKind::UnsupportedBody,
        ),
        (
            "function P({ x }) { for (const y of x) {} return <p />; }",
            ConvertErrorKind::UnsupportedBody,
        ),
        (
            "function P({ x }) { using y = x; return <p />; }",
            ConvertErrorKind::UnsupportedBody,
        ),
        (
            "function P() { const [v] = useState(0); return <p>{v}</p>; }",
            ConvertErrorKind::HookCall,
        ),
        (
            "function P() { return <p>{React.use(data)}</p>; }",
            ConvertErrorKind::HookCall,
        ),
        (
            "function P() { const v = this.value; return <p>{v}</p>; }",
            ConvertErrorKind::ContextDependent,
        ),
        (
            "function P() { const v = arguments[0]; return <p>{v}</p>; }",
            ConvertErrorKind::ContextDependent,
        ),
        (
            "function P() { const v = import.meta.url; return <p>{v}</p>; }",
            ConvertErrorKind::ContextDependent,
        ),
        (
            "function P() { const Astro = 1; return <p>{Astro}</p>; }",
            ConvertErrorKind::ReservedName,
        ),
        (
            "function P({ children }) { const c = children; return <p>{c}</p>; }",
            ConvertErrorKind::ChildrenOutsideSlot,
        ),
        (
            "function P() { const label = P.name; return <p>{label}</p>; }",
            ConvertErrorKind::DroppedBinding,
        ),
        (
            "function P({ x }, y) { return <p />; }",
            ConvertErrorKind::UnsupportedFunction,
        ),
        (
            "function* P() { return <p />; }",
            ConvertErrorKind::UnsupportedFunction,
        ),
        (
            "const P = (props) => <p />;",
            ConvertErrorKind::UnsupportedFunction,
        ),
        (
            "function P({ x }) { const a = x; return /* dropped */ <p>{a}</p>; }",
            ConvertErrorKind::CommentInMarkup,
        ),
        (
            "function P({ x }) { return <p>{x}</p>; /* dropped */ }",
            ConvertErrorKind::CommentInMarkup,
        ),
    ];
    for (code, expected) in cases {
        let text = source(code);
        assert_eq!(
            convert_function(&text, PropsSource::AstroProps, &[]),
            Err(expected),
            "{code}"
        );
    }
}

#[test]
fn dropped_parameters_require_every_use_replaced() {
    let code = "async function P({ params }) { const { slug } = await params; return <p>{params.slug}</p>; }";
    let text = source(code);
    let awaited = spans_of(code, "await params");
    // The member read keeps `params`, which the dropped parameter declared.
    assert_eq!(
        convert_function(&text, PropsSource::Dropped, &awaited),
        Err(ConvertErrorKind::DroppedBinding)
    );
    // A span outside the body, overlapping spans, and a span inside a
    // discarded `key` are rejected.
    let header = spans_of(code, "P(");
    assert_eq!(
        convert_function(&text, PropsSource::Dropped, &header),
        Err(ConvertErrorKind::UnsupportedSubstitution)
    );
    let code = "async function P({ params }) { const { slug } = await /* x */ params; return <p>{slug}</p>; }";
    assert_eq!(
        convert_function(
            &source(code),
            PropsSource::Dropped,
            &spans_of(code, "await /* x */ params")
        ),
        Err(ConvertErrorKind::CommentInMarkup)
    );
    // Dropping a pattern would skip its defaults and computed keys.
    for code in [
        "function P({ missing: unused = record() }) { return <p />; }",
        "function P({ unused = record() }) { return <p />; }",
        "function P({ [key()]: unused }) { return <p />; }",
        "function P({ missing: { unused } }) { return <p />; }",
        "function P({ missing: [unused] }) { return <p />; }",
    ] {
        assert_eq!(
            convert_function(&source(code), PropsSource::Dropped, &[]),
            Err(ConvertErrorKind::UnsupportedFunction),
            "{code}"
        );
    }
    let inner = Span::new(awaited[0].start() + 6, awaited[0].end()).unwrap();
    assert_eq!(
        convert_function(&text, PropsSource::Dropped, &[awaited[0], inner]),
        Err(ConvertErrorKind::UnsupportedSubstitution)
    );
    let code = "function P({ params }) { return <ul><li key={params.id}>x</li></ul>; }";
    let text = source(code);
    assert_eq!(
        convert_function(
            &text,
            PropsSource::Dropped,
            &member_objects(code, "params.id")
        ),
        Err(ConvertErrorKind::UnsupportedSubstitution)
    );
}

#[test]
fn frontmatter_statements_reject_markup_and_fences() {
    let text = source("a\n--- b");
    let statement = |pieces| AstroComponent {
        frontmatter: vec![FrontmatterItem::Statement(frameshift_engine::Expression {
            pieces,
            anchor: anchor(0, 7),
        })],
        template: Vec::new(),
    };
    let fenced = statement(vec![ExpressionPiece::Source(Span::new(0, 7).unwrap())]);
    assert!(matches!(
        print_astro_component(
            &fenced,
            &[&text],
            PrintOptions::default(),
            EmitLimits::default()
        ),
        Err(PrintError::FrontmatterFence(_))
    ));
    let markup = statement(vec![ExpressionPiece::Markup(TemplateNode::Text {
        value: "x".into(),
        anchor: anchor(0, 1),
    })]);
    assert!(matches!(
        print_astro_component(
            &markup,
            &[&text],
            PrintOptions::default(),
            EmitLimits::default()
        ),
        Err(PrintError::FrontmatterMarkup(_))
    ));
}
