//! The read-only discovery walker (`D-064`).
//!
//! The walker sees the filesystem only through [`Tree`], so tests can supply
//! trees whose listings arrive in arbitrary order. It visits siblings sorted
//! by name bytes, with `/` appended to directory names, depth first: this is
//! byte-wise `SourcePath` order for all descendants, and the walker charges
//! every budget in that order.

use std::cmp::Ordering;
use std::fmt;
use std::path::Path;

use frameshift_core::{
    Budget, CollectionError, Finding, FindingCode, FindingCollector, FindingContext, FindingError,
    FindingLimits, FindingLocation, FindingText, KeyTable, PathError, PathLimits, RunLimits,
    Severity, SourcePath,
};

use crate::gitignore::{GitignoreFile, IgnoreDecision, IgnoreLimits};
use crate::inventory::{
    Entry, EntryKind, IgnoreReason, Inventory, LinkReference, LinkTarget, NotTraversed, Stop,
    Unresolved,
};
use crate::native::NativeTree;

/// The limits that discovery applies, with their documented defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryLimits {
    /// File count, file size, total size, directory depth, and entry count.
    pub run: RunLimits,
    /// Canonical path size and component count.
    pub path: PathLimits,
    /// `.gitignore` bytes and matching work.
    pub ignore: IgnoreLimits,
}

/// An operating system failure, reduced to a platform-independent category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IoFailure {
    /// Access was denied.
    PermissionDenied,
    /// The entry did not exist.
    NotFound,
    /// Any other failure.
    Other,
}

impl IoFailure {
    pub(crate) fn from_io(error: &std::io::Error) -> Self {
        // `ErrorKind` is not exhaustive, so only the two named kinds are tested.
        let kind = error.kind();
        if kind == std::io::ErrorKind::PermissionDenied {
            Self::PermissionDenied
        } else if kind == std::io::ErrorKind::NotFound {
            Self::NotFound
        } else {
            Self::Other
        }
    }

    pub(crate) const fn describe(self) -> &'static str {
        match self {
            Self::PermissionDenied => "permission denied",
            Self::NotFound => "not found",
            Self::Other => "an input or output error",
        }
    }
}

/// A failure that leaves no inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryError {
    /// The root could not be resolved or listed.
    Root(IoFailure),
    /// The root is not a directory.
    RootNotDirectory,
    /// A directory or `.gitignore` file changed between listing and use.
    SourceChanged,
    /// The finding collector rejected a finding, for example at its count limit.
    Findings(FindingError),
    /// An internal invariant failed. The result would not be trustworthy.
    Internal,
    /// Storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(failure) => {
                write!(f, "project root could not be read: {}", failure.describe())
            }
            Self::RootNotDirectory => f.write_str("project root is not a directory"),
            Self::SourceChanged => f.write_str("the project changed during discovery"),
            Self::Findings(error) => write!(f, "discovery findings could not be stored: {error}"),
            Self::Internal => f.write_str("internal discovery invariant failed"),
            Self::AllocationFailed => f.write_str("discovery storage allocation failed"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// Walks `root` read-only and accounts for every entry below it.
///
/// The root is canonicalized first; links in the root path itself are
/// therefore resolved, while no link below it is followed. Findings are
/// appended to `findings`. Only `.gitignore` files are opened.
pub fn discover(
    root: &Path,
    limits: DiscoveryLimits,
    findings: &mut FindingCollector,
) -> Result<Inventory, DiscoveryError> {
    let (root, identity) = NativeTree::open_root(root)?;
    walk(&NativeTree, root, identity, limits, findings)
}

/// The native location and listing identity of every admitted file, in the
/// order of the inventory's file table.
pub(crate) type FileLocations<T> = Vec<(<T as Tree>::Location, <T as Tree>::Identity)>;

/// Why an input-layer finding could not be stored.
pub(crate) enum ReportError {
    /// The code is not a valid finding code, an internal defect.
    Code,
    /// The collector rejected the finding.
    Findings(FindingError),
}

/// Builds an input-layer finding and appends it to the run's collector.
///
/// Findings are built with permissive construction limits; the collector
/// applies its own limits on insertion.
pub(crate) fn push_finding(
    findings: &mut FindingCollector,
    code: &str,
    severity: Severity,
    location: FindingLocation,
    text: FindingText<'_>,
) -> Result<(), ReportError> {
    let code = FindingCode::new(code).map_err(|_| ReportError::Code)?;
    let construction = FindingLimits {
        max_text_bytes: usize::MAX,
        max_findings: usize::MAX,
    };
    let finding = Finding::new(
        code,
        severity,
        location,
        FindingContext::default(),
        text,
        construction,
    )
    .map_err(ReportError::Findings)?;
    findings.push(finding).map_err(ReportError::Findings)
}

/// Filesystem access for the walker.
pub(crate) trait Tree {
    /// Where a directory or entry is.
    type Location;
    /// A native entry name, used to derive a child's location.
    type Name;
    /// What identifies a listed entry when it is used later.
    type Identity;

    /// Lists a directory, charging one entry per item read.
    fn list(
        &self,
        directory: &Self::Location,
        entries: &mut Budget,
    ) -> Result<Vec<Listed<Self>>, ListError>;
    /// Returns the location of a listed entry.
    fn child(&self, directory: &Self::Location, name: &Self::Name) -> Self::Location;
    /// Checks after listing that a directory is still the real directory that was listed.
    fn is_same_directory(&self, directory: &Self::Location, identity: &Self::Identity) -> bool;
    /// Reads at most `max + 1` bytes of a regular file without following links.
    fn read_regular(
        &self,
        file: &Self::Location,
        identity: &Self::Identity,
        max: u64,
    ) -> Result<Vec<u8>, ReadError>;
    /// Reads exactly `size` bytes of an admitted regular file without
    /// following links, or reports that it is no longer the listed file.
    fn read_file(
        &self,
        file: &Self::Location,
        identity: &Self::Identity,
        size: u64,
    ) -> Result<Vec<u8>, ReadError>;
    /// Reads a link's target without following it.
    fn read_link(&self, link: &Self::Location) -> RawTarget;
}

/// One listed entry.
pub(crate) struct Listed<T: Tree + ?Sized> {
    pub(crate) name: T::Name,
    /// The name as Unicode, or `None` when it is not Unicode.
    pub(crate) text: Option<String>,
    pub(crate) kind: RawKind,
    pub(crate) identity: T::Identity,
}

/// What a listing reports about an entry, without following links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawKind {
    Directory,
    File {
        size: u64,
    },
    Link,
    // Only Windows listings report reparse points, and only Unix ones
    // report special files; tests construct both everywhere.
    #[cfg_attr(not(windows), allow(dead_code))]
    ReparsePoint {
        directory: bool,
    },
    #[cfg_attr(not(unix), allow(dead_code))]
    Special,
    Uninspectable(IoFailure),
}

