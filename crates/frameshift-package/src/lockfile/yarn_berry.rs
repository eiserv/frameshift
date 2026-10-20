//! Yarn Berry lockfiles: `yarn.lock` as YAML with `__metadata`, format
//! versions 4, 5, 6, 8, 9, and 10 of stable Yarn 2 to 4 (`D-072`).
//!
//! Yarn reads the file with js-yaml's failsafe schema, so every scalar is a
//! string. An entry key lists the descriptors, `<name>@<range>`, that
//! resolve to the entry's `resolution` locator. Yarn normalizes a range
//! that is a valid semver range or a tag to `npm:<range>` and binds a local
//! path to the locator of the package that declares it, so lookups apply
//! both steps. Formats up to 6 may omit the `npm:` protocol in keys and
//! dependencies; they are normalized as well. Descriptors that the root's
//! `resolutions` or a catalog replace have no entry of their own, so a
//! lookup without the manifest and configuration finds nothing for them.

use std::collections::{BTreeMap, BTreeSet};

use frameshift_core::Span;
use frameshift_syntax::{YamlId, YamlScalarStyle, YamlStream, YamlValue};

use super::{
    Declared, LockedPackage, LockedWorkspace, LockfileError, LockfileErrorKind, Resolution,
};
use crate::DependencyKind;
use crate::range::VersionRange;
use crate::version::Version;

/// The format versions that stable Yarn releases write: 4 (2.x and 3.0),
/// 5 (3.1), 6 (3.2 to 3.8), 8 (4.0 to 4.13), 9 (4.14), and 10 (4.15 and
/// later). Version 7 appeared only in release candidates.
pub(super) const VERSIONS: [u64; 6] = [4, 5, 6, 8, 9, 10];

/// The version marker Yarn records for every workspace.
const WORKSPACE_VERSION: &str = "0.0.0-use.local";

/// Lookup tables of one Berry lockfile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Index {
    /// Normalized descriptors to the key of the entry they resolve to.
    pub descriptors: BTreeMap<Box<str>, Box<str>>,
    /// Workspace paths, `""` for the root, to their locators.
    pub workspaces: BTreeMap<Box<str>, Box<str>>,
}

pub(super) struct Read {
    pub version: u64,
    pub workspaces: Vec<LockedWorkspace>,
    pub packages: Vec<LockedPackage>,
    pub index: Index,
}

type Map = BTreeMap<Box<str>, YamlId>;

struct Reader<'a> {
    stream: &'a YamlStream,
    maps: BTreeMap<YamlId, Map>,
}

impl<'a> Reader<'a> {
    fn span(&self, id: YamlId) -> Span {
        self.stream
            .node(id)
            .map(|node| node.span)
            .unwrap_or_default()
    }

    fn error(&self, id: YamlId) -> LockfileError {
        LockfileError {
            kind: LockfileErrorKind::Structure,
            span: self.span(id),
        }
    }

    fn target(&self, id: YamlId) -> YamlId {
        match self.stream.node(id).map(|node| &node.value) {
            Some(YamlValue::Alias(target)) => *target,
            Some(
                YamlValue::Scalar { .. } | YamlValue::Mapping { .. } | YamlValue::Sequence { .. },
            )
            | None => id,
        }
    }

    /// A string under the failsafe schema: any scalar without a tag other
    /// than `!` or `!!str`, except an empty plain scalar, which is null.
    fn text(&self, id: YamlId) -> Result<&'a str, LockfileError> {
        let Some(node) = self.stream.node(self.target(id)) else {
            return Err(self.error(id));
        };
        let YamlValue::Scalar { text, style } = &node.value else {
            return Err(self.error(id));
        };
        let tagged = match node.tag.as_deref() {
            None => false,
            Some("!" | "tag:yaml.org,2002:str") => true,
            Some(_) => return Err(self.error(id)),
        };
        if !tagged && *style == YamlScalarStyle::Plain && text.is_empty() {
            return Err(self.error(id));
        }
        Ok(text)
    }

    fn map(&self, id: YamlId) -> Result<&Map, LockfileError> {
        let target = self.target(id);
        let tag = self
            .stream
            .node(target)
            .and_then(|node| node.tag.as_deref());
        if tag.is_some_and(|tag| !matches!(tag, "!" | "tag:yaml.org,2002:map")) {
            return Err(self.error(id));
        }
        self.maps.get(&target).ok_or_else(|| self.error(id))
    }

    fn field(&self, map: YamlId, name: &str) -> Result<Option<YamlId>, LockfileError> {
        Ok(self.map(map)?.get(name).copied())
    }

    fn string_field(&self, map: YamlId, name: &str) -> Result<Option<&'a str>, LockfileError> {
        self.field(map, name)?.map(|id| self.text(id)).transpose()
    }

    /// A boolean as Yarn's `parseBoolean` reads a failsafe string.
    fn flag(&self, map: YamlId, name: &str) -> Result<bool, LockfileError> {
        let Some(id) = self.field(map, name)? else {
            return Ok(false);
        };
        match self.text(id)? {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(self.error(id)),
        }
    }

    /// A map of package names to ranges, sorted by name.
    fn declarations(&self, id: Option<YamlId>) -> Result<Vec<Declared>, LockfileError> {
        let Some(id) = id else {
            return Ok(Vec::new());
        };
        self.map(id)?
            .iter()
            .map(|(name, value)| {
                if !is_ident(name) {
                    return Err(self.error(*value));
                }
                Ok(Declared {
                    name: name.clone(),
                    spec: self.text(*value)?.into(),
                })
            })
            .collect()
    }
}

