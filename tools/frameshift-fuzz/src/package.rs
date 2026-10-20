//! The `package.json` and lockfile models on arbitrary JSON and JSONC
//! (`D-069`).
//!
//! Every accepted document must interpret repeatably. Manifest issues and
//! lockfile errors must point into the text at character boundaries,
//! modeled dependencies must agree with specifier classification, and
//! accepted lockfiles must be sorted without duplicates and resolve every
//! declared and locked dependency without failing.

use frameshift_engine::foundation::Span;
use frameshift_engine::package::{
    DependencyKind, DependencySpec, Lockfile, LockfileKind, ManifestField, PackageManifest,
};
use frameshift_engine::syntax::{JsonDialect, JsonLimits, parse_json};

use crate::targets;

/// Interprets the input as a manifest and as npm and Bun lockfiles.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    let source = targets::source(text)?;
    for dialect in [JsonDialect::Json, JsonDialect::Jsonc] {
        let Ok(document) = parse_json(&source, dialect, JsonLimits::default()) else {
            continue;
        };
        let manifest = PackageManifest::from_json(&document);
        if PackageManifest::from_json(&document) != manifest {
            return Err("manifest interpretation is not repeatable".to_owned());
        }
        match &manifest {
            Ok(manifest) => check_manifest(text, manifest)?,
            Err(error) => {
                let frameshift_engine::package::ManifestError::NotAnObject { span } = error;
                check_span(text, *span)?;
            }
        }
        for kind in [LockfileKind::NpmPackageLock, LockfileKind::BunText] {
            let lockfile = Lockfile::from_json(kind, &document);
            if Lockfile::from_json(kind, &document) != lockfile {
                return Err(format!("{kind:?}: interpretation is not repeatable"));
            }
            match lockfile {
                Ok(lockfile) => check_lockfile(&lockfile, manifest.as_ref().ok())?,
                Err(error) => check_span(text, error.span)?,
            }
        }
    }
    Ok(())
}

fn check_span(text: &str, span: Span) -> Result<(), String> {
    if span.end() > text.len()
        || !text.is_char_boundary(span.start())
        || !text.is_char_boundary(span.end())
    {
        return Err(format!("span {span:?} is off the text"));
    }
    Ok(())
}

fn check_manifest(text: &str, manifest: &PackageManifest) -> Result<(), String> {
    for issue in &manifest.issues {
        check_span(text, issue.span)?;
    }
    for kind in DependencyKind::ALL {
        let entries = manifest.dependencies_of(kind);
        for (index, entry) in entries.iter().enumerate() {
            check_span(text, entry.span)?;
            check_span(text, entry.key_span)?;
            if entry.value.spec != DependencySpec::parse(&entry.value.text)
                || entry.value.spec == DependencySpec::Invalid
            {
                return Err(format!(
                    "{}: specifier differs from its classification",
                    kind.key()
                ));
            }
            if entries[..index].iter().any(|other| other.key == entry.key) {
                return Err(format!("{}: a name occurs twice", kind.key()));
            }
        }
    }
    for member in &manifest.other {
        if ManifestField::ALL
            .iter()
            .any(|field| field.key() == &*member.key)
        {
            return Err("a modeled field is listed as another member".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn check_lockfile(
    lockfile: &Lockfile,
    manifest: Option<&PackageManifest>,
) -> Result<(), String> {
    if !lockfile
        .packages()
        .windows(2)
        .all(|pair| pair[0].key < pair[1].key)
        || !lockfile
            .workspaces()
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
    {
        return Err("lockfile entries are not strictly sorted".to_owned());
    }
    for workspace in lockfile.workspaces().iter().take(64) {
        for kind in DependencyKind::ALL {
            for declared in workspace.declared(kind).iter().take(64) {
                if let Some(package) = lockfile.resolve(&workspace.path, &declared.name)
                    && lockfile.package(&package.key) != Some(package)
                {
                    return Err("a resolution is not a locked package".to_owned());
                }
                if let Some(package) =
                    lockfile.resolve_declared(&workspace.path, &declared.name, &declared.spec)
                    && lockfile.package(&package.key) != Some(package)
                {
                    return Err("a declared resolution is not a locked package".to_owned());
                }
            }
        }
    }
    for package in lockfile.packages().iter().take(64) {
        for declared in package
            .dependencies
            .iter()
            .chain(&package.peer_dependencies)
            .take(64)
        {
            if let Some(found) = lockfile.resolve_from(package, &declared.name)
                && lockfile.package(&found.key) != Some(found)
            {
                return Err("a transitive resolution is not a locked package".to_owned());
            }
        }
    }
    if let Some(manifest) = manifest {
        lockfile.out_of_sync(manifest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_cases_hold() {
        for input in [
            r#"{ "name": "x", "dependencies": { "a": "^1", "a": 2 }, "exports": { ".": 1 } }"#,
            r#"{ "lockfileVersion": 3, "packages": { "": {}, "node_modules/a": { "version": "1.0.0" }, "node_modules/b": { "resolved": "node_modules/a", "link": true } } }"#,
            r#"{ "lockfileVersion": 2, "workspaces": { "": { "dependencies": { "a": "1" } } }, "packages": { "a": ["a@1.0.0", "", {}], "a/b": ["b@1.0.0"], }, }"#,
            "[]",
            "",
        ] {
            run(input.as_bytes()).unwrap_or_else(|error| panic!("{input:?}: {error}"));
        }
    }
}
