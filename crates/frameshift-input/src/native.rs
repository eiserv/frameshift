//! The walker's view of the host filesystem through `std::fs` (`D-064`).
//!
//! Nothing here follows a link below the root. Directory traversal is path
//! based, because std offers no directory-relative opens; after listing a
//! directory the walker checks that it is still the same real directory.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use frameshift_core::Budget;

use crate::walk::{
    DiscoveryError, IoFailure, ListError, Listed, RawKind, RawTarget, ReadError, TargetComponent,
    Tree,
};

#[cfg(not(any(unix, windows)))]
compile_error!("discovery supports Unix and Windows hosts only");

/// The host filesystem.
pub(crate) struct NativeTree;

/// The identity of a listed entry: device and inode on Unix. Windows offers
/// no stable file identity in std, so attributes are rechecked instead.
#[cfg(unix)]
pub(crate) type Identity = (u64, u64);
#[cfg(windows)]
pub(crate) type Identity = ();

#[cfg(windows)]
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

impl NativeTree {
    /// Canonicalizes the root, on Windows into a verbatim path that keeps
    /// names with trailing dots or spaces intact, and checks that it is a
    /// directory.
    pub(crate) fn open_root(root: &Path) -> Result<(PathBuf, Identity), DiscoveryError> {
        let root = fs::canonicalize(root)
            .map_err(|error| DiscoveryError::Root(IoFailure::from_io(&error)))?;
        let metadata = fs::symlink_metadata(&root)
            .map_err(|error| DiscoveryError::Root(IoFailure::from_io(&error)))?;
        if !metadata.is_dir() {
            return Err(DiscoveryError::RootNotDirectory);
        }
        Ok((root, identity(&metadata)))
    }
}

impl Tree for NativeTree {
    type Location = PathBuf;
    type Name = OsString;
    type Identity = Identity;

    fn list(
        &self,
        directory: &PathBuf,
        entries: &mut Budget,
    ) -> Result<Vec<Listed<Self>>, ListError> {
        let io = |error: std::io::Error| ListError::Io(IoFailure::from_io(&error));
        let mut listed = Vec::new();
        for item in fs::read_dir(directory).map_err(io)? {
            let entry = item.map_err(io)?;
            entries.charge(1).map_err(|_| ListError::EntryLimit)?;
            let name = entry.file_name();
            let text = name.to_str().map(str::to_owned);
            let (kind, identity) = match entry.metadata() {
                // A directory entry's metadata never follows a link.
                Ok(metadata) => (kind(&metadata), identity(&metadata)),
                Err(error) => (
                    RawKind::Uninspectable(IoFailure::from_io(&error)),
                    Identity::default(),
                ),
            };
            listed.push(Listed {
                name,
                text,
                kind,
                identity,
            });
        }
        Ok(listed)
    }

    fn child(&self, directory: &PathBuf, name: &OsString) -> PathBuf {
        directory.join(name)
    }

    fn is_same_directory(&self, directory: &PathBuf, expected: &Identity) -> bool {
        fs::symlink_metadata(directory).is_ok_and(|metadata| {
            kind(&metadata) == RawKind::Directory && same(&metadata, expected)
        })
    }

    fn read_regular(
        &self,
        file: &PathBuf,
        expected: &Identity,
        max: u64,
    ) -> Result<Vec<u8>, ReadError> {
        let io = |error: std::io::Error| ReadError::Io(IoFailure::from_io(&error));
        let handle = open_unfollowed(file).map_err(io)?;
        let metadata = handle.metadata().map_err(io)?;
        if !matches!(kind(&metadata), RawKind::File { .. }) || !same(&metadata, expected) {
            return Err(ReadError::Changed);
        }
        let mut content = Vec::new();
        handle
            .take(max.saturating_add(1))
            .read_to_end(&mut content)
            .map_err(io)?;
        Ok(content)
    }

    fn read_link(&self, link: &PathBuf) -> RawTarget {
        match fs::read_link(link) {
            Ok(target) => raw_target(&target),
            Err(_) => RawTarget::Unreadable,
        }
    }
}