/// Whether `name` is a package name as Yarn's `parseIdent` reads it, and
/// one that a lockfile can install.
fn is_ident(name: &str) -> bool {
    super::is_installed_name(name) && split_ident(name).is_some()
}

/// Splits `<ident>@<rest>` as Yarn's strict descriptor and locator pattern
/// does: an optional `@scope/`, a name without `@` or `/`, then `@` and a
/// nonempty rest.
fn split_ident(text: &str) -> Option<(&str, Option<&str>)> {
    let name_start = match text.strip_prefix('@') {
        Some(scoped) => {
            let slash = scoped.find('/')?;
            if slash == 0 {
                return None;
            }
            slash + 2
        }
        None => 0,
    };
    let rest = &text[name_start..];
    let end = rest.find(['@', '/']).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let ident = &text[..name_start + end];
    match &rest[end..] {
        "" => Some((ident, None)),
        tail => tail
            .strip_prefix('@')
            .filter(|range| !range.is_empty())
            .map(|range| (ident, Some(range))),
    }
}

/// Whether Yarn's `TAG_REGEXP` matches: ASCII letters, digits, `.`, `_`,
/// and `-`, not starting with `v` or `V`.
fn is_tag(range: &str) -> bool {
    !range.is_empty()
        && !range.starts_with(['v', 'V'])
        && range
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Applies Yarn's `normalizeDependency`: a valid semver range or a tag
/// becomes `npm:<range>`. Ranges with a colon are never semver ranges.
pub(super) fn normalize(range: &str) -> String {
    if !range.contains(':') && (VersionRange::parse(range).is_ok() || is_tag(range)) {
        format!("npm:{range}")
    } else {
        range.to_owned()
    }
}

/// Whether Yarn's file resolvers take a range as a path: a drive letter
/// with a separator, or `/`, `./`, or `../`.
fn is_path(range: &str) -> bool {
    let bytes = range.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || range.starts_with('/')
        || range.starts_with("./")
        || range.starts_with("../")
}

/// Percent-encodes as `encodeURIComponent` and Node's `querystring.escape`
/// do.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            escaped.push(char::from(byte));
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

/// The descriptor that Yarn looks up for a dependency that `parent`
/// declares: the range normalized, and a local path bound to the declaring
/// package. Other protocols, such as `patch:`, `exec:`, `catalog:`, Git,
/// and URLs, are looked up as written.
pub(super) fn descriptor(name: &str, range: &str, parent: Option<&str>) -> String {
    let mut range = normalize(range);
    if is_path(&range) {
        range = format!("file:{range}");
    }
    let local = ["file:", "link:", "portal:"]
        .iter()
        .any(|protocol| range.starts_with(protocol));
    if local
        && !range.contains("::")
        && let Some(parent) = parent
    {
        range = format!("{range}::locator={}", escape(parent));
    }
    format!("{name}@{range}")
}

fn workspace_path(reference: &str) -> Option<&str> {
    let path = reference.strip_prefix("workspace:")?;
    if path == "." {
        return Some("");
    }
    let valid = !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    valid.then_some(path)
}

pub(super) fn read(stream: &YamlStream) -> Result<Read, LockfileError> {
    let unsupported = |span| LockfileError {
        kind: LockfileErrorKind::UnsupportedVersion,
        span,
    };
    let documents = stream.documents();
    let Some(document) = documents.first() else {
        // Yarn reads an empty file as an empty object without metadata,
        // which it does not use.
        return Err(unsupported(Span::default()));
    };
    let mut reader = Reader {
        stream,
        maps: BTreeMap::new(),
    };
    let root = document.root;
    if documents.len() != 1 {
        return Err(reader.error(documents[1].root));
    }
    for (id, node) in stream.identities().zip(stream.nodes()) {
        if let YamlValue::Mapping { entries, .. } = &node.value {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                let key = reader.text(*key)?;
                if map.insert(Box::from(key), *value).is_some() {
                    return Err(reader.error(*value));
                }
            }
            reader.maps.insert(id, map);
        }
    }
    // Yarn reads a document without content, such as one of comments only,
    // as an empty object without metadata.
    let empty = matches!(
        stream.node(root),
        Some(node) if node.tag.is_none()
            && node.value == YamlValue::Scalar { text: "".into(), style: YamlScalarStyle::Plain }
    );
    if empty {
        return Err(unsupported(reader.span(root)));
    }
    let Some(metadata) = reader.field(root, "__metadata")? else {
        return Err(unsupported(reader.span(root)));
    };
    let version_id = reader
        .field(metadata, "version")?
        .ok_or_else(|| unsupported(reader.span(metadata)))?;
    let version_text = reader.text(version_id)?;
    let version = (!version_text.starts_with('0'))
        .then(|| version_text.parse::<u64>().ok())
        .flatten()
        .filter(|version| VERSIONS.contains(version))
        .ok_or_else(|| unsupported(reader.span(version_id)))?;
    reader.string_field(metadata, "cacheKey")?;
    let legacy = version <= 6;
    let mut packages = Vec::new();
    let mut workspaces = Vec::new();
    let mut index = Index::default();
    let mut seen = BTreeSet::new();
    for (key, &id) in reader.map(root)? {
        if key.as_ref() == "__metadata" {
            continue;
        }
        let resolution_id = reader
            .field(id, "resolution")?
            .ok_or_else(|| reader.error(id))?;
        let locator = reader.text(resolution_id)?;
        let Some((name, Some(reference))) = split_ident(locator) else {
            return Err(reader.error(resolution_id));
        };
        if reference == "unknown" || !is_ident(name) {
            return Err(reader.error(resolution_id));
        }
        let recorded = reader.string_field(id, "version")?;
        for field in ["languageName", "checksum", "conditions"] {
            reader.string_field(id, field)?;
        }
        if !matches!(reader.string_field(id, "linkType")?, Some("hard" | "soft")) {
            return Err(reader.error(id));
        }
        if let Some(bin) = reader.field(id, "bin")?
            && reader.text(bin).is_err()
        {
            for value in reader.map(bin)?.values() {
                reader.text(*value)?;
            }
        }
        let dependencies = reader.declarations(reader.field(id, "dependencies")?)?;
        let peer_dependencies = reader.declarations(reader.field(id, "peerDependencies")?)?;
        if let Some(meta) = reader.field(id, "dependenciesMeta")? {
            for (pattern, value) in reader.map(meta)? {
                if split_ident(pattern).is_none_or(|(ident, _)| !is_ident(ident)) {
                    return Err(reader.error(*value));
                }
                for field in ["built", "optional", "unplugged"] {
                    reader.flag(*value, field)?;
                }
            }
        }
        let mut optional_peers = Vec::new();
        if let Some(meta) = reader.field(id, "peerDependenciesMeta")? {
            for (peer, value) in reader.map(meta)? {
                if !is_ident(peer) {
                    return Err(reader.error(*value));
                }
                if reader.flag(*value, "optional")? {
                    optional_peers.push(peer.clone());
                }
            }
        }
        let resolution = if reference.starts_with("workspace:") {
            let path = workspace_path(reference).ok_or_else(|| reader.error(resolution_id))?;
            let mut declared: [Vec<Declared>; 4] = Default::default();
            declared[DependencyKind::Production as usize] = dependencies.clone();
            declared[DependencyKind::Peer as usize] = peer_dependencies.clone();
            if index
                .workspaces
                .insert(path.into(), locator.into())
                .is_some()
            {
                return Err(reader.error(resolution_id));
            }
            workspaces.push(LockedWorkspace {
                path: path.into(),
                name: Some(name.into()),
                version: recorded
                    .filter(|version| *version != WORKSPACE_VERSION)
                    .map(Box::from),
                declared,
            });
            Resolution::Workspace(path.into())
        } else if let Some(text) = reference.strip_prefix("npm:")
            && let Ok(exact) = Version::parse(text)
        {
            if recorded.is_some_and(|recorded| recorded != text) {
                return Err(reader.error(id));
            }
            Resolution::Registry(exact)
        } else {
            Resolution::Other(locator.into())
        };
        if !seen.insert(locator) {
            return Err(reader.error(resolution_id));
        }
        // Yarn splits the key at commas and the spaces around them.
        let parts: Vec<&str> = key.split(',').collect();
        for (position, part) in parts.iter().enumerate() {
            let mut descriptor = *part;
            if position > 0 {
                descriptor = descriptor.trim_start_matches(' ');
            }
            if position + 1 < parts.len() {
                descriptor = descriptor.trim_end_matches(' ');
            }
            let Some((ident, Some(range))) = split_ident(descriptor) else {
                return Err(reader.error(id));
            };
            if !is_ident(ident) {
                return Err(reader.error(id));
            }
            let range = if legacy {
                normalize(range)
            } else {
                range.to_owned()
            };
            let normalized: Box<str> = format!("{ident}@{range}").into();
            if index
                .descriptors
                .insert(normalized, locator.into())
                .is_some()
            {
                return Err(reader.error(id));
            }
        }
        packages.push(LockedPackage {
            key: locator.into(),
            span: reader.span(id),
            name: name.into(),
            resolution,
            dependencies,
            peer_dependencies,
            optional_peers,
            dev: false,
            optional: false,
        });
    }
    packages.sort_by(|left, right| left.key.cmp(&right.key));
    workspaces.sort_by(|left, right| left.path.cmp(&right.path));
    // A dependency may have no entry: Yarn replaces the descriptors that
    // the root's `resolutions` or a catalog select before it locks them, so
    // such a dependency stays unresolved rather than resolving wrongly.
    Ok(Read {
        version,
        workspaces,
        packages,
        index,
    })
}