pub(crate) enum ListError {
    EntryLimit,
    Io(IoFailure),
}

pub(crate) enum ReadError {
    /// The entry is no longer the regular file that was listed, or it
    /// changed while it was read.
    Changed,
    Io(IoFailure),
    AllocationFailed,
}

/// A link target as the link stores it.
pub(crate) enum RawTarget {
    Absolute,
    Empty,
    Unreadable,
    Relative {
        components: Vec<TargetComponent>,
        /// The target ends with a separator, so it must be a directory.
        directory: bool,
    },
}

pub(crate) enum TargetComponent {
    Current,
    Parent,
    /// A name, or `None` when it is not Unicode.
    Name(Option<String>),
}

#[derive(Clone, Copy)]
enum Unrepresentable {
    NotUnicode,
    Separator,
    Backslash,
    DrivePrefix,
    PathTooLong(usize),
    TooManyComponents(usize),
    InvalidName,
}

impl Unrepresentable {
    fn from_path(error: PathError) -> Result<Self, DiscoveryError> {
        Ok(match error {
            PathError::Backslash { .. } => Self::Backslash,
            PathError::DrivePrefix => Self::DrivePrefix,
            PathError::TooLong { limit, .. } => Self::PathTooLong(limit),
            PathError::TooManyComponents { limit, .. } => Self::TooManyComponents(limit),
            PathError::AllocationFailed => return Err(DiscoveryError::AllocationFailed),
            PathError::Empty
            | PathError::InvalidUtf8 { .. }
            | PathError::Absolute
            | PathError::EmptyComponent { .. }
            | PathError::DotComponent { .. }
            | PathError::Nul { .. }
            | PathError::OutputControl { .. }
            | PathError::OutputCharacter { .. }
            | PathError::OutputEnding { .. }
            | PathError::OutputDevice { .. }
            | PathError::OutputComponentTooLong { .. } => Self::InvalidName,
        })
    }

    fn describe(self) -> String {
        match self {
            Self::NotUnicode => "the name is not valid Unicode".to_owned(),
            Self::Separator => "the name contains a path separator".to_owned(),
            Self::Backslash => "the name contains a backslash".to_owned(),
            Self::DrivePrefix => "the first path component looks like a drive prefix".to_owned(),
            Self::PathTooLong(limit) => {
                format!("the path exceeds the limit of {limit} bytes (path.max-path-bytes)")
            }
            Self::TooManyComponents(limit) => {
                format!("the path exceeds the limit of {limit} components (path.max-components)")
            }
            Self::InvalidName => "the name is not a valid path component".to_owned(),
        }
    }
}

/// The patterns of one `.gitignore` file and the directory they apply to.
struct IgnoreLevel {
    depth: usize,
    /// Byte length of the directory's path plus its slash, so that
    /// `path[base..]` is relative to the directory.
    base: usize,
    file: GitignoreFile,
}

struct Child<T: Tree> {
    path: SourcePath,
    name: T::Name,
    kind: RawKind,
    identity: T::Identity,
}

