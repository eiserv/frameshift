//! Workspace patterns evaluated over a directory listing instead of a
//! snapshot (`D-073`), for diagnostics and fuzzing.

use std::collections::BTreeMap;

use frameshift_core::{PathLimits, SourcePath, SourceText, Span, TextLimits, TextLocation};

use super::pattern::ParseBounds;
use super::semantics::{
    Bounds, Declaration, Facts, Files, Node, PatternInput, Presence, Read, Tree, evaluate,
};
use super::{PatternRejection, WorkspaceLimits, WorkspaceSemantics};
use crate::load::LoadError;

/// A directory of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedDirectory {
    /// The directory below the workspace root.
    pub path: SourcePath,
    /// Whether its entries are listed; an untraversed directory, such as an
    /// ignored one, may hold members that the listing does not show.
    pub traversed: bool,
    /// Its `package.json`, if it has one.
    pub manifest: Option<ListedManifest>,
}

/// The fields of a listed `package.json` that membership depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListedManifest {
    /// `name`.
    pub name: Option<Box<str>>,
    /// `version`.
    pub version: Option<Box<str>>,
    /// `workspaces`, which Yarn 2 and later read in members.
    pub workspaces: Option<Vec<Box<str>>>,
}

/// The members that one release selects from a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingSelection {
    /// Member directories, sorted by path.
    pub members: Vec<SourcePath>,
    /// The root declaration's patterns that are not evaluated, by index,
    /// with the reason.
    pub rejected: Vec<(usize, PatternRejection)>,
    /// Whether the members are known exactly.
    pub exact: bool,
    /// Whether the release would reject the workspace.
    pub rejects: bool,
}

/// Evaluates the root's workspace patterns with one release's semantics
/// over a listing of directories, whose paths must be unique.
pub fn select_members(
    release: WorkspaceSemantics,
    root_private: bool,
    patterns: &[&str],
    listing: &[ListedDirectory],
    limits: WorkspaceLimits,
) -> Result<ListingSelection, LoadError> {
    let root_file =
        SourcePath::new("package.json", PathLimits::default()).map_err(|_| LoadError::Internal)?;
    let declaration = declaration(&root_file, patterns)?;
    let mut tree = Tree::default();
    let mut manifests = BTreeMap::new();
    let mut sorted: Vec<&ListedDirectory> = listing.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    sorted.dedup_by(|right, left| right.path == left.path);
    for directory in sorted {
        let node = Node {
            path: directory.path.clone(),
            segments: directory.path.as_str().split('/').map(Box::from).collect(),
        };
        if directory.traversed {
            tree.dirs.push(node);
            if let Some(manifest) = &directory.manifest {
                manifests.insert(directory.path.clone(), manifest.clone());
            }
        } else {
            tree.hidden.push(node);
        }
    }
    let mut files = ListingFiles { manifests };
    let root = Facts {
        private: root_private,
        ..Facts::default()
    };
    let bounds = Bounds {
        parse: ParseBounds {
            max_bytes: limits.max_pattern_bytes,
            max_alternatives: limits.max_alternatives,
        },
        max_match_work: limits.max_match_work,
    };
    let outcome = evaluate(
        release,
        &tree,
        &mut files,
        &root,
        &declaration,
        bounds,
        false,
    )?;
    let rejected = outcome
        .rejected
        .iter()
        .filter(|(location, _)| location.file() == &root_file)
        .map(|(location, rejection)| {
            let index = declaration
                .patterns
                .iter()
                .position(|input| input.location.span() == location.span())
                .unwrap_or(0);
            (index, *rejection)
        })
        .collect();
    Ok(ListingSelection {
        exact: outcome.is_exact(),
        rejects: !outcome.failures.is_empty(),
        members: outcome.members.into_iter().collect(),
        rejected,
    })
}

/// A declaration whose patterns carry locations in a synthetic text that
/// lists them one per line.
fn declaration(file: &SourcePath, patterns: &[&str]) -> Result<Declaration, LoadError> {
    let mut text = String::new();
    let mut spans = Vec::new();
    for pattern in patterns {
        let start = text.len();
        text.push_str(pattern);
        spans.push((start, text.len()));
        text.push('\n');
    }
    let source = SourceText::new(
        text,
        TextLimits {
            max_source_bytes: usize::MAX,
        },
    )
    .map_err(|_| LoadError::Internal)?;
    let mut inputs = Vec::new();
    for (pattern, (start, end)) in patterns.iter().zip(spans) {
        let span = Span::new(start, end).map_err(|_| LoadError::Internal)?;
        inputs.push(PatternInput {
            text: (*pattern).into(),
            location: TextLocation::new(file.clone(), &source, span)
                .map_err(|_| LoadError::Internal)?,
        });
    }
    Ok(Declaration {
        patterns: inputs,
        complete: true,
    })
}

struct ListingFiles {
    manifests: BTreeMap<SourcePath, ListedManifest>,
}

impl Files for ListingFiles {
    fn presence(&self, directory: &SourcePath, name: &str) -> Presence {
        if name == "package.json" && self.manifests.contains_key(directory) {
            Presence::Admitted
        } else {
            Presence::Absent
        }
    }

    fn manifest(&mut self, directory: &SourcePath) -> Result<Read, LoadError> {
        let Some(manifest) = self.manifests.get(directory) else {
            return Ok(Read::NotAnalyzed);
        };
        let file = SourcePath::new(
            &format!("{}/package.json", directory.as_str()),
            PathLimits::default(),
        )
        .map_err(|_| LoadError::Internal)?;
        let workspaces = match &manifest.workspaces {
            Some(patterns) => {
                let patterns: Vec<&str> = patterns.iter().map(AsRef::as_ref).collect();
                Some(declaration(&file, &patterns)?)
            }
            None => None,
        };
        Ok(Read::Analyzed(Facts {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            private: false,
            workspaces,
        }))
    }
}
