//! The complete, explicit resource limits of one engine run.
//!
//! Every limit is a size or count, never a time, so results do not depend on
//! machine speed. Limits influence results, because input beyond a limit
//! becomes a finding, so future plans and reports record the effective values
//! through [`Limits::entries`] (ENGINEERING.md, section 6, rule 8).

use std::fmt;

use frameshift_core::{
    FindingLimits, HashLimits, IdentifierLimits, PathLimits, RunLimits, TextLimits, UnitId,
};
use frameshift_emit::EmitLimits;
use frameshift_input::{DiscoveryLimits, IgnoreLimits, SnapshotLimits};
use frameshift_package::{LoadLimits, PackageLimits};
use frameshift_syntax::{JsonLimits, LexicalLimits, ParseLimits, YamlLimits, YarnLimits};
use frameshift_transform::EditLimits;

// Entry values widen `usize` limits without loss.
const _: () = assert!(usize::BITS <= u64::BITS);

/// Every resource limit type of the implemented subsystems, with one default set.
///
/// The depth of one JSX conversion is a per-call option of
/// `JsxConversion` and is not part of this set. Which limits become user
/// options, and from which sources, is decided with the configuration key set
/// (M6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Discovery and snapshot input bounds.
    pub run: RunLimits,
    /// `.gitignore` bytes and matching work during discovery.
    pub ignore: IgnoreLimits,
    /// Decoded source text size.
    pub text: TextLimits,
    /// Canonical source and output path values.
    pub path: PathLimits,
    /// Catalog identifiers and unit identities.
    pub identifier: IdentifierLimits,
    /// Finding prose and collector size.
    pub finding: FindingLimits,
    /// Bytes hashed by one SHA-256 operation.
    pub hash: HashLimits,
    /// One parse, including its lexical scans.
    pub parse: ParseLimits,
    /// One JSON or JSONC parse.
    pub json: JsonLimits,
    /// One YAML stream, with aliases retained as references.
    pub yaml: YamlLimits,
    /// One Yarn classic lockfile.
    pub yarn: YarnLimits,
    /// The package model, such as the lockfile size.
    pub package: PackageLimits,
    /// One span rewrite.
    pub edit: EditLimits,
    /// One emitted output file.
    pub emit: EmitLimits,
}

/// One named limit value in canonical order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitEntry {
    /// Dotted lowercase name, such as `run.max-files`, using the catalog name
    /// grammar of feature identifiers.
    pub name: &'static str,
    /// The configured inclusive value.
    pub value: u64,
}

/// A violated relation between limits, without any input text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitsError {
    /// `run.max-file-bytes` exceeds `run.max-total-bytes`, so the per-file
    /// bound could never be reached.
    FileExceedsTotal,
    /// `run.max-directory-depth` is not below `path.max-components`, so a
    /// traversed directory could hold files without representable paths.
    DepthExceedsPathComponents,
    /// `run.max-file-bytes` exceeds the effective hash bound, so an admitted
    /// file could not be identified by its content hash.
    FileExceedsHash,
    /// `identifier.max-bytes` is below the largest file unit identity for a
    /// path within `path.max-path-bytes`.
    PathExceedsUnitId,
    /// `text.max-source-bytes` exceeds `edit.max-output-bytes`, so an empty
    /// edit set could fail to reproduce an accepted source.
    TextExceedsEditOutput,
    /// `run.max-files` exceeds `run.max-entries`, so the file bound could
    /// never be reached.
    FilesExceedEntries,
}

impl fmt::Display for LimitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FileExceedsTotal => "run.max-file-bytes exceeds run.max-total-bytes",
            Self::DepthExceedsPathComponents => {
                "run.max-directory-depth is not below path.max-components"
            }
            Self::FileExceedsHash => "run.max-file-bytes exceeds the effective hash limit",
            Self::PathExceedsUnitId => {
                "identifier.max-bytes cannot hold a file unit identity for every path"
            }
            Self::TextExceedsEditOutput => "text.max-source-bytes exceeds edit.max-output-bytes",
            Self::FilesExceedEntries => "run.max-files exceeds run.max-entries",
        })
    }
}

impl std::error::Error for LimitsError {}

impl Limits {
    /// Checks the relations that let each stage accept what an earlier stage
    /// admitted, in a fixed order, and reports the first violation.
    ///
    /// Other limits are independent: a source within the run limits may still
    /// exceed the text, parse, or emit limits, which callers report as findings.
    pub fn validate(&self) -> Result<(), LimitsError> {
        let run = self.run;
        if run.max_file_bytes > run.max_total_bytes {
            return Err(LimitsError::FileExceedsTotal);
        }
        if run.max_directory_depth >= self.path.max_components {
            return Err(LimitsError::DepthExceedsPathComponents);
        }
        if run.max_file_bytes > self.hash.effective_max_bytes() {
            return Err(LimitsError::FileExceedsHash);
        }
        if UnitId::max_file_id_bytes(self.path.max_path_bytes) > self.identifier.max_bytes {
            return Err(LimitsError::PathExceedsUnitId);
        }
        if self.text.max_source_bytes > self.edit.max_output_bytes {
            return Err(LimitsError::TextExceedsEditOutput);
        }
        if run.max_files > run.max_entries {
            return Err(LimitsError::FilesExceedEntries);
        }
        Ok(())
    }

