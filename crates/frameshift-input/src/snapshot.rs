//! The snapshot store: read-once file contents and a framed identity of the
//! whole input (`D-067`).
//!
//! Every later stage reads source files through a [`Snapshot`], so a file is
//! opened at most once per run and all stages see the same bytes.

use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use frameshift_core::{
    DenseId, FindingCollector, FindingError, FindingLocation, FindingText, HashError, HashLimits,
    Severity, Sha256Digest, Sha256Hasher, SourcePath,
};

use crate::inventory::{
    EntryKind, IgnoreReason, Inventory, LinkReference, LinkTarget, NotTraversed, Stop, Unresolved,
};
use crate::native::NativeTree;
use crate::walk::{
    self, DiscoveryError, DiscoveryLimits, FileLocations, IoFailure, ReadError, ReportError, Tree,
};

/// The domain tag that starts every snapshot identity stream.
const DOMAIN: &[u8] = b"frameshift.snapshot";
/// The version of the identity stream layout (IMPLEMENTATION.md section 24).
const FORMAT_VERSION: u64 = 1;

/// The limits that a snapshot applies: discovery's limits and the bound of
/// each SHA-256 operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// File count, sizes, depth, entries, paths, and `.gitignore` limits.
    pub discovery: DiscoveryLimits,
    /// Bytes hashed by one file digest or by the identity stream.
    pub hash: HashLimits,
}

/// Discovers `root` read-only and returns a snapshot that reads admitted
/// files on demand.
///
/// Discovery runs exactly as [`crate::discover`] does and appends the same
/// findings. No file content is read until [`Snapshot::read`] or
/// [`Snapshot::identity`] requests it.
pub fn snapshot(
    root: &Path,
    limits: SnapshotLimits,
    findings: &mut FindingCollector,
) -> Result<Snapshot, DiscoveryError> {
    let (root, identity) = NativeTree::open_root(root)?;
    let (inventory, files) =
        walk::walk_files(&NativeTree, root, identity, limits.discovery, findings)?;
    Snapshot::new(inventory, NativeTree, files, limits.hash)
}

/// The admitted files of one discovery, each read at most once.
///
/// Reads take `&self` and are independent per file, so stages may read in
/// parallel; every outcome, including a failure, is retained for the run.
pub struct Snapshot {
    inventory: Inventory,
    reader: Box<dyn Reader>,
    outcomes: Vec<OnceLock<Outcome>>,
    hash: HashLimits,
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let read = self
            .outcomes
            .iter()
            .filter(|slot| slot.get().is_some())
            .count();
        f.debug_struct("Snapshot")
            .field("inventory", &self.inventory)
            .field("files_read", &read)
            .finish_non_exhaustive()
    }
}

/// The retained content of one admitted file.
#[derive(Debug)]
pub struct SnapshotFile {
    bytes: Vec<u8>,
    digest: Sha256Digest,
}

impl SnapshotFile {
    /// Borrows the exact bytes that were read, without any decoding.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the SHA-256 digest of the exact bytes.
    pub fn digest(&self) -> Sha256Digest {
        self.digest
    }
}

/// The outcome of reading one admitted file.
#[derive(Clone, Copy, Debug)]
pub enum FileRead<'a> {
    /// The file's content.
    Content(&'a SnapshotFile),
    /// The file is unchanged, but the operating system refused to read it.
    /// Its content is not analyzed; [`Snapshot::report_unreadable`] reports it.
    Unreadable(IoFailure),
}

/// A failure that makes the run's results untrustworthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// The identity does not belong to this snapshot's file table.
    UnknownFile,
    /// A file is no longer the listed regular file, or it changed while it
    /// was read. The run must stop without results.
    SourceChanged,
    /// A digest exceeded the hash limit.
    Hash(HashError),
    /// The finding collector rejected a finding.
    Findings(FindingError),
    /// An internal invariant failed.
    Internal,
    /// Storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFile => f.write_str("the file does not belong to this snapshot"),
            Self::SourceChanged => f.write_str("the project changed while it was read"),
            Self::Hash(error) => write!(f, "a snapshot digest failed: {error}"),
            Self::Findings(error) => write!(f, "snapshot findings could not be stored: {error}"),
            Self::Internal => f.write_str("internal snapshot invariant failed"),
            Self::AllocationFailed => f.write_str("snapshot storage allocation failed"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// The identity of a whole snapshot: the SHA-256 digest of its framed
/// record stream (`D-067`). It identifies content; it does not authenticate it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId(Sha256Digest);

impl SnapshotId {
    /// Returns the digest of the identity stream.
    pub const fn digest(self) -> Sha256Digest {
        self.0
    }
}

impl fmt::Display for SnapshotId {
    /// Writes the canonical `sha256:` spelling of the digest.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SnapshotId({})", self.0)
    }
}

