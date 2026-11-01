//! Whether an existing output may be replaced (`D-106`).
//!
//! A location is managed only when its manifest names every file in the tree
//! with its exact digest, so no manual edit can be lost. The manifest is
//! untrusted data: it can only make a replacement fail or succeed, and the
//! replaced tree is moved aside, never deleted.

use std::collections::BTreeMap;
use std::path::Path;

use frameshift_core::{Sha256Digest, SourceText};
use frameshift_syntax::{JsonDialect, JsonValue, parse_json};

use super::{MANIFEST, MANIFEST_SCHEMA_VERSION, MigrateError, MigrateErrorKind};
use crate::limits::Limits;

fn refused(message: &str) -> MigrateError {
    MigrateError {
        kind: MigrateErrorKind::Output,
        message: message.to_owned(),
    }
}

// The path and digest of every file that the manifest lists.
fn listed_files(bytes: Vec<u8>, limits: &Limits) -> Result<BTreeMap<String, String>, MigrateError> {
    let invalid = || refused("the existing output has no valid Frameshift manifest");
    let text = SourceText::from_bytes(bytes, limits.text).map_err(|_| invalid())?;
    let document = parse_json(&text, JsonDialect::Json, limits.json).map_err(|_| invalid())?;
    let member = |object: frameshift_syntax::JsonObject<'_>, key: &str| {
        object.get(key).and_then(|id| document.value(id))
    };
    let Some(JsonValue::Object(root)) = document.value(document.root()) else {
        return Err(invalid());
    };
    if !matches!(
        member(root, "kind"),
        Some(JsonValue::String("frameshift.manifest"))
    ) {
        return Err(invalid());
    }
    let version = match member(root, "schemaVersion") {
        Some(JsonValue::Number(number)) => number.as_u64(),
        _ => None,
    };
    if version != Some(MANIFEST_SCHEMA_VERSION) {
        return Err(invalid());
    }
    let Some(JsonValue::Array(files)) = member(root, "files") else {
        return Err(invalid());
    };
    let mut listed = BTreeMap::new();
    for id in files.iter() {
        let Some(JsonValue::Object(file)) = document.value(id) else {
            return Err(invalid());
        };
        let (Some(JsonValue::String(output)), Some(JsonValue::String(digest))) =
            (member(file, "output"), member(file, "sha256"))
        else {
            return Err(invalid());
        };
        if listed
            .insert(output.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(invalid());
        }
    }
    Ok(listed)
}

/// Checks that the tree at `output` is exactly what its manifest records.
pub(super) fn check(output: &Path, limits: &Limits) -> Result<(), MigrateError> {
    let modified = || refused("the existing output was modified since it was generated");
    let mut listed = None;
    let mut digests = BTreeMap::new();
    let mut hash_failure = false;
    frameshift_output::visit_tree(output, limits.run, &mut |path, bytes| {
        if path == MANIFEST {
            listed = Some(bytes.to_vec());
        } else if let Ok(digest) = Sha256Digest::hash(bytes, limits.hash) {
            digests.insert(path.to_owned(), digest.to_string());
        } else {
            hash_failure = true;
        }
    })?;
    if hash_failure {
        return Err(modified());
    }
    let Some(listed) = listed else {
        return Err(refused(
            "the existing output has no valid Frameshift manifest",
        ));
    };
    if listed_files(listed, limits)? != digests {
        return Err(modified());
    }
    Ok(())
}