struct Frame<T: Tree> {
    location: T::Location,
    depth: usize,
    /// Unvisited children in descending canonical order, visited from the end.
    children: Vec<Child<T>>,
}

/// The outcome of looking up a path in the walked inventory.
enum Found {
    Directory,
    File,
    Link,
    Untraversed,
    Absent,
    Unrepresentable,
}

pub(crate) fn walk<T: Tree>(
    tree: &T,
    root: T::Location,
    identity: T::Identity,
    limits: DiscoveryLimits,
    findings: &mut FindingCollector,
) -> Result<Inventory, DiscoveryError> {
    walk_files(tree, root, identity, limits, findings).map(|(inventory, _)| inventory)
}

/// Walks like [`walk`] and also returns where each admitted file is, for
/// the snapshot store.
pub(crate) fn walk_files<T: Tree>(
    tree: &T,
    root: T::Location,
    identity: T::Identity,
    limits: DiscoveryLimits,
    findings: &mut FindingCollector,
) -> Result<(Inventory, FileLocations<T>), DiscoveryError> {
    let mut walker = Walker {
        tree,
        limits,
        findings,
        files: Budget::new(limits.run.max_files as u64),
        bytes: Budget::new(limits.run.max_total_bytes),
        entries: Budget::new(limits.run.max_entries as u64),
        ignore_bytes: Budget::new(limits.ignore.max_total_bytes),
        work: Budget::new(limits.ignore.max_match_work),
        admitted: Vec::new(),
        locations: Vec::new(),
        recorded: Vec::new(),
        links: Vec::new(),
        ignores: Vec::new(),
        unrepresentable: 0,
        complete: true,
        stop: None,
    };
    walker.run(root, identity)?;
    walker.finish()
}

struct Walker<'a, T: Tree> {
    tree: &'a T,
    limits: DiscoveryLimits,
    findings: &'a mut FindingCollector,
    files: Budget,
    bytes: Budget,
    entries: Budget,
    ignore_bytes: Budget,
    work: Budget,
    admitted: Vec<(SourcePath, u64)>,
    locations: FileLocations<T>,
    recorded: Vec<Entry>,
    links: Vec<(SourcePath, RawTarget)>,
    ignores: Vec<IgnoreLevel>,
    unrepresentable: usize,
    complete: bool,
    stop: Option<Stop>,
}

