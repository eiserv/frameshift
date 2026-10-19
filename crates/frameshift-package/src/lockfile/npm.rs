//! `package-lock.json` and `npm-shrinkwrap.json`, versions 2 and 3.
//!
//! Both versions list every installed location in `packages`: the root
//! `""`, workspace and local folders without a `node_modules` segment, and
//! installed packages below `node_modules`. Version 2 also repeats the tree
//! of version 1 in `dependencies`, which is not read.

use frameshift_syntax::{JsonDocument, JsonId};

use super::{
    LockedPackage, LockedWorkspace, LockfileError, LockfileErrorKind, Resolution, declared, flag,
    format_version, is_installed_name, object, optional_string, structure, unsupported,
};
use crate::manifest::{members, span};
use crate::version::Version;

type Read = (u64, Vec<LockedWorkspace>, Vec<LockedPackage>);

pub(super) fn read(document: &JsonDocument<'_>) -> Result<Read, LockfileError> {
    let root = object(document, document.root())?;
    let (version, version_id) = format_version(document, root)?;
    if version != 2 && version != 3 {
        return Err(unsupported(document, version_id));
    }
    let packages_id = root
        .get("packages")
        .ok_or_else(|| structure(document, document.root()))?;
    let mut workspaces = Vec::new();
    let mut packages = Vec::new();
    for (key, key_span, value) in members(object(document, packages_id)?) {
        let entry = object(document, value)?;
        if !valid_location(key) {
            return Err(LockfileError {
                kind: LockfileErrorKind::Structure,
                span: key_span,
            });
        }
        let field = |name: &str| entry.get(name);
        let recorded_name = optional_string(document, field("name"))?;
        let version_text = optional_string(document, field("version"))?;
        let resolved = optional_string(document, field("resolved"))?;
        let link = flag(document, field("link"))?;
        let dev = flag(document, field("dev"))?;
        let optional = flag(document, field("optional"))?;
        for other in ["devOptional", "peer", "inBundle"] {
            flag(document, field(other))?;
        }
        let production = declared(document, field("dependencies"))?;
        let development = declared(document, field("devDependencies"))?;
        let optional_dependencies = declared(document, field("optionalDependencies"))?;
        let peer = declared(document, field("peerDependencies"))?;
        let optional_peers = optional_peers(document, field("peerDependenciesMeta"))?;
        let Some(installed) = installed_name(key) else {
            workspaces.push(LockedWorkspace {
                path: key.into(),
                name: recorded_name,
                version: version_text,
                declared: [production, development, peer, optional_dependencies],
            });
            continue;
        };
        if !is_installed_name(installed) {
            return Err(LockfileError {
                kind: LockfileErrorKind::Structure,
                span: key_span,
            });
        }
        let resolution = if link {
            let target = resolved.ok_or_else(|| structure(document, value))?;
            if !valid_location(&target) {
                return Err(structure(document, value));
            }
            Resolution::Link(target)
        } else {
            // Registry tarballs are HTTP URLs; bundled packages record none.
            let from_registry = resolved
                .as_deref()
                .is_none_or(|url| url.starts_with("https://") || url.starts_with("http://"));
            match (version_text.as_deref().map(Version::parse), from_registry) {
                (Some(Ok(version)), true) => Resolution::Registry(version),
                _ => Resolution::Other(resolved.or(version_text).unwrap_or_default()),
            }
        };
        let mut dependencies = production;
        dependencies.extend(optional_dependencies);
        dependencies.sort();
        packages.push(LockedPackage {
            key: key.into(),
            span: span(document, value),
            name: recorded_name.unwrap_or_else(|| installed.into()),
            resolution,
            dependencies,
            peer_dependencies: peer,
            optional_peers,
            dev,
            optional,
        });
    }
    Ok((version, workspaces, packages))
}

/// A location key: `""`, or relative segments without `.`, empty segments,
/// or backslashes. `..` segments occur for local packages outside the root.
fn valid_location(key: &str) -> bool {
    key.is_empty()
        || (!key.contains('\\')
            && key
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "."))
}

/// The package name after the last `node_modules/` segment, or `None` for a
/// folder outside `node_modules`.
fn installed_name(key: &str) -> Option<&str> {
    let start = match key.rfind("/node_modules/") {
        Some(index) => index + "/node_modules/".len(),
        None => key
            .strip_prefix("node_modules/")
            .map(|_| "node_modules/".len())?,
    };
    Some(&key[start..])
}

fn optional_peers(
    document: &JsonDocument<'_>,
    id: Option<JsonId>,
) -> Result<Vec<Box<str>>, LockfileError> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for (name, _, value) in members(object(document, id)?) {
        let meta = object(document, value)?;
        if flag(document, meta.get("optional"))? {
            names.push(Box::from(name));
        }
    }
    names.sort();
    Ok(names)
}
