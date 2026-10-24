//! Resolution-relevant `package.json` fields and package-map key selection.
//!
//! The pinned reference reads manifests with `JSON.parse` and walks objects
//! in JavaScript own-key order. Duplicate keys and invalid JSON fail closed
//! here instead of being repaired.

use std::sync::Arc;

use frameshift_core::SourceText;
use frameshift_syntax::{JsonDialect, JsonDocument, JsonId, JsonLimits, JsonValue, parse_json};

use crate::{
    FileKind, ModuleError, Resolution, ResolutionMode, Resolver, UnresolvedReason as U, parent,
};

/// Nested bare targets of package `imports` restart resolution; the
/// reference has no bound and overflows its stack on a cycle.
const MAX_TARGET_DEPTH: usize = 16;

/// A package-map value, with object members in JavaScript own-key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    String(String),
    Null,
    Array(Vec<Target>),
    Object(Vec<(String, Target)>),
    /// A boolean or number; only its JavaScript truthiness is observable.
    Scalar(bool),
}

impl Target {
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Self::String(text) => !text.is_empty(),
            Self::Null => false,
            Self::Array(_) | Self::Object(_) => true,
            Self::Scalar(truthy) => *truthy,
        }
    }
}

/// Resolution-relevant fields of one `package.json`.
///
/// Only the fields that the pinned bundler lookup reads for local code are
/// retained: `name`, `typings`, `types`, `main`, `typesVersions`, `exports`,
/// and `imports`. Dependency declarations are the package model's concern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageManifest {
    pub(crate) name: Option<String>,
    pub(crate) typings: Option<String>,
    pub(crate) types: Option<String>,
    pub(crate) main: Option<String>,
    pub(crate) types_versions: bool,
    pub(crate) exports: Option<Target>,
    pub(crate) imports: Option<Target>,
}

impl PackageManifest {
    /// Parses strict JSON. Returns `None` for invalid JSON, duplicate keys,
    /// or a top level that is not an object; such a manifest never
    /// authorizes a lookup.
    pub fn parse(source: &SourceText, limits: JsonLimits) -> Option<Self> {
        let doc = parse_json(source, JsonDialect::Json, limits).ok()?;
        if doc.duplicate_keys().next().is_some() {
            return None;
        }
        let root = doc.value(doc.root())?.as_object()?;
        let string = |name: &str| -> Option<String> {
            let value = doc.value(root.get(name)?)?.as_str()?;
            Some(value.to_owned())
        };
        // Path fields that are empty strings are absent for the reference.
        let path = |name: &str| string(name).filter(|value| !value.is_empty());
        let map = |name: &str| -> Option<Option<Target>> {
            match root.get(name) {
                None => Some(None),
                Some(id) => Some(Some(target(&doc, id)?)),
            }
        };
        Some(Self {
            name: string("name"),
            typings: path("typings"),
            types: path("types"),
            main: path("main"),
            types_versions: root.get("typesVersions").is_some_and(|id| {
                matches!(
                    doc.value(id),
                    Some(JsonValue::Object(_) | JsonValue::Array(_))
                )
            }),
            exports: map("exports")?,
            imports: map("imports")?,
        })
    }
}

fn target(doc: &JsonDocument<'_>, id: JsonId) -> Option<Target> {
    Some(match doc.value(id)? {
        JsonValue::Null => Target::Null,
        JsonValue::Bool(value) => Target::Scalar(value),
        JsonValue::Number(number) => Target::Scalar(
            number
                .text()
                .split(['e', 'E'])
                .next()
                .is_some_and(|mantissa| mantissa.bytes().any(|b| (b'1'..=b'9').contains(&b))),
        ),
        JsonValue::String(text) => Target::String(text.to_owned()),
        JsonValue::Array(array) => {
            let mut items = Vec::new();
            items.try_reserve(array.len()).ok()?;
            for id in array.iter() {
                items.push(target(doc, id)?);
            }
            Target::Array(items)
        }
        JsonValue::Object(object) => {
            let mut members = Vec::new();
            members.try_reserve(object.len()).ok()?;
            for member in object.members() {
                members.push((member.key().to_owned(), target(doc, member.value())?));
            }
            // Array-index keys precede other keys in ascending numeric order;
            // the stable sort keeps insertion order for the rest.
            members.sort_by_key(|(key, _)| array_index(key).map_or((1, 0), |i| (0, i)));
            Target::Object(members)
        }
    })
}

