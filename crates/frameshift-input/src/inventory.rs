//! The result of discovery: admitted files and every other accounted entry.

use frameshift_core::{DenseId, DenseMap, KeyTable, SourcePath};

/// What discovery found below one root, in canonical path order.
///
/// Admitted files are regular files within the run limits that later stages
/// may read; every other entry that discovery accounted for is listed with
/// its kind. Entries that could not be represented as paths are only
/// counted. Paths need context-specific escaping before rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub(crate) files: KeyTable<SourcePath>,
    pub(crate) sizes: DenseMap<SourcePath, u64>,
    pub(crate) entries: Vec<Entry>,
    pub(crate) unrepresentable: usize,
    pub(crate) stop: Option<Stop>,
    pub(crate) complete: bool,
}

impl Inventory {
    /// Returns the admitted regular files with their dense identities.
    pub fn files(&self) -> &KeyTable<SourcePath> {
        &self.files
    }

    /// Returns the size in bytes that discovery recorded for an admitted file.
    pub fn file_size(&self, id: DenseId<SourcePath>) -> Option<u64> {
        self.sizes.get(id).copied()
    }

    /// Returns every accounted entry that is not an admitted file, sorted by path.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Finds the entry with this path, if it is not an admitted file.
    pub fn entry(&self, path: &SourcePath) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.path.cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Returns the number of entries whose names or paths cannot be
    /// represented as [`SourcePath`] values.
    pub fn unrepresentable(&self) -> usize {
        self.unrepresentable
    }

    /// Returns the cumulative limit that stopped discovery, if any.
    pub fn stop(&self) -> Option<Stop> {
        self.stop
    }

    /// Returns whether every file below the root is accounted for, as
    /// admitted, listed, ignored, excluded, or counted as unrepresentable.
    ///
    /// A stop, a directory that was not traversed for a limit, an error, or
    /// an unrepresentable name, and an entry whose metadata could not be
    /// read make the inventory incomplete. Later stages must not treat an
    /// incomplete inventory as the whole project.
    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

/// An accounted entry other than an admitted file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub(crate) path: SourcePath,
    pub(crate) kind: EntryKind,
}

impl Entry {
    /// Borrows the canonical path. Escape it before rendering.
    pub fn path(&self) -> &SourcePath {
        &self.path
    }

    /// Returns what discovery recorded for the entry.
    pub fn kind(&self) -> &EntryKind {
        &self.kind
    }
}

/// The kind of an accounted entry that is not an admitted file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A real directory whose entries were all read.
    Directory,
    /// An entry that is never opened or traversed.
    Ignored {
        /// The rule that ignores it.
        reason: IgnoreReason,
        /// Whether it is a real directory rather than a file, link, or other entry.
        directory: bool,
    },
    /// A regular file larger than the per-file limit; accounted, not admitted.
    Oversized {
        /// Its size in bytes.
        bytes: u64,
    },
    /// A symbolic link or junction, which is never followed.
    Link(LinkTarget),
    /// A Windows reparse point that is not a link, such as a cloud
    /// placeholder; never opened.
    ReparsePoint {
        /// Whether the reparse point carries the directory attribute.
        directory: bool,
    },
    /// A device file, FIFO, or socket; never opened.
    Special,
    /// A real directory whose entries were not discovered.
    NotTraversed(NotTraversed),
    /// An entry whose type and size could not be read.
    Uninspectable,
}

/// Why an entry is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IgnoreReason {
    /// A version control directory or file: `.git`, `.hg`, or `.svn`.
    Vcs,
    /// Installed dependencies: `node_modules`.
    Dependencies,
    /// Framework build output: `.next`.
    BuildOutput,
    /// An environment file: `.env` or `.env.*`, except examples.
    Environment,
    /// A pattern of a `.gitignore` file.
    Gitignore,
}

impl IgnoreReason {
    /// Returns the stable lowercase spelling used in reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vcs => "vcs",
            Self::Dependencies => "dependencies",
            Self::BuildOutput => "build-output",
            Self::Environment => "environment",
            Self::Gitignore => "gitignore",
        }
    }
}

/// Why a real directory was not traversed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotTraversed {
    /// It lies deeper than the directory depth limit.
    DepthLimit,
    /// Its entries could not be read.
    Unreadable,
    /// Its `.gitignore` file could not be read, so its entries could not be classified.
    IgnoreFileUnreadable,
}

/// Where a link points, as far as discovery can tell without following it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// The relative target resolves inside the root through traversed real
    /// directories. Look the path up in the inventory to learn its kind.
    Inside(LinkReference),
    /// The relative target leaves the root.
    Escapes,
    /// No entry exists at the target.
    Dangling,
    /// The target could not be resolved without following other links or
    /// leaving the inventory.
    Unresolved(Unresolved),
}

/// A link target inside the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkReference {
    /// The root directory itself.
    Root,
    /// The entry at this path.
    Path(SourcePath),
}

/// Why a link target was not resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unresolved {
    /// The target is an absolute or drive-relative path.
    Absolute,
    /// The target passes through another link or reparse point.
    ThroughLink,
    /// The target passes through a directory whose entries were not discovered.
    Untraversed,
    /// The target could not be read.
    Unreadable,
    /// A target component or the resolved path cannot be represented as a path.
    Unrepresentable,
    /// Discovery stopped or failed before the target could be checked.
    Incomplete,
}

/// The cumulative limit that stopped discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stop {
    /// `run.max-files`.
    Files,
    /// `run.max-total-bytes`.
    TotalBytes,
    /// `run.max-entries`.
    Entries,
    /// `ignore.max-total-bytes`.
    IgnoreBytes,
    /// `ignore.max-match-work`.
    IgnoreWork,
}

impl Stop {
    /// Returns the canonical name of the limit, as `Limits::entries` lists it.
    pub const fn limit_name(self) -> &'static str {
        match self {
            Self::Files => "run.max-files",
            Self::TotalBytes => "run.max-total-bytes",
            Self::Entries => "run.max-entries",
            Self::IgnoreBytes => "ignore.max-total-bytes",
            Self::IgnoreWork => "ignore.max-match-work",
        }
    }
}
