use std::collections::{BTreeMap, BTreeSet};

use frameshift_core::SourcePath;

use crate::{ModuleError, ModuleLimits, ResolutionConfig, Work, below, join, parent};

/// Classification by exact, case-sensitive source extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// JavaScript, JSX, TypeScript, or TSX implementation.
    Module,
    /// TypeScript declaration file, which supplies no runtime implementation.
    Declaration,
    /// CSS, SCSS, or indented Sass.
    Stylesheet,
    /// JSON data.
    Json,
    /// A file with another extension, without semantic validation.
    Asset,
}

impl FileKind {
    /// Classifies a path without reading content.
    pub fn of(path: &SourcePath) -> Self {
        let path = path.as_str();
        if path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts") {
            return Self::Declaration;
        }
        match path.rsplit_once('.').map(|(_, ext)| ext) {
            Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts") => Self::Module,
            Some("css" | "scss" | "sass") => Self::Stylesheet,
            Some("json") => Self::Json,
            _ => Self::Asset,
        }
    }
}

/// Why a reference has no established target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// No candidate is admitted in this snapshot.
    NotFound,
    /// A lexical parent reference would leave the run or selected package root.
    OutsideRoot,
    /// Invalid, ambiguous, or unsupported path spelling or URL scheme.
    InvalidSpecifier,
    /// A path is excluded, ignored, oversized, or behind an untraversed entry.
    Excluded,
    /// Configuration is invalid, unavailable, or has unmodeled resolver semantics.
    Configuration,
    /// Package `imports` needs condition and package-scope analysis.
    PackageImports,
    /// A local directory has a package manifest whose entry points are not modeled.
    DirectoryPackage,
    /// The specifier expression is not a string literal with a UTF-8 StringValue.
    Computed,
    /// A CommonJS-looking call needs binding analysis before it is a dependency.
    BindingAnalysis,
}

/// A resolution fact, without a claim about evaluation or migration eligibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// An admitted local file, preserving its exact source spelling.
    Local {
        /// Root-relative admitted path.
        path: SourcePath,
        /// Extension classification, independent of parse success.
        kind: FileKind,
    },
    /// A bare dependency reference; its contents are never read.
    External {
        /// Package name, including the scope, without its subpath.
        package: String,
    },
    /// An explicit `node:` built-in reference, without an availability claim.
    NodeBuiltin,
    /// No established target, with a specific reason.
    Unresolved(UnresolvedReason),
}

/// Pure local lookup over an explicit, complete set of admitted paths.
///
/// The caller must supply every candidate path, not only parsed modules.
/// The snapshot graph builder additionally records excluded entries, so a
/// higher-priority unread path cannot silently select a lower-priority file.
pub struct Resolver {
    pub(crate) files: BTreeMap<String, SourcePath>,
    pub(crate) excluded: BTreeSet<String>,
    pub(crate) config: ResolutionConfig,
    pub(crate) root: String,
    pub(crate) work: Work,
}

impl Resolver {
    /// Builds a resolver for the explicit TypeScript bundler lookup profile.
    /// Paths outside `package_root` cannot be resolved as local files.
    pub fn new(
        files: Vec<SourcePath>,
        package_root: Option<&SourcePath>,
        config: ResolutionConfig,
        limits: ModuleLimits,
    ) -> Result<Self, ModuleError> {
        if files.len() > limits.max_files {
            return Err(ModuleError::FileLimit);
        }
        let mut work = Work {
            remaining: limits.max_work,
        };
        let mut indexed = BTreeMap::new();
        for path in files {
            work.charge(path.as_str().len() + 1)?;
            indexed.insert(path.as_str().to_owned(), path);
        }
        Ok(Self {
            files: indexed,
            excluded: BTreeSet::new(),
            config,
            root: package_root.map_or("", SourcePath::as_str).to_owned(),
            work,
        })
    }

