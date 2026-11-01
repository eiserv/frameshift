//! The only product crate that writes files (`D-101`).
//!
//! A migration output is written completely into a marked staging directory
//! next to the output location and then renamed into place, so the output
//! either appears whole or not at all. The source tree is never opened for
//! writing: the destination must lie outside it, and the source must not lie
//! inside the destination. Only `std::fs` is used.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

use frameshift_core::{OutputPath, RunLimits};

/// Name of the marker file that identifies a Frameshift staging directory.
pub const STAGING_MARKER: &str = ".frameshift-staging";

/// Why the output could not be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputErrorKind {
    /// The location is unsafe or occupied; nothing was created.
    Refused,
    /// Writing failed; the staging directory of this run was removed.
    Io,
}

/// An output failure. Messages never contain paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputError {
    kind: OutputErrorKind,
    message: String,
}

impl OutputError {
    /// The failure category.
    pub const fn kind(&self) -> OutputErrorKind {
        self.kind
    }

    fn refused(message: &str) -> Self {
        Self {
            kind: OutputErrorKind::Refused,
            message: message.to_owned(),
        }
    }

    fn io(context: &str, error: &io::Error) -> Self {
        Self {
            kind: OutputErrorKind::Io,
            message: format!("{context}: {}", error.kind()),
        }
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OutputError {}

/// One file of the output tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    /// Validated portable path relative to the output root.
    pub path: OutputPath,
    /// Exact content.
    pub bytes: Vec<u8>,
}

/// A checked output location. Checking creates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    output: PathBuf,
    staging: PathBuf,
    // Where a replaced output moves, which never exists beforehand.
    previous: Option<PathBuf>,
}

/// Checks that `output` can receive a new tree for the source at `source`.
///
/// The output must not exist, its parent directory must exist, it must not
/// lie inside the source or contain it, and no staging directory of an
/// earlier run may be present next to it.
pub fn prepare(source: &Path, output: &Path) -> Result<Destination, OutputError> {
    let source = fs::canonicalize(source)
        .map_err(|_| OutputError::refused("the source directory cannot be resolved"))?;
    if fs::symlink_metadata(output).is_ok() {
        return Err(OutputError::refused("the output location already exists"));
    }
    let (output, staging) = locations(output)?;
    checked(&source, output, staging, None)
}

/// Checks that the existing directory at `output` can be replaced by a new
/// tree. The caller has established that the tree is safe to replace. The
/// replaced tree moves next to the output; nothing is deleted.
pub fn prepare_replacement(source: &Path, output: &Path) -> Result<Destination, OutputError> {
    let source = fs::canonicalize(source)
        .map_err(|_| OutputError::refused("the source directory cannot be resolved"))?;
    let is_directory =
        fs::symlink_metadata(output).is_ok_and(|metadata| metadata.file_type().is_dir());
    if !is_directory {
        return Err(OutputError::refused(
            "the output location is not a real directory",
        ));
    }
    let (resolved, staging) = locations(output)?;
    let mut previous = staging.clone();
    previous.set_extension("frameshift-previous");
    if fs::symlink_metadata(&previous).is_ok() {
        return Err(OutputError::refused(
            "a previous output that was moved aside still exists next to the output",
        ));
    }
    checked(&source, resolved, staging, Some(previous))
}

fn checked(
    source: &Path,
    output: PathBuf,
    staging: PathBuf,
    previous: Option<PathBuf>,
) -> Result<Destination, OutputError> {
    if output.starts_with(source) || source.starts_with(&output) {
        return Err(OutputError::refused(
            "the output must not be inside the source or contain it",
        ));
    }
    if fs::symlink_metadata(&staging).is_ok() {
        return Err(OutputError::refused(
            "a staging directory from an interrupted run exists next to the output",
        ));
    }
    Ok(Destination {
        output,
        staging,
        previous,
    })
}

