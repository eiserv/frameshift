//! The `package.json` model (`D-069`).
//!
//! The model interprets a parsed JSON document. Every modeled value keeps
//! its exact span, so later stages can locate findings and edit the source.
//! Members the model does not interpret are listed with their spans. A
//! member whose key occurs again later in the same object is skipped, as
//! `JSON.parse` keeps the last occurrence; the loader reports duplicates.

use std::fmt;

use frameshift_core::Span;
use frameshift_syntax::{JsonDocument, JsonId, JsonObject, JsonValue};

use crate::spec::DependencySpec;
use crate::version::Version;

/// A modeled `package.json` field, named as findings name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ManifestField {
    /// `name`
    Name,
    /// `version`
    Version,
    /// `private`
    Private,
    /// `type`
    Type,
    /// `packageManager`
    PackageManager,
    /// `engines`
    Engines,
    /// `workspaces`
    Workspaces,
    /// `scripts`
    Scripts,
    /// `dependencies`
    Dependencies,
    /// `devDependencies`
    DevDependencies,
    /// `peerDependencies`
    PeerDependencies,
    /// `optionalDependencies`
    OptionalDependencies,
    /// `peerDependenciesMeta`
    PeerDependenciesMeta,
    /// `overrides`
    Overrides,
    /// `resolutions`
    Resolutions,
    /// `pnpm`, whose `overrides` member is modeled
    Pnpm,
    /// `exports`
    Exports,
    /// `main`
    Main,
    /// `module`
    Module,
    /// `browser`
    Browser,
    /// `bin`
    Bin,
    /// `files`
    Files,
    /// `browserslist`
    Browserslist,
}

impl ManifestField {
    /// Every modeled field, in the order of [`ManifestField::key`].
    pub const ALL: [Self; 23] = [
        Self::Name,
        Self::Version,
        Self::Private,
        Self::Type,
        Self::PackageManager,
        Self::Engines,
        Self::Workspaces,
        Self::Scripts,
        Self::Dependencies,
        Self::DevDependencies,
        Self::PeerDependencies,
        Self::OptionalDependencies,
        Self::PeerDependenciesMeta,
        Self::Overrides,
        Self::Resolutions,
        Self::Pnpm,
        Self::Exports,
        Self::Main,
        Self::Module,
        Self::Browser,
        Self::Bin,
        Self::Files,
        Self::Browserslist,
    ];

    /// Returns the member key.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Version => "version",
            Self::Private => "private",
            Self::Type => "type",
            Self::PackageManager => "packageManager",
            Self::Engines => "engines",
            Self::Workspaces => "workspaces",
            Self::Scripts => "scripts",
            Self::Dependencies => "dependencies",
            Self::DevDependencies => "devDependencies",
            Self::PeerDependencies => "peerDependencies",
            Self::OptionalDependencies => "optionalDependencies",
            Self::PeerDependenciesMeta => "peerDependenciesMeta",
            Self::Overrides => "overrides",
            Self::Resolutions => "resolutions",
            Self::Pnpm => "pnpm",
            Self::Exports => "exports",
            Self::Main => "main",
            Self::Module => "module",
            Self::Browser => "browser",
            Self::Bin => "bin",
            Self::Files => "files",
            Self::Browserslist => "browserslist",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.key() == key)
    }
}

impl fmt::Display for ManifestField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// What is wrong with a modeled field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    /// The value or one of its entries has an unexpected JSON type; it is
    /// not used.
    UnexpectedType,
    /// The value or one of its entries has the right type but an invalid
    /// value, such as an unknown `type` or an invalid dependency
    /// specifier; it is not used.
    InvalidValue,
}

/// A field problem with the span of the offending value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestIssue {
    /// The top-level field that contains the problem.
    pub field: ManifestField,
    /// What is wrong.
    pub kind: IssueKind,
    /// The span of the offending value or entry.
    pub span: Span,
}

/// A rejected manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The document is not a JSON object.
    NotAnObject {
        /// The span of the top-level value.
        span: Span,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject { .. } => f.write_str("package.json is not a JSON object"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// A top-level value with the spans of its key and value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field<T> {
    /// The span of the key, including quotes.
    pub key_span: Span,
    /// The span of the value.
    pub span: Span,
    /// The interpreted value.
    pub value: T,
}

/// A member of a modeled object with the spans of its key and value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry<T> {
    /// The decoded key.
    pub key: Box<str>,
    /// The span of the key, including quotes.
    pub key_span: Span,
    /// The span of the value.
    pub span: Span,
    /// The interpreted value.
    pub value: T,
}

