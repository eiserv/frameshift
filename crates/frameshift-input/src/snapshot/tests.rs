//! Snapshot contracts on in-memory trees: read-once retention, change
//! detection, unreadable files, and the framed identity.

use std::sync::atomic::Ordering;

use frameshift_core::{FindingLimits, KeyTable, PathLimits};

use super::*;
use crate::walk::tests::{Memory, Node, file, link};

fn open_with(tree: &Memory, limits: SnapshotLimits) -> (Snapshot, FindingCollector) {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let (inventory, files) = walk::walk_files(tree, 0, 0, limits.discovery, &mut findings).unwrap();
    let snapshot = Snapshot::new(inventory, tree.clone(), files, limits.hash).unwrap();
    (snapshot, findings)
}

fn open(tree: &Memory) -> Snapshot {
    open_with(tree, SnapshotLimits::default()).0
}

fn id(snapshot: &Snapshot, path: &str) -> DenseId<SourcePath> {
    let path = SourcePath::new(path, PathLimits::default()).unwrap();
    snapshot.inventory().files().find(&path).unwrap()
}

fn content<'a>(snapshot: &'a Snapshot, path: &str) -> &'a SnapshotFile {
    match snapshot.read(id(snapshot, path)).unwrap() {
        FileRead::Content(file) => file,
        FileRead::Unreadable(failure) => panic!("{path}: {failure:?}"),
    }
}

/// The identity stream of section 24 of IMPLEMENTATION.md, written out by hand.
struct Expected(Vec<u8>);

impl Expected {
    fn new() -> Self {
        let mut stream = Self(Vec::new());
        stream.field(b"frameshift.snapshot");
        stream.number(1);
        stream
    }

    fn number(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_be_bytes());
    }

    fn tag(&mut self, tag: u8) {
        self.0.push(tag);
    }

    fn field(&mut self, bytes: &[u8]) {
        self.number(bytes.len() as u64);
        self.0.extend_from_slice(bytes);
    }

    fn digest(&self) -> Sha256Digest {
        Sha256Digest::hash(&self.0, HashLimits::default()).unwrap()
    }
}

#[test]
fn files_are_read_on_demand_and_at_most_once() {
    let tree = Memory::new(&[
        ("package.json", file("{}")),
        ("src/a.ts", file("export {};")),
    ]);
    let snapshot = open(&tree);
    assert_eq!(tree.reads.load(Ordering::Relaxed), 0);
    let manifest = id(&snapshot, "package.json");
    assert!(!snapshot.is_read(manifest));

    let first = content(&snapshot, "package.json");
    assert_eq!(first.bytes(), b"{}");
    assert_eq!(
        first.digest(),
        Sha256Digest::hash(b"{}", HashLimits::default()).unwrap()
    );
    let again = content(&snapshot, "package.json");
    assert!(std::ptr::eq(first, again));
    assert!(snapshot.is_read(manifest));
    assert!(!snapshot.is_read(id(&snapshot, "src/a.ts")));
    assert_eq!(tree.reads.load(Ordering::Relaxed), 1);

    // The identity reads the remaining file once and nothing twice.
    let identity = snapshot.identity().unwrap();
    assert_eq!(snapshot.identity().unwrap(), identity);
    assert_eq!(tree.reads.load(Ordering::Relaxed), 2);
}

#[test]
fn parallel_reads_share_one_outcome_per_file() {
    let spec: Vec<(String, Node)> = (0..32)
        .map(|index| (format!("f{index:02}"), file(&format!("content {index}"))))
        .collect();
    let spec: Vec<(&str, Node)> = spec
        .iter()
        .map(|(path, node)| (path.as_str(), node.clone()))
        .collect();
    let tree = Memory::new(&spec);
    let snapshot = open(&tree);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for (id, _) in snapshot.inventory().files().iter() {
                    assert!(matches!(snapshot.read(id), Ok(FileRead::Content(_))));
                }
            });
        }
    });
    assert_eq!(tree.reads.load(Ordering::Relaxed), 32);
}

#[test]
fn identity_matches_the_documented_stream() {
    let mut tree = Memory::new(&[
        ("a.txt", file("A")),
        ("dir/b", file("")),
        ("dir/node_modules/x", file("x")),
        ("link", link("dir/b")),
        ("pipe", Node::Special),
    ]);
    tree.seed = 7;
    let snapshot = open(&tree);

    let mut expected = Expected::new();
    expected.tag(1); // complete
    expected.tag(0); // no stop
    expected.number(0); // unrepresentable
    expected.number(2); // admitted files
    for (path, bytes) in [("a.txt", &b"A"[..]), ("dir/b", &b""[..])] {
        expected.field(path.as_bytes());
        expected.number(bytes.len() as u64);
        expected.tag(1);
        let digest = Sha256Digest::hash(bytes, HashLimits::default()).unwrap();
        expected.0.extend_from_slice(digest.as_bytes());
    }
    expected.number(4); // other entries, in canonical order
    expected.field(b"dir");
    expected.tag(1); // directory
    expected.field(b"dir/node_modules");
    expected.tag(2); // ignored
    expected.tag(2); // dependencies
    expected.tag(1); // a directory
    expected.field(b"link");
    expected.tag(4); // link
    expected.tag(2); // inside, at a path
    expected.field(b"dir/b");
    expected.field(b"pipe");
    expected.tag(6); // special

    let identity = snapshot.identity().unwrap();
    assert_eq!(identity.digest(), expected.digest());
    assert_eq!(identity.to_string(), expected.digest().to_string());
}

