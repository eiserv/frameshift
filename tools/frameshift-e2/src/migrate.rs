//! The hand-wired E2 migration of small Next.js App Router projects.
//!
//! This is experiment code, not the rule engine: it recognizes exactly the
//! constructs of the three E2 fixtures (a root layout with optional static
//! metadata, static pages, server components, `next/link`, global CSS, and
//! client components used as islands) and fails closed on everything else.
//! The engine's printer, JSX conversion, and span edits do the rewriting.

use frameshift_engine::{
    Anchor, Assumption, AstroComponent, AttributeItem, AttributeName, AttributeValue,
    ClientDirective, ComponentName, ComponentReference, ComponentStrategy, Edit, EditLimits,
    EditOrder, Element, ElementResolver, EmitLimits, EmittedText, Expression, ExpressionPiece,
    FrontmatterItem, FunctionKind, HtmlTagName, ImportItem, Indent, JsDialect, JsIdentifier,
    JsxConversion, LineEnding, NodeId, OutputBuilder, ParseLimits, PrintOptions, PropMember,
    PropType, RegionOrigin, SourceGoal, SourceId, SourceText, Span, SyntaxNodeKind, SyntaxTree,
    TagName, TemplateNode, TemplatePart, TypeScriptKind, VariableKind, apply_edits, convert_jsx,
    parse_js_program, print_astro_component,
};

/// The target profile whose versions the generated project pins.
pub const PROFILE: &str = "astro7-preact10";
const ASTRO_VERSION: &str = "7.3.8";
const ASTRO_PREACT_VERSION: &str = "6.0.6";
const PREACT_VERSION: &str = "10.29.8";
const PREACT_COMPAT_VERSION: &str = "18.3.2";

// Props of `next/link` that change navigation behavior and have no HTML equivalent.
const NEXT_LINK_PROPS: &[&str] = &[
    "as",
    "legacyBehavior",
    "locale",
    "onNavigate",
    "passHref",
    "prefetch",
    "replace",
    "scroll",
    "shallow",
];

/// A source file of the project, identified by its index in the sorted list.
pub struct SourceFile {
    /// Slash-separated path relative to the project root.
    pub path: String,
    /// File contents.
    pub text: SourceText,
}

/// One file of the generated project.
pub struct Output {
    /// Slash-separated path relative to the output root.
    pub path: String,
    /// Contents with provenance.
    pub emitted: EmittedText,
}

/// What happened to a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Converted or rewritten into the listed outputs.
    Transformed,
    /// Copied unchanged.
    Copied,
    /// Replaced by generated files from the target profile; E2 does not
    /// transform package manifests.
    Superseded,
    /// Not migrated by E2 and not part of the output.
    OutOfScope,
}

impl Disposition {
    /// Returns the identifier used in provenance listings.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Transformed => "transformed",
            Self::Copied => "copied",
            Self::Superseded => "superseded",
            Self::OutOfScope => "out-of-scope",
        }
    }
}

/// The accounting of one source file.
pub struct Account {
    /// Source path.
    pub source: String,
    /// Disposition.
    pub disposition: Disposition,
    /// Output paths that derive from the file, sorted.
    pub outputs: Vec<String>,
}

/// An assumption recorded for an output file.
pub struct RecordedAssumption {
    /// Output path.
    pub output: String,
    /// The assumption.
    pub assumption: Assumption,
}

/// The result of migrating one project.
pub struct Migration {
    /// Generated files, sorted by path.
    pub outputs: Vec<Output>,
    /// One account per source file, in source order.
    pub accounts: Vec<Account>,
    /// Assumptions, sorted by output path and source position.
    pub assumptions: Vec<RecordedAssumption>,
}