    /// Returns the limits that discovery applies.
    pub fn discovery(&self) -> DiscoveryLimits {
        DiscoveryLimits {
            run: self.run,
            path: self.path,
            ignore: self.ignore,
        }
    }

    /// Returns the limits that a snapshot applies: discovery's limits and
    /// the hash bound of each file digest and of the identity stream.
    pub fn snapshot(&self) -> SnapshotLimits {
        SnapshotLimits {
            discovery: self.discovery(),
            hash: self.hash,
        }
    }

    /// Returns the limits that loading a package directory applies: the
    /// text bound of `package.json`, each JSON, YAML, and Yarn classic
    /// lockfile parse, paths, and the lockfile size.
    pub fn package(&self) -> LoadLimits {
        LoadLimits {
            text: self.text,
            json: self.json,
            yaml: self.yaml,
            yarn: self.yarn,
            path: self.path,
            package: self.package,
        }
    }

    /// Lists every configured limit, sorted by name bytes.
    ///
    /// The list is complete by construction: adding a field to any limit type
    /// fails to compile until it is listed here.
    pub fn entries(&self) -> [LimitEntry; 35] {
        let Self {
            run,
            ignore,
            text,
            path,
            identifier,
            finding,
            hash,
            parse,
            json,
            yaml,
            yarn,
            package,
            edit,
            emit,
        } = *self;
        let RunLimits {
            max_files,
            max_file_bytes,
            max_total_bytes,
            max_directory_depth,
            max_entries,
        } = run;
        let IgnoreLimits {
            max_total_bytes: ignore_bytes,
            max_match_work,
        } = ignore;
        let TextLimits { max_source_bytes } = text;
        let PathLimits {
            max_path_bytes,
            max_components,
        } = path;
        let IdentifierLimits {
            max_bytes: identifier_bytes,
        } = identifier;
        let FindingLimits {
            max_text_bytes,
            max_findings,
        } = finding;
        let HashLimits {
            max_bytes: hash_bytes,
        } = hash;
        let ParseLimits {
            lexical,
            max_nodes,
            max_tokens,
            max_nesting_depth,
        } = parse;
        let JsonLimits {
            max_depth: json_depth,
            max_nodes: json_nodes,
        } = json;
        let PackageLimits { max_lockfile_bytes } = package;
        let YamlLimits {
            max_depth: yaml_depth,
            max_nodes: yaml_nodes,
        } = yaml;
        let YarnLimits {
            max_depth: yarn_depth,
            max_nodes: yarn_nodes,
        } = yarn;
        let LexicalLimits {
            max_comments,
            max_string_bytes,
            max_number_bytes,
            max_template_bytes,
            max_identifier_bytes,
            max_regexp_bytes,
            max_jsx_text_bytes,
        } = lexical;
        let EditLimits {
            max_edits,
            max_output_bytes: edit_output_bytes,
        } = edit;
        let EmitLimits {
            max_output_bytes: emit_output_bytes,
            max_depth,
        } = emit;
        let entry = |name, value| LimitEntry { name, value };
        let wide = |value: usize| value as u64;
        [
            entry("edit.max-edits", wide(max_edits)),
            entry("edit.max-output-bytes", wide(edit_output_bytes)),
            entry("emit.max-depth", wide(max_depth)),
            entry("emit.max-output-bytes", wide(emit_output_bytes)),
            entry("finding.max-findings", wide(max_findings)),
            entry("finding.max-text-bytes", wide(max_text_bytes)),
            entry("hash.max-bytes", hash_bytes),
            entry("identifier.max-bytes", wide(identifier_bytes)),
            entry("ignore.max-match-work", max_match_work),
            entry("ignore.max-total-bytes", ignore_bytes),
            entry("json.max-depth", wide(json_depth)),
            entry("json.max-nodes", wide(json_nodes)),
            entry("package.max-lockfile-bytes", max_lockfile_bytes),
            entry("parse.lexical.max-comments", wide(max_comments)),
            entry(
                "parse.lexical.max-identifier-bytes",
                wide(max_identifier_bytes),
            ),
            entry("parse.lexical.max-jsx-text-bytes", wide(max_jsx_text_bytes)),
            entry("parse.lexical.max-number-bytes", wide(max_number_bytes)),
            entry("parse.lexical.max-regexp-bytes", wide(max_regexp_bytes)),
            entry("parse.lexical.max-string-bytes", wide(max_string_bytes)),
            entry("parse.lexical.max-template-bytes", wide(max_template_bytes)),
            entry("parse.max-nesting-depth", wide(max_nesting_depth)),
            entry("parse.max-nodes", wide(max_nodes)),
            entry("parse.max-tokens", wide(max_tokens)),
            entry("path.max-components", wide(max_components)),
            entry("path.max-path-bytes", wide(max_path_bytes)),
            entry("run.max-directory-depth", wide(max_directory_depth)),
            entry("run.max-entries", wide(max_entries)),
            entry("run.max-file-bytes", max_file_bytes),
            entry("run.max-files", wide(max_files)),
            entry("run.max-total-bytes", max_total_bytes),
            entry("text.max-source-bytes", wide(max_source_bytes)),
            entry("yaml.max-depth", wide(yaml_depth)),
            entry("yaml.max-nodes", wide(yaml_nodes)),
            entry("yarn.max-depth", wide(yarn_depth)),
            entry("yarn.max-nodes", wide(yarn_nodes)),
        ]
    }
}
