//! Migrate v0 (`D-101`): planning and safe execution of the first, reduced
//! Next.js App Router to Astro + Preact migration.
//!
//! The plan is a pure function of the analysis and the snapshot contents it
//! was computed from. Only units with status `auto` or `review` and the
//! `astro` or `preact-static` strategy are converted; every other file is
//! preserved verbatim below `.frameshift/unmigrated/` with a manual task.
//! Preservation propagates to importers until a fixed point, so the output
//! never references a file that was not written. Execution writes only
//! through `frameshift-output`, into a new directory outside the source.

mod convert;
mod scaffold;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use frameshift_catalog::Status;
use frameshift_core::{OutputPath, SourcePath, SourceText, UnitKind};
use frameshift_emit::{Indent, LineEnding, PrintOptions};
use frameshift_input::{EntryKind, FileRead, Snapshot};
use frameshift_nextjs::{AppRoute, Convention, Metadata, RouteKind, SegmentKind};

use crate::analysis::{self, AnalyzeErrorKind, AnalyzeOptions, FileClass, Strategy, Unit};
use crate::limits::Limits;
use crate::project::FileState;
use convert::{Context, Role as AstroRole, RootFacts, Target, TargetKind};

pub use frameshift_output::Destination;

/// The migration rules of this implementation.
pub const RULESET: &str = "nextjs-to-astro-preact/migrate-v0";

/// Directory of preserved, unmigrated source files in the output.
pub const UNMIGRATED: &str = ".frameshift/unmigrated";

/// Path of the human-readable summary in the output.
pub const SUMMARY: &str = ".frameshift/SUMMARY.md";

/// Explicit inputs to one migration plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MigrateOptions {
    /// One selected workspace member; `None` selects the root.
    pub package: Option<SourcePath>,
    /// All source, assessment, and emission resource bounds.
    pub limits: Limits,
}

/// What an operation does to produce its output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationKind {
    /// A new Astro file generated from a source file.
    Create,
    /// A source file kept as JavaScript or TypeScript with span edits.
    Transform,
    /// A file copied verbatim.
    Copy,
    /// A generated project file.
    Config,
    /// An unmigrated source file copied below `.frameshift/unmigrated/`.
    Preserve,
}

impl OperationKind {
    /// Stable lowercase spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Transform => "transform",
            Self::Copy => "copy",
            Self::Config => "config",
            Self::Preserve => "preserve",
        }
    }
}

/// One output file and its origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    /// What produces the file.
    pub kind: OperationKind,
    /// Portable path relative to the output root.
    pub output: OutputPath,
    /// The rule that produced the file.
    pub rule: &'static str,
    /// The source file, absent for generated project files.
    pub source: Option<SourcePath>,
    /// Whether the analysis rated a converted unit `review`.
    pub review: bool,
    bytes: Vec<u8>,
}

impl Operation {
    /// The exact bytes that execution writes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The category of a manual task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskKind {
    /// Project-wide work, such as reinstalling dependencies.
    Project,
    /// A preserved file that needs manual migration.
    Preserved,
    /// A converted file whose analysis status is `review`.
    Review,
    /// A source file that is not part of the output.
    NotCopied,
}

/// Work that remains for a person after the migration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualTask {
    /// The category.
    pub kind: TaskKind,
    /// The source file concerned, if any.
    pub source: Option<SourcePath>,
    /// Static English text; it never contains source content.
    pub text: String,
}

/// The complete plan: every output file with its bytes, and the remaining work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Established Next.js version, if known.
    pub source_version: Option<String>,
    /// Operations sorted by output path bytes.
    pub operations: Vec<Operation>,
    /// Manual tasks sorted by kind and source path.
    pub tasks: Vec<ManualTask>,
}

/// Failure category of planning or execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateErrorKind {
    /// The source cannot be analyzed.
    Input,
    /// An explicit resource bound prevents a complete result.
    Limit,
    /// The output location is unsafe or occupied, or writing failed.
    Output,
    /// A violated engine invariant.
    Internal,
}

/// A failure without partial output. Messages never contain paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrateError {
    kind: MigrateErrorKind,
    message: String,
}

