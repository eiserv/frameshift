//! The `frameshift.plan` document and its hash (`D-102`, `P-005`).
//!
//! The document is an approval artifact, never an instruction stream: it
//! holds the binding, the digest of every output file, and the manual tasks,
//! but no output bytes. `migrate --plan` recomputes the plan and compares the
//! recomputed document with the stored one byte for byte.

use frameshift_core::{Sha256Digest, Sha256Hasher, SourcePath};
use frameshift_input::SnapshotId;

use super::{ManualTask, MigrateError, MigrateErrorKind, Operation, rules};
use crate::document::{self, Value as V};
use crate::limits::Limits;

/// The version of the `frameshift.plan` document.
pub const PLAN_SCHEMA_VERSION: u64 = 1;

/// The version of the `frameshift.manifest` document.
pub const MANIFEST_SCHEMA_VERSION: u64 = 1;

// Starts the hashed stream, so this hash can equal no other Frameshift digest.
const HASH_DOMAIN: &[u8] = b"frameshift.plan.v1\n";

/// Everything that a plan depends on (ARCHITECTURE.md section 13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The identity of the whole source snapshot.
    pub snapshot: SnapshotId,
    /// The engine version.
    pub engine_version: &'static str,
    /// The shipped ruleset.
    pub ruleset: &'static str,
    /// The shipped knowledge bases.
    pub knowledge_bases: &'static [&'static str],
    /// The target profile.
    pub target_profile: &'static str,
    /// Established Next.js version, if known.
    pub source_version: Option<String>,
    /// The selected workspace member; `None` selects the root.
    pub package: Option<SourcePath>,
}

/// The hash, the printed document, and the manifest of one plan.
pub(super) struct Printed {
    pub(super) hash: Sha256Digest,
    pub(super) document: String,
    pub(super) manifest: Vec<u8>,
}

fn too_large() -> MigrateError {
    MigrateError {
        kind: MigrateErrorKind::Limit,
        message: "the plan document exceeds its size limit".to_owned(),
    }
}

fn optional_path(path: Option<&SourcePath>) -> V {
    V::optional(path, |path| V::string(path.as_str()))
}

/// Prints the plan and computes its hash, which covers every other member.
pub(super) fn print(
    binding: &Binding,
    operations: &[Operation],
    tasks: &[ManualTask],
    limits: &Limits,
) -> Result<Printed, MigrateError> {
    let mut entries = Vec::with_capacity(operations.len());
    for operation in operations {
        let digest = Sha256Digest::hash(operation.bytes(), limits.hash).map_err(|_| too_large())?;
        let rule = rules::find(operation.rule)
            .ok_or_else(|| MigrateError::internal("an operation names an unregistered rule"))?;
        entries.push(V::Object(vec![
            ("kind", V::string(operation.kind.as_str())),
            ("output", V::string(operation.output.as_str())),
            ("rule", V::string(rule.id)),
            ("ruleVersion", V::Number(rule.version)),
            ("source", optional_path(operation.source.as_ref())),
            ("review", V::Bool(operation.review)),
            ("size", V::count(operation.bytes().len())),
            ("sha256", V::String(digest.to_string())),
        ]));
    }
    let tasks = tasks
        .iter()
        .map(|task| {
            V::Object(vec![
                ("kind", V::string(task.kind.as_str())),
                ("source", optional_path(task.source.as_ref())),
                ("text", V::string(&task.text)),
            ])
        })
        .collect();
    let bases = binding
        .knowledge_bases
        .iter()
        .map(|base| V::string(base))
        .collect();
    let binding = V::Object(vec![
        ("snapshot", V::String(binding.snapshot.to_string())),
        ("engineVersion", V::string(binding.engine_version)),
        ("ruleset", V::string(binding.ruleset)),
        ("knowledgeBases", V::Array(bases)),
        ("targetProfile", V::string(binding.target_profile)),
        (
            "source",
            V::Object(vec![
                ("framework", V::string("nextjs")),
                (
                    "version",
                    V::optional(binding.source_version.as_deref(), V::string),
                ),
            ]),
        ),
        (
            "options",
            V::Object(vec![
                ("to", V::string("astro-preact")),
                ("package", optional_path(binding.package.as_ref())),
            ]),
        ),
    ]);
    let header = [
        ("kind", V::string("frameshift.plan")),
        ("schemaVersion", V::Number(PLAN_SCHEMA_VERSION)),
    ];
    let content = [
        ("binding", binding.clone()),
        ("operations", V::Array(entries.clone())),
        ("tasks", V::Array(tasks)),
    ];
    let max = limits.analysis.max_report_bytes;
    let body: Vec<_> = header.iter().chain(&content).cloned().collect();
    let body = document::print_bounded(&V::Object(body), max).ok_or_else(too_large)?;
    let mut hasher = Sha256Hasher::new(limits.hash);
    hasher.update(HASH_DOMAIN).map_err(|_| too_large())?;
    hasher.update(body.as_bytes()).map_err(|_| too_large())?;
    let hash = hasher.finish();
    let member = ("planHash", V::String(hash.to_string()));
    let members: Vec<_> = header
        .iter()
        .chain([&member])
        .chain(&content)
        .cloned()
        .collect();
    let document = document::print_bounded(&V::Object(members), max).ok_or_else(too_large)?;
    // The manifest names the plan by its hash, so the plan cannot cover it.
    let manifest = V::Object(vec![
        ("kind", V::string("frameshift.manifest")),
        ("schemaVersion", V::Number(MANIFEST_SCHEMA_VERSION)),
        member,
        ("binding", binding),
        ("files", V::Array(entries)),
    ]);
    let manifest = document::print_bounded(&manifest, max)
        .ok_or_else(too_large)?
        .into_bytes();
    Ok(Printed {
        hash,
        document,
        manifest,
    })
}
