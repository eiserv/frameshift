//! Opt-in measurement of the component graph and environment classification
//! (`D-096`) on the approved `D-043` sample, outside the repository.

use std::collections::{BTreeMap, BTreeSet};

use frameshift_engine::foundation::{FindingCollector, FindingLimits, PathLimits, SourcePath};
use frameshift_engine::input::snapshot;
use frameshift_engine::limits::Limits;
use frameshift_engine::nextjs::{ApiKind, Rendered, Wrapper};
use frameshift_engine::project;

const EXTENSIONS: &[&str] = &["tsx", "ts", "jsx", "js"];

/// Builds the source model of every App Router package in the directories
/// of `FRAMESHIFT_MODEL_DIRS` (separated by `;`) and prints how render sites
/// resolve and where modules run. Files are read through snapshots and
/// never built, installed, or executed.
#[test]
#[ignore = "requires the approved real-world sample outside the repository"]
fn external_model_sample() {
    let dirs = std::env::var("FRAMESHIFT_MODEL_DIRS").expect("set FRAMESHIFT_MODEL_DIRS");
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut count = |key: String| *counts.entry(key).or_default() += 1;
    for dir in dirs.split(';').filter(|d| !d.is_empty()) {
        let mut limits = Limits::default();
        limits.run.max_files = 400_000;
        limits.run.max_entries = 800_000;
        limits.run.max_total_bytes = 4 << 30;
        let snapshot = snapshot(
            std::path::Path::new(dir),
            limits.snapshot(),
            &mut FindingCollector::new(FindingLimits::default()),
        )
        .unwrap();
        let files: BTreeSet<&str> = snapshot
            .inventory()
            .files()
            .as_slice()
            .iter()
            .map(SourcePath::as_str)
            .collect();
        // Packages with a manifest and an App Router directory holding a
        // page, preferring `app` to `src/app` as Next.js does.
        let mut packages = Vec::new();
        for manifest in files.iter().filter(|f| f.ends_with("package.json")) {
            let package = manifest.strip_suffix("package.json").unwrap();
            let app = ["app/", "src/app/"].into_iter().find(|app| {
                let prefix = format!("{package}{app}");
                files
                    .range(prefix.as_str()..)
                    .take_while(|f| f.starts_with(&prefix))
                    .any(|f| {
                        let name = f.rsplit('/').next().unwrap();
                        EXTENSIONS.iter().any(|e| name == format!("page.{e}"))
                    })
            });
            if let Some(app) = app {
                packages.push((
                    package.to_owned(),
                    format!("{package}{}", &app[..app.len() - 1]),
                ));
            }
        }
        for (package, app) in packages {
            let root = package
                .strip_suffix('/')
                .map(|p| SourcePath::new(p, PathLimits::default()).unwrap());
            let app = SourcePath::new(&app, PathLimits::default()).unwrap();
            let model = match project::build(&snapshot, root.as_ref(), &app, EXTENSIONS, &limits) {
                Ok(model) => model,
                Err(e) => {
                    count(format!("package error {e:?}"));
                    continue;
                }
            };
            count("package".to_owned());
            if std::env::var("FRAMESHIFT_MODEL_VERBOSE").is_ok() {
                for node in model.modules().nodes() {
                    for edge in &node.edges {
                        if let frameshift_engine::modules::Resolution::Unresolved(reason) =
                            &edge.resolution
                        {
                            let specifier = edge.specifier.as_deref().unwrap_or("?");
                            let head: String = specifier.chars().take(12).collect();
                            count(format!("edge {package} {reason:?} {head}"));
                        }
                    }
                }
            }
            let graph = model.components();
            for site in &graph.sites {
                let class = match &site.resolved.rendered {
                    Rendered::Intrinsic => "intrinsic".to_owned(),
                    Rendered::Definition(_) => "definition".to_owned(),
                    Rendered::External { .. } => "external".to_owned(),
                    Rendered::Unknown { reason, .. } => {
                        let detail = format!("{reason:?}");
                        let short = detail.split('(').next().unwrap();
                        if short == "Unresolved"
                            && std::env::var("FRAMESHIFT_MODEL_VERBOSE").is_ok()
                        {
                            let repo = dir.trim_end_matches('/').rsplit('/').next().unwrap();
                            count(format!("detail {repo} {detail}"));
                        }
                        format!("unknown {short}")
                    }
                };
                count(format!("site {class}"));
                for wrapper in &site.resolved.wrappers {
                    let wrapper = match wrapper {
                        Wrapper::Dynamic(_) => "Dynamic".to_owned(),
                        Wrapper::Memo | Wrapper::ForwardRef | Wrapper::Lazy => {
                            format!("{wrapper:?}")
                        }
                    };
                    count(format!("wrapper {wrapper}"));
                }
            }
            for entry in &graph.entries {
                let class = match &entry.resolved.rendered {
                    Rendered::Definition(_) => "definition".to_owned(),
                    Rendered::Intrinsic | Rendered::External { .. } => "other".to_owned(),
                    Rendered::Unknown { reason, .. } => format!("unknown {reason:?}"),
                };
                count(format!("entry {class}"));
            }
            for _ in &graph.browser_imports {
                count("browser import".to_owned());
            }
            for module in &model.environment().modules {
                count(format!("module {:?}", module.runtime()));
                for variable in &module.variables {
                    let named = if variable.name.is_some() {
                        "named"
                    } else {
                        "unnamed"
                    };
                    count(format!("variable {:?} {named}", variable.access));
                }
                for api in &module.apis {
                    let at = if api.deferred {
                        "deferred"
                    } else {
                        "module-level"
                    };
                    count(format!("api {:?} {at}", api.kind));
                    if api.kind == ApiKind::BrowserGlobal
                        && !api.deferred
                        && !api.typeof_guard
                        && module.build
                    {
                        count("api browser global at build-evaluated module level".to_owned());
                    }
                }
            }
        }
    }
    for (key, value) in &counts {
        println!("{key}: {value}");
    }
}
