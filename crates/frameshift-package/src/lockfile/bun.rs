//! `bun.lock`, Bun's text lockfile, versions 1 and 2.
//!
//! `workspaces` maps each workspace path, `""` for the root, to what it
//! declares. `packages` maps a path of package names to an array whose
//! first element is `<name>@<resolution>` and whose first object element,
//! if any, holds the package's dependencies. A key nests a package below
//! the package, alias, or workspace package named by the key's prefix.

use frameshift_syntax::{JsonDocument, JsonId, JsonValue};

use super::{
    LockedPackage, LockedWorkspace, LockfileError, LockfileErrorKind, Resolution, declared,
    format_version, is_installed_name, object, optional_string, string, structure, unsupported,
};
use crate::manifest::{members, span};
use crate::version::Version;

type Read = (u64, Vec<LockedWorkspace>, Vec<LockedPackage>);

pub(super) fn read(document: &JsonDocument<'_>) -> Result<Read, LockfileError> {
    let root = object(document, document.root())?;
    let (version, version_id) = format_version(document, root)?;
    if version != 1 && version != 2 {
        return Err(unsupported(document, version_id));
    }
    let workspaces_id = root
        .get("workspaces")
        .ok_or_else(|| structure(document, document.root()))?;
    let mut workspaces = Vec::new();
    for (path, key_span, value) in members(object(document, workspaces_id)?) {
        let valid = path.is_empty()
            || (!path.contains('\\')
                && path
                    .split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != ".."));
        if !valid {
            return Err(LockfileError {
                kind: LockfileErrorKind::Structure,
                span: key_span,
            });
        }
        let entry = object(document, value)?;
        let field = |name: &str| entry.get(name);
        let mut declared = [
            declared(document, field("dependencies"))?,
            declared(document, field("devDependencies"))?,
            declared(document, field("peerDependencies"))?,
            declared(document, field("optionalDependencies"))?,
        ];
        names(document, field("optionalPeers"))?;
        declared.iter_mut().for_each(|list| list.sort());
        workspaces.push(LockedWorkspace {
            path: path.into(),
            name: optional_string(document, field("name"))?,
            version: optional_string(document, field("version"))?,
            declared,
        });
    }
    let mut packages = Vec::new();
    if let Some(packages_id) = root.get("packages") {
        for (key, key_span, value) in members(object(document, packages_id)?) {
            if split_names(key).is_none_or(|names| names.is_empty()) {
                return Err(LockfileError {
                    kind: LockfileErrorKind::Structure,
                    span: key_span,
                });
            }
            packages.push(package(document, key, value)?);
        }
    }
    Ok((version, workspaces, packages))
}

fn package(
    document: &JsonDocument<'_>,
    key: &str,
    value: JsonId,
) -> Result<LockedPackage, LockfileError> {
    let Some(JsonValue::Array(array)) = document.value(value) else {
        return Err(structure(document, value));
    };
    let first = array.get(0).ok_or_else(|| structure(document, value))?;
    let ident = string(document, first)?;
    let split = ident
        .get(1..)
        .and_then(|tail| tail.find('@'))
        .map(|index| index + 1)
        .ok_or_else(|| structure(document, first))?;
    let (name, resolution) = (&ident[..split], &ident[split + 1..]);
    if !is_installed_name(name) || resolution.is_empty() {
        return Err(structure(document, first));
    }
    let resolution = if let Some(path) = resolution.strip_prefix("workspace:") {
        Resolution::Workspace(path.into())
    } else {
        match Version::parse(resolution) {
            Ok(version) => Resolution::Registry(version),
            Err(_) => Resolution::Other(resolution.into()),
        }
    };
    let metadata = array
        .iter()
        .skip(1)
        .find(|element| matches!(document.value(*element), Some(JsonValue::Object(_))));
    let (mut dependencies, peer_dependencies, optional_peers) = match metadata {
        None => (Vec::new(), Vec::new(), Vec::new()),
        Some(metadata) => {
            let metadata = object(document, metadata)?;
            let field = |name: &str| metadata.get(name);
            let mut dependencies = declared(document, field("dependencies"))?;
            dependencies.extend(declared(document, field("optionalDependencies"))?);
            (
                dependencies,
                declared(document, field("peerDependencies"))?,
                names(document, field("optionalPeers"))?,
            )
        }
    };
    dependencies.sort();
    Ok(LockedPackage {
        key: key.into(),
        span: span(document, value),
        name: name.into(),
        resolution,
        dependencies,
        peer_dependencies,
        optional_peers,
        dev: false,
        optional: false,
    })
}

/// Reads an array of strings, sorted.
fn names(document: &JsonDocument<'_>, id: Option<JsonId>) -> Result<Vec<Box<str>>, LockfileError> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let Some(JsonValue::Array(array)) = document.value(id) else {
        return Err(structure(document, id));
    };
    let mut names = Vec::new();
    for element in array.iter() {
        names.push(Box::from(string(document, element)?));
    }
    names.sort();
    Ok(names)
}

/// Splits a key into package names, keeping `@scope/name` together; `""`
/// has no names. Returns `None` when a name is invalid.
pub(super) fn split_names(key: &str) -> Option<Vec<&str>> {
    let mut names = Vec::new();
    if key.is_empty() {
        return Some(names);
    }
    let mut rest = key;
    loop {
        let end = if rest.starts_with('@') {
            let scope = rest.find('/')?;
            rest[scope + 1..]
                .find('/')
                .map_or(rest.len(), |index| scope + 1 + index)
        } else {
            rest.find('/').unwrap_or(rest.len())
        };
        let name = &rest[..end];
        if !is_installed_name(name) {
            return None;
        }
        names.push(name);
        if end == rest.len() {
            return Some(names);
        }
        rest = &rest[end + 1..];
    }
}