    /// Resolves one reference and charges the cumulative deterministic work budget.
    pub fn resolve(
        &mut self,
        importer: &SourcePath,
        specifier: &str,
    ) -> Result<Resolution, ModuleError> {
        use UnresolvedReason as U;
        self.work
            .charge(specifier.len() + importer.as_str().len() + 1)?;
        if !self.files.contains_key(importer.as_str()) || !below(importer.as_str(), &self.root) {
            return Err(ModuleError::InvalidRoot);
        }
        if !self.config.issues.is_empty() {
            return Ok(Resolution::Unresolved(U::Configuration));
        }
        if specifier.is_empty()
            || specifier.contains(['\\', '\0', '?'])
            || specifier.contains('#') && !specifier.starts_with('#')
        {
            return Ok(Resolution::Unresolved(U::InvalidSpecifier));
        }
        if specifier.starts_with('#') {
            return Ok(Resolution::Unresolved(U::PackageImports));
        }
        if let Some(name) = specifier.strip_prefix("node:") {
            return Ok(
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_/-".contains(&b))
                {
                    Resolution::NodeBuiltin
                } else {
                    Resolution::Unresolved(U::InvalidSpecifier)
                },
            );
        }
        if specifier.contains(':') || specifier.starts_with('/') {
            return Ok(Resolution::Unresolved(U::InvalidSpecifier));
        }
        if specifier == "."
            || specifier == ".."
            || specifier.starts_with("./")
            || specifier.starts_with("../")
        {
            let Some(path) = join(parent(importer.as_str()), specifier.trim_end_matches('/'))
            else {
                return Ok(Resolution::Unresolved(U::OutsideRoot));
            };
            return Ok(self
                .lookup(&path)?
                .unwrap_or(Resolution::Unresolved(U::NotFound)));
        }
        let mapping = if let Some((origin, mappings)) = &self.config.paths {
            let mut best = None;
            let mut best_prefix = 0;
            for mapping in mappings {
                self.work
                    .charge(mapping.pattern.len() + specifier.len() + 1)?;
                if mapping.pattern == specifier {
                    best = Some((mapping.clone(), String::new()));
                    break;
                }
                if let Some((prefix, suffix)) = mapping.pattern.split_once('*')
                    && specifier.len() >= prefix.len() + suffix.len()
                    && specifier.starts_with(prefix)
                    && specifier.ends_with(suffix)
                    && (best.is_none() || prefix.len() > best_prefix)
                {
                    best_prefix = prefix.len();
                    best = Some((
                        mapping.clone(),
                        specifier[prefix.len()..specifier.len() - suffix.len()].to_owned(),
                    ));
                }
            }
            best.map(|(mapping, capture)| {
                (
                    self.config.base.as_ref().unwrap_or(origin).clone(),
                    mapping,
                    capture,
                )
            })
        } else {
            None
        };
        if let Some((base, mapping, capture)) = mapping {
            for target in mapping.targets {
                self.work.charge(target.len() + capture.len() + 1)?;
                let target = target.replacen('*', &capture, 1);
                let Some(path) = join(&base, &target) else {
                    return Ok(Resolution::Unresolved(U::OutsideRoot));
                };
                if let Some(result) = self.lookup(&path)? {
                    return Ok(result);
                }
            }
        }
        if let Some(base) = &self.config.base {
            let Some(path) = join(base, specifier) else {
                return Ok(Resolution::Unresolved(U::InvalidSpecifier));
            };
            if let Some(result) = self.lookup(&path)? {
                return Ok(result);
            }
        }
        let parts: Vec<_> = specifier.split('/').collect();
        if parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            return Ok(Resolution::Unresolved(U::InvalidSpecifier));
        }
        let package = if specifier.starts_with('@') {
            if parts.len() < 2 || parts[0].len() == 1 {
                return Ok(Resolution::Unresolved(U::InvalidSpecifier));
            }
            format!("{}/{}", parts[0], parts[1])
        } else {
            parts[0].to_owned()
        };
        Ok(Resolution::External { package })
    }

    fn probe(&mut self, path: &str) -> Result<Option<Resolution>, ModuleError> {
        self.work.charge(path.len() + 1)?;
        if !below(path, &self.root) && path != self.root {
            return Ok(Some(Resolution::Unresolved(UnresolvedReason::OutsideRoot)));
        }
        let mut prefix = path;
        loop {
            if self.excluded.contains(prefix) {
                return Ok(Some(Resolution::Unresolved(UnresolvedReason::Excluded)));
            }
            let Some((next, _)) = prefix.rsplit_once('/') else {
                break;
            };
            prefix = next;
        }
        Ok(self.files.get(path).map(|path| Resolution::Local {
            path: path.clone(),
            kind: FileKind::of(path),
        }))
    }

    fn lookup(&mut self, path: &str) -> Result<Option<Resolution>, ModuleError> {
        let mut candidates = Vec::new();
        let families: &[(&[&str], &[&str])] = &[
            (&[".d.mts", ".mts", ".mjs"], &["mts", "d.mts", "mjs"]),
            (&[".d.cts", ".cts", ".cjs"], &["cts", "d.cts", "cjs"]),
            (&[".tsx", ".jsx"], &["tsx", "ts", "d.ts", "jsx", "js"]),
            (
                &[".d.ts", ".ts", ".js"],
                &["ts", "tsx", "d.ts", "js", "jsx"],
            ),
            (&[".json"], &["d.json.ts", "json"]),
        ];
        let mut substituted = false;
        for (suffixes, replacements) in families {
            if let Some(stem) = suffixes.iter().find_map(|suffix| path.strip_suffix(suffix)) {
                for ext in *replacements {
                    candidates.push(format!("{stem}.{ext}"));
                }
                substituted = true;
                break;
            }
        }
        if !substituted
            && path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.contains('.'))
        {
            // Assets are real snapshot references, even when TypeScript has
            // no declaration for their content. No CSS or image validity is implied.
            candidates.push(path.to_owned());
        }
        for ext in ["ts", "tsx", "d.ts", "js", "jsx"] {
            candidates.push(format!("{path}.{ext}"));
        }
        if !substituted {
            candidates.push(path.to_owned());
        }
        for candidate in candidates {
            if let Some(result) = self.probe(&candidate)? {
                return Ok(Some(result));
            }
        }
        let directory = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        if let Some(result) = self.probe(&format!("{directory}package.json"))? {
            return Ok(Some(if matches!(result, Resolution::Local { .. }) {
                Resolution::Unresolved(UnresolvedReason::DirectoryPackage)
            } else {
                result
            }));
        }
        for ext in ["ts", "tsx", "d.ts", "js", "jsx"] {
            if let Some(result) = self.probe(&format!("{directory}index.{ext}"))? {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}