impl MigrateError {
    /// The failure category.
    pub const fn kind(&self) -> MigrateErrorKind {
        self.kind
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: MigrateErrorKind::Internal,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MigrateError {}

impl From<frameshift_output::OutputError> for MigrateError {
    fn from(error: frameshift_output::OutputError) -> Self {
        Self {
            kind: MigrateErrorKind::Output,
            message: error.to_string(),
        }
    }
}

/// Checks the output location without creating anything.
pub fn check_output(source: &Path, output: &Path) -> Result<Destination, MigrateError> {
    Ok(frameshift_output::prepare(source, output)?)
}

/// Writes every operation of `plan` into the checked destination, through
/// a staging directory and an atomic rename.
pub fn execute(plan: &Plan, destination: &Destination) -> Result<(), MigrateError> {
    let files: Vec<_> = plan
        .operations
        .iter()
        .map(|operation| frameshift_output::OutputFile {
            path: operation.output.clone(),
            bytes: operation.bytes.clone(),
        })
        .collect();
    Ok(frameshift_output::write(destination, &files)?)
}

/// How a planned file is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Role {
    Page {
        route: usize,
    },
    Layout {
        name: String,
        parent: Option<SourcePath>,
    },
    Component,
    Script,
    Stylesheet,
    Verbatim,
}

struct Rendered {
    bytes: Vec<u8>,
    assumptions: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    role: Role,
    output: String,
    rule: &'static str,
    kind: OperationKind,
    review: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Fate {
    Convert(Candidate),
    Preserve(String),
    NotCopied(String),
    Gitignore,
}

/// Computes the migration plan for the package at `root` without writing.
pub fn plan(root: &Path, options: &MigrateOptions) -> Result<Plan, MigrateError> {
    let parts = analysis::analyze_parts(
        root,
        &AnalyzeOptions {
            package: options.package.clone(),
            limits: options.limits,
        },
    )
    .map_err(|error| MigrateError {
        kind: match error.kind() {
            AnalyzeErrorKind::Input => MigrateErrorKind::Input,
            AnalyzeErrorKind::Limit => MigrateErrorKind::Limit,
            AnalyzeErrorKind::Internal => MigrateErrorKind::Internal,
        },
        message: error.to_string(),
    })?;
    Planner::new(&parts, &options.limits).plan()
}

struct Planner<'a> {
    parts: &'a analysis::Parts,
    limits: &'a Limits,
    prefix: String,
    app: String,
    units: BTreeMap<&'a SourcePath, Vec<&'a Unit>>,
    classes: BTreeMap<&'a SourcePath, FileClass>,
    pages: BTreeMap<&'a SourcePath, usize>,
    fates: BTreeMap<SourcePath, Fate>,
    texts: BTreeMap<SourcePath, SourceText>,
}

impl<'a> Planner<'a> {
    fn new(parts: &'a analysis::Parts, limits: &'a Limits) -> Self {
        let prefix = parts
            .project
            .package()
            .map_or(String::new(), |p| format!("{}/", p.as_str()));
        let app = parts.project.nextjs().routes.app.as_str();
        let app = app.strip_prefix(&prefix).unwrap_or(app).to_owned();
        let mut units: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for unit in &parts.analysis.assessment.units {
            if let Some(path) = &unit.path {
                units.entry(path).or_default().push(unit);
            }
        }
        let classes = parts
            .analysis
            .assessment
            .files
            .iter()
            .map(|file| (&file.path, file.class))
            .collect();
        let pages = parts
            .project
            .nextjs()
            .routes
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.kind == RouteKind::Page)
            .map(|(index, route)| (&route.file, index))
            .collect();
        Self {
            parts,
            limits,
            prefix,
            app,
            units,
            classes,
            pages,
            fates: BTreeMap::new(),
            texts: BTreeMap::new(),
        }
    }

    fn snapshot(&self) -> &Snapshot {
        &self.parts.snapshot
    }