/// An array element with its span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element<T> {
    /// The span of the element.
    pub span: Span,
    /// The interpreted value.
    pub value: T,
}

/// A dependency entry: the package name and its specifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// The decoded specifier text.
    pub text: Box<str>,
    /// The classified specifier.
    pub spec: DependencySpec,
}

/// The `type` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleType {
    /// `"module"`: `.js` files are ES modules.
    Module,
    /// `"commonjs"`: `.js` files are CommonJS.
    CommonJs,
}

/// An `overrides` value of npm: a specifier or nested overrides, where the
/// key `.` overrides the package itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Override {
    /// A specifier text, possibly a `$name` reference.
    Spec(Box<str>),
    /// Overrides below a package.
    Nested(Vec<Entry<Override>>),
}

/// An `exports` value, following Node.js's package entry points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exports {
    /// A target path.
    Target(Box<str>),
    /// `null`: the subpath is not exported.
    Null,
    /// Fallback targets in order.
    Array(Vec<Element<Exports>>),
    /// An object whose keys are conditions such as `import`, `default`.
    Conditions(Vec<Entry<Exports>>),
    /// An object whose keys are subpaths, all starting with `.`.
    Subpaths(Vec<Entry<Exports>>),
}

/// A `browser` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Browser {
    /// A replacement for the main entry.
    Path(Box<str>),
    /// Replacements by module or path.
    Map(Vec<Entry<BrowserTarget>>),
}

/// A `browser` replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserTarget {
    /// The replacement path or module.
    Path(Box<str>),
    /// `false`: the module is replaced by an empty module.
    Disabled,
}

/// A `bin` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bin {
    /// One executable named after the package.
    Path(Box<str>),
    /// Executables by name.
    Map(Vec<Entry<Box<str>>>),
}

/// A `browserslist` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Browserslist {
    /// One query.
    Query(Box<str>),
    /// Queries.
    Queries(Vec<Element<Box<str>>>),
    /// Queries by environment; a single string is one query.
    Environments(Vec<Entry<Vec<Element<Box<str>>>>>),
}

/// An uninterpreted top-level member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OtherMember {
    /// The decoded key.
    pub key: Box<str>,
    /// The span of the key, including quotes.
    pub key_span: Span,
    /// The span of the value.
    pub span: Span,
}

/// A `package.json` document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageManifest {
    /// `name`.
    pub name: Option<Field<Box<str>>>,
    /// `version`, as written; see [`PackageManifest::parsed_version`].
    pub version: Option<Field<Box<str>>>,
    /// `private`.
    pub private: Option<Field<bool>>,
    /// `type`.
    pub module_type: Option<Field<ModuleType>>,
    /// `packageManager`, as written; the package manager module parses it.
    pub package_manager: Option<Field<Box<str>>>,
    /// `engines`, with range texts.
    pub engines: Vec<Entry<Box<str>>>,
    /// `workspaces` patterns, from an array or Yarn's `packages` member.
    pub workspaces: Option<Field<Vec<Element<Box<str>>>>>,
    /// `scripts`.
    pub scripts: Vec<Entry<Box<str>>>,
    /// `dependencies`.
    pub dependencies: Vec<Entry<Dependency>>,
    /// `devDependencies`.
    pub dev_dependencies: Vec<Entry<Dependency>>,
    /// `peerDependencies`.
    pub peer_dependencies: Vec<Entry<Dependency>>,
    /// `optionalDependencies`.
    pub optional_dependencies: Vec<Entry<Dependency>>,
    /// `peerDependenciesMeta`, with each entry's `optional` flag.
    pub optional_peers: Vec<Entry<bool>>,
    /// npm's `overrides`.
    pub overrides: Vec<Entry<Override>>,
    /// Yarn's `resolutions`.
    pub resolutions: Vec<Entry<Box<str>>>,
    /// pnpm's `pnpm.overrides`.
    pub pnpm_overrides: Vec<Entry<Box<str>>>,
    /// `exports`.
    pub exports: Option<Field<Exports>>,
    /// `main`.
    pub main: Option<Field<Box<str>>>,
    /// `module`.
    pub module: Option<Field<Box<str>>>,
    /// `browser`.
    pub browser: Option<Field<Browser>>,
    /// `bin`.
    pub bin: Option<Field<Bin>>,
    /// `files`.
    pub files: Option<Field<Vec<Element<Box<str>>>>>,
    /// `browserslist`.
    pub browserslist: Option<Field<Browserslist>>,
    /// Every other top-level member, in source order.
    pub other: Vec<OtherMember>,
    /// Problems with modeled fields, in source order.
    pub issues: Vec<ManifestIssue>,
}

