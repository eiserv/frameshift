//! Module configuration and reference lookup over fictional admitted paths.

use frameshift_engine::foundation::{PathLimits, SourcePath, SourceText, TextLimits};
use frameshift_engine::modules::{
    ModuleLimits, Resolution, ResolutionConfig, Resolver, collect_dependencies,
};
use frameshift_engine::syntax::{JsonLimits, ParseLimits, SourceGoal, parse_js_program};

/// Checks deterministic configuration, path containment, admitted targets,
/// order independence, and bounded failures for arbitrary UTF-8 inputs.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = crate::targets::text(input)? else {
        return Ok(());
    };
    let (configuration, reference) = text.split_once('\n').unwrap_or(("{}", text));
    let source = SourceText::new(configuration.to_owned(), TextLimits::default())
        .map_err(|e| e.to_string())?;
    let path =
        |p: &str| SourcePath::new(p, PathLimits::default()).expect("fictional canonical path");
    let configuration =
        ResolutionConfig::parse(&source, &path("tsconfig.json"), JsonLimits::default());
    let files: Vec<_> = [
        "src/main.ts",
        "src/a.ts",
        "src/a.js",
        "src/d/index.ts",
        "src/a.d.ts",
        "src/data.json",
        "src/style.scss",
        "src/style.d.scss.ts",
        "src/logo.d.svg.ts",
        "sibling/a.ts",
    ]
    .iter()
    .map(|p| path(p))
    .collect();
    let config_again =
        ResolutionConfig::parse(&source, &path("tsconfig.json"), JsonLimits::default());
    if configuration != config_again {
        return Err("module configuration changed on repetition".to_owned());
    }
    let mut reversed = files.clone();
    reversed.reverse();
    for root in [None, Some(path("src"))] {
        for max_work in [0, input.len() as u64, ModuleLimits::default().max_work] {
            let limits = ModuleLimits {
                max_work,
                ..ModuleLimits::default()
            };
            let resolve = |files, config| {
                Resolver::new(files, root.as_ref(), config, limits)
                    .and_then(|mut r| r.resolve(&path("src/main.ts"), reference))
            };
            let first = resolve(files.clone(), configuration.clone());
            let second = resolve(reversed.clone(), config_again.clone());
            if first != second {
                return Err("module lookup depends on input order".to_owned());
            }
            if let Ok(Resolution::Local { path, .. }) = first {
                if !files.contains(&path) {
                    return Err("module target was not admitted".to_owned());
                }
                if root.is_some() && !path.as_str().starts_with("src/") {
                    return Err("module target escaped the package boundary".to_owned());
                }
                if !configuration.issues().is_empty() {
                    return Err("invalid configuration authorized a local target".to_owned());
                }
            }
        }
    }
    let program =
        SourceText::new(text.to_owned(), TextLimits::default()).map_err(|e| e.to_string())?;
    for dialect in crate::targets::DIALECTS {
        let Ok(tree) = parse_js_program(
            &program,
            SourceGoal::Module,
            dialect,
            ParseLimits::default(),
        ) else {
            continue;
        };
        let collect = || {
            let mut resolver = Resolver::new(
                files.clone(),
                None,
                ResolutionConfig::default(),
                ModuleLimits::default(),
            )
            .map_err(|e| e.to_string())?;
            collect_dependencies(&tree, &path("src/main.ts"), &mut resolver, 1024)
                .map_err(|e| e.to_string())
        };
        let first = collect()?;
        if first != collect()? {
            return Err("dependency extraction changed on repetition".to_owned());
        }
        if first
            .windows(2)
            .any(|edges| edges[0].span.start() > edges[1].span.start())
        {
            return Err("dependency edges are not in source order".to_owned());
        }
        for edge in first {
            if program.slice(edge.span).is_err() {
                return Err("dependency span is outside source".to_owned());
            }
            if let Resolution::Local { path, .. } = edge.resolution
                && !files.contains(&path)
            {
                return Err("dependency target was not admitted".to_owned());
            }
        }
    }
    Ok(())
}