    fn local<'p>(&self, path: &'p SourcePath) -> &'p str {
        path.as_str()
            .strip_prefix(&self.prefix)
            .unwrap_or(path.as_str())
    }

    fn bytes(&self, path: &SourcePath) -> Result<Option<&[u8]>, MigrateError> {
        let Some(id) = self.snapshot().inventory().files().find(path) else {
            return Ok(None);
        };
        match self.snapshot().read(id) {
            Ok(FileRead::Content(file)) => Ok(Some(file.bytes())),
            Ok(FileRead::Unreadable(_)) => Ok(None),
            Err(error) => Err(MigrateError {
                kind: MigrateErrorKind::Input,
                message: error.to_string(),
            }),
        }
    }

    fn plan(mut self) -> Result<Plan, MigrateError> {
        let files: Vec<_> = self.parts.project.files().to_vec();
        for file in &files {
            let fate = match &file.state {
                FileState::Other(EntryKind::Directory) => continue,
                FileState::Other(_) => Fate::NotCopied("excluded by discovery".into()),
                FileState::Admitted => self.classify(&file.path, file.convention),
            };
            self.fates.insert(file.path.clone(), fate);
        }
        self.load_texts()?;
        self.name_components();
        self.check_collisions();
        let rendered = self.converge()?;
        self.finish(rendered)
    }

    fn classify(&self, path: &SourcePath, convention: Option<Convention>) -> Fate {
        let local = self.local(path);
        let name = local.rsplit('/').next().unwrap_or(local);
        let root_file = !local.contains('/');
        if root_file {
            match name {
                "package.json" => {
                    return Fate::NotCopied("replaced by the generated package.json".into());
                }
                "package-lock.json"
                | "npm-shrinkwrap.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "bun.lock"
                | "bun.lockb" => {
                    return Fate::NotCopied("lockfile; reinstall dependencies".into());
                }
                "tsconfig.json" | "jsconfig.json" => {
                    return Fate::NotCopied("replaced by the generated tsconfig.json".into());
                }
                "next-env.d.ts" => return Fate::NotCopied("Next.js type declarations".into()),
                ".gitignore" => return Fate::Gitignore,
                _ if name.starts_with("next.config.") => {
                    return Fate::NotCopied("replaced by the generated astro.config.mjs".into());
                }
                _ => {}
            }
        }
        match self.classes.get(path).copied() {
            Some(FileClass::Ignored) | None => {
                return Fate::NotCopied("excluded by discovery".into());
            }
            Some(FileClass::OutOfScope) => {
                return Fate::NotCopied("tests and repository tooling are outside V1".into());
            }
            Some(FileClass::Verbatim) => {
                let output = if local.starts_with("public/") {
                    local.to_owned()
                } else if let Some(output) = self.app_output(local, "assets") {
                    output
                } else {
                    local.to_owned()
                };
                return Fate::Convert(Candidate {
                    role: Role::Verbatim,
                    output,
                    rule: "asset.verbatim",
                    kind: OperationKind::Copy,
                    review: false,
                });
            }
            Some(FileClass::Units) => {}
        }
        let units = self.units.get(path).map(Vec::as_slice).unwrap_or_default();
        if let Some(unit) = units
            .iter()
            .find(|u| !matches!(u.status, Status::Auto | Status::Review))
        {
            return Fate::Preserve(format!(
                "the analysis rates this file's units {}",
                unit.status.as_str()
            ));
        }
        let review = units.iter().any(|u| u.status == Status::Review);
        if units.iter().any(|u| u.client) {
            return Fate::Preserve(
                "client components and islands are not converted by migrate v0".into(),
            );
        }
        let convert = |role, output, rule, kind| {
            Fate::Convert(Candidate {
                role,
                output,
                rule,
                kind,
                review,
            })
        };
        if let Some(&route) = self.pages.get(path) {
            let route_facts = &self.parts.project.nextjs().routes.routes[route];
            if let Some(reason) = page_refusal(route_facts) {
                return Fate::Preserve(reason.into());
            }
            if units
                .iter()
                .any(|u| u.kind == UnitKind::Route && u.strategy != Strategy::Astro)
            {
                return Fate::Preserve("the page is not convertible to an Astro component".into());
            }
            let pattern = route_facts.pattern.trim_start_matches('/');
            let output = if pattern.is_empty() {
                "src/pages/index.astro".to_owned()
            } else {
                format!("src/pages/{pattern}.astro")
            };
            return convert(
                Role::Page { route },
                output,
                "route.static-page",
                OperationKind::Create,
            );
        }
        match convention {
            Some(Convention::Layout) => {
                if units
                    .iter()
                    .any(|u| u.kind == UnitKind::Layout && u.strategy != Strategy::Astro)
                {
                    return Fate::Preserve(
                        "the layout is not convertible to an Astro component".into(),
                    );
                }
                return convert(
                    Role::Layout {
                        name: String::new(),
                        parent: None,
                    },
                    String::new(),
                    "layout.chain",
                    OperationKind::Create,
                );
            }
            Some(Convention::Metadata(Metadata::Favicon))
                if local == format!("{}/favicon.ico", self.app) =>
            {
                return convert(
                    Role::Verbatim,
                    "public/favicon.ico".into(),
                    "metadata.favicon",
                    OperationKind::Copy,
                );
            }
            Some(_) => {
                return Fate::Preserve("Next.js special files other than pages and layouts are not converted by migrate v0".into());
            }
            None => {}
        }
        if root_file {
            return Fate::Preserve("package-root modules are not converted by migrate v0".into());
        }
        let components: Vec<_> = units
            .iter()
            .filter(|u| u.kind == UnitKind::Component)
            .collect();
        if !components.is_empty() {
            let output = self.code_output(local, "components");
            if components.len() == 1 && components[0].strategy == Strategy::Astro {
                return convert(
                    Role::Component,
                    output,
                    "component.astro",
                    OperationKind::Create,
                );
            }
            if components
                .iter()
                .all(|u| u.strategy == Strategy::PreactStatic)
            {
                return convert(
                    Role::Script,
                    output,
                    "component.preact-static",
                    OperationKind::Transform,
                );
            }
            return Fate::Preserve("its components have no common v0 strategy".into());
        }
        if units.iter().any(|u| u.kind == UnitKind::Stylesheet) {
            return convert(
                Role::Stylesheet,
                self.code_output(local, "styles"),
                "style.relocate",
                OperationKind::Copy,
            );
        }
        if units.iter().all(|u| u.kind == UnitKind::Module) && convert::dialect(local).is_some()
            || local.ends_with(".d.ts")
        {
            return convert(
                Role::Script,
                self.code_output(local, "lib"),
                "module.relocate",
                OperationKind::Transform,
            );
        }
        Fate::Preserve("the file kind is not converted by migrate v0".into())
    }

    // Colocated files in the app directory move to `src/<category>/<route
    // directory>/`, without route groups and private folders (D-024).
    fn app_output(&self, local: &str, category: &str) -> Option<String> {
        let rest = local.strip_prefix(&self.app)?.strip_prefix('/')?;
        let mut parts: Vec<&str> = rest.split('/').collect();
        let name = parts.pop()?;
        parts.retain(|part| {
            !(part.starts_with('(') && part.ends_with(')')) && !part.starts_with('_')
        });
        parts.push(name);
        Some(format!("src/{category}/{}", parts.join("/")))
    }

    fn code_output(&self, local: &str, category: &str) -> String {
        if let Some(output) = self.app_output(local, category) {
            output
        } else if local.starts_with("src/") {
            local.to_owned()
        } else {
            format!("src/{local}")
        }
    }

    fn load_texts(&mut self) -> Result<(), MigrateError> {
        let paths: Vec<SourcePath> = self
            .fates
            .iter()
            .filter(|(_, fate)| match fate {
                Fate::Convert(candidate) => candidate.role != Role::Verbatim,
                Fate::Gitignore => true,
                Fate::Preserve(_) | Fate::NotCopied(_) => false,
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in paths {
            let text = self
                .bytes(&path)?
                .and_then(|bytes| SourceText::from_bytes(bytes.to_vec(), self.limits.text).ok());
            match text {
                Some(text) => {
                    self.texts.insert(path, text);
                }
                None => {
                    self.fates.insert(
                        path,
                        Fate::Preserve("the file is not readable UTF-8 text".into()),
                    );
                }
            }
        }
        Ok(())
    }

    // Components are named after their function; layouts after their
    // route position, with deterministic suffixes for collisions.
    fn name_components(&mut self) {
        let mut layout_dirs: BTreeMap<String, SourcePath> = BTreeMap::new();
        let mut used = BTreeSet::new();
        let paths: Vec<SourcePath> = self.fates.keys().cloned().collect();
        for path in &paths {
            let local = self.local(path).to_owned();
            let Some(Fate::Convert(candidate)) = self.fates.get(path) else {
                continue;
            };
            match candidate.role {
                Role::Layout { .. } => {
                    let directory = convert::directory(&local).to_owned();
                    let base = layout_name(directory.strip_prefix(&self.app).unwrap_or(&directory));
                    let mut name = base.clone();
                    let mut suffix = 2;
                    while !used.insert(name.clone()) {
                        name = format!("{}{suffix}", base.trim_end_matches("Layout")) + "Layout";
                        suffix += 1;
                    }
                    layout_dirs.insert(directory, path.clone());
                    let mut candidate = candidate.clone();
                    candidate.output = format!("src/layouts/{name}.astro");
                    candidate.role = Role::Layout { name, parent: None };
                    self.fates.insert(path.clone(), Fate::Convert(candidate));
                }
                Role::Component => {
                    let fate = match convert::component_name(&local, &self.texts[path], self.limits)
                    {
                        Ok(name) => {
                            let mut candidate = candidate.clone();
                            let directory = convert::directory(&candidate.output);
                            candidate.output = format!("{directory}/{name}.astro");
                            Fate::Convert(candidate)
                        }
                        Err(reason) => Fate::Preserve(reason),
                    };
                    self.fates.insert(path.clone(), fate);
                }
                Role::Page { .. } | Role::Script | Role::Stylesheet | Role::Verbatim => {}
            }
        }
        // A layout's parent is the layout of the nearest ancestor directory.
        for (directory, path) in &layout_dirs {
            let mut current = directory.as_str();
            let parent = loop {
                let Some((up, _)) = current.rsplit_once('/') else {
                    break None;
                };
                if let Some(parent) = layout_dirs.get(up) {
                    break Some(parent.clone());
                }
                current = up;
            };
            if let Some(Fate::Convert(candidate)) = self.fates.get_mut(path)
                && let Role::Layout { parent: slot, .. } = &mut candidate.role
            {
                *slot = parent;
            }
        }
    }

    // Generated files are reserved first; among converted files, the first
    // in source path order keeps a contested path, compared without case.
    fn check_collisions(&mut self) {
        let mut taken: BTreeSet<String> = scaffold::GENERATED
            .iter()
            .map(|path| path.to_lowercase())
            .collect();
        let paths: Vec<SourcePath> = self.fates.keys().cloned().collect();
        for path in paths {
            let Some(Fate::Convert(candidate)) = self.fates.get(&path) else {
                continue;
            };
            let valid = OutputPath::new(&candidate.output, self.limits.path).is_ok();
            if !valid {
                self.fates.insert(
                    path,
                    Fate::Preserve("its output path is not portable".into()),
                );
            } else if !taken.insert(candidate.output.to_lowercase()) {
                self.fates.insert(
                    path,
                    Fate::Preserve("its output path collides with another output file".into()),
                );
            }
        }
    }

    fn targets(&self) -> BTreeMap<SourcePath, Target> {
        self.fates
            .iter()
            .filter_map(|(path, fate)| {
                let Fate::Convert(candidate) = fate else {
                    return None;
                };
                let kind = match candidate.role {
                    Role::Page { .. } | Role::Layout { .. } | Role::Component => TargetKind::Astro,
                    Role::Script => TargetKind::Script,
                    Role::Stylesheet | Role::Verbatim => TargetKind::Asset,
                };
                Some((
                    path.clone(),
                    Target {
                        output: candidate.output.clone(),
                        kind,
                    },
                ))
            })
            .collect()
    }

    // Renders every candidate; any refusal preserves that file and starts
    // another round, because importers and pages may depend on it.
    fn converge(&mut self) -> Result<BTreeMap<SourcePath, Rendered>, MigrateError> {
        let options = print_options(self.texts.iter().map(|(path, text)| (path.as_str(), text)));
        loop {
            let targets = self.targets();
            let context = Context {
                graph: self.parts.project.modules(),
                targets: &targets,
                options,
                limits: self.limits,
            };
            let mut rendered = BTreeMap::new();
            let mut refused = Vec::new();
            for (path, fate) in &self.fates {
                let Fate::Convert(candidate) = fate else {
                    continue;
                };
                match self.render(&context, path, candidate)? {
                    Ok(output) => {
                        rendered.insert(path.clone(), output);
                    }
                    Err(reason) => refused.push((path.clone(), reason)),
                }
            }
            if refused.is_empty() {
                return Ok(rendered);
            }
            for (path, reason) in refused {
                self.fates.insert(path, Fate::Preserve(reason));
            }
        }
    }

    fn render(
        &self,
        context: &Context<'_>,
        path: &SourcePath,
        candidate: &Candidate,
    ) -> Result<Result<Rendered, String>, MigrateError> {
        let text = || {
            self.texts
                .get(path)
                .ok_or_else(|| MigrateError::internal("a planned text is missing"))
        };
        let plain = |bytes: Vec<u8>| Rendered {
            bytes,
            assumptions: Vec::new(),
        };
        let astro =
            |(emitted, assumptions): (frameshift_emit::EmittedText, Vec<&'static str>)| Rendered {
                bytes: emitted.as_str().as_bytes().to_vec(),
                assumptions,
            };
        Ok(match &candidate.role {
            Role::Verbatim => match self.bytes(path)? {
                Some(bytes) => Ok(plain(bytes.to_vec())),
                None => Err("the file is not readable".into()),
            },
            Role::Stylesheet => {
                let text = text()?;
                if convert::stylesheet_relocatable(text.as_str()) {
                    Ok(plain(text.as_str().as_bytes().to_vec()))
                } else {
                    Err("the stylesheet has relative references that relocation would break".into())
                }
            }
            Role::Script => convert::relocate(context, path, text()?, &candidate.output)
                .map(|emitted| plain(emitted.as_str().as_bytes().to_vec())),
            Role::Component => convert::astro(
                context,
                path,
                text()?,
                &candidate.output,
                &AstroRole::Component,
            )
            .map(astro),
            Role::Layout { parent, .. } => {
                let chain = self.layout_chain(path);
                let rendered = match (parent, chain) {
                    (_, Err(reason)) => Err(reason),
                    (None, Ok(_)) => convert::astro(
                        context,
                        path,
                        text()?,
                        &candidate.output,
                        &AstroRole::RootLayout,
                    ),
                    (Some(parent), Ok((root, _))) => {
                        let Some(Fate::Convert(parent_candidate)) = self.fates.get(parent) else {
                            return Ok(Err("its parent layout is not migrated".into()));
                        };
                        let Role::Layout {
                            name: parent_name, ..
                        } = &parent_candidate.role
                        else {
                            return Err(MigrateError::internal("a layout parent is not a layout"));
                        };
                        let props = convert::layout_props(Some(&root.metadata));
                        convert::astro(
                            context,
                            path,
                            text()?,
                            &candidate.output,
                            &AstroRole::NestedLayout {
                                parent_name,
                                parent_output: &parent_candidate.output,
                                props: &props,
                            },
                        )
                    }
                };
                rendered.map(astro)
            }
            Role::Page { route } => {
                let route = &self.parts.project.nextjs().routes.routes[*route];
                let Some(nearest) = route.layouts().last() else {
                    return Ok(Err("the page has no layout".into()));
                };
                let Some(Fate::Convert(layout)) = self.fates.get(nearest) else {
                    return Ok(Err("its layout is not migrated".into()));
                };
                let Role::Layout { name, .. } = &layout.role else {
                    return Err(MigrateError::internal("a page layout is not a layout"));
                };
                let (root, root_path) = match self.layout_chain(nearest) {
                    Ok(chain) => chain,
                    Err(reason) => return Ok(Err(reason)),
                };
                let props = convert::layout_props(Some(&root.metadata));
                convert::astro(
                    context,
                    path,
                    text()?,
                    &candidate.output,
                    &AstroRole::Page {
                        layout_name: name,
                        layout_output: &layout.output,
                        layout_props: &props,
                        root: &root,
                        root_source: &self.texts[&root_path],
                    },
                )
                .map(astro)
            }
        })
    }

    // The root layout facts of a layout's chain; every layout in the chain
    // must still be converted.
    fn layout_chain(&self, layout: &SourcePath) -> Result<(RootFacts, SourcePath), String> {
        let mut current = layout.clone();
        loop {
            let Some(Fate::Convert(candidate)) = self.fates.get(&current) else {
                return Err("a layout of its chain is not migrated".into());
            };
            let Role::Layout { parent, .. } = &candidate.role else {
                return Err("a layout of its chain is not a layout".into());
            };
            match parent {
                Some(parent) => current = parent.clone(),
                None => {
                    let facts = convert::root_facts(&current, &self.texts[&current], self.limits)?;
                    return Ok((facts, current));
                }
            }
        }
    }

    fn finish(self, mut rendered: BTreeMap<SourcePath, Rendered>) -> Result<Plan, MigrateError> {
        let mut operations = Vec::new();
        // Preserved paths of a case-sensitive source may collide elsewhere.
        let mut preserved = BTreeSet::new();
        let mut tasks = Vec::new();
        let output = |path: &str| {
            OutputPath::new(path, self.limits.path)
                .map_err(|_| MigrateError::internal("a planned output path is invalid"))
        };
        for (path, fate) in &self.fates {
            match fate {
                Fate::Convert(candidate) => {
                    let Rendered { bytes, assumptions } =
                        rendered.remove(path).ok_or_else(|| {
                            MigrateError::internal("a converted file was not rendered")
                        })?;
                    let mut reasons = Vec::new();
                    if candidate.review {
                        reasons.push("the analysis rates it review".to_owned());
                    }
                    if !assumptions.is_empty() {
                        reasons.push(format!(
                            "the conversion assumes values that React and Astro render alike ({})",
                            assumptions.join(", ")
                        ));
                    }
                    if !reasons.is_empty() {
                        tasks.push(ManualTask {
                            kind: TaskKind::Review,
                            source: Some(path.clone()),
                            text: format!(
                                "converted; check the result, because {}",
                                reasons.join(", and ")
                            ),
                        });
                    }
                    operations.push(Operation {
                        kind: candidate.kind,
                        output: output(&candidate.output)?,
                        rule: candidate.rule,
                        source: Some(path.clone()),
                        review: candidate.review || !assumptions.is_empty(),
                        bytes,
                    });
                }
                Fate::Preserve(reason) => {
                    let target = format!("{UNMIGRATED}/{}", self.local(path));
                    match (
                        OutputPath::new(&target, self.limits.path),
                        self.bytes(path)?,
                    ) {
                        (Ok(target), Some(bytes))
                            if preserved.insert(target.as_str().to_lowercase()) =>
                        {
                            operations.push(Operation {
                                kind: OperationKind::Preserve,
                                output: target,
                                rule: "preserve.unmigrated",
                                source: Some(path.clone()),
                                review: false,
                                bytes: bytes.to_vec(),
                            });
                            tasks.push(ManualTask {
                                kind: TaskKind::Preserved,
                                source: Some(path.clone()),
                                text: format!("preserved for manual migration: {reason}"),
                            });
                        }
                        (Ok(_), _) | (Err(_), _) => tasks.push(ManualTask {
                            kind: TaskKind::NotCopied,
                            source: Some(path.clone()),
                            text: format!(
                                "not copied, because it cannot be preserved verbatim; {reason}"
                            ),
                        }),
                    }
                }
                Fate::NotCopied(reason) => tasks.push(ManualTask {
                    kind: TaskKind::NotCopied,
                    source: Some(path.clone()),
                    text: reason.clone(),
                }),
                Fate::Gitignore => {}
            }
        }
        let gitignore = self
            .fates
            .iter()
            .find(|(_, fate)| **fate == Fate::Gitignore)
            .map(|(path, _)| path.clone());
        let line_ending =
            print_options(self.texts.iter().map(|(p, t)| (p.as_str(), t))).line_ending;
        let generated = scaffold::generate(
            &self.parts.manifest,
            gitignore.as_ref().and_then(|path| self.texts.get(path)),
            line_ending,
        );
        for (path, rule, bytes) in generated.files {
            operations.push(Operation {
                kind: OperationKind::Config,
                output: output(path)?,
                rule,
                source: if path == ".gitignore" {
                    gitignore.clone()
                } else {
                    None
                },
                review: false,
                bytes,
            });
        }
        tasks.push(ManualTask {
            kind: TaskKind::Project,
            source: None,
            text: "install dependencies with your package manager; lockfiles are not carried over"
                .into(),
        });
        if !generated.removed.is_empty() {
            tasks.push(ManualTask {
                kind: TaskKind::Project,
                source: None,
                text: format!(
                    "{} Next.js and React dependencies were removed from package.json; check that nothing else needs them",
                    generated.removed.len()
                ),
            });
        }
        // Concerns that the scaffold or the lockfile task already covers.
        let handled = [
            "config:nextjs.project.app-router",
            "config:npm.manifest",
            "config:npm.lockfile",
            "config:npm.lockfile.bun-binary",
            "config:ts.config",
        ];
        for unit in &self.parts.analysis.assessment.units {
            if unit.kind == UnitKind::Config && !handled.contains(&unit.id.as_str()) {
                tasks.push(ManualTask {
                    kind: TaskKind::Project,
                    source: None,
                    text: format!(
                        "configuration concern {} ({}) is not migrated by v0; see `frameshift analyze`",
                        unit.id.as_str(),
                        unit.status.as_str()
                    ),
                });
            }
        }
        tasks.sort_by(|a, b| (a.kind, &a.source).cmp(&(b.kind, &b.source)));
        let summary = scaffold::summary(
            self.parts.analysis.source_version.as_deref(),
            &operations,
            &tasks,
            line_ending,
        );
        operations.push(Operation {
            kind: OperationKind::Config,
            output: output(SUMMARY)?,
            rule: "report.summary",
            source: None,
            review: false,
            bytes: summary.into_bytes(),
        });
        operations.sort_by(|a, b| {
            a.output
                .as_str()
                .as_bytes()
                .cmp(b.output.as_str().as_bytes())
        });
        if operations.windows(2).any(|pair| {
            pair[0]
                .output
                .as_str()
                .eq_ignore_ascii_case(pair[1].output.as_str())
        }) {
            return Err(MigrateError::internal(
                "two operations write one output path",
            ));
        }
        Ok(Plan {
            source_version: self.parts.analysis.source_version.clone(),
            operations,
            tasks,
        })
    }
}

// Why a page is outside v0 before any conversion is attempted.
fn page_refusal(route: &AppRoute) -> Option<&'static str> {
    if route.segments.iter().any(|s| {
        s.interception.is_some() || !matches!(s.kind, SegmentKind::Static | SegmentKind::Group)
    }) {
        return Some(
            "dynamic, parallel, and intercepting route segments are not converted by migrate v0",
        );
    }
    if route
        .chain
        .iter()
        .any(|file| file.convention == Convention::Template)
    {
        return Some("templates in the layout chain are not converted by migrate v0");
    }
    None
}