// The resolved output path and the staging directory next to it.
fn locations(output: &Path) -> Result<(PathBuf, PathBuf), OutputError> {
    let name = match output.components().next_back() {
        Some(Component::Normal(name)) => name.to_owned(),
        Some(
            Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir,
        )
        | None => {
            return Err(OutputError::refused(
                "the output location must name a new directory",
            ));
        }
    };
    let parent = match output.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        Some(_) | None => PathBuf::from("."),
    };
    let parent = fs::canonicalize(&parent)
        .map_err(|_| OutputError::refused("the output's parent directory does not exist"))?;
    if !parent.is_dir() {
        return Err(OutputError::refused(
            "the output's parent is not a directory",
        ));
    }
    let mut staging_name = OsString::from(".");
    staging_name.push(&name);
    staging_name.push(".frameshift-staging");
    Ok((parent.join(&name), parent.join(staging_name)))
}

/// Removes the staging directory that an interrupted run left next to
/// `output`, and returns whether there was one.
///
/// Only a real directory that holds the staging marker is removed. Anything
/// else at that name is refused, so this cannot delete a directory that a
/// person created.
pub fn clean_staging(output: &Path) -> Result<bool, OutputError> {
    let (_, staging) = locations(output)?;
    let Ok(metadata) = fs::symlink_metadata(&staging) else {
        return Ok(false);
    };
    let marked = fs::symlink_metadata(staging.join(STAGING_MARKER))
        .is_ok_and(|marker| marker.file_type().is_file());
    if !metadata.file_type().is_dir() || !marked {
        return Err(OutputError::refused(
            "the entry at the staging location is not a marked staging directory",
        ));
    }
    fs::remove_dir_all(&staging)
        .map_err(|error| OutputError::io("removing the staging directory failed", &error))?;
    Ok(true)
}

/// Writes `files` into a new staging directory and renames it to the output.
///
/// Paths must be distinct and must not name the staging marker. On failure,
/// the staging directory created by this call is removed, and the output
/// location stays absent.
pub fn write(destination: &Destination, files: &[OutputFile]) -> Result<(), OutputError> {
    if files
        .iter()
        .any(|file| file.path.as_str() == STAGING_MARKER)
    {
        return Err(OutputError::refused(
            "an output file uses the reserved staging marker name",
        ));
    }
    fs::create_dir(&destination.staging).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            OutputError::refused(
                "a staging directory from an interrupted run exists next to the output",
            )
        } else {
            OutputError::io("creating the staging directory failed", &error)
        }
    })?;
    let result = fill(&destination.staging, files).and_then(|()| commit(destination));
    if result.is_err() {
        // Only this run's own staging directory is removed.
        let _ = fs::remove_dir_all(&destination.staging);
    }
    result
}

fn commit(destination: &Destination) -> Result<(), OutputError> {
    let failed = |error: &io::Error| OutputError::io("moving the output into place failed", error);
    let Some(previous) = &destination.previous else {
        if fs::symlink_metadata(&destination.output).is_ok() {
            return Err(OutputError::refused(
                "the output location appeared during the run",
            ));
        }
        return rename(&destination.staging, &destination.output).map_err(|e| failed(&e));
    };
    rename(&destination.output, previous).map_err(|e| failed(&e))?;
    if let Err(error) = rename(&destination.staging, &destination.output) {
        // The replaced tree goes back, so a failed run changes nothing.
        let _ = rename(previous, &destination.output);
        return Err(failed(&error));
    }
    Ok(())
}

/// Calls `visit` with every file below `root`, by slash-separated relative
/// path, in byte order. A link or any other entry that is not a regular file
/// or directory stops the walk, and so does exceeding `limits`.
pub fn visit_tree(
    root: &Path,
    limits: RunLimits,
    visit: &mut dyn FnMut(&str, &[u8]),
) -> Result<(), OutputError> {
    let mut files = 0;
    let mut total = 0;
    walk(root, "", 0, limits, &mut files, &mut total, visit)
}