enum Outcome {
    Content(SnapshotFile),
    Unreadable(IoFailure),
    Failed(SnapshotError),
}

/// Reads admitted files by their index in the inventory's file table.
trait Reader: Send + Sync {
    /// Returns `None` for an index outside the table.
    fn read(&self, index: usize, size: u64) -> Option<Result<Vec<u8>, ReadError>>;
}

struct TreeReader<T: Tree> {
    tree: T,
    files: FileLocations<T>,
}

impl<T> Reader for TreeReader<T>
where
    T: Tree + Send + Sync,
    T::Location: Send + Sync,
    T::Identity: Send + Sync,
{
    fn read(&self, index: usize, size: u64) -> Option<Result<Vec<u8>, ReadError>> {
        let (location, identity) = self.files.get(index)?;
        Some(self.tree.read_file(location, identity, size))
    }
}

impl Snapshot {
    pub(crate) fn new<T>(
        inventory: Inventory,
        tree: T,
        files: FileLocations<T>,
        hash: HashLimits,
    ) -> Result<Self, DiscoveryError>
    where
        T: Tree + Send + Sync + 'static,
        T::Location: Send + Sync + 'static,
        T::Identity: Send + Sync + 'static,
    {
        if files.len() != inventory.files().len() {
            return Err(DiscoveryError::Internal);
        }
        let mut outcomes = Vec::new();
        outcomes
            .try_reserve_exact(files.len())
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        outcomes.resize_with(files.len(), OnceLock::new);
        Ok(Self {
            inventory,
            reader: Box::new(TreeReader { tree, files }),
            outcomes,
            hash,
        })
    }

    /// Borrows what discovery found.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Reads an admitted file on first request and returns the retained
    /// outcome on every later one, without touching the filesystem again.
    ///
    /// The file must still be the listed regular file with its listed size,
    /// and it must not change while it is read; otherwise the result is
    /// [`SnapshotError::SourceChanged`] and the run must stop.
    pub fn read(&self, id: DenseId<SourcePath>) -> Result<FileRead<'_>, SnapshotError> {
        let slot = self
            .outcomes
            .get(id.index())
            .ok_or(SnapshotError::UnknownFile)?;
        let size = self
            .inventory
            .file_size(id)
            .ok_or(SnapshotError::UnknownFile)?;
        match slot.get_or_init(|| self.load(id.index(), size)) {
            Outcome::Content(file) => Ok(FileRead::Content(file)),
            Outcome::Unreadable(failure) => Ok(FileRead::Unreadable(*failure)),
            Outcome::Failed(error) => Err(*error),
        }
    }

    /// Returns whether the file has been read, successfully or not.
    pub fn is_read(&self, id: DenseId<SourcePath>) -> bool {
        self.outcomes
            .get(id.index())
            .is_some_and(|slot| slot.get().is_some())
    }

    /// Reads every file not yet read, in canonical order, and returns the
    /// identity of the whole snapshot: the admitted files with their
    /// contents and every other inventory entry (`D-067`).
    pub fn identity(&self) -> Result<SnapshotId, SnapshotError> {
        let inventory = &self.inventory;
        let mut stream = Stream {
            hasher: Sha256Hasher::new(self.hash),
        };
        stream.field(DOMAIN)?;
        stream.number(FORMAT_VERSION)?;
        stream.tag(u8::from(inventory.is_complete()))?;
        stream.tag(stop_tag(inventory.stop()))?;
        stream.number(inventory.unrepresentable() as u64)?;
        stream.number(inventory.files().len() as u64)?;
        for (id, path) in inventory.files().iter() {
            let size = inventory.file_size(id).ok_or(SnapshotError::Internal)?;
            stream.field(path.as_str().as_bytes())?;
            stream.number(size)?;
            match self.read(id)? {
                FileRead::Content(file) => {
                    stream.tag(1)?;
                    stream.bytes(file.digest.as_bytes())?;
                }
                FileRead::Unreadable(failure) => {
                    stream.tag(2)?;
                    stream.tag(failure_tag(failure))?;
                }
            }
        }
        stream.number(inventory.entries().len() as u64)?;
        for entry in inventory.entries() {
            stream.field(entry.path().as_str().as_bytes())?;
            stream.entry(entry.kind())?;
        }
        Ok(SnapshotId(stream.hasher.finish()))
    }

    /// Reports every file read so far whose read failed with `FS1019`, in
    /// canonical order. Call it once, after the last read of the run.
    pub fn report_unreadable(&self, findings: &mut FindingCollector) -> Result<(), SnapshotError> {
        for ((_, path), slot) in self.inventory.files().iter().zip(&self.outcomes) {
            let Some(Outcome::Unreadable(failure)) = slot.get() else {
                continue;
            };
            let explanation = format!(
                "The operating system reported: {}. The file's content was not read, so it is \
                 not analyzed or migrated.",
                failure.describe()
            );
            let text = FindingText {
                message: "File could not be read",
                explanation: &explanation,
                remediation: "Check the file's permissions, then run Frameshift again.",
            };
            let location = FindingLocation::File(path.clone());
            walk::push_finding(findings, "FS1019", Severity::Error, location, text).map_err(
                |error| match error {
                    ReportError::Code => SnapshotError::Internal,
                    ReportError::Findings(error) => SnapshotError::Findings(error),
                },
            )?;
        }
        Ok(())
    }

    fn load(&self, index: usize, size: u64) -> Outcome {
        match self.reader.read(index, size) {
            None => Outcome::Failed(SnapshotError::Internal),
            Some(Ok(bytes)) => match Sha256Digest::hash(&bytes, self.hash) {
                Ok(digest) => Outcome::Content(SnapshotFile { bytes, digest }),
                Err(error) => Outcome::Failed(SnapshotError::Hash(error)),
            },
            Some(Err(ReadError::Changed)) => Outcome::Failed(SnapshotError::SourceChanged),
            Some(Err(ReadError::Io(failure))) => Outcome::Unreadable(failure),
            Some(Err(ReadError::AllocationFailed)) => {
                Outcome::Failed(SnapshotError::AllocationFailed)
            }
        }
    }
}

