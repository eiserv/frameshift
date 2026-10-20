//! Yarn classic lockfiles, `yarn.lock` format version 1 (`D-072`).
//!
//! An entry lists the patterns, `<name>@<range>`, that resolve to it.
//! Yarn 1 records no workspaces: a dependency is found by its pattern, so
//! resolving a workspace's declaration needs the declared range.

use std::collections::BTreeMap;

use frameshift_core::Span;
use frameshift_syntax::{YarnDocument, YarnId, YarnObject, YarnValue};

use super::{
    Declared, LockedPackage, LockfileError, LockfileErrorKind, Resolution, is_installed_name,
};
use crate::spec::DependencySpec;
use crate::version::Version;

/// Patterns to the key of the entry that they resolve to.
pub(super) type Patterns = BTreeMap<Box<str>, Box<str>>;

pub(super) struct Read {
    pub packages: Vec<LockedPackage>,
    pub patterns: Patterns,
}

/// Whether a `yarn.lock` is a classic lockfile: its leading `#` comment
/// lines, after an optional byte order mark, include the line
/// `# yarn lockfile v1`. This is the test of Yarn Berry's lockfile reader,
/// whose expression `^(#.*(\r?\n))*?#\s+yarn\s+lockfile\s+v1\r?\n`, matched
/// case-insensitively, takes whitespace including line terminators for
/// `\s` and stops `.` at line terminators.
pub(crate) fn has_classic_header(text: &str) -> bool {
    let mut rest = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    loop {
        if header(rest) {
            return true;
        }
        let Some(line) = rest.strip_prefix('#') else {
            return false;
        };
        let end = line.find(is_line_terminator).unwrap_or(line.len());
        let after = &line[end..];
        match after
            .strip_prefix("\r\n")
            .or_else(|| after.strip_prefix('\n'))
        {
            Some(next) => rest = next,
            None => return false,
        }
    }
}

/// Matches `#\s+yarn\s+lockfile\s+v1\r?\n` case-insensitively at the start.
fn header(text: &str) -> bool {
    let Some(mut rest) = text.strip_prefix('#') else {
        return false;
    };
    for word in ["yarn", "lockfile", "v1"] {
        let trimmed = rest.trim_start_matches(is_js_space);
        if trimmed.len() == rest.len()
            || !trimmed
                .get(..word.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(word))
        {
            return false;
        }
        rest = &trimmed[word.len()..];
    }
    rest.starts_with('\n') || rest.starts_with("\r\n")
}

fn is_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// JavaScript's `\s`: white space and line terminators.
fn is_js_space(ch: char) -> bool {
    is_line_terminator(ch)
        || matches!(
            ch,
            '\t' | '\u{B}' | '\u{C}' | ' ' | '\u{A0}' | '\u{FEFF}' | '\u{1680}' | '\u{2000}'
                ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        )
}

/// Splits a pattern as Yarn's `normalizePattern` does: the name ends at the
/// first `@` after an optional leading `@`. A pattern without a range
/// requests `latest`.
pub(super) fn split_pattern(pattern: &str) -> (&str, Option<&str>) {
    let search = usize::from(pattern.starts_with('@'));
    match pattern[search..].find('@') {
        Some(at) => (&pattern[..search + at], Some(&pattern[search + at + 1..])),
        None => (pattern, None),
    }
}

fn structure(span: Span) -> LockfileError {
    LockfileError {
        kind: LockfileErrorKind::Structure,
        span,
    }
}

struct Reader<'a, 's> {
    document: &'a YarnDocument<'s>,
}

impl<'a> Reader<'a, '_> {
    fn span(&self, id: YarnId) -> Span {
        self.document.span(id).unwrap_or_default()
    }

    fn object(&self, id: YarnId) -> Result<YarnObject<'a>, LockfileError> {
        self.document
            .value(id)
            .and_then(YarnValue::as_object)
            .ok_or_else(|| structure(self.span(id)))
    }

    fn string(&self, id: YarnId) -> Result<&'a str, LockfileError> {
        self.document
            .value(id)
            .and_then(YarnValue::as_str)
            .ok_or_else(|| structure(self.span(id)))
    }

    fn optional_string(
        &self,
        object: YarnObject<'a>,
        field: &str,
    ) -> Result<Option<&'a str>, LockfileError> {
        object.get(field).map(|id| self.string(id)).transpose()
    }

    /// Reads a map of dependency names to ranges.
    fn declared(
        &self,
        object: YarnObject<'a>,
        field: &str,
        into: &mut BTreeMap<Box<str>, Box<str>>,
    ) -> Result<(), LockfileError> {
        let Some(id) = object.get(field) else {
            return Ok(());
        };
        for member in self.object(id)?.members() {
            let range = self.string(member.value())?;
            for key in member.keys() {
                if !is_installed_name(key.text()) {
                    return Err(structure(key.span()));
                }
                into.insert(key.text().into(), range.into());
            }
        }
        Ok(())
    }
}