impl<T: Tree> Walker<'_, T> {
    fn run(&mut self, root: T::Location, identity: T::Identity) -> Result<(), DiscoveryError> {
        let mut stack = Vec::new();
        if let Some(frame) = self.open(None, root, &identity, 0)? {
            stack.push(frame);
        }
        while self.stop.is_none() {
            let Some(frame) = stack.last_mut() else {
                break;
            };
            let Some(child) = frame.children.pop() else {
                let depth = frame.depth;
                stack.pop();
                while self
                    .ignores
                    .last()
                    .is_some_and(|level| level.depth >= depth)
                {
                    self.ignores.pop();
                }
                continue;
            };
            let location = self.tree.child(&frame.location, &child.name);
            let depth = frame.depth + 1;
            if let Some(next) = self.visit(child, location, depth)? {
                stack
                    .try_reserve(1)
                    .map_err(|_| DiscoveryError::AllocationFailed)?;
                stack.push(next);
            }
        }
        Ok(())
    }

    /// Lists a directory, reads its `.gitignore` file, and returns its
    /// frame, or `None` when it is not traversed or discovery stopped.
    fn open(
        &mut self,
        path: Option<&SourcePath>,
        location: T::Location,
        identity: &T::Identity,
        depth: usize,
    ) -> Result<Option<Frame<T>>, DiscoveryError> {
        let listed = match self.tree.list(&location, &mut self.entries) {
            Ok(listed) => listed,
            Err(ListError::EntryLimit) => {
                let explanation = format!(
                    "Reading this directory would exceed the limit of {} directory entries \
                     (run.max-entries). Discovery stopped here, so the inventory is incomplete.",
                    self.limits.run.max_entries
                );
                self.halt(Stop::Entries, scope(path), &explanation)?;
                return Ok(None);
            }
            Err(ListError::Io(failure)) => {
                let Some(path) = path else {
                    return Err(DiscoveryError::Root(failure));
                };
                self.record(
                    path.clone(),
                    EntryKind::NotTraversed(NotTraversed::Unreadable),
                )?;
                self.complete = false;
                let explanation = format!(
                    "The operating system reported: {}. The directory's contents were not \
                     discovered, so the inventory is incomplete.",
                    failure.describe()
                );
                self.report(
                    "FS1016",
                    Severity::Error,
                    FindingLocation::File(path.clone()),
                    "Directory could not be read",
                    &explanation,
                    "Check the directory's permissions, then run Frameshift again.",
                )?;
                return Ok(None);
            }
        };
        if !self.tree.is_same_directory(&location, identity) {
            return Err(DiscoveryError::SourceChanged);
        }
        let mut children = Vec::new();
        children
            .try_reserve_exact(listed.len())
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        for item in listed {
            match self.child_path(path, item.text.as_deref())? {
                Ok(child) => children.push(Child {
                    path: child,
                    name: item.name,
                    kind: item.kind,
                    identity: item.identity,
                }),
                Err(reason) => {
                    self.unrepresentable(path, reason, item.kind == RawKind::Directory)?;
                }
            }
        }
        children.sort_unstable_by(|left, right| canonical(right, left));
        if !self.read_gitignore(path, &location, &children, depth)? {
            return Ok(None);
        }
        if let Some(path) = path {
            self.record(path.clone(), EntryKind::Directory)?;
        }
        Ok(Some(Frame {
            location,
            depth,
            children,
        }))
    }

    /// Reads and applies the directory's `.gitignore` file, if it has a
    /// regular one. Returns whether the directory can be traversed.
    fn read_gitignore(
        &mut self,
        directory: Option<&SourcePath>,
        location: &T::Location,
        children: &[Child<T>],
        depth: usize,
    ) -> Result<bool, DiscoveryError> {
        let Some(child) = children
            .iter()
            .find(|child| name(&child.path) == ".gitignore")
        else {
            return Ok(true);
        };
        let RawKind::File { size } = child.kind else {
            // Git does not read a linked or special `.gitignore` either.
            return Ok(true);
        };
        let file = FindingLocation::File(child.path.clone());
        let over = |limit: u64| {
            format!(
                "Reading this .gitignore file would exceed the limit of {limit} bytes for all \
                 .gitignore files (ignore.max-total-bytes). Discovery stopped here, so the \
                 inventory is incomplete."
            )
        };
        let remaining = self.ignore_bytes.remaining();
        if size > remaining {
            self.halt(Stop::IgnoreBytes, file, &over(self.ignore_bytes.limit()))?;
            return Ok(false);
        }
        let location = self.tree.child(location, &child.name);
        let content = match self
            .tree
            .read_regular(&location, &child.identity, remaining)
        {
            Ok(content) => content,
            Err(ReadError::Changed) => return Err(DiscoveryError::SourceChanged),
            Err(ReadError::AllocationFailed) => return Err(DiscoveryError::AllocationFailed),
            Err(ReadError::Io(failure)) => {
                self.complete = false;
                if let Some(directory) = directory {
                    self.record(
                        directory.clone(),
                        EntryKind::NotTraversed(NotTraversed::IgnoreFileUnreadable),
                    )?;
                }
                let explanation = format!(
                    "The operating system reported: {}. Without its patterns the directory's \
                     entries cannot be classified, so its contents were not discovered and the \
                     inventory is incomplete.",
                    failure.describe()
                );
                self.report(
                    "FS1018",
                    Severity::Error,
                    file,
                    ".gitignore file could not be read",
                    &explanation,
                    "Check the file's permissions, then run Frameshift again.",
                )?;
                return Ok(false);
            }
        };
        if self.ignore_bytes.charge(content.len() as u64).is_err() {
            self.halt(Stop::IgnoreBytes, file, &over(self.ignore_bytes.limit()))?;
            return Ok(false);
        }
        let patterns =
            GitignoreFile::parse(&content).map_err(|_| DiscoveryError::AllocationFailed)?;
        if !patterns.is_empty() {
            self.ignores
                .try_reserve(1)
                .map_err(|_| DiscoveryError::AllocationFailed)?;
            self.ignores.push(IgnoreLevel {
                depth,
                base: directory.map_or(0, |directory| directory.as_str().len() + 1),
                file: patterns,
            });
        }
        Ok(true)
    }

    /// Classifies one child in canonical order and returns the frame of a
    /// directory to traverse next.
    fn visit(
        &mut self,
        child: Child<T>,
        location: T::Location,
        depth: usize,
    ) -> Result<Option<Frame<T>>, DiscoveryError> {
        let directory = child.kind == RawKind::Directory;
        if let Some(reason) = builtin(name(&child.path)) {
            self.record(child.path, EntryKind::Ignored { reason, directory })?;
            return Ok(None);
        }
        match self.gitignored(&child.path, directory) {
            Ok(false) => {}
            Ok(true) => {
                let reason = IgnoreReason::Gitignore;
                self.record(child.path, EntryKind::Ignored { reason, directory })?;
                return Ok(None);
            }
            Err(()) => {
                let explanation = format!(
                    "Matching this entry against .gitignore patterns would exceed the work limit \
                     of {} (ignore.max-match-work). Discovery stopped here, so the inventory is \
                     incomplete.",
                    self.work.limit()
                );
                self.halt(
                    Stop::IgnoreWork,
                    FindingLocation::File(child.path),
                    &explanation,
                )?;
                return Ok(None);
            }
        }
        match child.kind {
            RawKind::Directory => {
                if depth <= self.limits.run.max_directory_depth {
                    return self.open(Some(&child.path), location, &child.identity, depth);
                }
                self.complete = false;
                let explanation = format!(
                    "The directory lies at depth {depth}, deeper than the limit of {} \
                     (run.max-directory-depth). Its contents were not discovered, so the \
                     inventory is incomplete.",
                    self.limits.run.max_directory_depth
                );
                self.report(
                    "FS1009",
                    Severity::Unknown,
                    FindingLocation::File(child.path.clone()),
                    "Directory exceeds the depth limit",
                    &explanation,
                    "Flatten the directory structure, or exclude the directory if it is not \
                     project source.",
                )?;
                self.record(
                    child.path,
                    EntryKind::NotTraversed(NotTraversed::DepthLimit),
                )?;
            }
            RawKind::File { size } => self.admit(child.path, size, location, child.identity)?,
            RawKind::Link => {
                let target = self.tree.read_link(&location);
                self.links
                    .try_reserve(1)
                    .map_err(|_| DiscoveryError::AllocationFailed)?;
                self.links.push((child.path.clone(), target));
                let placeholder = LinkTarget::Unresolved(Unresolved::Incomplete);
                self.record(child.path, EntryKind::Link(placeholder))?;
            }
            RawKind::ReparsePoint { directory } => {
                self.report(
                    "FS1008",
                    Severity::Unknown,
                    FindingLocation::File(child.path.clone()),
                    "Reparse point not followed",
                    "This Windows reparse point is neither a symbolic link nor a junction, for \
                     example a cloud placeholder. Frameshift never opens reparse points, so its \
                     content is not analyzed or migrated.",
                    "Make the content available as a regular local file or directory, for \
                     example by keeping cloud files on the device.",
                )?;
                self.record(child.path, EntryKind::ReparsePoint { directory })?;
            }
            RawKind::Special => {
                self.report(
                    "FS1002",
                    Severity::Info,
                    FindingLocation::File(child.path.clone()),
                    "Special file skipped",
                    "Device files, named pipes, and sockets are not project source. Frameshift \
                     never opens them and does not migrate them.",
                    "No action is needed unless the project expects this entry in its output.",
                )?;
                self.record(child.path, EntryKind::Special)?;
            }
            RawKind::Uninspectable(failure) => {
                self.complete = false;
                let explanation = format!(
                    "The operating system did not report this entry's type or size ({}). It is \
                     recorded without a kind, so the inventory is incomplete.",
                    failure.describe()
                );
                self.report(
                    "FS1017",
                    Severity::Error,
                    FindingLocation::File(child.path.clone()),
                    "Entry could not be inspected",
                    &explanation,
                    "Check the entry's permissions and whether another program is changing the \
                     project, then run Frameshift again.",
                )?;
                self.record(child.path, EntryKind::Uninspectable)?;
            }
        }
        Ok(None)
    }

    /// Admits a regular file, or accounts for it when it is too large, or
    /// stops discovery when a cumulative budget would be exceeded.
    fn admit(
        &mut self,
        path: SourcePath,
        size: u64,
        location: T::Location,
        identity: T::Identity,
    ) -> Result<(), DiscoveryError> {
        let run = self.limits.run;
        if size > run.max_file_bytes {
            let explanation = format!(
                "The file has {size} bytes, more than the limit of {} bytes per file \
                 (run.max-file-bytes). It is accounted for but not read, analyzed, or migrated.",
                run.max_file_bytes
            );
            self.report(
                "FS1004",
                Severity::Unknown,
                FindingLocation::File(path.clone()),
                "File exceeds the size limit",
                &explanation,
                "Reduce the file, or exclude it if it is not project source.",
            )?;
            return self.record(path, EntryKind::Oversized { bytes: size });
        }
        if self.files.remaining() == 0 {
            let explanation = format!(
                "Admitting this file would exceed the limit of {} files (run.max-files). \
                 Discovery stopped here, in canonical path order, so the inventory is incomplete.",
                run.max_files
            );
            return self.halt(Stop::Files, FindingLocation::File(path), &explanation);
        }
        if self.bytes.remaining() < size {
            let explanation = format!(
                "Admitting this file of {size} bytes would exceed the limit of {} bytes in total \
                 (run.max-total-bytes). Discovery stopped here, in canonical path order, so the \
                 inventory is incomplete.",
                run.max_total_bytes
            );
            return self.halt(Stop::TotalBytes, FindingLocation::File(path), &explanation);
        }
        if self.files.charge(1).is_err() || self.bytes.charge(size).is_err() {
            return Err(DiscoveryError::Internal);
        }
        // Budgets are only meaningful in canonical order; verify it.
        if self.admitted.last().is_some_and(|(last, _)| *last >= path) {
            return Err(DiscoveryError::Internal);
        }
        self.admitted
            .try_reserve(1)
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        self.locations
            .try_reserve(1)
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        self.admitted.push((path, size));
        self.locations.push((location, identity));
        Ok(())
    }

    /// Applies the `.gitignore` levels from the deepest to the root. The
    /// first file whose patterns match decides.
    fn gitignored(&mut self, path: &SourcePath, directory: bool) -> Result<bool, ()> {
        for level in self.ignores.iter().rev() {
            let relative = &path.as_str()[level.base..];
            match level.file.decide(relative, directory, &mut self.work) {
                Ok(Some(decision)) => return Ok(decision == IgnoreDecision::Ignored),
                Ok(None) => {}
                Err(_) => return Err(()),
            }
        }
        Ok(false)
    }

    /// Builds a child's path, or the reason it cannot be represented.
    fn child_path(
        &self,
        parent: Option<&SourcePath>,
        name: Option<&str>,
    ) -> Result<Result<SourcePath, Unrepresentable>, DiscoveryError> {
        let Some(name) = name else {
            return Ok(Err(Unrepresentable::NotUnicode));
        };
        if name.contains('/') {
            return Ok(Err(Unrepresentable::Separator));
        }
        let mut joined = String::new();
        let prefix = parent.map_or("", SourcePath::as_str);
        joined
            .try_reserve_exact(prefix.len() + 1 + name.len())
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        if !prefix.is_empty() {
            joined.push_str(prefix);
            joined.push('/');
        }
        joined.push_str(name);
        match SourcePath::new(&joined, self.limits.path) {
            Ok(path) => Ok(Ok(path)),
            Err(error) => Unrepresentable::from_path(error).map(Err),
        }
    }

    fn unrepresentable(
        &mut self,
        parent: Option<&SourcePath>,
        reason: Unrepresentable,
        directory: bool,
    ) -> Result<(), DiscoveryError> {
        self.unrepresentable += 1;
        let consequence = if directory {
            self.complete = false;
            "It is a directory, so its contents were not discovered and the inventory is \
             incomplete."
        } else {
            "It is excluded from the inventory and only counted."
        };
        let explanation = format!(
            "An entry in this directory cannot be represented as a canonical project path: {}. \
             Its name is not shown. {consequence}",
            reason.describe()
        );
        self.report(
            "FS1001",
            Severity::Unknown,
            scope(parent),
            "Entry name cannot be represented as a project path",
            &explanation,
            "Rename the entry to valid Unicode without backslashes, or shorten its path, if it \
             belongs to the project.",
        )
    }

    fn halt(
        &mut self,
        stop: Stop,
        location: FindingLocation,
        explanation: &str,
    ) -> Result<(), DiscoveryError> {
        self.stop = Some(stop);
        self.complete = false;
        let (code, message) = match stop {
            Stop::Files => ("FS1011", "File count limit reached; discovery stopped"),
            Stop::TotalBytes => ("FS1012", "Total size limit reached; discovery stopped"),
            Stop::Entries => ("FS1013", "Entry limit reached; discovery stopped"),
            Stop::IgnoreBytes => ("FS1014", "Ignore file limit reached; discovery stopped"),
            Stop::IgnoreWork => ("FS1015", "Ignore matching limit reached; discovery stopped"),
        };
        self.report(
            code,
            Severity::Unknown,
            location,
            message,
            explanation,
            "Exclude generated or vendored content, for example with .gitignore, or analyze a \
             smaller directory.",
        )
    }

    fn record(&mut self, path: SourcePath, kind: EntryKind) -> Result<(), DiscoveryError> {
        self.recorded
            .try_reserve(1)
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        self.recorded.push(Entry { path, kind });
        Ok(())
    }

    fn report(
        &mut self,
        code: &str,
        severity: Severity,
        location: FindingLocation,
        message: &str,
        explanation: &str,
        remediation: &str,
    ) -> Result<(), DiscoveryError> {
        let text = FindingText {
            message,
            explanation,
            remediation,
        };
        push_finding(self.findings, code, severity, location, text).map_err(|error| match error {
            ReportError::Code => DiscoveryError::Internal,
            ReportError::Findings(error) => DiscoveryError::Findings(error),
        })
    }

    /// Sorts the entries, resolves link targets against them, and builds
    /// the file identities.
    fn finish(mut self) -> Result<(Inventory, FileLocations<T>), DiscoveryError> {
        self.recorded
            .sort_unstable_by(|left, right| left.path.cmp(&right.path));
        if self
            .recorded
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(DiscoveryError::Internal);
        }
        for (path, target) in std::mem::take(&mut self.links) {
            let resolved = self.resolve(&path, &target)?;
            self.report_link(&path, &resolved)?;
            let index = self
                .recorded
                .binary_search_by(|entry| entry.path.cmp(&path))
                .map_err(|_| DiscoveryError::Internal)?;
            self.recorded[index].kind = EntryKind::Link(resolved);
        }
        let mut paths = Vec::new();
        let mut sizes = Vec::new();
        paths
            .try_reserve_exact(self.admitted.len())
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        sizes
            .try_reserve_exact(self.admitted.len())
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        for (path, size) in self.admitted {
            paths.push(path);
            sizes.push(size);
        }
        let collection = |error| match error {
            CollectionError::AllocationFailed => DiscoveryError::AllocationFailed,
            CollectionError::TooManyEntries { .. }
            | CollectionError::DuplicateKey { .. }
            | CollectionError::UnknownId { .. }
            | CollectionError::OccupiedSlot { .. }
            | CollectionError::MissingResult { .. } => DiscoveryError::Internal,
        };
        let files = KeyTable::new(paths, self.limits.run.max_files).map_err(collection)?;
        let mut slots = files.slots().map_err(collection)?;
        // Admission order is the table's order, as verified in `admit`.
        for ((id, _), size) in files.iter().zip(sizes) {
            slots.insert(id, size).map_err(collection)?;
        }
        let sizes = slots.finish().map_err(collection)?;
        let inventory = Inventory {
            files,
            sizes,
            entries: self.recorded,
            unrepresentable: self.unrepresentable,
            stop: self.stop,
            complete: self.complete,
        };
        Ok((inventory, self.locations))
    }

    /// Resolves a link target lexically. Every directory that resolution
    /// passes through must be a traversed real directory, so the lexical
    /// result equals what following the link would reach.
    fn resolve(&self, link: &SourcePath, target: &RawTarget) -> Result<LinkTarget, DiscoveryError> {
        let (components, directory) = match target {
            RawTarget::Absolute => return Ok(LinkTarget::Unresolved(Unresolved::Absolute)),
            RawTarget::Unreadable => return Ok(LinkTarget::Unresolved(Unresolved::Unreadable)),
            RawTarget::Empty => return Ok(LinkTarget::Dangling),
            RawTarget::Relative {
                components,
                directory,
            } => (components, *directory),
        };
        let mut stack: Vec<&str> = link.components().collect();
        stack.pop();
        // The link's own directory was traversed, so it is verified.
        let mut verified = true;
        for component in components {
            let step = match component {
                TargetComponent::Current => continue,
                TargetComponent::Name(None) => {
                    return Ok(LinkTarget::Unresolved(Unresolved::Unrepresentable));
                }
                TargetComponent::Parent => None,
                TargetComponent::Name(Some(name)) => Some(name.as_str()),
            };
            if !verified {
                if let Some(outcome) = self.through_directory(&stack)? {
                    return Ok(outcome);
                }
                verified = true;
            }
            match step {
                None => {
                    // The parent of a traversed directory is traversed too.
                    if stack.pop().is_none() {
                        return Ok(LinkTarget::Escapes);
                    }
                }
                Some(name) => {
                    stack.push(name);
                    verified = false;
                }
            }
        }
        if stack.is_empty() {
            return Ok(LinkTarget::Inside(LinkReference::Root));
        }
        if !verified {
            let outcome = if directory {
                self.through_directory(&stack)?
            } else {
                match self.lookup(&stack)? {
                    Found::Absent => Some(self.absent()),
                    Found::Unrepresentable => {
                        Some(LinkTarget::Unresolved(Unresolved::Unrepresentable))
                    }
                    Found::Directory | Found::File | Found::Link | Found::Untraversed => None,
                }
            };
            if let Some(outcome) = outcome {
                return Ok(outcome);
            }
        }
        match self.join(&stack)? {
            Some(path) => Ok(LinkTarget::Inside(LinkReference::Path(path))),
            None => Ok(LinkTarget::Unresolved(Unresolved::Unrepresentable)),
        }
    }

    /// Checks that resolution may continue through `stack` as a directory,
    /// or returns the outcome that ends it.
    fn through_directory(&self, stack: &[&str]) -> Result<Option<LinkTarget>, DiscoveryError> {
        Ok(match self.lookup(stack)? {
            Found::Directory => None,
            Found::Link => Some(LinkTarget::Unresolved(Unresolved::ThroughLink)),
            Found::Untraversed => Some(LinkTarget::Unresolved(Unresolved::Untraversed)),
            // Using a file as a directory fails, as following the link would.
            Found::File => Some(LinkTarget::Dangling),
            Found::Absent => Some(self.absent()),
            Found::Unrepresentable => Some(LinkTarget::Unresolved(Unresolved::Unrepresentable)),
        })
    }

    fn absent(&self) -> LinkTarget {
        if self.complete {
            LinkTarget::Dangling
        } else {
            LinkTarget::Unresolved(Unresolved::Incomplete)
        }
    }

    fn lookup(&self, stack: &[&str]) -> Result<Found, DiscoveryError> {
        let Some(path) = self.join(stack)? else {
            return Ok(Found::Unrepresentable);
        };
        if self
            .admitted
            .binary_search_by(|(admitted, _)| admitted.cmp(&path))
            .is_ok()
        {
            return Ok(Found::File);
        }
        let Ok(index) = self
            .recorded
            .binary_search_by(|entry| entry.path.cmp(&path))
        else {
            return Ok(Found::Absent);
        };
        Ok(match self.recorded[index].kind {
            EntryKind::Directory => Found::Directory,
            EntryKind::Oversized { .. } | EntryKind::Special => Found::File,
            EntryKind::Link(_) | EntryKind::ReparsePoint { .. } => Found::Link,
            EntryKind::Ignored { .. } | EntryKind::NotTraversed(_) | EntryKind::Uninspectable => {
                Found::Untraversed
            }
        })
    }

    /// Joins components into a path, or `None` when the path limits reject it.
    fn join(&self, stack: &[&str]) -> Result<Option<SourcePath>, DiscoveryError> {
        let length = stack.iter().map(|part| part.len() + 1).sum::<usize>();
        let mut joined = String::new();
        joined
            .try_reserve_exact(length)
            .map_err(|_| DiscoveryError::AllocationFailed)?;
        for (index, part) in stack.iter().enumerate() {
            if index > 0 {
                joined.push('/');
            }
            joined.push_str(part);
        }
        match SourcePath::new(&joined, self.limits.path) {
            Ok(path) => Ok(Some(path)),
            Err(PathError::AllocationFailed) => Err(DiscoveryError::AllocationFailed),
            Err(_) => Ok(None),
        }
    }

    fn report_link(
        &mut self,
        path: &SourcePath,
        target: &LinkTarget,
    ) -> Result<(), DiscoveryError> {
        let location = FindingLocation::File(path.clone());
        let remediation = "Replace the link with the content it should provide, or recreate it \
                           in the migrated project manually.";
        match target {
            LinkTarget::Inside(_) => self.report(
                "FS1005",
                Severity::Info,
                location,
                "Link not followed; target inside the project recorded",
                "Frameshift never follows links. The target lies inside the project, where it \
                 is processed at its own path, so it is not read twice.",
                "Recreate the link in the migrated project if it is still needed.",
            ),
            LinkTarget::Escapes => self.report(
                "FS1003",
                Severity::Unknown,
                location,
                "Link target leaves the project root",
                "Frameshift never follows links. This link's target resolves outside the \
                 project root, so its content is not analyzed or migrated.",
                remediation,
            ),
            LinkTarget::Dangling => self.report(
                "FS1007",
                Severity::Info,
                location,
                "Link target does not exist",
                "The link's target does not exist in the project. Frameshift records the link, \
                 but there is no content to migrate for it.",
                "Remove the link or restore its target.",
            ),
            LinkTarget::Unresolved(reason) => {
                let detail = match reason {
                    Unresolved::Absolute => "the target is an absolute path",
                    Unresolved::ThroughLink => "the target passes through another link",
                    Unresolved::Untraversed => {
                        "the target passes through a directory that was not traversed"
                    }
                    Unresolved::Unreadable => "the target could not be read",
                    Unresolved::Unrepresentable => {
                        "the target cannot be represented as a project path"
                    }
                    Unresolved::Incomplete => "discovery did not complete",
                };
                let explanation = format!(
                    "Frameshift never follows links and could not resolve this target: {detail}. \
                     Its content is not analyzed or migrated."
                );
                self.report(
                    "FS1006",
                    Severity::Unknown,
                    location,
                    "Link target not resolved",
                    &explanation,
                    remediation,
                )
            }
        }
    }
}

