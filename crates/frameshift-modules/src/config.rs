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

/// The emitted module format, which selects package-map conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleFormat {
    CommonJs,
    Esm,
    Preserve,
    Classic,
}

/// Explicit source-lookup semantics, independent of a reference's import mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResolutionProfile {
    /// TypeScript 6.0.3 bundler lookup, including package maps.
    #[default]
    Bundler,
    /// TypeScript 6.0.3 legacy Node10 lookup, without package maps.
    Node10,
}

/// Parsed local-resolution settings for the selected profile.
///
/// Unknown configuration suppresses resolution rather than falling back to
/// defaults. Other compiler options are preserved by their original snapshot;
/// they are not interpreted as migration instructions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolutionConfig {
    pub(crate) profile: Option<ResolutionProfile>,
    pub(crate) base: Option<String>,
    pub(crate) paths: Option<(String, Vec<Mapping>)>,
    pub(crate) issues: Vec<ConfigIssue>,
    // Options are kept per name so that inheritance overrides each one
    // separately; `finish` validates their combination.
    pub(crate) module: Option<ModuleFormat>,
    pub(crate) es5_target: Option<bool>,
    pub(crate) allow_js: Option<bool>,
    pub(crate) check_js: Option<bool>,
    pub(crate) output_directory: bool,
    pub(crate) unsupported_module: Option<SourcePath>,
}

impl ResolutionConfig {
    /// Parses one configuration without loading `extends` references.
    /// Inheritance in this API produces an explicit unsupported issue.
    pub fn parse(source: &SourceText, path: &SourcePath, limits: JsonLimits) -> Self {
        let (mut config, extends) = parse(source, path, limits);
        if !extends.is_empty() {
            config.issue(path, ConfigProblem::Unsupported);
        }
        config.finish();
        config
    }

    /// The emitted format, with TypeScript 6.0.3's defaults: an ES5 target
    /// emits CommonJS, every other target an ECMAScript module.
    pub(crate) fn format(&self) -> ModuleFormat {
        self.module.unwrap_or(if self.es5_target == Some(true) {
            ModuleFormat::CommonJs
        } else {
            ModuleFormat::Esm
        })
    }

    /// `allowJs`, which defaults to `checkJs`.
    pub(crate) fn allows_js(&self) -> bool {
        self.allow_js.or(self.check_js).unwrap_or(false)
    }

    // Unsupported module kinds select classic or Node.js resolution, or
    // derive file formats from package scopes; they are rejected only after
    // inheritance has combined every file's options.
    fn finish(&mut self) {
        if let Some(origin) = self.unsupported_module.take()
            && !(self.profile() == ResolutionProfile::Node10
                && self.module == Some(ModuleFormat::Classic))
        {
            self.issue(&origin, ConfigProblem::Unsupported);
        }
    }

    /// The selected profile. An absent setting retains the bundler default.
    pub fn profile(&self) -> ResolutionProfile {
        self.profile.unwrap_or_default()
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
        if let Some(id) = options.get("moduleResolution") {
            config.profile = match doc.value(id)?.as_str()?.to_ascii_lowercase().as_str() {
                "bundler" => Some(ResolutionProfile::Bundler),
                "node" | "node10" => Some(ResolutionProfile::Node10),
                _ => {
                    config.issue(path, ConfigProblem::Unsupported);
                    None
                }
            };
        }
        if let Some(id) = options.get("module") {
            // Option values are case-insensitive in the reference.
            config.module = match doc.value(id)?.as_str()?.to_ascii_lowercase().as_str() {
                "commonjs" => Some(ModuleFormat::CommonJs),
                "es6" | "es2015" | "es2020" | "es2022" | "esnext" => Some(ModuleFormat::Esm),
                "preserve" => Some(ModuleFormat::Preserve),
                "none" | "amd" | "system" | "umd" => {
                    config.unsupported_module = Some(path.clone());
                    Some(ModuleFormat::Classic)
                }
                "node16" | "node18" | "node20" | "nodenext" => {
                    config.unsupported_module = Some(path.clone());
                    None
                }
                _ => return None,
            };
        }
        if let Some(id) = options.get("target") {
            config.es5_target = match doc.value(id)?.as_str()?.to_ascii_lowercase().as_str() {
                "es5" => Some(true),
                "es3" | "es6" | "es2015" | "es2016" | "es2017" | "es2018" | "es2019" | "es2020"
                | "es2021" | "es2022" | "es2023" | "es2024" | "es2025" | "esnext" => Some(false),
                _ => return None,
            };
        }
        for (name, slot) in [
            ("allowJs", &mut config.allow_js),
            ("checkJs", &mut config.check_js),
        ] {
            if let Some(id) = options.get(name) {
                *slot = Some(doc.value(id)?.as_bool()?);
            }
        }
        for name in ["outDir", "declarationDir"] {
            if let Some(id) = options.get(name) {
                doc.value(id)?.as_str()?;
                config.output_directory = true;
            }
        }
        if let Some(id) = options.get("baseUrl") {
            let base = doc.value(id)?.as_str()?;
            if base.starts_with('/') {
                return None;
            }
            let base = base.strip_suffix('/').unwrap_or(base);
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
        if !present && !target.ends_with(".json") {
            target.push_str(".json");
        }
        let Ok(target) = SourcePath::new(&target, limits.path) else {
            result.issue(path, ConfigProblem::Invalid);
            continue;
        };
        let inherited = load(snapshot, &target, limits, active, work)?;
        result.inherit(inherited);
    }
    active.pop();
    result.inherit(own);
    if active.is_empty() {
        result.finish();
    }
    Ok(result)
}

impl ResolutionConfig {
    // Later configurations override each option separately; issues and
    // unsupported kinds accumulate.
    fn inherit(&mut self, later: Self) {
        self.profile = later.profile.or(self.profile);
        if later.base.is_some() {
            self.base = later.base;
        }
        if later.paths.is_some() {
            self.paths = later.paths;
        }
        if later.module.is_some() || later.unsupported_module.is_some() {
            self.module = later.module;
            self.unsupported_module = later.unsupported_module;
        }
        self.es5_target = later.es5_target.or(self.es5_target);
        self.allow_js = later.allow_js.or(self.allow_js);
        self.check_js = later.check_js.or(self.check_js);
        self.output_directory |= later.output_directory;
        self.issues.extend(later.issues);
    }
}