/// A construct that E2 does not migrate, or an inconsistency.
#[derive(Debug, PartialEq, Eq)]
pub struct MigrationError {
    /// Source path.
    pub path: String,
    /// What failed.
    pub message: String,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

type Result<T> = std::result::Result<T, MigrationError>;

fn fail<T>(path: &str, message: impl Into<String>) -> Result<T> {
    Err(MigrationError {
        path: path.to_owned(),
        message: message.into(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Role {
    RootLayout,
    Page { route: String },
    Component { name: String },
}

fn role(path: &str) -> Option<Role> {
    if path == "app/layout.tsx" {
        return Some(Role::RootLayout);
    }
    if path == "app/page.tsx" {
        return Some(Role::Page {
            route: String::new(),
        });
    }
    if let Some(route) = path
        .strip_prefix("app/")
        .and_then(|rest| rest.strip_suffix("/page.tsx"))
    {
        let static_route = route.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
        return static_route.then(|| Role::Page {
            route: route.to_owned(),
        });
    }
    let name = path.strip_prefix("components/")?.strip_suffix(".tsx")?;
    ComponentName::new(name)
        .filter(|_| !name.contains('.'))
        .map(|_| Role::Component {
            name: name.to_owned(),
        })
}

struct Module<'t> {
    id: SourceId,
    path: &'t str,
    role: Role,
    tree: SyntaxTree<'t>,
    island: bool,
}

/// Migrates `files`, which must be sorted by path.
pub fn migrate(files: &[SourceFile]) -> Result<Migration> {
    let sources: Vec<&SourceText> = files.iter().map(|file| &file.text).collect();
    let options = print_options(files);
    let mut modules = Vec::new();
    let mut accounts = Vec::new();
    let mut outputs = Vec::new();
    let mut package_json = None;
    for (index, file) in files.iter().enumerate() {
        let id = SourceId(u32::try_from(index).map_err(|_| MigrationError {
            path: file.path.clone(),
            message: "too many files".to_owned(),
        })?);
        let path = file.path.as_str();
        if path.ends_with(".tsx") {
            let Some(role) = role(path) else {
                return fail(path, "not a root layout, static page, or component of E2");
            };
            let tree = parse_js_program(
                &file.text,
                SourceGoal::Module,
                JsDialect::Tsx,
                ParseLimits::default(),
            )
            .map_err(|error| MigrationError {
                path: path.to_owned(),
                message: format!("syntax error at byte {}: {:?}", error.offset, error.kind),
            })?;
            let island = has_use_client(&tree)?;
            if island && !matches!(role, Role::Component { .. }) {
                return fail(path, "only components may be client components in E2");
            }
            modules.push(Module {
                id,
                path,
                role,
                tree,
                island,
            });
        } else if let Some(rest) = path.strip_prefix("app/")
            && path.ends_with(".css")
        {
            let output = format!("src/styles/{rest}");
            let mut builder = OutputBuilder::new(&sources, EmitLimits::default());
            builder
                .push_copied(Anchor {
                    source: id,
                    span: whole(&file.text),
                })
                .map_err(|error| MigrationError {
                    path: path.to_owned(),
                    message: error.to_string(),
                })?;
            outputs.push(Output {
                path: output.clone(),
                emitted: builder.finish(),
            });
            accounts.push(Account {
                source: path.to_owned(),
                disposition: Disposition::Copied,
                outputs: vec![output],
            });
        } else if path == "package.json" {
            package_json = Some(id);
        } else if path == "tsconfig.json" {
            accounts.push(Account {
                source: path.to_owned(),
                disposition: Disposition::OutOfScope,
                outputs: Vec::new(),
            });
        } else {
            return fail(path, "file kind not handled by E2");
        }
    }
    let Some(package_json) = package_json else {
        return fail("package.json", "missing package manifest");
    };
    let layout = modules
        .iter()
        .position(|module| module.role == Role::RootLayout)
        .ok_or_else(|| MigrationError {
            path: "app/layout.tsx".to_owned(),
            message: "missing root layout".to_owned(),
        })?;
    let project = Project {
        files,
        sources: &sources,
        modules: &modules,
        options,
    };
    let mut assumptions = Vec::new();
    let layout_facts = project.layout_facts(&modules[layout])?;
    for module in &modules {
        let (path, emitted, module_assumptions) = if module.island {
            project.island(module)?
        } else {
            project.component(module, &layout_facts)?
        };
        assumptions.extend(
            module_assumptions
                .into_iter()
                .map(|assumption| RecordedAssumption {
                    output: path.clone(),
                    assumption,
                }),
        );
        accounts.push(Account {
            source: module.path.to_owned(),
            disposition: Disposition::Transformed,
            outputs: vec![path.clone()],
        });
        outputs.push(Output { path, emitted });
    }
    let islands: Vec<&Module<'_>> = modules.iter().filter(|module| module.island).collect();
    let generated = project.generated_files(package_json, &islands)?;
    accounts.push(Account {
        source: "package.json".to_owned(),
        disposition: Disposition::Superseded,
        outputs: generated.iter().map(|output| output.path.clone()).collect(),
    });
    outputs.extend(generated);
    outputs.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    accounts.sort_by(|left, right| left.source.as_bytes().cmp(right.source.as_bytes()));
    for account in &mut accounts {
        account.outputs.sort();
    }
    assumptions.sort_by_key(|recorded| {
        (
            recorded.output.clone(),
            recorded.assumption.anchor.source,
            recorded.assumption.anchor.span.start(),
            recorded.assumption.anchor.span.end(),
        )
    });
    for output in &outputs {
        output
            .emitted
            .verify(&sources)
            .map_err(|error| MigrationError {
                path: output.path.clone(),
                message: format!("provenance: {error}"),
            })?;
    }
    check_comments(&modules, &outputs)?;
    Ok(Migration {
        outputs,
        accounts,
        assumptions,
    })
}

// Every comment of a converted module must be copied into some output.
fn check_comments(modules: &[Module<'_>], outputs: &[Output]) -> Result<()> {
    let mut copied: Vec<Anchor> = outputs
        .iter()
        .flat_map(|output| output.emitted.regions())
        .filter_map(|region| match region.origin {
            RegionOrigin::Copied(anchor) => Some(anchor),
            RegionOrigin::Generated(_) => None,
        })
        .collect();
    copied.sort();
    for module in modules {
        for comment in module.tree.comments() {
            let span = comment.span;
            let covered = copied.iter().any(|anchor| {
                anchor.source == module.id
                    && anchor.span.start() <= span.start()
                    && anchor.span.end() >= span.end()
            });
            if !covered {
                return fail(
                    module.path,
                    format!(
                        "comment at {}..{} would be dropped",
                        span.start(),
                        span.end()
                    ),
                );
            }
        }
    }
    Ok(())
}

fn whole(text: &SourceText) -> Span {
    Span::new(0, text.len()).unwrap_or_else(|_| unreachable!("whole source span"))
}

fn span(start: usize, end: usize) -> Result<Span> {
    Span::new(start, end).map_err(|error| MigrationError {
        path: String::new(),
        message: format!("invalid span: {error}"),
    })
}

// The project's dominant line ending and indentation (section 14.3 of the
// architecture), with LF and two spaces as fallbacks.
fn print_options(files: &[SourceFile]) -> PrintOptions {
    let (mut crlf, mut lf, mut tabs, mut spaces) = (0usize, 0usize, 0usize, Vec::new());
    for file in files.iter().filter(|file| file.path.ends_with(".tsx")) {
        let text = file.text.as_str();
        crlf += text.matches("\r\n").count();
        lf += text.matches('\n').count();
        for line in text.lines() {
            if line.starts_with('\t') {
                tabs += 1;
            } else {
                let width = line.len() - line.trim_start_matches(' ').len();
                if width > 0 && width < line.len() {
                    spaces.push(width);
                }
            }
        }
    }
    let line_ending = if crlf * 2 > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    let indent = if tabs > spaces.len() {
        Indent::Tab
    } else {
        let width = spaces.iter().copied().min().unwrap_or(2).clamp(1, 16);
        Indent::Spaces(u8::try_from(width).unwrap_or(2))
    };
    PrintOptions {
        indent,
        line_ending,
    }
}

fn has_use_client(tree: &SyntaxTree<'_>) -> Result<bool> {
    let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root())? else {
        return fail("", "tree root is not a program");
    };
    let Some(&first) = list(tree, body)?.first() else {
        return Ok(false);
    };
    if let SyntaxNodeKind::Directive { expression } = kind(tree, first)? {
        return Ok(tree.string_literal_value(expression).as_deref() == Some("use client"));
    }
    Ok(false)
}

fn kind(tree: &SyntaxTree<'_>, id: NodeId) -> Result<SyntaxNodeKind> {
    tree.node(id)
        .map(|node| node.kind())
        .ok_or_else(|| MigrationError {
            path: String::new(),
            message: "node outside the tree".to_owned(),
        })
}

fn node_span(tree: &SyntaxTree<'_>, id: NodeId) -> Result<Span> {
    tree.node(id)
        .map(|node| node.span())
        .ok_or_else(|| MigrationError {
            path: String::new(),
            message: "node outside the tree".to_owned(),
        })
}

fn node_text<'t>(tree: &SyntaxTree<'t>, id: NodeId) -> Result<&'t str> {
    let span = node_span(tree, id)?;
    tree.source().slice(span).map_err(|error| MigrationError {
        path: String::new(),
        message: error.to_string(),
    })
}

fn list(tree: &SyntaxTree<'_>, list: frameshift_engine::ChildList) -> Result<Vec<NodeId>> {
    tree.children(list)
        .map(<[NodeId]>::to_vec)
        .ok_or_else(|| MigrationError {
            path: String::new(),
            message: "child list outside the tree".to_owned(),
        })
}

fn string_value(tree: &SyntaxTree<'_>, path: &str, id: NodeId) -> Result<String> {
    tree.string_literal_value(id).ok_or_else(|| MigrationError {
        path: path.to_owned(),
        message: "expected a string literal".to_owned(),
    })
}

fn contains_jsx(tree: &SyntaxTree<'_>, id: NodeId) -> Result<bool> {
    if matches!(
        kind(tree, id)?,
        SyntaxNodeKind::JsxElement { .. } | SyntaxNodeKind::JsxFragment { .. }
    ) {
        return Ok(true);
    }
    for child in tree.child_nodes(id).into_iter().flatten() {
        if contains_jsx(tree, child)? {
            return Ok(true);
        }
    }
    Ok(false)
}

// The span from the first to the last comment between `start` and `end`.
fn comments_between(tree: &SyntaxTree<'_>, start: usize, end: usize) -> Option<Span> {
    let inside: Vec<Span> = tree
        .comments()
        .iter()
        .map(|comment| comment.span)
        .filter(|comment| comment.start() >= start && comment.end() <= end)
        .collect();
    let first = inside.first()?;
    let last = inside.last()?;
    Span::new(first.start(), last.end()).ok()
}

fn relative_path(from_directory: &str, to: &str) -> String {
    let from: Vec<&str> = from_directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let target: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&target[common..]);
    let joined = parts.join("/");
    if joined.starts_with("../") {
        joined
    } else {
        format!("./{joined}")
    }
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

fn normalize(directory: &str, specifier: &str) -> Option<String> {
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in specifier.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            "" => return None,
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[derive(Clone, Debug)]
enum Title {
    Plain(String, Span),
    Template {
        default: String,
        default_span: Span,
        template: String,
    },
}

#[derive(Clone, Debug, Default)]
struct Metadata {
    title: Option<Title>,
    description: Option<(String, Span)>,
}

struct LayoutFacts {
    name: String,
    output: String,
    metadata: Metadata,
    source: SourceId,
}

enum Import {
    Link {
        local: String,
    },
    Dropped,
    Css {
        output: String,
        anchor: Anchor,
    },
    Component {
        local: String,
        target: String,
        anchor: Anchor,
    },
}

struct Analysis {
    imports: Vec<Import>,
    copies: Vec<Span>,
    metadata: Option<Metadata>,
    function: Span,
    function_name: String,
    props: Vec<String>,
    returned: NodeId,
}

struct Project<'p, 't> {
    files: &'p [SourceFile],
    sources: &'p [&'p SourceText],
    modules: &'p [Module<'t>],
    options: PrintOptions,
}

impl Project<'_, '_> {
    fn module(&self, path: &str) -> Option<&Module<'_>> {
        self.modules.iter().find(|module| module.path == path)
    }

