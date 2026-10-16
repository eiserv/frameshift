//! Test support: a temporary directory outside the repository.

use std::path::{Path, PathBuf};

/// Cargo's temporary directory, or the system one when a test binary runs on
/// another host than it was built for, such as a Linux binary built on
/// Windows and run in WSL, where the embedded Windows path is not absolute.
pub fn temporary() -> PathBuf {
    let cargo = Path::new(env!("CARGO_TARGET_TMPDIR"));
    if cargo.is_absolute() {
        cargo.to_path_buf()
    } else {
        std::env::temp_dir().join("frameshift-tests")
    }
}
