//! Configuration data retains the origin of inherited relative settings.

use frameshift_core::{SourcePath, SourceText};
use frameshift_input::{EntryKind, FileRead, Snapshot};
use frameshift_syntax::{JsonDialect, JsonLimits, JsonValue, parse_json};

use crate::{ModuleError, Work, join, parent};

/// Why a configuration cannot establish local resolution semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigProblem {
    /// Invalid JSONC, duplicate keys, or a setting of the wrong shape.
    Invalid,
    /// A referenced configuration is absent from the admitted files.
    Missing,
    /// The file is unreadable or is not accepted UTF-8 text.
    Unreadable,
    /// A package-based inheritance or an unmodeled resolver option is present.
    Unsupported,
    /// Inheritance contains a cycle.
    Cycle,
    /// Inheritance exceeds its explicit depth bound or implementation ceiling of 64.
    DepthLimit,
}

/// A configuration problem, with a root-relative path and no source snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigIssue {
    /// Configuration that introduced the problem.
    pub path: SourcePath,
    /// The problem category.
    pub problem: ConfigProblem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mapping {
    pub pattern: String,
    pub targets: Vec<String>,
}

/// Parsed local-resolution settings for the explicit bundler profile.
///
/// Unknown configuration suppresses resolution rather than falling back to
/// defaults. Other compiler options are preserved by their original snapshot;
/// they are not interpreted as migration instructions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolutionConfig {
    pub(crate) base: Option<String>,
    pub(crate) paths: Option<(String, Vec<Mapping>)>,
    pub(crate) issues: Vec<ConfigIssue>,
}

impl ResolutionConfig {
    /// Parses one configuration without loading `extends` references.
    /// Inheritance in this API produces an explicit unsupported issue.
    pub fn parse(source: &SourceText, path: &SourcePath, limits: JsonLimits) -> Self {
        let (mut config, extends) = parse(source, path, limits);
        if !extends.is_empty() {
            config.issue(path, ConfigProblem::Unsupported);
        }
        config
    }

    /// Problems that prevent this configuration from authorizing resolution.
    pub fn issues(&self) -> &[ConfigIssue] {
        &self.issues
    }

    fn issue(&mut self, path: &SourcePath, problem: ConfigProblem) {
        self.issues.push(ConfigIssue {
            path: path.clone(),
            problem,
        });
    }
}

fn parse(
    source: &SourceText,
    path: &SourcePath,
    limits: JsonLimits,
) -> (ResolutionConfig, Vec<String>) {
    let mut config = ResolutionConfig::default();
    let mut extends = Vec::new();
    let valid = (|| {
        let doc = parse_json(source, JsonDialect::Jsonc, limits).ok()?;
        if doc.duplicate_keys().next().is_some() {
            return None;
        }
        let root = doc.value(doc.root())?.as_object()?;
        if let Some(id) = root.get("extends") {
            match doc.value(id)? {
                JsonValue::String(s) => extends.push(s.to_owned()),
                JsonValue::Array(values) => {
                    for id in values.iter() {
                        extends.push(doc.value(id)?.as_str()?.to_owned());
                    }
                }
                JsonValue::Null
                | JsonValue::Bool(_)
                | JsonValue::Number(_)
                | JsonValue::Object(_) => return None,
            }
        }
        let Some(id) = root.get("compilerOptions") else {
            return Some(());
        };
        let options = doc.value(id)?.as_object()?;
        // These options change lookup or import meaning. The initial profile
        // rejects them rather than approximating another compiler/bundler.
        for name in [
            "rootDirs",
            "moduleSuffixes",
            "customConditions",
            "noResolve",
            "resolvePackageJsonImports",
            "resolvePackageJsonExports",
        ] {
            if options.get(name).is_some() {
                config.issue(path, ConfigProblem::Unsupported);
            }
        }
        if let Some(id) = options.get("moduleResolution")
            && !doc.value(id)?.as_str()?.eq_ignore_ascii_case("bundler")
        {
            config.issue(path, ConfigProblem::Unsupported);
        }
        if let Some(id) = options.get("baseUrl") {
            let base = doc.value(id)?.as_str()?;
            config.base = Some(if base.is_empty() {
                parent(path.as_str()).to_owned()
            } else {
                join(parent(path.as_str()), base)?
            });
        }
        if let Some(id) = options.get("paths") {
            let object = doc.value(id)?.as_object()?;
            let mut mappings = Vec::new();
            for member in object.members() {
                let pattern = member.key();
                if pattern.matches('*').count() > 1 || pattern.is_empty() {
                    return None;
                }
                let array = doc.value(member.value())?.as_array()?;
                if array.is_empty() {
                    return None;
                }
                let mut targets = Vec::new();
                for id in array.iter() {
                    let text = doc.value(id)?.as_str()?;
                    if text.matches('*').count() > 1 || text.is_empty() {
                        return None;
                    }
                    targets.push(text.to_owned());
                }
                mappings.push(Mapping {
                    pattern: pattern.to_owned(),
                    targets,
                });
            }
            config.paths = Some((parent(path.as_str()).to_owned(), mappings));
        }
        Some(())
    })();
    if valid.is_none() {
        config.issue(path, ConfigProblem::Invalid);
    }
    (config, extends)
}