    fn output_path(&self, module: &Module<'_>) -> Result<String> {
        Ok(match &module.role {
            Role::RootLayout => {
                let analysis = self.analyze(module)?;
                format!("src/layouts/{}.astro", analysis.function_name)
            }
            Role::Page { route } if route.is_empty() => "src/pages/index.astro".to_owned(),
            Role::Page { route } => format!("src/pages/{route}.astro"),
            Role::Component { name } if module.island => format!("src/components/{name}.tsx"),
            Role::Component { name } => format!("src/components/{name}.astro"),
        })
    }

    // Resolves a relative import to a project file and its output path.
    fn resolve_import(&self, from: &str, specifier: &str) -> Result<(String, String)> {
        let Some(target) = normalize(directory(from), specifier) else {
            return fail(from, format!("import {specifier:?} leaves the project"));
        };
        if specifier.ends_with(".css") {
            let Some(rest) = target.strip_prefix("app/") else {
                return fail(from, format!("stylesheet {specifier:?} outside app/"));
            };
            if !self.files.iter().any(|file| file.path == target) {
                return fail(from, format!("missing stylesheet {specifier:?}"));
            }
            return Ok((target.clone(), format!("src/styles/{rest}")));
        }
        let module_path = format!("{target}.tsx");
        let Some(module) = self.module(&module_path) else {
            return fail(from, format!("unresolved import {specifier:?}"));
        };
        Ok((module_path, self.output_path(module)?))
    }

    // The specifier that the output file at `output` uses for `target_output`.
    fn specifier(output: &str, target_output: &str) -> String {
        let relative = relative_path(directory(output), target_output);
        match relative.strip_suffix(".tsx") {
            Some(without) => without.to_owned(),
            None => relative,
        }
    }