/// A canonical array index, which JavaScript enumerates before other keys.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || key.len() > 1 && key.starts_with('0') {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u64 = key.parse().ok()?;
    (value < u64::from(u32::MAX)).then_some(value as u32)
}

/// The reference's map-key match: the target, the captured subpath, and
/// whether the key was a pattern.
pub(crate) fn select<'t>(
    table: &'t [(String, Target)],
    name: &str,
) -> Option<(&'t Target, String, bool)> {
    if !name.ends_with('/')
        && !name.contains('*')
        && let Some((_, target)) = table.iter().find(|(key, _)| key == name)
    {
        return Some((target, String::new(), false));
    }
    let mut expanding: Vec<&(String, Target)> = table
        .iter()
        .filter(|(key, _)| key.matches('*').count() == 1 || key.ends_with('/'))
        .collect();
    expanding.sort_by(|(a, _), (b, _)| compare_pattern_keys(a, b));
    for (key, target) in expanding {
        if let Some(star) = key.find('*')
            && !key.ends_with('*')
        {
            let (prefix, suffix) = (&key[..star], &key[star + 1..]);
            if name.starts_with(prefix) && name.ends_with(suffix) {
                // JavaScript `substring` swaps reversed bounds, which yields the
                // overlap when a short name matches both prefix and suffix.
                let (start, end) = (prefix.len(), name.len() - suffix.len());
                let subpath = &name[start.min(end)..start.max(end)];
                return Some((target, subpath.to_owned(), true));
            }
        }
        if let Some(prefix) = key.strip_suffix('*')
            && name.starts_with(prefix)
        {
            return Some((target, name[prefix.len()..].to_owned(), true));
        }
        if let Some(rest) = name.strip_prefix(key.as_str()) {
            return Some((target, rest.to_owned(), false));
        }
    }
    None
}

// The reference compares UTF-16 lengths: longer bases first, then patterns
// before exact keys, then longer keys.
fn compare_pattern_keys(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering as O;
    let units = |s: &str| s.encode_utf16().count();
    let base = |s: &str| s.find('*').map_or(units(s), |i| units(&s[..i]) + 1);
    match base(b).cmp(&base(a)) {
        O::Equal => {}
        other @ (O::Less | O::Greater) => return other,
    }
    match (a.contains('*'), b.contains('*')) {
        (false, _) => return O::Greater,
        (_, false) => return O::Less,
        (true, true) => {}
    }
    units(b).cmp(&units(a))
}

/// Extension classes, as the reference's `Extensions` flags.
#[derive(Clone, Copy)]
pub(crate) struct Extensions {
    ts: bool,
    js: bool,
    declaration: bool,
    json: bool,
}

pub(crate) const ALL: Extensions = Extensions {
    ts: true,
    js: true,
    declaration: true,
    json: true,
};

pub(crate) const TYPED: Extensions = Extensions {
    ts: true,
    js: false,
    declaration: true,
    json: false,
};
pub(crate) const UNTYPED: Extensions = Extensions {
    ts: false,
    js: true,
    declaration: false,
    json: true,
};

impl Extensions {
    pub(crate) fn accepts(self, path: &str) -> bool {
        if declaration_name(base_name(path)) {
            self.declaration
        } else if [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|ext| path.ends_with(ext))
        {
            self.ts
        } else if path.ends_with(".json") {
            self.json
        } else {
            self.js
        }
    }
}

