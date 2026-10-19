//! Snapshot contracts on real directories created at test time (`D-067`).

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, HashLimits, PathLimits, Sha256Digest, SourcePath,
};
use frameshift_engine::input::{
    FileRead, Snapshot, SnapshotError, SnapshotFile, SnapshotId, SnapshotLimits, snapshot,
};
use frameshift_engine::limits::Limits;

mod support;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("snapshot").join(name);
    if let Ok(existing) = fs::canonicalize(&directory) {
        fs::remove_dir_all(existing).unwrap();
    }
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn write(root: &Path, path: &str, content: &[u8]) {
    let native = root.join(path);
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(native, content).unwrap();
}

fn open(root: &Path) -> Snapshot {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(root, Limits::default().snapshot(), &mut findings).unwrap();
    assert!(findings.finish().as_slice().is_empty());
    snapshot
}

fn read<'a>(snapshot: &'a Snapshot, path: &str) -> Result<&'a SnapshotFile, SnapshotError> {
    let path = SourcePath::new(path, PathLimits::default()).unwrap();
    let id = snapshot.inventory().files().find(&path).unwrap();
    snapshot.read(id).map(|outcome| match outcome {
        FileRead::Content(file) => file,
        FileRead::Unreadable(failure) => panic!("{path:?}: {failure:?}"),
    })
}

fn identity(root: &Path) -> SnapshotId {
    open(root).identity().unwrap()
}

const FILES: [(&str, &[u8]); 4] = [
    (
        "app/page.tsx",
        b"\xEF\xBB\xBFexport default function Page() {}\r\n",
    ),
    ("data.bin", b"\x00\xFF\xFE\x80"),
    ("empty.txt", b""),
    ("package.json", b"{\"name\": \"fictional\"}\n"),
];

#[test]
fn files_are_read_exactly_once_and_the_source_stays_unchanged() {
    let root = scratch("exact");
    for (path, content) in FILES {
        write(&root, path, content);
    }
    write(&root, ".env", b"FICTIONAL_SECRET=1\n");
    let snapshot = open(&root);
    assert!(
        snapshot
            .inventory()
            .files()
            .iter()
            .all(|(id, _)| !snapshot.is_read(id))
    );
    for (path, content) in FILES {
        let file = read(&snapshot, path).unwrap();
        assert_eq!(file.bytes(), content, "{path}");
        assert_eq!(
            file.digest(),
            Sha256Digest::hash(content, HashLimits::default()).unwrap()
        );
    }
    // Retained content survives a change on disk after the read.
    write(&root, "package.json", b"{}\n");
    assert_eq!(read(&snapshot, "package.json").unwrap().bytes(), FILES[3].1);
    write(&root, "package.json", FILES[3].1);
    for (path, content) in FILES {
        assert_eq!(fs::read(root.join(path)).unwrap(), content);
    }

    let first = snapshot.identity().unwrap();
    assert_eq!(identity(&root), first);
    assert!(first.to_string().starts_with("sha256:"));
}

#[test]
fn changes_between_listing_and_reading_stop_the_run() {
    let root = scratch("changed");
    for name in ["grown.txt", "removed.txt", "replaced.txt", "kept.txt"] {
        write(&root, name, b"original");
    }
    let snapshot = open(&root);
    write(&root, "grown.txt", b"original and more");
    fs::remove_file(root.join("removed.txt")).unwrap();
    fs::remove_file(root.join("replaced.txt")).unwrap();
    fs::create_dir(root.join("replaced.txt")).unwrap();

    for name in ["grown.txt", "removed.txt", "replaced.txt"] {
        assert_eq!(
            read(&snapshot, name).unwrap_err(),
            SnapshotError::SourceChanged,
            "{name}"
        );
    }
    assert_eq!(read(&snapshot, "kept.txt").unwrap().bytes(), b"original");
    assert_eq!(snapshot.identity(), Err(SnapshotError::SourceChanged));
}

#[cfg(unix)]
#[test]
fn a_link_swapped_in_for_a_file_is_a_change() {
    let root = scratch("swapped");
    write(&root, "a.txt", b"same size");
    write(&root, "b.txt", b"same size");
    let snapshot = open(&root);
    fs::remove_file(root.join("a.txt")).unwrap();
    std::os::unix::fs::symlink("b.txt", root.join("a.txt")).unwrap();
    assert_eq!(
        read(&snapshot, "a.txt").unwrap_err(),
        SnapshotError::SourceChanged
    );
}

#[cfg(unix)]
#[test]
fn unreadable_files_are_per_file_outcomes() {
    use std::os::unix::fs::PermissionsExt;

    let root = scratch("unreadable");
    write(&root, "locked.txt", b"fictional");
    let native = root.join("locked.txt");
    fs::set_permissions(&native, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&native).is_ok() {
        // Privileged users read any file; the outcome cannot be produced here.
        return;
    }
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(&root, SnapshotLimits::default(), &mut findings).unwrap();
    let (id, _) = snapshot.inventory().files().iter().next().unwrap();
    assert!(matches!(snapshot.read(id), Ok(FileRead::Unreadable(_))));
    snapshot.identity().unwrap();
    snapshot.report_unreadable(&mut findings).unwrap();
    let findings = findings.finish();
    assert_eq!(findings.as_slice().len(), 1);
    assert_eq!(findings.as_slice()[0].code().as_str(), "FS1019");
}

#[test]
fn identities_follow_contents_and_the_inventory() {
    let root = scratch("identity");
    write(&root, "a.txt", b"one");
    let original = identity(&root);
    assert_eq!(identity(&root), original);

    write(&root, "a.txt", b"two");
    let content = identity(&root);
    write(&root, "node_modules/pkg/index.js", b"module.exports = 1;\n");
    let ignored = identity(&root);
    write(&root, "a.txt", b"one");
    fs::remove_dir_all(root.join("node_modules")).unwrap();
    assert_eq!(identity(&root), original);

    assert_ne!(content, original);
    assert_ne!(ignored, original);
    assert_ne!(ignored, content);
}

#[test]
fn engine_limits_feed_the_snapshot() {
    let mut limits = Limits::default();
    assert_eq!(limits.snapshot(), SnapshotLimits::default());
    limits.hash.max_bytes = 3;
    limits.run.max_files = 7;
    let snapshot = limits.snapshot();
    assert_eq!(snapshot.hash.max_bytes, 3);
    assert_eq!(snapshot.discovery, limits.discovery());

    let root = scratch("limits");
    write(&root, "a.txt", b"four");
    let mut findings = FindingCollector::new(FindingLimits::default());
    let small = frameshift_engine::input::snapshot(&root, snapshot, &mut findings).unwrap();
    assert!(matches!(small.identity(), Err(SnapshotError::Hash(_))));
}