    fn analyze(&self, module: &Module<'_>) -> Result<Analysis> {
        let tree = &module.tree;
        let path = module.path;
        let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root())? else {
            return fail(path, "not a program");
        };
        let mut imports = Vec::new();
        let mut copies = Vec::new();
        let mut metadata = None;
        let mut function = None;
        let mut previous_end = 0;
        for item in list(tree, body)? {
            let item_span = node_span(tree, item)?;
            if let Some(comments) = comments_between(tree, previous_end, item_span.start()) {
                copies.push(comments);
            }
            previous_end = item_span.end();
            let item_kind = kind(tree, item)?;
            if let SyntaxNodeKind::ImportDeclaration { .. } = item_kind {
                imports.push(self.import(module, item)?);
            } else if let SyntaxNodeKind::TypeScript {
                kind: TypeScriptKind::TypeOnly,
                children,
            } = item_kind
            {
                let declaration = list(tree, children)?;
                let [declaration] = declaration.as_slice() else {
                    return fail(path, "unexpected type-only declaration");
                };
                let SyntaxNodeKind::ImportDeclaration { source, .. } = kind(tree, *declaration)?
                else {
                    return fail(path, "only type-only imports are dropped");
                };
                if string_value(tree, path, source)? != "next" {
                    return fail(path, "type-only imports are dropped only from `next`");
                }
                imports.push(Import::Dropped);
            } else if let SyntaxNodeKind::ExportDeclaration { declaration } = item_kind {
                if metadata.is_some() || matches!(module.role, Role::Component { .. }) {
                    return fail(path, "unexpected exported declaration");
                }
                metadata = Some(self.metadata(module, declaration)?);
            } else if let SyntaxNodeKind::ExportDefault { declaration } = item_kind {
                function = Some(declaration);
            } else if let SyntaxNodeKind::VariableDeclaration {
                kind: VariableKind::Const,
                ..
            } = item_kind
            {
                if contains_jsx(tree, item)? {
                    return fail(path, "module-level JSX is not migrated by E2");
                }
                copies.push(item_span);
            } else {
                return fail(
                    path,
                    format!("unsupported module item at byte {}", item_span.start()),
                );
            }
        }
        if let Some(comments) = comments_between(tree, previous_end, tree.source().len()) {
            copies.push(comments);
        }
        let Some(function) = function else {
            return fail(path, "missing default export");
        };
        let SyntaxNodeKind::FunctionDeclaration {
            name: Some(name),
            parameters,
            body,
            kind: function_kind,
        } = kind(tree, function)?
        else {
            return fail(
                path,
                "the default export must be a named function declaration",
            );
        };
        if !matches!(function_kind, FunctionKind::Normal | FunctionKind::Async) {
            return fail(path, "generator components are not migrated");
        }
        let function_name = node_text(tree, name)?.to_owned();
        let props = self.props(module, parameters)?;
        let SyntaxNodeKind::FunctionBody {
            body: statements, ..
        } = kind(tree, body)?
        else {
            return fail(path, "missing function body");
        };
        let statements = list(tree, statements)?;
        let Some((&last, leading)) = statements.split_last() else {
            return fail(path, "empty component body");
        };
        let body_span = node_span(tree, body)?;
        let mut previous_end = body_span.start() + 1;
        for &statement in leading {
            let statement_span = node_span(tree, statement)?;
            if let Some(comments) = comments_between(tree, previous_end, statement_span.start()) {
                copies.push(comments);
            }
            previous_end = statement_span.end();
            let SyntaxNodeKind::VariableDeclaration { .. } = kind(tree, statement)? else {
                return fail(path, "only declarations may precede the final return");
            };
            if contains_jsx(tree, statement)? {
                return fail(path, "JSX outside the returned markup is not migrated");
            }
            copies.push(statement_span);
        }
        let last_span = node_span(tree, last)?;
        if let Some(comments) = comments_between(tree, previous_end, last_span.start()) {
            copies.push(comments);
        }
        let SyntaxNodeKind::ReturnStatement {
            argument: Some(returned),
        } = kind(tree, last)?
        else {
            return fail(path, "the body must end with a return of JSX");
        };
        Ok(Analysis {
            imports,
            copies,
            metadata,
            function: node_span(tree, function)?,
            function_name,
            props,
            returned,
        })
    }

    fn import(&self, module: &Module<'_>, item: NodeId) -> Result<Import> {
        let tree = &module.tree;
        let path = module.path;
        let SyntaxNodeKind::ImportDeclaration {
            specifiers,
            source,
            attributes: None,
        } = kind(tree, item)?
        else {
            return fail(path, "import attributes are not migrated");
        };
        let specifier = string_value(tree, path, source)?;
        let specifiers = list(tree, specifiers)?;
        let default_local = |specifiers: &[NodeId]| -> Result<Option<String>> {
            let [only] = specifiers else {
                return Ok(None);
            };
            let SyntaxNodeKind::ImportDefaultSpecifier { local } = kind(tree, *only)? else {
                return Ok(None);
            };
            Ok(Some(node_text(tree, local)?.to_owned()))
        };
        let anchor = Anchor {
            source: module.id,
            span: node_span(tree, item)?,
        };
        if specifier == "next/link" {
            let Some(local) = default_local(&specifiers)? else {
                return fail(path, "`next/link` must be a default import");
            };
            return Ok(Import::Link { local });
        }
        if !specifier.starts_with("./") && !specifier.starts_with("../") {
            return fail(
                path,
                format!("package import {specifier:?} is not migrated by E2"),
            );
        }
        let (target, output) = self.resolve_import(path, &specifier)?;
        if specifier.ends_with(".css") {
            if !specifiers.is_empty() {
                return fail(path, "stylesheet imports must have no bindings");
            }
            return Ok(Import::Css { output, anchor });
        }
        let Some(local) = default_local(&specifiers)? else {
            return fail(path, "components must be default imports");
        };
        Ok(Import::Component {
            local,
            target,
            anchor,
        })
    }

    fn props(&self, module: &Module<'_>, parameters: NodeId) -> Result<Vec<String>> {
        let tree = &module.tree;
        let path = module.path;
        let SyntaxNodeKind::FormalParameters { parameters } = kind(tree, parameters)? else {
            return fail(path, "missing parameters");
        };
        let parameters = list(tree, parameters)?;
        let Some(&parameter) = parameters.first() else {
            return Ok(Vec::new());
        };
        if parameters.len() > 1 {
            return fail(path, "components take one props parameter");
        }
        let mut pattern = parameter;
        if let SyntaxNodeKind::TypeScript {
            kind:
                TypeScriptKind::Parameter {
                    optional: false,
                    rest: false,
                },
            children,
        } = kind(tree, parameter)?
        {
            let children = list(tree, children)?;
            let (binding, annotation) = match children.as_slice() {
                [binding] => (*binding, None),
                [binding, annotation] => (*binding, Some(*annotation)),
                _ => return fail(path, "unsupported parameter form"),
            };
            if let Some(annotation) = annotation
                && !matches!(
                    kind(tree, annotation)?,
                    SyntaxNodeKind::TypeScript {
                        kind: TypeScriptKind::Annotation,
                        ..
                    }
                )
            {
                return fail(path, "unsupported parameter form");
            }
            pattern = binding;
        }
        let SyntaxNodeKind::ObjectPattern { properties, .. } = kind(tree, pattern)? else {
            return fail(path, "props must be an object pattern");
        };
        let mut names = Vec::new();
        for property in list(tree, properties)? {
            let SyntaxNodeKind::ShorthandPatternProperty {
                target,
                default: None,
            } = kind(tree, property)?
            else {
                return fail(path, "props must be shorthand properties without defaults");
            };
            names.push(node_text(tree, target)?.to_owned());
        }
        Ok(names)
    }

    fn metadata(&self, module: &Module<'_>, declaration: NodeId) -> Result<Metadata> {
        let tree = &module.tree;
        let path = module.path;
        let SyntaxNodeKind::VariableDeclaration {
            kind: VariableKind::Const,
            declarators,
        } = kind(tree, declaration)?
        else {
            return fail(path, "only `export const metadata` is migrated");
        };
        let declarators = list(tree, declarators)?;
        let [declarator] = declarators.as_slice() else {
            return fail(path, "one metadata declarator expected");
        };
        let SyntaxNodeKind::VariableDeclarator {
            target,
            init: Some(init),
        } = kind(tree, *declarator)?
        else {
            return fail(path, "metadata needs an initializer");
        };
        let binding = if let SyntaxNodeKind::TypeScript {
            kind: TypeScriptKind::TypedBinding { .. },
            children,
        } = kind(tree, target)?
        {
            *list(tree, children)?
                .first()
                .ok_or_else(|| MigrationError {
                    path: path.to_owned(),
                    message: "empty typed binding".to_owned(),
                })?
        } else {
            target
        };
        if node_text(tree, binding)? != "metadata" {
            return fail(path, "only the `metadata` export is migrated");
        }
        let SyntaxNodeKind::ObjectExpression { properties, .. } = kind(tree, init)? else {
            return fail(path, "metadata must be an object literal");
        };
        let mut metadata = Metadata::default();
        for property in list(tree, properties)? {
            let SyntaxNodeKind::Property {
                key,
                value,
                computed: false,
            } = kind(tree, property)?
            else {
                return fail(path, "unsupported metadata property");
            };
            let value_span = node_span(tree, value)?;
            match node_text(tree, key)? {
                "title" if matches!(kind(tree, value)?, SyntaxNodeKind::StringLiteral) => {
                    metadata.title =
                        Some(Title::Plain(string_value(tree, path, value)?, value_span));
                }
                "title" if module.role == Role::RootLayout => {
                    metadata.title = Some(self.title_template(module, value)?);
                }
                "description" => {
                    metadata.description = Some((string_value(tree, path, value)?, value_span));
                }
                other => return fail(path, format!("metadata field {other:?} is not migrated")),
            }
        }
        Ok(metadata)
    }

    fn title_template(&self, module: &Module<'_>, value: NodeId) -> Result<Title> {
        let tree = &module.tree;
        let path = module.path;
        let SyntaxNodeKind::ObjectExpression { properties, .. } = kind(tree, value)? else {
            return fail(path, "unsupported title");
        };
        let (mut default, mut template) = (None, None);
        for property in list(tree, properties)? {
            let SyntaxNodeKind::Property {
                key,
                value,
                computed: false,
            } = kind(tree, property)?
            else {
                return fail(path, "unsupported title property");
            };
            let text = string_value(tree, path, value)?;
            match node_text(tree, key)? {
                "default" => default = Some((text, node_span(tree, value)?)),
                "template" => template = Some(text),
                other => return fail(path, format!("title field {other:?} is not migrated")),
            }
        }
        match (default, template) {
            (Some((default, default_span)), Some(template))
                if template.matches("%s").count() == 1 =>
            {
                Ok(Title::Template {
                    default,
                    default_span,
                    template,
                })
            }
            _ => fail(path, "a title template needs `default` and one `%s`"),
        }
    }

    fn layout_facts(&self, layout: &Module<'_>) -> Result<LayoutFacts> {
        let analysis = self.analyze(layout)?;
        Ok(LayoutFacts {
            name: analysis.function_name,
            output: self.output_path(layout)?,
            metadata: analysis.metadata.unwrap_or_default(),
            source: layout.id,
        })
    }

    // A client component: the file is kept with its directive removed and
    // relative imports rewritten through span edits.
    fn island(&self, module: &Module<'_>) -> Result<(String, EmittedText, Vec<Assumption>)> {
        let tree = &module.tree;
        let path = module.path;
        let output = self.output_path(module)?;
        let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root())? else {
            return fail(path, "not a program");
        };
        let items = list(tree, body)?;
        let directive = node_span(tree, items[0])?;
        let text = tree.source().as_str();
        // The directive and the blank lines after it go; a comment after it stays.
        let after = text[directive.end()..]
            .find(|ch: char| !matches!(ch, ' ' | '\t' | '\r' | '\n'))
            .map_or(text.len(), |offset| directive.end() + offset);
        let mut replacements = Vec::new();
        for &item in &items[1..] {
            if let SyntaxNodeKind::ImportDeclaration { source, .. } = kind(tree, item)? {
                let specifier = string_value(tree, path, source)?;
                if specifier.starts_with("./") || specifier.starts_with("../") {
                    let (target, target_output) = self.resolve_import(path, &specifier)?;
                    if self.module(&target).is_some_and(|target| !target.island)
                        || target_output.ends_with(".astro")
                    {
                        return fail(path, "a client component cannot import an Astro component");
                    }
                    let quote = node_text(tree, source)?.chars().next().unwrap_or('\'');
                    let new = Self::specifier(&output, &target_output);
                    if new != specifier {
                        replacements
                            .push((node_span(tree, source)?, format!("{quote}{new}{quote}")));
                    }
                }
            }
        }
        let mut edits = vec![Edit {
            span: span(directive.start(), after)?,
            replacement: "",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        }];
        for (index, (source_span, replacement)) in replacements.iter().enumerate() {
            edits.push(Edit {
                span: *source_span,
                replacement,
                order: EditOrder {
                    operation: 1,
                    edit: u32::try_from(index).unwrap_or(u32::MAX),
                },
            });
        }
        let rewritten =
            apply_edits(tree.source(), &edits, EditLimits::default()).map_err(|error| {
                MigrationError {
                    path: path.to_owned(),
                    message: error.to_string(),
                }
            })?;
        Ok((
            output,
            EmittedText::from_rewritten(module.id, &rewritten),
            Vec::new(),
        ))
    }

    // A layout, page, or server component converted to an Astro component.
    fn component(
        &self,
        module: &Module<'_>,
        layout: &LayoutFacts,
    ) -> Result<(String, EmittedText, Vec<Assumption>)> {
        let tree = &module.tree;
        let path = module.path;
        let output = self.output_path(module)?;
        let analysis = self.analyze(module)?;
        let anchor = |span: Span| Anchor {
            source: module.id,
            span,
        };
        let function_anchor = anchor(analysis.function);
        let mut frontmatter = Vec::new();
        let mut resolver = Resolver {
            link: None,
            components: Vec::new(),
        };
        for import in &analysis.imports {
            match import {
                Import::Link { local } => resolver.link = Some(local.clone()),
                Import::Dropped => {}
                Import::Css {
                    output: target_output,
                    anchor,
                } => {
                    frontmatter.push(FrontmatterItem::Import(ImportItem {
                        default: None,
                        named: Vec::new(),
                        specifier: Self::specifier(&output, target_output),
                        anchor: *anchor,
                    }));
                }
                Import::Component {
                    local,
                    target,
                    anchor,
                } => {
                    let Some(target_module) = self.module(target) else {
                        return fail(path, "unresolved component import");
                    };
                    let target_output = self.output_path(target_module)?;
                    let identifier = identifier(path, local)?;
                    let name = ComponentName::new(local).ok_or_else(|| MigrationError {
                        path: path.to_owned(),
                        message: format!("{local:?} is not a component name"),
                    })?;
                    resolver.components.push((
                        local.clone(),
                        name,
                        target_module.island.then_some(ClientDirective::Load),
                    ));
                    frontmatter.push(FrontmatterItem::Import(ImportItem {
                        default: Some(identifier),
                        named: Vec::new(),
                        specifier: Self::specifier(&output, &target_output),
                        anchor: *anchor,
                    }));
                }
            }
        }
        let is_page = matches!(module.role, Role::Page { .. });
        if is_page {
            frontmatter.push(FrontmatterItem::Import(ImportItem {
                default: Some(identifier(path, &layout.name)?),
                named: Vec::new(),
                specifier: Self::specifier(&output, &layout.output),
                anchor: function_anchor,
            }));
        }
        let is_layout = module.role == Role::RootLayout;
        let children_binding = analysis.props.iter().any(|prop| prop == "children");
        if children_binding && !is_layout {
            return fail(path, "only the root layout renders children in E2");
        }
        let mut props: Vec<String> = analysis
            .props
            .iter()
            .filter(|prop| prop.as_str() != "children")
            .cloned()
            .collect();
        if is_page && !props.is_empty() {
            return fail(path, "page props are not migrated by E2");
        }
        if is_layout {
            if analysis
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.title.is_some())
            {
                props.push("title".to_owned());
            }
            if analysis
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.description.is_some())
            {
                props.push("description".to_owned());
            }
            let members = props
                .iter()
                .map(|prop| {
                    Ok(PropMember {
                        name: identifier(path, prop)?,
                        ty: PropType::String,
                        optional: false,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if !members.is_empty() {
                frontmatter.push(FrontmatterItem::PropsInterface {
                    members,
                    anchor: function_anchor,
                });
            }
        }
        if !props.is_empty() {
            frontmatter.push(FrontmatterItem::PropsBinding {
                names: props
                    .iter()
                    .map(|prop| identifier(path, prop))
                    .collect::<Result<Vec<_>>>()?,
                anchor: function_anchor,
            });
        }
        frontmatter.extend(
            analysis
                .copies
                .iter()
                .map(|span| FrontmatterItem::Copied(anchor(*span))),
        );
        check_links(module, resolver.link.as_deref(), analysis.returned)?;
        let conversion = JsxConversion {
            children_binding: children_binding.then_some("children"),
            ..JsxConversion::default()
        };
        let markup = convert_jsx(
            tree,
            module.id,
            analysis.returned,
            &conversion,
            &mut resolver,
        )
        .map_err(|error| MigrationError {
            path: path.to_owned(),
            message: format!("JSX conversion: {error}"),
        })?;
        let mut template = markup.nodes;
        if is_layout {
            template = self.layout_template(module, template, &analysis, function_anchor)?;
        }
        if let Role::Page { .. } = module.role {
            template = self.page_template(module, template, &analysis, layout, function_anchor)?;
        }
        let component = AstroComponent {
            frontmatter,
            template,
        };
        let emitted = print_astro_component(
            &component,
            self.sources,
            self.options,
            EmitLimits::default(),
        )
        .map_err(|error| MigrationError {
            path: path.to_owned(),
            message: format!("printing: {error}"),
        })?;
        Ok((output, emitted, markup.assumptions))
    }

    // Inserts the document head that Next.js renders for the root layout:
    // the charset and viewport tags it adds by default, then the metadata.
    fn layout_template(
        &self,
        module: &Module<'_>,
        mut template: Vec<TemplateNode>,
        analysis: &Analysis,
        function_anchor: Anchor,
    ) -> Result<Vec<TemplateNode>> {
        let path = module.path;
        let [TemplateNode::Element(html)] = template.as_mut_slice() else {
            return fail(path, "the root layout must render one `html` element");
        };
        if html.name != TagName::Html(tag("html")) {
            return fail(path, "the root layout must render one `html` element");
        }
        if html.children.iter().any(|child| {
            matches!(child, TemplateNode::Element(element) if element.name == TagName::Html(tag("head")))
        }) {
            return fail(path, "a custom `head` is not migrated by E2");
        }
        let meta = |attributes: Vec<(&str, AttributeValue)>, anchor: Anchor| {
            TemplateNode::Element(Element {
                name: TagName::Html(tag("meta")),
                attributes: attributes
                    .into_iter()
                    .map(|(name, value)| AttributeItem::Named {
                        name: attribute(name),
                        value,
                        anchor,
                    })
                    .collect(),
                children: Vec::new(),
                anchor,
            })
        };
        let text = |value: &str| AttributeValue::Text(value.to_owned());
        let mut head = vec![
            meta(vec![("charset", text("utf-8"))], function_anchor),
            meta(
                vec![
                    ("name", text("viewport")),
                    ("content", text("width=device-width, initial-scale=1")),
                ],
                function_anchor,
            ),
        ];
        let metadata = analysis.metadata.clone().unwrap_or_default();
        let module_anchor = |span: Span| Anchor {
            source: module.id,
            span,
        };
        if let Some(title) = &metadata.title {
            let title_span = match title {
                Title::Plain(_, span) => *span,
                Title::Template { default_span, .. } => *default_span,
            };
            let anchor = module_anchor(title_span);
            head.push(TemplateNode::Element(Element {
                name: TagName::Html(tag("title")),
                attributes: Vec::new(),
                children: vec![TemplateNode::Expression(Expression {
                    pieces: vec![ExpressionPiece::Identifier(identifier(path, "title")?)],
                    anchor,
                })],
                anchor,
            }));
        }
        if let Some((_, description_span)) = &metadata.description {
            let anchor = module_anchor(*description_span);
            head.push(meta(
                vec![
                    ("name", text("description")),
                    (
                        "content",
                        AttributeValue::Expression(Expression {
                            pieces: vec![ExpressionPiece::Identifier(identifier(
                                path,
                                "description",
                            )?)],
                            anchor,
                        }),
                    ),
                ],
                anchor,
            ));
        }
        html.children.insert(
            0,
            TemplateNode::Element(Element {
                name: TagName::Html(tag("head")),
                attributes: Vec::new(),
                children: head,
                anchor: function_anchor,
            }),
        );
        Ok(template)
    }

    // Wraps a page in the root layout with the metadata that Next.js would
    // resolve for it: the page title through the layout's template, and the
    // nearest description.
    fn page_template(
        &self,
        module: &Module<'_>,
        template: Vec<TemplateNode>,
        analysis: &Analysis,
        layout: &LayoutFacts,
        function_anchor: Anchor,
    ) -> Result<Vec<TemplateNode>> {
        let path = module.path;
        let page = analysis.metadata.clone().unwrap_or_default();
        let mut attributes = Vec::new();
        let page_anchor = |span: Span| Anchor {
            source: module.id,
            span,
        };
        let layout_anchor = |span: Span| Anchor {
            source: layout.source,
            span,
        };
        let title = match (&page.title, &layout.metadata.title) {
            (Some(Title::Plain(title, span)), Some(Title::Template { template, .. })) => {
                Some((template.replacen("%s", title, 1), page_anchor(*span)))
            }
            (Some(Title::Plain(title, span)), Some(Title::Plain(..))) => {
                Some((title.clone(), page_anchor(*span)))
            }
            (
                None,
                Some(Title::Template {
                    default,
                    default_span,
                    ..
                }),
            ) => Some((default.clone(), layout_anchor(*default_span))),
            (None, Some(Title::Plain(title, span))) => Some((title.clone(), layout_anchor(*span))),
            (None, None) => None,
            (Some(_), None) | (Some(Title::Template { .. }), Some(_)) => {
                return fail(path, "the root layout must define a title for page titles");
            }
        };
        if let Some((title, anchor)) = title {
            attributes.push(AttributeItem::Named {
                name: attribute("title"),
                value: AttributeValue::Text(title),
                anchor,
            });
        }
        let description = match (&page.description, &layout.metadata.description) {
            (Some((text, span)), Some(_)) => Some((text.clone(), page_anchor(*span))),
            (None, Some((text, span))) => Some((text.clone(), layout_anchor(*span))),
            (None, None) => None,
            (Some(_), None) => {
                return fail(path, "the root layout must define a description for pages");
            }
        };
        if let Some((description, anchor)) = description {
            attributes.push(AttributeItem::Named {
                name: attribute("description"),
                value: AttributeValue::Text(description),
                anchor,
            });
        }
        let name = ComponentName::new(&layout.name).ok_or_else(|| MigrationError {
            path: path.to_owned(),
            message: "the layout name is not a component name".to_owned(),
        })?;
        Ok(vec![TemplateNode::Element(Element {
            name: TagName::Component(name),
            attributes,
            children: template,
            anchor: function_anchor,
        })])
    }

    // `package.json` and `astro.config.mjs` from the target profile.
    fn generated_files(
        &self,
        package_json: SourceId,
        islands: &[&Module<'_>],
    ) -> Result<Vec<Output>> {
        let eol = match self.options.line_ending {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        };
        let manifest = Anchor {
            source: package_json,
            span: whole(self.sources[package_json.0 as usize]),
        };
        let mut dependencies = Vec::new();
        if !islands.is_empty() {
            dependencies.push(("@astrojs/preact", ASTRO_PREACT_VERSION.to_owned()));
        }
        dependencies.push(("astro", ASTRO_VERSION.to_owned()));
        if !islands.is_empty() {
            dependencies.push(("preact", PREACT_VERSION.to_owned()));
            let compat = format!("npm:@preact/compat@{PREACT_COMPAT_VERSION}");
            dependencies.push(("react", compat.clone()));
            dependencies.push(("react-dom", compat));
        }
        let dependency_lines: Vec<String> = dependencies
            .iter()
            .map(|(name, version)| format!("    \"{name}\": \"{version}\""))
            .collect();
        let package = [
            "{".to_owned(),
            "  \"private\": true,".to_owned(),
            "  \"type\": \"module\",".to_owned(),
            "  \"scripts\": {".to_owned(),
            "    \"dev\": \"astro dev\",".to_owned(),
            "    \"build\": \"astro build\",".to_owned(),
            "    \"preview\": \"astro preview\"".to_owned(),
            "  },".to_owned(),
            "  \"dependencies\": {".to_owned(),
            dependency_lines.join(&format!(",{eol}")),
            "  }".to_owned(),
            "}".to_owned(),
            String::new(),
        ]
        .join(eol);
        let mut package_builder = OutputBuilder::new(self.sources, EmitLimits::default());
        push(&mut package_builder, &package, manifest)?;
        let mut config = OutputBuilder::new(self.sources, EmitLimits::default());
        push(
            &mut config,
            &format!("import {{ defineConfig }} from 'astro/config';{eol}"),
            manifest,
        )?;
        if let Some(first) = islands.first() {
            let directive = Anchor {
                source: first.id,
                span: node_span(
                    &first.tree,
                    list(&first.tree, program_body(&first.tree)?)?[0],
                )?,
            };
            push(
                &mut config,
                &format!("import preact from '@astrojs/preact';{eol}"),
                directive,
            )?;
            push(
                &mut config,
                &format!("{eol}export default defineConfig({{{eol}"),
                manifest,
            )?;
            push(
                &mut config,
                &format!("  integrations: [preact({{ compat: true }})],{eol}"),
                directive,
            )?;
            push(&mut config, &format!("}});{eol}"), manifest)?;
        } else {
            push(
                &mut config,
                &format!("{eol}export default defineConfig({{}});{eol}"),
                manifest,
            )?;
        }
        Ok(vec![
            Output {
                path: "astro.config.mjs".to_owned(),
                emitted: config.finish(),
            },
            Output {
                path: "package.json".to_owned(),
                emitted: package_builder.finish(),
            },
        ])
    }
}

fn program_body(tree: &SyntaxTree<'_>) -> Result<frameshift_engine::ChildList> {
    let SyntaxNodeKind::Program { body, .. } = kind(tree, tree.root())? else {
        return fail("", "not a program");
    };
    Ok(body)
}

fn push(builder: &mut OutputBuilder<'_>, text: &str, anchor: Anchor) -> Result<()> {
    builder
        .push_generated(text, anchor)
        .map_err(|error| MigrationError {
            path: String::new(),
            message: error.to_string(),
        })
}

fn identifier(path: &str, name: &str) -> Result<JsIdentifier> {
    JsIdentifier::new(name).ok_or_else(|| MigrationError {
        path: path.to_owned(),
        message: format!("{name:?} is not a plain identifier"),
    })
}

fn tag(name: &str) -> HtmlTagName {
    HtmlTagName::new(name).unwrap_or_else(|| unreachable!("constant tag name {name}"))
}

fn attribute(name: &str) -> AttributeName {
    AttributeName::new(name).unwrap_or_else(|| unreachable!("constant attribute name {name}"))
}

// Next.js-specific link props and spreads that could carry them are rejected,
// and `href` must be a string, because `next/link` formats URL objects itself.
fn check_links(module: &Module<'_>, link: Option<&str>, root: NodeId) -> Result<()> {
    let Some(link) = link else {
        return Ok(());
    };
    let tree = &module.tree;
    let path = module.path;
    let node_kind = kind(tree, root)?;
    if let SyntaxNodeKind::JsxOpeningElement {
        name, attributes, ..
    } = node_kind
        && node_text(tree, name)? == link
    {
        let mut has_href = false;
        for attribute in list(tree, attributes)? {
            let SyntaxNodeKind::JsxAttribute { name, value } = kind(tree, attribute)? else {
                return fail(path, "spread props on `Link` are not migrated");
            };
            let name = node_text(tree, name)?;
            if NEXT_LINK_PROPS.contains(&name) {
                return fail(path, format!("`Link` prop {name:?} is not migrated"));
            }
            if name == "href" {
                has_href = true;
                let string = value.is_some_and(|value| {
                    matches!(kind(tree, value), Ok(SyntaxNodeKind::JsxStringLiteral))
                }) || value
                    .map(|value| static_string_container(tree, value))
                    .transpose()?
                    .unwrap_or(false);
                if !string {
                    return fail(path, "`Link` href must be a string");
                }
            }
        }
        if !has_href {
            return fail(path, "`Link` without href");
        }
    }
    for child in tree.child_nodes(root).into_iter().flatten() {
        check_links(module, Some(link), child)?;
    }
    Ok(())
}

fn static_string_container(tree: &SyntaxTree<'_>, value: NodeId) -> Result<bool> {
    let SyntaxNodeKind::JsxExpressionContainer {
        expression: Some(expression),
    } = kind(tree, value)?
    else {
        return Ok(false);
    };
    let expression_kind = kind(tree, expression)?;
    if let SyntaxNodeKind::TemplateLiteral { parts } = expression_kind {
        let parts = tree.template_parts(parts).unwrap_or_default();
        return Ok(parts
            .iter()
            .all(|part| matches!(part, TemplatePart::Segment(_))));
    }
    Ok(matches!(expression_kind, SyntaxNodeKind::StringLiteral))
}

struct Resolver {
    link: Option<String>,
    components: Vec<(String, ComponentName, Option<ClientDirective>)>,
}

impl ElementResolver for Resolver {
    fn resolve(&mut self, reference: ComponentReference<'_>) -> Option<ComponentStrategy> {
        if self.link.as_deref() == Some(reference.spelling) {
            return HtmlTagName::new("a").map(ComponentStrategy::Html);
        }
        self.components
            .iter()
            .find(|(local, ..)| local == reference.spelling)
            .map(|(_, name, directive)| ComponentStrategy::Component {
                name: name.clone(),
                directive: *directive,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_and_specifiers() {
        assert_eq!(
            relative_path("src/pages", "src/layouts/RootLayout.astro"),
            "../layouts/RootLayout.astro"
        );
        assert_eq!(
            relative_path("src/pages/blog", "src/styles/a.css"),
            "../../styles/a.css"
        );
        assert_eq!(
            relative_path("src/components", "src/components/A.astro"),
            "./A.astro"
        );
        assert_eq!(
            Project::specifier("src/pages/index.astro", "src/components/Counter.tsx"),
            "../components/Counter"
        );
        assert_eq!(
            normalize("app", "../components/Counter").as_deref(),
            Some("components/Counter")
        );
        assert_eq!(normalize("app", "../../x"), None);
        assert_eq!(normalize("app", ".//x"), None);
    }

    use frameshift_engine::TextLimits;

    const LAYOUT: &str = "export default function RootLayout({ children }) {
  return (
    <html lang=\"en\">
      <body>{children}</body>
    </html>
  );
}
";

    fn project(files: &[(&str, &str)]) -> Vec<SourceFile> {
        let mut files: Vec<SourceFile> = files
            .iter()
            .map(|(path, text)| SourceFile {
                path: (*path).to_owned(),
                text: SourceText::new((*text).to_owned(), TextLimits::default()).unwrap(),
            })
            .collect();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }

    fn error(page: &str) -> String {
        let files = project(&[
            ("package.json", "{}"),
            ("app/layout.tsx", LAYOUT),
            ("app/page.tsx", page),
        ]);
        match migrate(&files) {
            Ok(_) => panic!("expected the page to fail: {page}"),
            Err(error) => error.message,
        }
    }

    #[test]
    fn minimal_project_migrates() {
        let files = project(&[
            ("package.json", "{}"),
            ("app/layout.tsx", LAYOUT),
            (
                "app/page.tsx",
                "export default function Home() {
  return <p>Hi</p>;
}
",
            ),
        ]);
        let migration = migrate(&files).unwrap();
        let paths: Vec<&str> = migration
            .outputs
            .iter()
            .map(|output| output.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "astro.config.mjs",
                "package.json",
                "src/layouts/RootLayout.astro",
                "src/pages/index.astro"
            ]
        );
    }

    #[test]
    fn unsupported_constructs_fail_closed() {
        let link = "import Link from 'next/link';
";
        for (page, expected) in [
            (
                format!(
                    "{link}export default function Home() {{
  return <Link href=\"/\" prefetch={{false}}>x</Link>;
}}
"
                ),
                "prop \"prefetch\"",
            ),
            (
                format!(
                    "{link}export default function Home() {{
  return <Link href={{{{ pathname: '/' }}}}>x</Link>;
}}
"
                ),
                "href must be a string",
            ),
            (
                "export default function Home() {
  return (
    // gone
    <p>x</p>
  );
}
"
                .to_owned(),
                "would be dropped",
            ),
            (
                "export default function Home() {
  if (Math.random()) return null;
  return <p>x</p>;
}
"
                .to_owned(),
                "only declarations",
            ),
            (
                "import { useState } from 'react';
export default function Home() {
  return <p>x</p>;
}
"
                .to_owned(),
                "package import",
            ),
            (
                "export default function Home() {
  return <p onClick={() => 1}>x</p>;
}
"
                .to_owned(),
                "unsupported-attribute",
            ),
            (
                "export const metadata = { title: 'A', robots: 'none' };
export default function Home() {
  return <p>x</p>;
}
"
                .to_owned(),
                "metadata field",
            ),
        ] {
            let message = error(&page);
            assert!(message.contains(expected), "{message} for {page}");
        }
        let files = project(&[
            ("package.json", "{}"),
            ("app/layout.tsx", LAYOUT),
            ("app/[slug]/page.tsx", ""),
        ]);
        assert!(migrate(&files).is_err());
        let files = project(&[
            ("package.json", "{}"),
            ("app/layout.tsx", LAYOUT),
            ("lib/data.ts", ""),
        ]);
        assert!(migrate(&files).is_err());
    }

    #[test]
    fn roles_cover_only_the_e2_file_kinds() {
        assert_eq!(role("app/layout.tsx"), Some(Role::RootLayout));
        assert_eq!(
            role("app/page.tsx"),
            Some(Role::Page {
                route: String::new()
            })
        );
        assert_eq!(
            role("app/docs/getting-started/page.tsx"),
            Some(Role::Page {
                route: "docs/getting-started".to_owned()
            })
        );
        assert_eq!(
            role("components/SiteHeader.tsx"),
            Some(Role::Component {
                name: "SiteHeader".to_owned()
            })
        );
        for other in [
            "app/[slug]/page.tsx",
            "app/(group)/page.tsx",
            "app/blog/layout.tsx",
            "components/header.tsx",
            "lib/util.tsx",
        ] {
            assert_eq!(role(other), None, "{other}");
        }
    }
}