pub(crate) fn load(
    snapshot: &Snapshot,
    path: &SourcePath,
    limits: crate::GraphLimits,
    active: &mut Vec<SourcePath>,
    work: &mut Work,
) -> Result<ResolutionConfig, ModuleError> {
    work.charge(1)?;
    let mut result = ResolutionConfig::default();
    if active.contains(path) {
        result.issue(path, ConfigProblem::Cycle);
        return Ok(result);
    }
    if active.len() >= limits.modules.max_config_depth.min(64) {
        result.issue(path, ConfigProblem::DepthLimit);
        return Ok(result);
    }
    let Some(id) = snapshot.inventory().files().find(path) else {
        result.issue(path, ConfigProblem::Missing);
        return Ok(result);
    };
    let FileRead::Content(file) = snapshot.read(id).map_err(ModuleError::Snapshot)? else {
        result.issue(path, ConfigProblem::Unreadable);
        return Ok(result);
    };
    work.charge(file.bytes().len())?;
    let Ok(source) = SourceText::from_bytes(file.bytes().to_vec(), limits.text) else {
        result.issue(path, ConfigProblem::Unreadable);
        return Ok(result);
    };
    let (own, extends) = parse(&source, path, limits.json);
    active.push(path.clone());
    for reference in extends {
        if !reference.starts_with("./") && !reference.starts_with("../") {
            result.issue(path, ConfigProblem::Unsupported);
            continue;
        }
        let Some(mut target) = join(parent(path.as_str()), &reference) else {
            result.issue(path, ConfigProblem::Invalid);
            continue;
        };
        let Ok(exact) = SourcePath::new(&target, limits.path) else {
            result.issue(path, ConfigProblem::Invalid);
            continue;
        };
        let present = snapshot.inventory().files().find(&exact).is_some();
        if !present
            && snapshot
                .inventory()
                .entry(&exact)
                .is_some_and(|entry| !matches!(entry.kind(), EntryKind::Directory))
        {
            result.issue(&exact, ConfigProblem::Unreadable);
            continue;
        }
        if !present {
            target.push_str(".json");
        }
        let Ok(target) = SourcePath::new(&target, limits.path) else {
            result.issue(path, ConfigProblem::Invalid);
            continue;
        };
        let inherited = load(snapshot, &target, limits, active, work)?;
        if inherited.base.is_some() {
            result.base = inherited.base;
        }
        if inherited.paths.is_some() {
            result.paths = inherited.paths;
        }
        result.issues.extend(inherited.issues);
    }
    active.pop();
    if own.base.is_some() {
        result.base = own.base;
    }
    if own.paths.is_some() {
        result.paths = own.paths;
    }
    result.issues.extend(own.issues);
    Ok(result)
}