/// Whether a pattern's range requests a registry package: a range or a tag,
/// also behind an `npm:` alias.
fn registry_range(range: Option<&str>) -> bool {
    let Some(range) = range else {
        return true;
    };
    match DependencySpec::parse(range) {
        DependencySpec::Range(_) | DependencySpec::Tag(_) => true,
        DependencySpec::Alias { target, .. } => {
            matches!(*target, DependencySpec::Range(_) | DependencySpec::Tag(_))
        }
        DependencySpec::Workspace(_)
        | DependencySpec::Catalog(_)
        | DependencySpec::Local
        | DependencySpec::Git
        | DependencySpec::Url
        | DependencySpec::OtherProtocol
        | DependencySpec::Invalid => false,
    }
}

pub(super) fn read(document: &YarnDocument<'_>) -> Result<Read, LockfileError> {
    // Yarn uses the last occurrence of a duplicate key; the lockfile is
    // ambiguous, so it is rejected.
    if let Some(key) = document.duplicate_keys().next() {
        return Err(structure(key.span()));
    }
    let reader = Reader { document };
    let root = reader.object(document.root())?;
    let mut packages = Vec::new();
    let mut patterns = Patterns::new();
    for member in root.members() {
        let entry = reader.object(member.value())?;
        let mut name = None;
        let mut alias = None;
        let mut registry = true;
        let mut first_range = None;
        let mut keys = Vec::new();
        for key in member.keys() {
            let (pattern_name, range) = split_pattern(key.text());
            if !is_installed_name(pattern_name) || name.is_some_and(|name| name != pattern_name) {
                return Err(structure(key.span()));
            }
            name = Some(pattern_name);
            // Every pattern of one entry must agree on what it installs.
            let target = match range.map(DependencySpec::parse) {
                Some(DependencySpec::Alias { name, .. }) => Some(name),
                Some(_) | None => None,
            };
            if keys.is_empty() {
                alias = Some(target);
                first_range = range;
            } else if alias.as_ref() != Some(&target) {
                return Err(structure(key.span()));
            }
            registry &= registry_range(range);
            keys.push(key.text());
        }
        let key: Box<str> = keys.join(", ").into();
        let version = reader.string(
            entry
                .get("version")
                .ok_or_else(|| structure(reader.span(member.value())))?,
        )?;
        let resolved = reader.optional_string(entry, "resolved")?;
        for field in ["integrity", "uid", "registry"] {
            reader.optional_string(entry, field)?;
        }
        for field in ["permissions", "prebuiltVariants"] {
            if let Some(id) = entry.get(field) {
                reader.object(id)?;
            }
        }
        let installed: Box<str> = match (reader.optional_string(entry, "name")?, alias.flatten()) {
            (Some(recorded), _) => recorded.into(),
            (None, Some(target)) => target,
            (None, None) => name.unwrap_or_default().into(),
        };
        if !is_installed_name(&installed) {
            return Err(structure(reader.span(member.value())));
        }
        registry &=
            resolved.is_none_or(|url| url.starts_with("https://") || url.starts_with("http://"));
        let resolution = match Version::parse(version) {
            Ok(version) if registry => Resolution::Registry(version),
            Ok(_) | Err(_) => Resolution::Other(resolved.or(first_range).unwrap_or(version).into()),
        };
        // Optional dependencies take precedence, as in the other models.
        let mut dependencies = BTreeMap::new();
        reader.declared(entry, "dependencies", &mut dependencies)?;
        reader.declared(entry, "optionalDependencies", &mut dependencies)?;
        for pattern in &keys {
            patterns.insert((*pattern).into(), key.clone());
        }
        packages.push(LockedPackage {
            key,
            span: reader.span(member.value()),
            name: installed,
            resolution,
            dependencies: dependencies
                .into_iter()
                .map(|(name, spec)| Declared { name, spec })
                .collect(),
            peer_dependencies: Vec::new(),
            optional_peers: Vec::new(),
            dev: false,
            optional: false,
        });
    }
    packages.sort_by(|left, right| left.key.cmp(&right.key));
    // Every dependency of an entry must have its own entry, or Yarn would
    // resolve it anew.
    for package in &packages {
        for dependency in &package.dependencies {
            let pattern = format!("{}@{}", dependency.name, dependency.spec);
            if !patterns.contains_key(pattern.as_str()) {
                return Err(structure(package.span));
            }
        }
    }
    Ok(Read { packages, patterns })
}

/// The pattern that Yarn 1 looks up for a declaration: the bare name if the
/// lockfile has it, as `yarn add` once wrote it, and `<name>@<range>`
/// otherwise.
pub(super) fn find<'a>(patterns: &'a Patterns, name: &str, spec: &str) -> Option<&'a str> {
    patterns
        .get(name)
        .or_else(|| patterns.get(format!("{name}@{spec}").as_str()))
        .map(AsRef::as_ref)
}
