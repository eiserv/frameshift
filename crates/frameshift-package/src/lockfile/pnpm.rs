//! pnpm lockfile 9.0: importer declarations, package metadata, and snapshots.

use std::collections::BTreeMap;

use frameshift_syntax::{YamlId, YamlScalarStyle, YamlStream, YamlValue};

use super::{
    Declared, LockedPackage, LockedWorkspace, LockfileError, LockfileErrorKind, Resolution,
    is_installed_name,
};
use crate::{DependencyKind, Version};

pub(super) type References = BTreeMap<(Box<str>, Box<str>), Box<str>>;
type Map = BTreeMap<Box<str>, YamlId>;

pub(super) struct Read {
    pub workspaces: Vec<LockedWorkspace>,
    pub packages: Vec<LockedPackage>,
    pub references: References,
}

struct Reader<'a> {
    stream: &'a YamlStream,
    maps: BTreeMap<YamlId, Map>,
}

impl<'a> Reader<'a> {
    fn error(&self, id: YamlId) -> LockfileError {
        LockfileError {
            kind: LockfileErrorKind::Structure,
            span: self.stream.node(id).expect("YAML identity").span,
        }
    }
    fn target(&self, id: YamlId) -> YamlId {
        match self.stream.node(id).expect("YAML identity").value {
            YamlValue::Alias(target) => target,
            YamlValue::Scalar { .. } | YamlValue::Mapping { .. } | YamlValue::Sequence { .. } => id,
        }
    }
    fn text(&self, id: YamlId) -> Result<&'a str, LockfileError> {
        let node = self.stream.node(self.target(id)).expect("YAML identity");
        let YamlValue::Scalar { text, style } = &node.value else {
            return Err(self.error(id));
        };
        if node
            .tag
            .as_deref()
            .is_some_and(|tag| !matches!(tag, "!" | "tag:yaml.org,2002:str"))
        {
            return Err(self.error(id));
        }
        if *style == YamlScalarStyle::Plain
            && node.tag.is_none()
            && (matches!(
                text.as_ref(),
                "" | "~"
                    | "null"
                    | "Null"
                    | "NULL"
                    | "true"
                    | "True"
                    | "TRUE"
                    | "false"
                    | "False"
                    | "FALSE"
            ) || core_number(text))
        {
            return Err(self.error(id));
        }
        Ok(text)
    }
    fn map(&self, id: YamlId) -> Result<&Map, LockfileError> {
        let target = self.target(id);
        let node = self.stream.node(target).expect("YAML identity");
        if node
            .tag
            .as_deref()
            .is_some_and(|tag| !matches!(tag, "!" | "tag:yaml.org,2002:map"))
        {
            return Err(self.error(id));
        }
        self.maps.get(&target).ok_or_else(|| self.error(id))
    }
    fn field(&self, map: YamlId, name: &str) -> Result<Option<YamlId>, LockfileError> {
        Ok(self.map(map)?.get(name).copied())
    }
    fn required(&self, map: YamlId, name: &str) -> Result<YamlId, LockfileError> {
        self.field(map, name)?.ok_or_else(|| self.error(map))
    }
    fn string_field(&self, map: YamlId, name: &str) -> Result<Option<&'a str>, LockfileError> {
        self.field(map, name)?.map(|id| self.text(id)).transpose()
    }
    fn boolean(&self, id: YamlId) -> Result<bool, LockfileError> {
        let node = self.stream.node(self.target(id)).expect("YAML identity");
        let YamlValue::Scalar {
            text,
            style: YamlScalarStyle::Plain,
        } = &node.value
        else {
            return Err(self.error(id));
        };
        if node
            .tag
            .as_deref()
            .is_some_and(|tag| tag != "tag:yaml.org,2002:bool")
        {
            return Err(self.error(id));
        }
        match text.as_ref() {
            "true" | "True" | "TRUE" => Ok(true),
            "false" | "False" | "FALSE" => Ok(false),
            _ => Err(self.error(id)),
        }
    }
    fn flag(&self, map: YamlId, name: &str) -> Result<bool, LockfileError> {
        self.field(map, name)?
            .map(|id| self.boolean(id))
            .transpose()
            .map(|flag| flag.unwrap_or(false))
    }
    fn strings(&self, id: YamlId) -> Result<Vec<Box<str>>, LockfileError> {
        let YamlValue::Sequence { elements, .. } = &self
            .stream
            .node(self.target(id))
            .expect("YAML identity")
            .value
        else {
            return Err(self.error(id));
        };
        elements
            .iter()
            .map(|id| self.text(*id).map(Box::from))
            .collect()
    }
    fn declarations(&self, id: Option<YamlId>) -> Result<Vec<Declared>, LockfileError> {
        let Some(id) = id else {
            return Ok(Vec::new());
        };
        self.map(id)?
            .iter()
            .map(|(name, value)| {
                if !is_installed_name(name) {
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

pub(super) fn read(stream: &YamlStream) -> Result<Read, LockfileError> {
    let Some(document) = stream.documents().first() else {
        return Err(LockfileError {
            kind: LockfileErrorKind::Structure,
            span: frameshift_core::Span::default(),
        });
    };
    let mut reader = Reader {
        stream,
        maps: BTreeMap::new(),
    };
    let root = document.root;
    if stream.documents().len() != 1 {
        return Err(reader.error(root));
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
    let version_id = reader.required(root, "lockfileVersion")?;
    let version_node = stream.node(reader.target(version_id)).unwrap();
    let supported = matches!(&version_node.value, YamlValue::Scalar { text, .. } if text.as_ref() == "9.0")
        && version_node.tag.as_deref().is_none_or(|tag| {
            matches!(
                tag,
                "!" | "tag:yaml.org,2002:str" | "tag:yaml.org,2002:float"
            )
        });
    if !supported {
        return Err(LockfileError {
            kind: LockfileErrorKind::UnsupportedVersion,
            span: stream.node(version_id).unwrap().span,
        });
    }
    let mut references = References::new();
    let mut workspaces = Vec::new();
    if let Some(importers) = reader.field(root, "importers")? {
        for (path, id) in reader.map(importers)? {
            let path = if path.as_ref() == "." {
                ""
            } else {
                path.as_ref()
            };
            if !valid_path(path, true) {
                return Err(reader.error(*id));
            }
            let mut declared: [Vec<Declared>; 4] = Default::default();
            // Optional wins over production, which wins over development.
            for kind in [
                DependencyKind::Development,
                DependencyKind::Production,
                DependencyKind::Optional,
            ] {
                if let Some(dependencies) = reader.field(*id, dependency_field(kind))? {
                    for (name, dependency) in reader.map(dependencies)? {
                        if !is_installed_name(name) {
                            return Err(reader.error(*dependency));
                        }
                        let specifier = reader.text(reader.required(*dependency, "specifier")?)?;
                        let version = reader.text(reader.required(*dependency, "version")?)?;
                        if version.is_empty() {
                            return Err(reader.error(*dependency));
                        }
                        declared[kind as usize].push(Declared {
                            name: name.clone(),
                            spec: specifier.into(),
                        });
                        references.insert((path.into(), name.clone()), version.into());
                    }
                }
            }
            workspaces.push(LockedWorkspace {
                path: path.into(),
                name: None,
                version: None,
                declared,
            });
        }
    }
    let metadata = reader
        .field(root, "packages")?
        .map(|id| reader.map(id))
        .transpose()?;
    if let Some(metadata) = metadata {
        for (key, id) in metadata {
            let resolution = reader.required(*id, "resolution")?;
            reader.map(resolution)?;
            for field in [
                "integrity",
                "tarball",
                "type",
                "directory",
                "repo",
                "commit",
                "path",
            ] {
                reader.string_field(resolution, field)?;
            }
            for field in ["name", "version", "id", "deprecated"] {
                reader.string_field(*id, field)?;
            }
            match reader.string_field(resolution, "type")? {
                None => {
                    if reader.string_field(resolution, "integrity")?.is_none()
                        && reader.string_field(resolution, "tarball")?.is_none()
                    {
                        return Err(reader.error(resolution));
                    }
                }
                Some("git") => {
                    for field in ["repo", "commit"] {
                        reader.text(reader.required(resolution, field)?)?;
                    }
                }
                Some("directory") => {
                    reader.text(reader.required(resolution, "directory")?)?;
                }
                Some(_) => {}
            }
            for field in ["hasBin", "patched"] {
                reader.flag(*id, field)?;
            }
            reader.declarations(reader.field(*id, "peerDependencies")?)?;
            if let Some(peers) = reader.field(*id, "peerDependenciesMeta")? {
                for (name, value) in reader.map(peers)? {
                    if !is_installed_name(name) {
                        return Err(reader.error(*value));
                    }
                    reader.flag(*value, "optional")?;
                }
            }
            if let Some(engines) = reader.field(*id, "engines")? {
                for value in reader.map(engines)?.values() {
                    reader.text(*value)?;
                }
            }
            if let Some(bundled) = reader.field(*id, "bundledDependencies")?
                && reader.boolean(bundled).is_err()
            {
                reader.strings(bundled)?;
            }
            for field in ["os", "cpu", "libc"] {
                if let Some(value) = reader.field(*id, field)? {
                    reader.strings(value)?;
                }
            }
            if key.is_empty() {
                return Err(reader.error(*id));
            }
        }
    }
    let mut packages = Vec::new();
    if let Some(snapshots) = reader.field(root, "snapshots")? {
        for (key, id) in reader.map(snapshots)? {
            let base = base_key(key).ok_or_else(|| reader.error(*id))?;
            let info = metadata
                .and_then(|map| map.get(base))
                .copied()
                .ok_or_else(|| reader.error(*id))?;
            let name = reader
                .string_field(info, "name")?
                .or_else(|| split_registry_key(base).map(|(name, _)| name))
                .ok_or_else(|| reader.error(info))?;
            if !is_installed_name(name) {
                return Err(reader.error(info));
            }
            let resolution_info = reader.required(info, "resolution")?;
            let recorded_type = reader.string_field(resolution_info, "type")?;
            let resolution = if recorded_type.is_none()
                && split_registry_key(base)
                    .is_some_and(|(_, version)| Version::parse(version).is_ok())
            {
                let (key_name, key_version) = split_registry_key(base).unwrap();
                if reader
                    .string_field(info, "name")?
                    .is_some_and(|recorded| recorded != key_name)
                    || reader
                        .string_field(info, "version")?
                        .is_some_and(|recorded| recorded != key_version)
                {
                    return Err(reader.error(info));
                }
                Resolution::Registry(Version::parse(key_version).unwrap())
            } else {
                Resolution::Other(base.into())
            };
            let mut dependencies: BTreeMap<Box<str>, Box<str>> = BTreeMap::new();
            for field in ["dependencies", "optionalDependencies"] {
                for declaration in reader.declarations(reader.field(*id, field)?)? {
                    dependencies.insert(declaration.name, declaration.spec);
                }
            }
            if let Some(peers) = reader.field(*id, "transitivePeerDependencies")? {
                reader.strings(peers)?;
            }
            let peer_dependencies = reader.declarations(reader.field(info, "peerDependencies")?)?;
            let mut optional_peers = Vec::new();
            if let Some(peers) = reader.field(info, "peerDependenciesMeta")? {
                for (name, value) in reader.map(peers)? {
                    if !is_installed_name(name) {
                        return Err(reader.error(*value));
                    }
                    if reader.flag(*value, "optional")? {
                        optional_peers.push(name.clone());
                    }
                }
            }
            packages.push(LockedPackage {
                key: key.clone(),
                span: stream.node(*id).unwrap().span,
                name: name.into(),
                resolution,
                dependencies: dependencies
                    .into_iter()
                    .map(|(name, spec)| Declared { name, spec })
                    .collect(),
                peer_dependencies,
                optional_peers,
                dev: false,
                optional: reader.flag(*id, "optional")?,
            });
        }
    }
    packages.sort_by(|a, b| a.key.cmp(&b.key));
    for ((_, name), reference) in &references {
        if !reference.starts_with("link:") && find(&packages, name, reference).is_none() {
            return Err(reader.error(root));
        }
    }
    for package in &packages {
        for dependency in &package.dependencies {
            if !dependency.spec.starts_with("link:")
                && find(&packages, &dependency.name, &dependency.spec).is_none()
            {
                return Err(LockfileError {
                    kind: LockfileErrorKind::Structure,
                    span: package.span,
                });
            }
        }
    }
    Ok(Read {
        workspaces,
        packages,
        references,
    })
}

pub(super) fn find<'a>(
    packages: &'a [LockedPackage],
    name: &str,
    reference: &str,
) -> Option<&'a LockedPackage> {
    let lookup = |key: &str| {
        packages
            .binary_search_by(|package| package.key.as_ref().cmp(key))
            .ok()
            .map(|index| &packages[index])
    };
    if reference.starts_with("link:") {
        return None;
    }
    lookup(reference).or_else(|| lookup(&format!("{name}@{reference}")))
}

fn split_registry_key(key: &str) -> Option<(&str, &str)> {
    let split = key.get(1..)?.find('@')? + 1;
    let name = &key[..split];
    (is_installed_name(name) && split + 1 < key.len()).then_some((name, &key[split + 1..]))
}

fn base_key(key: &str) -> Option<&str> {
    let end = key.find('(').unwrap_or(key.len());
    let mut depth = 0usize;
    for ch in key[end..].chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            _ if depth == 0 || ch.is_whitespace() => return None,
            _ => {}
        }
    }
    (depth == 0 && end > 0).then_some(&key[..end])
}

fn valid_path(path: &str, root: bool) -> bool {
    (root && path.is_empty())
        || (!path.contains('\\')
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."))
}

fn dependency_field(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Production => "dependencies",
        DependencyKind::Development => "devDependencies",
        DependencyKind::Optional => "optionalDependencies",
        DependencyKind::Peer => "peerDependencies",
    }
}

fn core_number(text: &str) -> bool {
    let value = text.strip_prefix(['+', '-']).unwrap_or(text);
    if matches!(value, ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    if let Some(hex) = value.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if let Some(octal) = value.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|b| matches!(b, b'0'..=b'7'));
    }
    let (mantissa, exponent) = value
        .find(['e', 'E'])
        .map_or((value, None), |i| (&value[..i], Some(&value[i + 1..])));
    if exponent.is_some_and(|exp| {
        let exp = exp.strip_prefix(['+', '-']).unwrap_or(exp);
        exp.is_empty() || !exp.bytes().all(|b| b.is_ascii_digit())
    }) {
        return false;
    }
    let mut dots = 0;
    let mut digits = 0;
    for byte in mantissa.bytes() {
        if byte.is_ascii_digit() {
            digits += 1;
        } else if byte == b'.' {
            dots += 1;
        } else {
            return false;
        }
    }
    digits > 0 && dots <= 1
}
