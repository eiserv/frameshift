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

use frameshift_core::OutputPath;

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
    let output = parent.join(&name);
    if output.starts_with(&source) || source.starts_with(&output) {
        return Err(OutputError::refused(
            "the output must not be inside the source or contain it",
        ));
    }
    let mut staging_name = OsString::from(".");
    staging_name.push(&name);
    staging_name.push(".frameshift-staging");
    let staging = parent.join(staging_name);
    if fs::symlink_metadata(&staging).is_ok() {
        return Err(OutputError::refused(
            "a staging directory from an interrupted run exists next to the output",
        ));
    }
    Ok(Destination { output, staging })
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
    let result = fill(&destination.staging, files).and_then(|()| {
        if fs::symlink_metadata(&destination.output).is_ok() {
            return Err(OutputError::refused(
                "the output location appeared during the run",
            ));
        }
        rename(&destination.staging, &destination.output)
            .map_err(|error| OutputError::io("moving the output into place failed", &error))
    });
    if result.is_err() {
        // Only this run's own staging directory is removed.
        let _ = fs::remove_dir_all(&destination.staging);
    }
    result
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