/// Orders siblings by name bytes, with `/` appended to directory names.
fn canonical<T: Tree>(left: &Child<T>, right: &Child<T>) -> Ordering {
    sort_key(left).cmp(sort_key(right))
}

fn sort_key<T: Tree>(child: &Child<T>) -> impl Iterator<Item = u8> + '_ {
    let slash = (child.kind == RawKind::Directory).then_some(b'/');
    name(&child.path).bytes().chain(slash)
}

fn name(path: &SourcePath) -> &str {
    path.components().next_back().unwrap_or("")
}

fn scope(path: Option<&SourcePath>) -> FindingLocation {
    match path {
        Some(path) => FindingLocation::File(path.clone()),
        None => FindingLocation::Project,
    }
}

/// Returns the built-in reason that ignores this name at any depth.
fn builtin(name: &str) -> Option<IgnoreReason> {
    let is = |candidate: &str| name.eq_ignore_ascii_case(candidate);
    if is(".git") || is(".hg") || is(".svn") {
        Some(IgnoreReason::Vcs)
    } else if is("node_modules") {
        Some(IgnoreReason::Dependencies)
    } else if is(".next") {
        Some(IgnoreReason::BuildOutput)
    } else if environment(name) {
        Some(IgnoreReason::Environment)
    } else {
        None
    }
}

/// `.env` and `.env.*`, except names whose last dot-separated segment is
/// `example`, `sample`, or `template`, compared ASCII-case-insensitively.
fn environment(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 4 || !bytes[..4].eq_ignore_ascii_case(b".env") {
        return false;
    }
    match bytes.get(4) {
        None => true,
        Some(b'.') => {
            let last = name.rsplit('.').next().unwrap_or("");
            !["example", "sample", "template"]
                .iter()
                .any(|example| last.eq_ignore_ascii_case(example))
        }
        Some(_) => false,
    }
}

#[cfg(test)]
pub(crate) mod tests;