// `RootLayout` for the app root, otherwise the route position in PascalCase.
fn layout_name(directory: &str) -> String {
    let mut name = String::new();
    for word in directory.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            name.push(first.to_ascii_uppercase());
            name.extend(chars);
        }
    }
    if name.is_empty() {
        return "RootLayout".into();
    }
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        name.insert_str(0, "Segment");
    }
    name + "Layout"
}

// The project's dominant line ending and indentation (ARCHITECTURE.md
// section 14.3), with LF and two spaces as fallbacks.
fn print_options<'t>(texts: impl Iterator<Item = (&'t str, &'t SourceText)>) -> PrintOptions {
    let (mut crlf, mut lf, mut tabs, mut spaces) = (0usize, 0usize, 0usize, Vec::new());
    for (_, text) in texts.filter(|(path, _)| convert::dialect(path).is_some()) {
        let text = text.as_str();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_names_follow_route_positions() {
        assert_eq!(layout_name(""), "RootLayout");
        assert_eq!(layout_name("/blog"), "BlogLayout");
        assert_eq!(layout_name("/(marketing)"), "MarketingLayout");
        assert_eq!(
            layout_name("/docs/getting-started"),
            "DocsGettingStartedLayout"
        );
        assert_eq!(layout_name("/2024"), "Segment2024Layout");
    }
}