fn walk(
    directory: &Path,
    prefix: &str,
    depth: usize,
    limits: RunLimits,
    files: &mut usize,
    total: &mut u64,
    visit: &mut dyn FnMut(&str, &[u8]),
) -> Result<(), OutputError> {
    let unreadable =
        |error: &io::Error| OutputError::io("reading the existing output failed", error);
    let too_large = || OutputError::refused("the existing output exceeds the run limits");
    if depth > limits.max_directory_depth {
        return Err(too_large());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|e| unreadable(&e))?
        .collect::<io::Result<Vec<_>>>()
        .map_err(|e| unreadable(&e))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(OutputError::refused(
                "the existing output holds a name that is not valid UTF-8",
            ));
        };
        let relative = format!("{prefix}{name}");
        let kind = entry.file_type().map_err(|e| unreadable(&e))?;
        if kind.is_dir() {
            walk(
                &entry.path(),
                &format!("{relative}/"),
                depth + 1,
                limits,
                files,
                total,
                visit,
            )?;
        } else if kind.is_file() {
            *files += 1;
            let bytes = fs::read(entry.path()).map_err(|e| unreadable(&e))?;
            *total += bytes.len() as u64;
            if *files > limits.max_files
                || bytes.len() as u64 > limits.max_file_bytes
                || *total > limits.max_total_bytes
            {
                return Err(too_large());
            }
            visit(&relative, &bytes);
        } else {
            return Err(OutputError::refused(
                "the existing output holds a link or another special entry",
            ));
        }
    }
    Ok(())
}

// On Windows, a scanner or indexer may briefly hold files of the new tree
// open, which makes the directory rename fail with access denied. Retrying
// a bounded number of times changes no output; other errors fail at once.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempts = 0;
    loop {
        match fs::rename(from, to) {
            Err(error)
                if cfg!(windows)
                    && error.kind() == io::ErrorKind::PermissionDenied
                    && attempts < 20 =>
            {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(50 * attempts));
            }
            result => return result,
        }
    }
}

fn fill(staging: &Path, files: &[OutputFile]) -> Result<(), OutputError> {
    let marker = staging.join(STAGING_MARKER);
    create(&marker, b"Frameshift staging directory\n")?;
    for file in files {
        let mut target = staging.to_path_buf();
        target.extend(file.path.as_str().split('/'));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| OutputError::io("creating an output directory failed", &error))?;
        }
        create(&target, &file.bytes)?;
    }
    fs::remove_file(&marker)
        .map_err(|error| OutputError::io("removing the staging marker failed", &error))
}