/// The outcome of one package-map step. `Continue` lets the caller try the
/// next condition, array element, or lookup stage, as an absent result does
/// in the reference; the other outcomes end the request.
enum Step {
    Continue,
    Null,
    Found(Resolution),
    Stop(U),
}

enum Scope {
    /// No `package.json` between the directory and the run root.
    Absent,
    /// The nearest manifest is invalid, unreadable, excluded, or not supplied.
    Unknown,
    Found(String, Arc<PackageManifest>),
}

fn extension_is(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

fn declaration_name(name: &str) -> bool {
    [".d.ts", ".d.cts", ".d.mts"]
        .iter()
        .any(|e| name.len() >= e.len() && name.ends_with(e))
        || extension_is(name, ".ts") && base_name(name).contains(".d.")
}

/// The candidates of the reference's `tryAddingExtensions`, in probe order.
fn adding(base: &str, original: &str, e: Extensions, out: &mut Vec<String>) {
    let mut push = |enabled: bool, extensions: &[&str]| {
        if enabled {
            out.extend(extensions.iter().map(|ext| format!("{base}{ext}")));
        }
    };
    match original {
        ".mjs" | ".mts" | ".d.mts" => {
            push(e.ts, &[".mts"]);
            push(e.declaration, &[".d.mts"]);
            push(e.js, &[".mjs"]);
        }
        ".cjs" | ".cts" | ".d.cts" => {
            push(e.ts, &[".cts"]);
            push(e.declaration, &[".d.cts"]);
            push(e.js, &[".cjs"]);
        }
        ".json" => {
            push(e.declaration, &[".d.json.ts"]);
            push(e.json, &[".json"]);
        }
        ".tsx" | ".jsx" => {
            push(e.ts, &[".tsx", ".ts"]);
            push(e.declaration, &[".d.ts"]);
            push(e.js, &[".jsx", ".js"]);
        }
        ".ts" | ".d.ts" | ".js" | "" => {
            push(e.ts, &[".ts", ".tsx"]);
            push(e.declaration, &[".d.ts"]);
            push(e.js, &[".js", ".jsx"]);
        }
        other => {
            let name = format!("{base}{other}");
            push(
                e.declaration && !declaration_name(&name),
                &[&format!(".d{other}.ts")],
            );
        }
    }
}

/// `loadModuleFromFileNoImplicitExtensions`: extension substitution only.
fn substituted(candidate: &str, e: Extensions, out: &mut Vec<String>) {
    if !base_name(candidate).contains('.') {
        return;
    }
    let known = [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ];
    let stem = known
        .iter()
        .find(|ext| extension_is(candidate, ext))
        .map_or_else(
            || &candidate[..candidate.rfind('.').unwrap_or(0)],
            |ext| &candidate[..candidate.len() - ext.len()],
        );
    adding(stem, &candidate[stem.len()..], e, out);
}

/// `loadModuleFromFile`: substitution, then appended implicit extensions.
fn from_file(candidate: &str, e: Extensions, out: &mut Vec<String>) {
    substituted(candidate, e, out);
    adding(candidate, "", e, out);
}

/// `loadFileNameFromPackageJsonField`: an explicit TypeScript surface is
/// probed exactly; other names only substitute their extension.
fn field_candidates(candidate: &str, e: Extensions, out: &mut Vec<String>) {
    let exact = e.ts
        && [".ts", ".tsx", ".mts", ".cts"]
            .iter()
            .any(|ext| extension_is(candidate, ext))
        || e.declaration
            && [".d.ts", ".d.cts", ".d.mts"]
                .iter()
                .any(|ext| extension_is(candidate, ext));
    if exact {
        out.push(candidate.to_owned());
    } else {
        substituted(candidate, e, out);
    }
}

fn child(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.to_owned()
    } else {
        format!("{directory}/{name}")
    }
}

/// The reference's `getEncodedRootLength` is nonzero.
fn rooted(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let bytes = path.as_bytes();
    path.starts_with('/')
        || bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'/')
        || path.contains("://")
}