impl PackageManifest {
    /// Interprets a parsed `package.json` document.
    pub fn from_json(document: &JsonDocument<'_>) -> Result<Self, ManifestError> {
        let root = document.root();
        let Some(JsonValue::Object(object)) = document.value(root) else {
            return Err(ManifestError::NotAnObject {
                span: span(document, root),
            });
        };
        let mut reader = Reader {
            document,
            field: ManifestField::Name,
            issues: Vec::new(),
        };
        let mut manifest = Self::default();
        for (key, key_span, value) in members(object) {
            let Some(field) = ManifestField::from_key(key) else {
                manifest.other.push(OtherMember {
                    key: key.into(),
                    key_span,
                    span: span(document, value),
                });
                continue;
            };
            reader.field = field;
            match field {
                ManifestField::Name => {
                    manifest.name = reader
                        .string(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Version => {
                    manifest.version = reader
                        .string(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Private => {
                    manifest.private = reader
                        .boolean(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Type => {
                    manifest.module_type = reader
                        .module_type(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::PackageManager => {
                    manifest.package_manager = reader
                        .string(value)
                        .map(wrap_with(key_span, document, value));
                }
                ManifestField::Engines => manifest.engines = reader.string_map(value),
                ManifestField::Workspaces => {
                    manifest.workspaces = reader
                        .workspaces(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Scripts => manifest.scripts = reader.string_map(value),
                ManifestField::Dependencies => manifest.dependencies = reader.dependencies(value),
                ManifestField::DevDependencies => {
                    manifest.dev_dependencies = reader.dependencies(value)
                }
                ManifestField::PeerDependencies => {
                    manifest.peer_dependencies = reader.dependencies(value)
                }
                ManifestField::OptionalDependencies => {
                    manifest.optional_dependencies = reader.dependencies(value);
                }
                ManifestField::PeerDependenciesMeta => {
                    manifest.optional_peers = reader.optional_peers(value)
                }
                ManifestField::Overrides => {
                    manifest.overrides = reader.overrides(value).unwrap_or_default()
                }
                ManifestField::Resolutions => manifest.resolutions = reader.string_map(value),
                ManifestField::Pnpm => manifest.pnpm_overrides = reader.pnpm_overrides(value),
                ManifestField::Exports => {
                    manifest.exports = reader
                        .exports(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Main => {
                    manifest.main = reader
                        .string(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Module => {
                    manifest.module = reader
                        .string(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Browser => {
                    manifest.browser = reader
                        .browser(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Bin => {
                    manifest.bin = reader.bin(value).map(wrap_with(key_span, document, value))
                }
                ManifestField::Files => {
                    manifest.files = reader
                        .string_array(value)
                        .map(wrap_with(key_span, document, value))
                }
                ManifestField::Browserslist => {
                    manifest.browserslist = reader
                        .browserslist(value)
                        .map(wrap_with(key_span, document, value));
                }
            }
        }
        manifest.issues = reader.issues;
        Ok(manifest)
    }

    /// Returns `version` as an exact SemVer version, if it is one.
    pub fn parsed_version(&self) -> Option<Version> {
        self.version
            .as_ref()
            .and_then(|field| Version::parse(&field.value).ok())
    }

    /// Returns the dependency entries of one kind.
    pub fn dependencies_of(&self, kind: DependencyKind) -> &[Entry<Dependency>] {
        match kind {
            DependencyKind::Production => &self.dependencies,
            DependencyKind::Development => &self.dev_dependencies,
            DependencyKind::Peer => &self.peer_dependencies,
            DependencyKind::Optional => &self.optional_dependencies,
        }
    }
}

/// The four dependency maps of a manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DependencyKind {
    /// `dependencies`
    Production,
    /// `devDependencies`
    Development,
    /// `peerDependencies`
    Peer,
    /// `optionalDependencies`
    Optional,
}

impl DependencyKind {
    /// Every kind, in manifest order.
    pub const ALL: [Self; 4] = [
        Self::Production,
        Self::Development,
        Self::Peer,
        Self::Optional,
    ];

    /// Returns the member key.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Production => "dependencies",
            Self::Development => "devDependencies",
            Self::Peer => "peerDependencies",
            Self::Optional => "optionalDependencies",
        }
    }
}

fn wrap_with<T>(
    key_span: Span,
    document: &JsonDocument<'_>,
    value: JsonId,
) -> impl FnOnce(T) -> Field<T> {
    let span = span(document, value);
    move |value| Field {
        key_span,
        span,
        value,
    }
}

pub(crate) fn span(document: &JsonDocument<'_>, id: JsonId) -> Span {
    document.span(id).unwrap_or_default()
}

/// The effective members of an object: every key's last occurrence, in
/// source order.
pub(crate) fn members<'a>(
    object: JsonObject<'a>,
) -> impl Iterator<Item = (&'a str, Span, JsonId)> + 'a {
    object
        .members()
        .filter(|member| !member.is_shadowed())
        .map(|member| (member.key(), member.key_span(), member.value()))
}

struct Reader<'d, 's> {
    document: &'d JsonDocument<'s>,
    field: ManifestField,
    issues: Vec<ManifestIssue>,
}

impl<'d> Reader<'d, '_> {
    fn issue(&mut self, kind: IssueKind, id: JsonId) {
        self.issues.push(ManifestIssue {
            field: self.field,
            kind,
            span: span(self.document, id),
        });
    }

    fn value(&self, id: JsonId) -> Option<JsonValue<'d>> {
        self.document.value(id)
    }

    fn string(&mut self, id: JsonId) -> Option<Box<str>> {
        if let Some(JsonValue::String(text)) = self.value(id) {
            return Some(text.into());
        }
        self.issue(IssueKind::UnexpectedType, id);
        None
    }

    fn boolean(&mut self, id: JsonId) -> Option<bool> {
        if let Some(JsonValue::Bool(value)) = self.value(id) {
            return Some(value);
        }
        self.issue(IssueKind::UnexpectedType, id);
        None
    }

    fn object(&mut self, id: JsonId) -> Option<JsonObject<'d>> {
        if let Some(JsonValue::Object(object)) = self.value(id) {
            return Some(object);
        }
        self.issue(IssueKind::UnexpectedType, id);
        None
    }

    fn entries<T>(
        &mut self,
        id: JsonId,
        mut read: impl FnMut(&mut Self, JsonId) -> Option<T>,
    ) -> Vec<Entry<T>> {
        let Some(object) = self.object(id) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for (key, key_span, value) in members(object) {
            if let Some(read) = read(self, value) {
                entries.push(Entry {
                    key: key.into(),
                    key_span,
                    span: span(self.document, value),
                    value: read,
                });
            }
        }
        entries
    }

    fn string_map(&mut self, id: JsonId) -> Vec<Entry<Box<str>>> {
        self.entries(id, Self::string)
    }

    fn string_array(&mut self, id: JsonId) -> Option<Vec<Element<Box<str>>>> {
        let Some(JsonValue::Array(array)) = self.value(id) else {
            self.issue(IssueKind::UnexpectedType, id);
            return None;
        };
        let mut elements = Vec::new();
        for element in array.iter() {
            if let Some(value) = self.string(element) {
                elements.push(Element {
                    span: span(self.document, element),
                    value,
                });
            }
        }
        Some(elements)
    }

    fn module_type(&mut self, id: JsonId) -> Option<ModuleType> {
        match self.string(id)?.as_ref() {
            "module" => Some(ModuleType::Module),
            "commonjs" => Some(ModuleType::CommonJs),
            _ => {
                self.issue(IssueKind::InvalidValue, id);
                None
            }
        }
    }

    fn workspaces(&mut self, id: JsonId) -> Option<Vec<Element<Box<str>>>> {
        if let Some(JsonValue::Object(object)) = self.value(id) {
            // Yarn's form: `{ "packages": [...], "nohoist": [...] }`.
            return match object.get("packages") {
                Some(packages) => self.string_array(packages),
                None => Some(Vec::new()),
            };
        }
        self.string_array(id)
    }

    fn dependencies(&mut self, id: JsonId) -> Vec<Entry<Dependency>> {
        self.entries(id, |reader, value| {
            let text = reader.string(value)?;
            let spec = DependencySpec::parse(&text);
            if spec == DependencySpec::Invalid {
                reader.issue(IssueKind::InvalidValue, value);
                return None;
            }
            Some(Dependency { text, spec })
        })
    }

    fn optional_peers(&mut self, id: JsonId) -> Vec<Entry<bool>> {
        self.entries(id, |reader, value| {
            let object = reader.object(value)?;
            match object.get("optional") {
                Some(optional) => reader.boolean(optional),
                None => Some(false),
            }
        })
    }

    fn overrides(&mut self, id: JsonId) -> Option<Vec<Entry<Override>>> {
        let entries = self.entries(id, |reader, value| match reader.value(value) {
            Some(JsonValue::String(text)) => Some(Override::Spec(text.into())),
            Some(JsonValue::Object(_)) => reader.overrides(value).map(Override::Nested),
            _ => {
                reader.issue(IssueKind::UnexpectedType, value);
                None
            }
        });
        Some(entries)
    }

    fn pnpm_overrides(&mut self, id: JsonId) -> Vec<Entry<Box<str>>> {
        match self.object(id).and_then(|object| object.get("overrides")) {
            Some(overrides) => self.string_map(overrides),
            None => Vec::new(),
        }
    }

    fn exports(&mut self, id: JsonId) -> Option<Exports> {
        match self.value(id) {
            Some(JsonValue::String(text)) => Some(Exports::Target(text.into())),
            Some(JsonValue::Null) => Some(Exports::Null),
            Some(JsonValue::Array(array)) => {
                let mut elements = Vec::new();
                for element in array.iter() {
                    if let Some(value) = self.exports(element) {
                        elements.push(Element {
                            span: span(self.document, element),
                            value,
                        });
                    }
                }
                Some(Exports::Array(elements))
            }
            Some(JsonValue::Object(object)) => {
                let subpaths = members(object)
                    .filter(|(key, _, _)| key.starts_with('.'))
                    .count();
                if subpaths != 0 && subpaths != members(object).count() {
                    self.issue(IssueKind::InvalidValue, id);
                    return None;
                }
                let entries = self.entries(id, Self::exports);
                Some(if subpaths == 0 {
                    Exports::Conditions(entries)
                } else {
                    Exports::Subpaths(entries)
                })
            }
            Some(JsonValue::Bool(_) | JsonValue::Number(_)) | None => {
                self.issue(IssueKind::UnexpectedType, id);
                None
            }
        }
    }

    fn browser(&mut self, id: JsonId) -> Option<Browser> {
        if let Some(JsonValue::String(text)) = self.value(id) {
            return Some(Browser::Path(text.into()));
        }
        let entries = self.entries(id, |reader, value| match reader.value(value) {
            Some(JsonValue::String(text)) => Some(BrowserTarget::Path(text.into())),
            Some(JsonValue::Bool(false)) => Some(BrowserTarget::Disabled),
            _ => {
                reader.issue(IssueKind::UnexpectedType, value);
                None
            }
        });
        matches!(self.value(id), Some(JsonValue::Object(_))).then_some(Browser::Map(entries))
    }

    fn bin(&mut self, id: JsonId) -> Option<Bin> {
        if let Some(JsonValue::String(text)) = self.value(id) {
            return Some(Bin::Path(text.into()));
        }
        let entries = self.string_map(id);
        matches!(self.value(id), Some(JsonValue::Object(_))).then_some(Bin::Map(entries))
    }

    fn browserslist(&mut self, id: JsonId) -> Option<Browserslist> {
        match self.value(id) {
            Some(JsonValue::String(text)) => Some(Browserslist::Query(text.into())),
            Some(JsonValue::Array(_)) => self.string_array(id).map(Browserslist::Queries),
            Some(JsonValue::Object(_)) => Some(Browserslist::Environments(self.entries(
                id,
                |reader, value| {
                    if let Some(JsonValue::String(text)) = reader.value(value) {
                        return Some(vec![Element {
                            span: span(reader.document, value),
                            value: text.into(),
                        }]);
                    }
                    reader.string_array(value)
                },
            ))),
            _ => {
                self.issue(IssueKind::UnexpectedType, id);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;
