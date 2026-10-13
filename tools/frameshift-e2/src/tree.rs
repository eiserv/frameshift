//! Reading and writing fixture file trees with slash-separated relative paths.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One file of a tree: its slash-separated path relative to the root, and its bytes.
pub type TreeFile = (String, Vec<u8>);

/// Reads every regular file below `root`, sorted byte-wise by relative path.
/// Symbolic links and other special files are rejected.
pub fn read_tree(root: &Path) -> io::Result<Vec<TreeFile>> {
    let mut files = Vec::new();
    read_directory(root, String::new(), &mut files)?;
    files.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    Ok(files)
}

fn read_directory(directory: &Path, prefix: String, files: &mut Vec<TreeFile>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|name| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("non-UTF-8 file name {name:?}"),
            )
        })?;
        let path = entry.path();
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_dir() {
            read_directory(&path, relative, files)?;
        } else if kind.is_file() {
            files.push((relative, fs::read(&path)?));
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a regular file or directory", path.display()),
            ));
        }
    }
    Ok(())
}

/// Writes `files` below `root`, creating directories as needed.
pub fn write_tree<'a>(
    root: &Path,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> io::Result<()> {
    for (relative, bytes) in files {
        let path = join(root, relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    Ok(())
}

// Joins a slash-separated relative path, rejecting anything that could leave `root`.
fn join(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsafe output path {relative:?}"),
            ));
        }
        path.push(part);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_relative_paths_are_rejected() {
        let root = Path::new("root");
        assert!(join(root, "src/pages/index.astro").is_ok());
        for unsafe_path in ["", "../x", "a//b", "a/./b", "C:x", "a\\b", "/abs"] {
            assert!(join(root, unsafe_path).is_err(), "{unsafe_path}");
        }
    }
}