/// The framed identity stream: 8-byte big-endian integers, single-byte
/// tags, and length-prefixed variable fields, so that different inventories
/// never produce the same bytes.
struct Stream {
    hasher: Sha256Hasher,
}

impl Stream {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        self.hasher.update(bytes).map_err(SnapshotError::Hash)
    }

    fn tag(&mut self, tag: u8) -> Result<(), SnapshotError> {
        self.bytes(&[tag])
    }

    fn number(&mut self, value: u64) -> Result<(), SnapshotError> {
        self.bytes(&value.to_be_bytes())
    }

    fn field(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        self.number(bytes.len() as u64)?;
        self.bytes(bytes)
    }

    fn entry(&mut self, kind: &EntryKind) -> Result<(), SnapshotError> {
        match kind {
            EntryKind::Directory => self.tag(1),
            EntryKind::Ignored { reason, directory } => {
                self.tag(2)?;
                self.tag(reason_tag(*reason))?;
                self.tag(u8::from(*directory))
            }
            EntryKind::Oversized { bytes } => {
                self.tag(3)?;
                self.number(*bytes)
            }
            EntryKind::Link(target) => {
                self.tag(4)?;
                match target {
                    LinkTarget::Inside(LinkReference::Root) => self.tag(1),
                    LinkTarget::Inside(LinkReference::Path(path)) => {
                        self.tag(2)?;
                        self.field(path.as_str().as_bytes())
                    }
                    LinkTarget::Escapes => self.tag(3),
                    LinkTarget::Dangling => self.tag(4),
                    LinkTarget::Unresolved(reason) => {
                        self.tag(5)?;
                        self.tag(unresolved_tag(*reason))
                    }
                }
            }
            EntryKind::ReparsePoint { directory } => {
                self.tag(5)?;
                self.tag(u8::from(*directory))
            }
            EntryKind::Special => self.tag(6),
            EntryKind::NotTraversed(reason) => {
                self.tag(7)?;
                self.tag(match reason {
                    NotTraversed::DepthLimit => 1,
                    NotTraversed::Unreadable => 2,
                    NotTraversed::IgnoreFileUnreadable => 3,
                })
            }
            EntryKind::Uninspectable => self.tag(8),
        }
    }
}

const fn stop_tag(stop: Option<Stop>) -> u8 {
    match stop {
        None => 0,
        Some(Stop::Files) => 1,
        Some(Stop::TotalBytes) => 2,
        Some(Stop::Entries) => 3,
        Some(Stop::IgnoreBytes) => 4,
        Some(Stop::IgnoreWork) => 5,
    }
}

const fn failure_tag(failure: IoFailure) -> u8 {
    match failure {
        IoFailure::PermissionDenied => 1,
        IoFailure::NotFound => 2,
        IoFailure::Other => 3,
    }
}

const fn reason_tag(reason: IgnoreReason) -> u8 {
    match reason {
        IgnoreReason::Vcs => 1,
        IgnoreReason::Dependencies => 2,
        IgnoreReason::BuildOutput => 3,
        IgnoreReason::Environment => 4,
        IgnoreReason::Gitignore => 5,
    }
}

const fn unresolved_tag(reason: Unresolved) -> u8 {
    match reason {
        Unresolved::Absolute => 1,
        Unresolved::ThroughLink => 2,
        Unresolved::Untraversed => 3,
        Unresolved::Unreadable => 4,
        Unresolved::Unrepresentable => 5,
        Unresolved::Incomplete => 6,
    }
}

#[cfg(test)]
mod tests;