fn create(path: &Path, bytes: &[u8]) -> Result<(), OutputError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| OutputError::io("creating an output file failed", &error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| OutputError::io("writing an output file failed", &error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_core::PathLimits;

    fn temporary(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("frameshift-output-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("source")).unwrap();
        root
    }

    fn file(path: &str, bytes: &[u8]) -> OutputFile {
        OutputFile {
            path: OutputPath::new(path, PathLimits::default()).unwrap(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn writes_a_complete_tree_and_leaves_no_staging() {
        let root = temporary("write");
        let out = root.join("out");
        let destination = prepare(&root.join("source"), &out).unwrap();
        write(
            &destination,
            &[file("a.txt", b"a"), file("src/pages/index.astro", b"b")],
        )
        .unwrap();
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"a");
        assert_eq!(fs::read(out.join("src/pages/index.astro")).unwrap(), b"b");
        assert!(!out.join(STAGING_MARKER).exists());
        assert!(!root.join(".out.frameshift-staging").exists());
        // The written output now refuses a second run.
        let error = prepare(&root.join("source"), &out).unwrap_err();
        assert_eq!(error.kind(), OutputErrorKind::Refused);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_unsafe_locations() {
        let root = temporary("refuse");
        let source = root.join("source");
        for output in [source.join("out"), root.clone(), source.clone()] {
            let error = prepare(&source, &output).unwrap_err();
            assert_eq!(error.kind(), OutputErrorKind::Refused, "{output:?}");
        }
        assert!(prepare(&source, &root.join("missing/out")).is_err());
        fs::create_dir(root.join(".out.frameshift-staging")).unwrap();
        let error = prepare(&source, &root.join("out")).unwrap_err();
        assert!(error.to_string().contains("staging"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacement_moves_the_old_tree_aside() {
        let root = temporary("replace");
        let out = root.join("out");
        let source = root.join("source");
        write(&prepare(&source, &out).unwrap(), &[file("a.txt", b"old")]).unwrap();
        let destination = prepare_replacement(&source, &out).unwrap();
        write(&destination, &[file("a.txt", b"new")]).unwrap();
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"new");
        let previous = root.join(".out.frameshift-previous");
        assert_eq!(fs::read(previous.join("a.txt")).unwrap(), b"old");
        // The moved tree is never overwritten by a second replacement.
        assert!(prepare_replacement(&source, &out).is_err());
        fs::remove_dir_all(&previous).unwrap();
        assert!(prepare_replacement(&source, &root.join("missing")).is_err());
        assert!(prepare_replacement(&source, &source).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_replacement_keeps_the_old_tree() {
        let root = temporary("replace-fail");
        let out = root.join("out");
        let source = root.join("source");
        write(&prepare(&source, &out).unwrap(), &[file("a.txt", b"old")]).unwrap();
        let destination = prepare_replacement(&source, &out).unwrap();
        // Duplicate paths fail in the staging directory, before the commit.
        assert!(write(&destination, &[file("a", b"1"), file("a", b"2")]).is_err());
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"old");
        assert!(!root.join(".out.frameshift-previous").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn visiting_a_tree_is_ordered_and_bounded() {
        let root = temporary("visit");
        let tree = root.join("tree");
        fs::create_dir_all(tree.join("b")).unwrap();
        fs::write(tree.join("b/c.txt"), "c").unwrap();
        fs::write(tree.join("a.txt"), "a").unwrap();
        let mut seen = Vec::new();
        visit_tree(&tree, RunLimits::default(), &mut |path, bytes| {
            seen.push((path.to_owned(), bytes.to_vec()));
        })
        .unwrap();
        assert_eq!(
            seen,
            [
                ("a.txt".to_owned(), b"a".to_vec()),
                ("b/c.txt".to_owned(), b"c".to_vec())
            ]
        );
        let limits = RunLimits {
            max_files: 1,
            ..RunLimits::default()
        };
        assert!(visit_tree(&tree, limits, &mut |_, _| {}).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleaning_removes_only_a_marked_staging_directory() {
        let root = temporary("clean");
        let out = root.join("out");
        let staging = root.join(".out.frameshift-staging");
        assert_eq!(clean_staging(&out), Ok(false));
        // An unmarked directory and a file at that name are never removed.
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("notes.txt"), "kept").unwrap();
        assert_eq!(
            clean_staging(&out).unwrap_err().kind(),
            OutputErrorKind::Refused
        );
        assert!(staging.join("notes.txt").exists());
        fs::remove_dir_all(&staging).unwrap();
        fs::write(&staging, "kept").unwrap();
        assert!(clean_staging(&out).is_err());
        fs::remove_file(&staging).unwrap();
        // A marked directory goes, and the location is usable again.
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(STAGING_MARKER), "").unwrap();
        fs::write(staging.join("partial.txt"), "gone").unwrap();
        assert!(prepare(&root.join("source"), &out).is_err());
        assert_eq!(clean_staging(&out), Ok(true));
        assert!(!staging.exists());
        assert!(prepare(&root.join("source"), &out).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_write_removes_its_staging_directory() {
        let root = temporary("fail");
        let out = root.join("out");
        let destination = prepare(&root.join("source"), &out).unwrap();
        // Duplicate paths make the second create fail.
        let error = write(&destination, &[file("a", b"1"), file("a", b"2")]).unwrap_err();
        assert_eq!(error.kind(), OutputErrorKind::Io);
        assert!(!out.exists());
        assert!(!root.join(".out.frameshift-staging").exists());
        let error = write(&destination, &[file(STAGING_MARKER, b"")]).unwrap_err();
        assert_eq!(error.kind(), OutputErrorKind::Refused);
        fs::remove_dir_all(root).unwrap();
    }
}