/// Classifies metadata that was read without following links.
#[cfg(unix)]
fn kind(metadata: &fs::Metadata) -> RawKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        RawKind::Link
    } else if file_type.is_dir() {
        RawKind::Directory
    } else if file_type.is_file() {
        RawKind::File {
            size: metadata.len(),
        }
    } else {
        RawKind::Special
    }
}

/// Classifies metadata that was read without following reparse points.
/// Every reparse point is a link in the sense of `D-064`: symbolic links and
/// junctions (name surrogates) are links, all others are excluded.
#[cfg(windows)]
fn kind(metadata: &fs::Metadata) -> RawKind {
    use std::os::windows::fs::MetadataExt;
    let attributes = metadata.file_attributes();
    let directory = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        if metadata.file_type().is_symlink() {
            RawKind::Link
        } else {
            RawKind::ReparsePoint { directory }
        }
    } else if directory {
        RawKind::Directory
    } else {
        RawKind::File {
            size: metadata.len(),
        }
    }
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(windows)]
fn identity(_metadata: &fs::Metadata) -> Identity {}

/// Whether metadata belongs to the entry with this identity. Windows has
/// no identity to compare, so the attribute checks of the callers decide.
#[cfg(unix)]
fn same(metadata: &fs::Metadata, expected: &Identity) -> bool {
    identity(metadata) == *expected
}

#[cfg(windows)]
fn same(_metadata: &fs::Metadata, _expected: &Identity) -> bool {
    true
}

/// Opens a file for reading. Unix follows a link here, which the caller
/// detects through the identity of the opened file; Windows opens a
/// reparse point itself, which the caller detects through its attributes.
#[cfg(unix)]
fn open_unfollowed(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

#[cfg(windows)]
fn open_unfollowed(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Splits a stored link target into components without touching the
/// filesystem.
fn raw_target(target: &Path) -> RawTarget {
    let bytes = target.as_os_str().as_encoded_bytes();
    if bytes.is_empty() {
        return RawTarget::Empty;
    }
    let mut components = Vec::new();
    for component in target.components() {
        components.push(match component {
            Component::Prefix(_) | Component::RootDir => return RawTarget::Absolute,
            Component::CurDir => TargetComponent::Current,
            Component::ParentDir => TargetComponent::Parent,
            Component::Normal(name) => TargetComponent::Name(name.to_str().map(str::to_owned)),
        });
    }
    let separator = |byte: &u8| *byte == b'/' || (cfg!(windows) && *byte == b'\\');
    RawTarget::Relative {
        components,
        directory: bytes.last().is_some_and(separator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(target: &RawTarget) -> Vec<Option<&str>> {
        let RawTarget::Relative { components, .. } = target else {
            return Vec::new();
        };
        components
            .iter()
            .map(|component| match component {
                TargetComponent::Current => Some("."),
                TargetComponent::Parent => Some(".."),
                TargetComponent::Name(name) => name.as_deref(),
            })
            .collect()
    }

    #[test]
    fn link_targets_split_without_filesystem_access() {
        assert!(matches!(raw_target(Path::new("")), RawTarget::Empty));
        assert!(matches!(raw_target(Path::new("/etc")), RawTarget::Absolute));
        let relative = raw_target(Path::new("../a/./b/"));
        assert_eq!(names(&relative), [Some(".."), Some("a"), Some("b")]);
        assert!(matches!(
            relative,
            RawTarget::Relative {
                directory: true,
                ..
            }
        ));
        assert!(matches!(
            raw_target(Path::new("./x")),
            RawTarget::Relative {
                directory: false,
                ..
            }
        ));
        assert_eq!(names(&raw_target(Path::new("./x"))), [Some("."), Some("x")]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_and_backslash_targets() {
        assert!(matches!(
            raw_target(Path::new(r"C:\x")),
            RawTarget::Absolute
        ));
        assert!(matches!(raw_target(Path::new("C:x")), RawTarget::Absolute));
        assert!(matches!(
            raw_target(Path::new(r"\\?\C:\x")),
            RawTarget::Absolute
        ));
        assert_eq!(
            names(&raw_target(Path::new(r"..\a"))),
            [Some(".."), Some("a")]
        );
    }
}