/// Root-relative normalization of `relative` against `base`, as the
/// reference's `normalizePath`: separators unify, empty and `.` components
/// vanish, and `..` pops. `None` when the path leaves the run root or is
/// rooted. The flag records a trailing separator.
fn normalize(base: &str, relative: &str) -> Option<(String, bool)> {
    let relative = relative.replace('\\', "/");
    if rooted(&relative) {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some((parts.join("/"), relative.ends_with('/')))
}

/// Path components after the root, as `getPathComponents` returns them for
/// a path without a root: a trailing empty component is dropped.
fn components(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// JavaScript `String.prototype.replace(/\*/g, replacement)`, including the
/// `$` substitution patterns. `before` is `None` when the text preceding a
/// match is not known in root-relative form; `$\`` then fails.
fn replace_stars(text: &str, replacement: &str, before: Option<&str>) -> Option<String> {
    let mut out = String::new();
    let mut star = 0;
    for (index, piece) in text.split('*').enumerate() {
        if index > 0 {
            let mut chars = replacement.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '$' {
                    out.push(c);
                    continue;
                }
                match chars.peek() {
                    Some('$') => out.push('$'),
                    Some('&') => out.push('*'),
                    Some('\'') => out.push_str(&text[star + 1..]),
                    Some('`') => {
                        out.push_str(before?);
                        out.push_str(&text[..star]);
                    }
                    _ => {
                        out.push('$');
                        continue;
                    }
                }
                chars.next();
            }
        }
        out.push_str(piece);
        star += piece.len() + usize::from(index > 0);
    }
    Some(out)
}

impl Resolver {
    fn excluded(&self, path: &str) -> bool {
        let mut prefix = path;
        loop {
            if self.excluded.contains(prefix) {
                return true;
            }
            let Some((next, _)) = prefix.rsplit_once('/') else {
                return false;
            };
            prefix = next;
        }
    }

    /// The nearest `package.json` at or above `directory` inside the run root.
    fn scope(&mut self, directory: &str) -> Result<Scope, ModuleError> {
        let mut directory = directory.to_owned();
        loop {
            let path = if directory.is_empty() {
                "package.json".to_owned()
            } else {
                format!("{directory}/package.json")
            };
            self.work.charge(path.len() + 1)?;
            if self.excluded(&path) {
                return Ok(Scope::Unknown);
            }
            if self.files.contains_key(&path) {
                return Ok(match self.manifests.get(&path) {
                    Some(Some(manifest)) => Scope::Found(directory, manifest.clone()),
                    Some(None) | None => Scope::Unknown,
                });
            }
            if directory.is_empty() {
                return Ok(Scope::Absent);
            }
            directory = parent(&directory).to_owned();
        }
    }

    fn finish(step: Step) -> Option<Resolution> {
        match step {
            Step::Continue => None,
            Step::Null => Some(Resolution::Unresolved(U::MappedToNull)),
            Step::Found(result) => Some(result),
            Step::Stop(reason) => Some(Resolution::Unresolved(reason)),
        }
    }

    /// `loadModuleFromImports` for a `#` specifier. `None` continues the
    /// lookup, as an absent reference result does.
    pub(crate) fn package_imports(
        &mut self,
        directory: &str,
        specifier: &str,
        mode: ResolutionMode,
        depth: usize,
    ) -> Result<Option<Resolution>, ModuleError> {
        if specifier == "#" {
            return Ok(None);
        }
        let (scope, manifest) = match self.scope(directory)? {
            Scope::Found(scope, manifest) => (scope, manifest),
            Scope::Absent | Scope::Unknown => {
                return Ok(Some(Resolution::Unresolved(U::PackageManifest)));
            }
        };
        let Some(Target::Object(table)) = &manifest.imports else {
            return Ok(None);
        };
        let step = self.map(table, specifier, &scope, true, ALL, mode, depth)?;
        Ok(Self::finish(step))
    }

    /// `loadModuleFromSelfNameReference`: a bare specifier that starts with
    /// the scope's own package name resolves through its `exports`. Without
    /// `allowJs`, TypeScript and declaration targets are tried in a first
    /// pass. A scope above the run root is not visible and never matches.
    pub(crate) fn self_reference(
        &mut self,
        directory: &str,
        specifier: &str,
        mode: ResolutionMode,
    ) -> Result<Option<Resolution>, ModuleError> {
        let (scope, manifest) = match self.scope(directory)? {
            Scope::Found(scope, manifest) => (scope, manifest),
            Scope::Absent => return Ok(None),
            Scope::Unknown => return Ok(Some(Resolution::Unresolved(U::PackageManifest))),
        };
        let (Some(exports), Some(name)) = (&manifest.exports, &manifest.name) else {
            return Ok(None);
        };
        let name = name.replace('\\', "/");
        if !exports.truthy() || rooted(&name) {
            return Ok(None);
        }
        let (names, parts) = (components(&name), components(specifier));
        if names
            .iter()
            .enumerate()
            .any(|(i, p)| parts.get(i) != Some(p))
        {
            return Ok(None);
        }
        let trailing = &parts[names.len()..];
        let subpath = if trailing.is_empty() {
            ".".to_owned()
        } else {
            format!("./{}", trailing.join("/"))
        };
        let passes: &[Extensions] =
            if self.config.allows_js() && !format!("/{directory}").contains("/node_modules/") {
                &[ALL]
            } else {
                &[
                    Extensions {
                        ts: true,
                        js: false,
                        declaration: true,
                        json: false,
                    },
                    Extensions {
                        ts: false,
                        js: true,
                        declaration: false,
                        json: true,
                    },
                ]
            };
        for &extensions in passes {
            let step = self.exports(exports, &subpath, &scope, extensions, mode)?;
            if let Some(result) = Self::finish(step) {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }

    /// `loadModuleFromExports` for one subpath of a scope.
    fn exports(
        &mut self,
        exports: &Target,
        subpath: &str,
        scope: &str,
        extensions: Extensions,
        mode: ResolutionMode,
    ) -> Result<Step, ModuleError> {
        if subpath == "." {
            let main = match exports {
                Target::String(_) | Target::Array(_) => Some(exports),
                Target::Object(members) if !members.iter().any(|(k, _)| k.starts_with('.')) => {
                    Some(exports)
                }
                Target::Object(members) => members.iter().find(|(k, _)| k == ".").map(|(_, t)| t),
                Target::Null | Target::Scalar(_) => None,
            };
            return match main {
                Some(main) if main.truthy() => {
                    self.target(main, "", false, scope, false, extensions, mode, 0)
                }
                Some(_) | None => Ok(Step::Continue),
            };
        }
        match exports {
            Target::Object(table) if table.iter().all(|(k, _)| k.starts_with('.')) => {
                self.map(table, subpath, scope, false, extensions, mode, 0)
            }
            Target::Object(_)
            | Target::String(_)
            | Target::Array(_)
            | Target::Null
            | Target::Scalar(_) => Ok(Step::Continue),
        }
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors the reference's state")]
    fn map(
        &mut self,
        table: &[(String, Target)],
        name: &str,
        scope: &str,
        imports: bool,
        extensions: Extensions,
        mode: ResolutionMode,
        depth: usize,
    ) -> Result<Step, ModuleError> {
        for (key, _) in table {
            self.work.charge(key.len() + name.len() + 1)?;
        }
        match select(table, name) {
            None => Ok(Step::Continue),
            Some((target, subpath, pattern)) => self.target(
                target, &subpath, pattern, scope, imports, extensions, mode, depth,
            ),
        }
    }

    /// `loadModuleFromTargetExportOrImport`.
    #[expect(clippy::too_many_arguments, reason = "mirrors the reference's state")]
    fn target(
        &mut self,
        target: &Target,
        subpath: &str,
        pattern: bool,
        scope: &str,
        imports: bool,
        extensions: Extensions,
        mode: ResolutionMode,
        depth: usize,
    ) -> Result<Step, ModuleError> {
        self.work.charge(subpath.len() + 1)?;
        match target {
            Target::String(text) => self.string_target(
                text, subpath, pattern, scope, imports, extensions, mode, depth,
            ),
            Target::Null => Ok(Step::Null),
            Target::Scalar(_) => Ok(Step::Continue),
            Target::Array(items) => {
                for item in items {
                    let step = self.target(
                        item, subpath, pattern, scope, imports, extensions, mode, depth,
                    )?;
                    if !matches!(step, Step::Continue) {
                        return Ok(step);
                    }
                }
                Ok(Step::Continue)
            }
            Target::Object(conditions) => {
                for (condition, item) in conditions {
                    self.work.charge(condition.len() + 1)?;
                    let matched = match (condition.as_str(), mode) {
                        ("default" | "types", _)
                        | ("import", ResolutionMode::Import)
                        | ("require", ResolutionMode::Require) => true,
                        // TypeScript evaluates versioned conditions with its own
                        // range grammar, which this profile does not model.
                        (other, _) if other.starts_with("types@") => {
                            return Ok(Step::Stop(U::PackageImports));
                        }
                        _ => false,
                    };
                    if matched {
                        let step = self.target(
                            item, subpath, pattern, scope, imports, extensions, mode, depth,
                        )?;
                        if !matches!(step, Step::Continue) {
                            return Ok(step);
                        }
                    }
                }
                Ok(Step::Continue)
            }
        }
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors the reference's state")]
    fn string_target(
        &mut self,
        text: &str,
        subpath: &str,
        pattern: bool,
        scope: &str,
        imports: bool,
        extensions: Extensions,
        mode: ResolutionMode,
        depth: usize,
    ) -> Result<Step, ModuleError> {
        if !pattern && !subpath.is_empty() && !text.ends_with('/') {
            return Ok(Step::Continue);
        }
        if !text.starts_with("./") {
            if !imports || text.starts_with("../") || text.starts_with('/') || rooted(text) {
                return Ok(Step::Continue);
            }
            let Some(lookup) = (if pattern {
                replace_stars(text, subpath, Some(""))
            } else {
                Some(format!("{text}{subpath}"))
            }) else {
                return Ok(Step::Stop(U::PackageImports));
            };
            if depth >= MAX_TARGET_DEPTH {
                return Ok(Step::Stop(U::PackageImports));
            }
            // A bare import target restarts the whole lookup from the scope.
            return Ok(match self.resolve_from(scope, &lookup, mode, depth + 1)? {
                Resolution::Unresolved(U::NotFound) => Step::Continue,
                Resolution::Unresolved(reason) => Step::Stop(reason),
                found @ (Resolution::Local { .. }
                | Resolution::External { .. }
                | Resolution::NodeBuiltin) => Step::Found(found),
            });
        }
        let text = text.replace('\\', "/");
        let invalid = |part: &&str| matches!(*part, "." | ".." | "node_modules");
        if components(&text).iter().skip(1).any(invalid)
            || components(&subpath.replace('\\', "/")).iter().any(invalid)
        {
            return Ok(Step::Continue);
        }
        if self.config.output_directory {
            // Output directories map package targets back to inputs through
            // the reference's project-root inference, which is not modeled.
            return Ok(Step::Stop(U::Configuration));
        }
        let Some(joined) = (if pattern {
            replace_stars(&text, subpath, None)
        } else {
            Some(format!("{text}{subpath}"))
        }) else {
            return Ok(Step::Stop(U::PackageImports));
        };
        let Some((path, _)) = normalize(scope, &joined) else {
            return Ok(Step::Stop(U::OutsideRoot));
        };
        let mut candidates = Vec::new();
        field_candidates(&path, extensions, &mut candidates);
        for candidate in candidates {
            if let Some(result) = self.probe(&candidate)? {
                return Ok(match result {
                    Resolution::Unresolved(reason) => Step::Stop(reason),
                    found @ (Resolution::Local { .. }
                    | Resolution::External { .. }
                    | Resolution::NodeBuiltin) => Step::Found(found),
                });
            }
        }
        // Stylesheets and other assets are a fallback after their arbitrary-
        // extension declarations, as for relative requests.
        if let Some(Resolution::Local { path, kind }) = self.probe(&path)?
            && matches!(kind, FileKind::Stylesheet | FileKind::Asset)
        {
            return Ok(Step::Found(Resolution::Local { path, kind }));
        }
        Ok(Step::Continue)
    }

    /// `loadNodeModuleFromDirectoryWorker` for a local directory with an
    /// admitted manifest: `typings`, `types`, then `main`, then the index.
    pub(crate) fn directory_package(
        &mut self,
        directory: &str,
        manifest: &str,
        extensions: Extensions,
    ) -> Result<Option<Resolution>, ModuleError> {
        let manifest = match self.manifests.get(manifest) {
            None => return Ok(Some(Resolution::Unresolved(U::DirectoryPackage))),
            Some(None) => return Ok(Some(Resolution::Unresolved(U::PackageManifest))),
            Some(Some(manifest)) => manifest.clone(),
        };
        if manifest.types_versions {
            return Ok(Some(Resolution::Unresolved(U::DirectoryPackage)));
        }
        let mut candidates = Vec::new();
        let types = extensions
            .declaration
            .then(|| manifest.typings.as_ref().or(manifest.types.as_ref()))
            .flatten();
        if let Some(field) = types.or(manifest.main.as_ref()) {
            let Some((path, trailing)) = normalize(directory, field) else {
                return Ok(Some(Resolution::Unresolved(U::OutsideRoot)));
            };
            // File candidates of the run root itself would lie above it.
            if path.is_empty() && !trailing {
                return Ok(Some(Resolution::Unresolved(U::OutsideRoot)));
            }
            let candidate = if trailing {
                format!("{path}/")
            } else {
                path.clone()
            };
            field_candidates(&candidate, extensions, &mut candidates);
            if !trailing {
                from_file(&path, extensions, &mut candidates);
            }
            from_file(&child(&path, "index"), extensions, &mut candidates);
        }
        from_file(&child(directory, "index"), extensions, &mut candidates);
        for candidate in candidates {
            if let Some(result) = self.probe(&candidate)? {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(keys: &[&str]) -> Vec<(String, Target)> {
        keys.iter()
            .map(|k| ((*k).to_owned(), Target::String((*k).to_owned())))
            .collect()
    }

    #[test]
    fn keys_follow_reference_priority() {
        let table = object(&["#a/*", "#a/b*", "#a/b/*.js", "#exact", "#dir/"]);
        let key = |name| match select(&table, name) {
            Some((Target::String(key), subpath, pattern)) => Some((key.clone(), subpath, pattern)),
            _ => None,
        };
        assert_eq!(key("#exact"), Some(("#exact".into(), String::new(), false)));
        assert_eq!(
            key("#a/b/c.js"),
            Some(("#a/b/*.js".into(), "c".into(), true))
        );
        assert_eq!(key("#a/bc"), Some(("#a/b*".into(), "c".into(), true)));
        assert_eq!(key("#a/x"), Some(("#a/*".into(), "x".into(), true)));
        assert_eq!(key("#dir/x/y"), Some(("#dir/".into(), "x/y".into(), false)));
        assert_eq!(key("#other"), None);
        // A name shorter than a trailer key yields the overlapping text.
        let table = object(&["#ab*b"]);
        assert_eq!(
            select(&table, "#ab").map(|(_, subpath, _)| subpath),
            Some("b".into())
        );
    }

    #[test]
    fn array_indices_precede_other_keys() {
        assert_eq!(array_index("0"), Some(0));
        assert_eq!(array_index("4294967294"), Some(4_294_967_294));
        for key in ["", "01", "4294967295", "-1", "1.0", "a"] {
            assert_eq!(array_index(key), None, "{key}");
        }
    }
}