#[test]
fn identity_is_independent_of_listing_and_read_order() {
    let spec = [
        ("b/c.d", file("1")),
        ("b/c/d.txt", file("2")),
        ("a.txt", file("3")),
        ("a/x.txt", file("4")),
    ];
    let mut identities = Vec::new();
    for seed in 0..16 {
        let mut tree = Memory::new(&spec);
        tree.seed = seed;
        let snapshot = open(&tree);
        // Read in reverse canonical order first.
        let ids: Vec<_> = snapshot
            .inventory()
            .files()
            .iter()
            .map(|(id, _)| id)
            .collect();
        for id in ids.into_iter().rev().take(seed as usize % 4) {
            snapshot.read(id).unwrap();
        }
        identities.push(snapshot.identity().unwrap());
    }
    assert!(identities.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn identity_covers_contents_and_the_whole_inventory() {
    let base = [
        ("a.txt", file("A")),
        ("dir/b", file("B")),
        ("link", link("a.txt")),
    ];
    let identity = |spec: &[(&str, Node)], limits: SnapshotLimits| {
        open_with(&Memory::new(spec), limits).0.identity().unwrap()
    };
    let default = SnapshotLimits::default();
    let original = identity(&base, default);

    let mut variants: Vec<SnapshotId> = Vec::new();
    let mut changed = base.to_vec();
    changed[0].1 = file("Z");
    variants.push(identity(&changed, default));
    let mut renamed = base.to_vec();
    renamed[1].0 = "dir/c";
    variants.push(identity(&renamed, default));
    let mut ignored = base.to_vec();
    ignored.push((".git/HEAD", file("ref")));
    variants.push(identity(&ignored, default));
    let mut retargeted = base.to_vec();
    retargeted[2].1 = link("dir/b");
    variants.push(identity(&retargeted, default));
    let mut special = base.to_vec();
    special.push(("pipe", Node::Special));
    variants.push(identity(&special, default));
    let mut stopped = default;
    stopped.discovery.run.max_files = 1;
    variants.push(identity(&base, stopped));

    for variant in &variants {
        assert_ne!(*variant, original);
    }
    for (index, variant) in variants.iter().enumerate() {
        assert!(!variants[index + 1..].contains(variant));
    }
}

#[test]
fn a_changed_file_fails_the_read_and_the_identity() {
    let mut tree = Memory::new(&[("a.txt", file("A")), ("b.txt", file("B"))]);
    tree.changed_file = Some(tree.find("b.txt"));
    let snapshot = open(&tree);
    assert!(matches!(
        snapshot.read(id(&snapshot, "a.txt")),
        Ok(FileRead::Content(_))
    ));
    let b = id(&snapshot, "b.txt");
    assert_eq!(snapshot.read(b).unwrap_err(), SnapshotError::SourceChanged);
    assert_eq!(snapshot.read(b).unwrap_err(), SnapshotError::SourceChanged);
    assert_eq!(tree.reads.load(Ordering::Relaxed), 2);
    assert_eq!(snapshot.identity(), Err(SnapshotError::SourceChanged));
    assert_eq!(tree.reads.load(Ordering::Relaxed), 2);
}

#[test]
fn unreadable_files_are_reported_once_without_names() {
    let tree = Memory::new(&[
        ("a.txt", file("A")),
        ("secret-name.txt", Node::UnreadableFile(4)),
    ]);
    let (snapshot, mut findings) = open_with(&tree, SnapshotLimits::default());
    let secret = id(&snapshot, "secret-name.txt");
    assert!(matches!(
        snapshot.read(secret),
        Ok(FileRead::Unreadable(IoFailure::PermissionDenied))
    ));
    let identity = snapshot.identity().unwrap();
    snapshot.report_unreadable(&mut findings).unwrap();
    let findings = findings.finish();
    let reported: Vec<_> = findings
        .as_slice()
        .iter()
        .map(|finding| {
            (
                finding.code().as_str(),
                finding.severity(),
                finding.location().file().map(SourcePath::as_str),
            )
        })
        .collect();
    assert_eq!(
        reported,
        [("FS1019", Severity::Error, Some("secret-name.txt"))]
    );
    for finding in findings.as_slice() {
        assert!(!finding.explanation().contains("secret"));
        assert!(!finding.message().contains("secret"));
    }

    // A readable file of the same size yields a different identity.
    let readable = Memory::new(&[("a.txt", file("A")), ("secret-name.txt", file("ABCD"))]);
    assert_ne!(open(&readable).identity().unwrap(), identity);
}

#[test]
fn foreign_identities_and_hash_limits_fail_explicitly() {
    let tree = Memory::new(&[("a.txt", file("ABC"))]);
    let snapshot = open(&tree);
    let paths = ["a", "b"].map(|path| SourcePath::new(path, PathLimits::default()).unwrap());
    let foreign = KeyTable::new(paths.to_vec(), 2).unwrap();
    let (outside, _) = foreign.iter().nth(1).unwrap();
    assert_eq!(
        snapshot.read(outside).unwrap_err(),
        SnapshotError::UnknownFile
    );
    assert!(!snapshot.is_read(outside));

    let mut limits = SnapshotLimits::default();
    limits.hash.max_bytes = 2;
    let (small, _) = open_with(&tree, limits);
    assert_eq!(
        small.read(id(&small, "a.txt")).unwrap_err(),
        SnapshotError::Hash(HashError::InputTooLong { limit: 2 })
    );
}

#[test]
fn snapshots_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Snapshot>();
}
