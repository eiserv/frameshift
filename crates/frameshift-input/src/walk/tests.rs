//! Walker contracts on in-memory trees whose listings arrive in shuffled order.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use frameshift_core::{FindingLimits, FindingList};

use super::*;

/// A node of an in-memory tree. Names are text; the name `<raw>` stands for
/// a native name that is not Unicode.
#[derive(Clone, Debug)]
pub(crate) enum Node {
    Directory(Vec<usize>),
    File(Vec<u8>),
    /// A link target as stored, or `None` when it cannot be read.
    Link(Option<String>),
    Reparse,
    Special,
    Uninspectable,
    Unreadable,
    UnreadableFile(u64),
}

#[derive(Clone, Debug)]
pub(crate) struct Memory {
    names: Vec<String>,
    nodes: Vec<Node>,
    pub(crate) seed: u64,
    /// A directory that fails the post-listing check.
    changed: Option<usize>,
    /// A file that reads as changed after listing.
    pub(crate) changed_file: Option<usize>,
    /// The number of admitted file reads, shared by clones.
    pub(crate) reads: Arc<AtomicUsize>,
}

impl Memory {
    pub(crate) fn new(spec: &[(&str, Node)]) -> Self {
        let mut tree = Self {
            names: vec![String::new()],
            nodes: vec![Node::Directory(Vec::new())],
            seed: 0,
            changed: None,
            changed_file: None,
            reads: Arc::new(AtomicUsize::new(0)),
        };
        for (path, node) in spec {
            let mut parent = 0;
            let parts: Vec<&str> = path.split('/').collect();
            for (index, part) in parts.iter().enumerate() {
                let last = index + 1 == parts.len();
                let existing = match &tree.nodes[parent] {
                    Node::Directory(children) => children
                        .iter()
                        .copied()
                        .find(|&child| tree.names[child] == *part),
                    Node::File(_)
                    | Node::Link(_)
                    | Node::Reparse
                    | Node::Special
                    | Node::Uninspectable
                    | Node::Unreadable
                    | Node::UnreadableFile(_) => panic!("{path}: parent is not a directory"),
                };
                parent = match existing {
                    Some(child) if !last => child,
                    Some(child) => {
                        tree.nodes[child] = node.clone();
                        child
                    }
                    None => {
                        let child = tree.nodes.len();
                        tree.names.push((*part).to_owned());
                        tree.nodes.push(if last {
                            node.clone()
                        } else {
                            Node::Directory(Vec::new())
                        });
                        if let Node::Directory(children) = &mut tree.nodes[parent] {
                            children.push(child);
                        }
                        child
                    }
                };
            }
        }
        tree
    }

    pub(crate) fn find(&self, path: &str) -> usize {
        let mut node = 0;
        for part in path.split('/') {
            let Node::Directory(children) = &self.nodes[node] else {
                panic!("{path}");
            };
            node = children
                .iter()
                .copied()
                .find(|&child| self.names[child] == part)
                .unwrap();
        }
        node
    }
}

impl Tree for Memory {
    type Location = usize;
    type Name = usize;
    type Identity = usize;

    fn list(
        &self,
        directory: &usize,
        entries: &mut Budget,
    ) -> Result<Vec<Listed<Self>>, ListError> {
        let Node::Directory(children) = &self.nodes[*directory] else {
            return Err(ListError::Io(IoFailure::PermissionDenied));
        };
        let mut order = children.clone();
        // A seeded Fisher-Yates shuffle stands in for filesystem order.
        let mut state = self.seed ^ (*directory as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        for index in (1..order.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            order.swap(index, (state >> 33) as usize % (index + 1));
        }
        let mut listed = Vec::new();
        for child in order {
            entries.charge(1).map_err(|_| ListError::EntryLimit)?;
            let name = &self.names[child];
            let kind = match &self.nodes[child] {
                Node::Directory(_) | Node::Unreadable => RawKind::Directory,
                Node::File(content) => RawKind::File {
                    size: content.len() as u64,
                },
                Node::UnreadableFile(size) => RawKind::File { size: *size },
                Node::Link(_) => RawKind::Link,
                Node::Reparse => RawKind::ReparsePoint { directory: false },
                Node::Special => RawKind::Special,
                Node::Uninspectable => RawKind::Uninspectable(IoFailure::Other),
            };
            listed.push(Listed {
                name: child,
                text: (name != "<raw>").then(|| name.clone()),
                kind,
                identity: child,
            });
        }
        Ok(listed)
    }

    fn child(&self, _directory: &usize, name: &usize) -> usize {
        *name
    }

    fn is_same_directory(&self, directory: &usize, identity: &usize) -> bool {
        directory == identity && self.changed != Some(*directory)
    }

    fn read_regular(&self, file: &usize, identity: &usize, max: u64) -> Result<Vec<u8>, ReadError> {
        assert_eq!(file, identity);
        match &self.nodes[*file] {
            Node::File(content) => {
                let end = content
                    .len()
                    .min(usize::try_from(max.saturating_add(1)).unwrap());
                Ok(content[..end].to_vec())
            }
            Node::Directory(_)
            | Node::Link(_)
            | Node::Reparse
            | Node::Special
            | Node::Uninspectable
            | Node::Unreadable
            | Node::UnreadableFile(_) => Err(ReadError::Io(IoFailure::PermissionDenied)),
        }
    }

    fn read_file(&self, file: &usize, identity: &usize, size: u64) -> Result<Vec<u8>, ReadError> {
        assert_eq!(file, identity);
        self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        if self.changed_file == Some(*file) {
            return Err(ReadError::Changed);
        }
        match &self.nodes[*file] {
            Node::File(content) if content.len() as u64 == size => Ok(content.clone()),
            Node::File(_) => Err(ReadError::Changed),
            Node::UnreadableFile(_) => Err(ReadError::Io(IoFailure::PermissionDenied)),
            Node::Directory(_)
            | Node::Link(_)
            | Node::Reparse
            | Node::Special
            | Node::Uninspectable
            | Node::Unreadable => Err(ReadError::Changed),
        }
    }

    fn read_link(&self, link: &usize) -> RawTarget {
        let Node::Link(Some(target)) = &self.nodes[*link] else {
            return RawTarget::Unreadable;
        };
        if target.is_empty() {
            return RawTarget::Empty;
        }
        if target.starts_with('/') {
            return RawTarget::Absolute;
        }
        let components = target
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| match part {
                "." => TargetComponent::Current,
                ".." => TargetComponent::Parent,
                "<raw>" => TargetComponent::Name(None),
                name => TargetComponent::Name(Some(name.to_owned())),
            })
            .collect();
        RawTarget::Relative {
            components,
            directory: target.ends_with('/'),
        }
    }
}

pub(crate) fn file(content: &str) -> Node {
    Node::File(content.as_bytes().to_vec())
}

pub(crate) fn link(target: &str) -> Node {
    Node::Link(Some(target.to_owned()))
}

struct Run {
    inventory: Result<Inventory, DiscoveryError>,
    findings: FindingList,
}

fn run_with(tree: &Memory, limits: DiscoveryLimits) -> Run {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let inventory = walk(tree, 0, 0, limits, &mut findings);
    Run {
        inventory,
        findings: findings.finish(),
    }
}

fn run(tree: &Memory) -> Inventory {
    run_with(tree, DiscoveryLimits::default())
        .inventory
        .unwrap()
}

fn admitted(inventory: &Inventory) -> Vec<&str> {
    inventory
        .files()
        .as_slice()
        .iter()
        .map(SourcePath::as_str)
        .collect()
}

fn kind<'a>(inventory: &'a Inventory, path: &str) -> &'a EntryKind {
    let path = SourcePath::new(path, PathLimits::default()).unwrap();
    inventory
        .entry(&path)
        .unwrap_or_else(|| panic!("{path:?} not recorded"))
        .kind()
}

fn codes(findings: &FindingList) -> Vec<(&str, Option<&str>)> {
    findings
        .as_slice()
        .iter()
        .map(|finding| {
            (
                finding.code().as_str(),
                finding.location().file().map(SourcePath::as_str),
            )
        })
        .collect()
}

fn ignored(reason: IgnoreReason, directory: bool) -> EntryKind {
    EntryKind::Ignored { reason, directory }
}

#[test]
fn shuffled_listings_give_identical_canonical_results() {
    let spec = [
        ("a/x.txt", file("1")),
        ("a.txt", file("22")),
        ("a-b", file("333")),
        ("a b/c", file("4")),
        ("Z", file("5")),
        ("é.txt", file("6")),
        ("b/c/d.txt", file("7")),
        ("b/c.d", file("8")),
        ("link", link("a/x.txt")),
        (".gitignore", file("*.log\n")),
        ("debug.log", file("9")),
        ("<raw>", file("raw")),
    ];
    let reference = run_with(&Memory::new(&spec), DiscoveryLimits::default());
    let inventory = reference.inventory.as_ref().unwrap();
    assert_eq!(
        admitted(inventory),
        [
            ".gitignore",
            "Z",
            "a b/c",
            "a-b",
            "a.txt",
            "a/x.txt",
            "b/c.d",
            "b/c/d.txt",
            "é.txt"
        ]
    );
    for seed in 1..64 {
        let mut tree = Memory::new(&spec);
        tree.seed = seed;
        let other = run_with(&tree, DiscoveryLimits::default());
        assert_eq!(other.inventory, reference.inventory, "seed {seed}");
        assert_eq!(other.findings, reference.findings, "seed {seed}");
    }
    let sizes: Vec<u64> = inventory
        .files()
        .iter()
        .map(|(id, _)| inventory.file_size(id).unwrap())
        .collect();
    assert_eq!(sizes, [6, 1, 1, 3, 2, 1, 1, 1, 1]);
    assert!(inventory.is_complete());
    assert_eq!(inventory.unrepresentable(), 1);
    assert_eq!(
        kind(inventory, "debug.log"),
        &ignored(IgnoreReason::Gitignore, false)
    );
}

#[test]
fn budgets_follow_canonical_path_order_not_sibling_name_order() {
    // Sibling names sort `a` before `a.txt`, but the path `a.txt` precedes
    // `a/x.txt`; a two-file budget therefore admits `a-b` and `a.txt`.
    let spec = [
        ("a/x.txt", file("1")),
        ("a.txt", file("2")),
        ("a-b", file("3")),
    ];
    let mut limits = DiscoveryLimits::default();
    limits.run.max_files = 2;
    let result = run_with(&Memory::new(&spec), limits);
    let inventory = result.inventory.unwrap();
    assert_eq!(admitted(&inventory), ["a-b", "a.txt"]);
    assert_eq!(inventory.stop(), Some(Stop::Files));
    assert!(!inventory.is_complete());
    assert_eq!(codes(&result.findings), [("FS1011", Some("a/x.txt"))]);
}

#[test]
fn builtin_ignores_and_environment_examples() {
    let spec = [
        (".git/HEAD", file("ref")),
        (".GIT", file("x")),
        ("sub/.hg/x", file("x")),
        ("sub/.svn", file("x")),
        ("node_modules/a/index.js", file("x")),
        ("pkg/Node_Modules/b.js", file("x")),
        (".next/cache", file("x")),
        (".env", file("SECRET")),
        (".env.local", file("SECRET")),
        (".ENV.Production", file("SECRET")),
        (".env.", file("SECRET")),
        ("app/.env.development.local", file("SECRET")),
        (".env.example", file("KEY=")),
        (".env.local.sample", file("KEY=")),
        (".Env.TEMPLATE", file("KEY=")),
        (".envrc", file("export X=1")),
        (".environment", file("x")),
        ("app/page.tsx", file("x")),
    ];
    let inventory = run(&Memory::new(&spec));
    assert_eq!(
        admitted(&inventory),
        [
            ".Env.TEMPLATE",
            ".env.example",
            ".env.local.sample",
            ".environment",
            ".envrc",
            "app/page.tsx",
        ]
    );
    let vcs = IgnoreReason::Vcs;
    assert_eq!(kind(&inventory, ".git"), &ignored(vcs, true));
    assert_eq!(kind(&inventory, ".GIT"), &ignored(vcs, false));
    assert_eq!(kind(&inventory, "sub/.hg"), &ignored(vcs, true));
    assert_eq!(kind(&inventory, "sub/.svn"), &ignored(vcs, false));
    let dependencies = ignored(IgnoreReason::Dependencies, true);
    assert_eq!(kind(&inventory, "node_modules"), &dependencies);
    assert_eq!(kind(&inventory, "pkg/Node_Modules"), &dependencies);
    let build = ignored(IgnoreReason::BuildOutput, true);
    assert_eq!(kind(&inventory, ".next"), &build);
    for secret in [
        ".env",
        ".env.local",
        ".ENV.Production",
        ".env.",
        "app/.env.development.local",
    ] {
        let environment = ignored(IgnoreReason::Environment, false);
        assert_eq!(kind(&inventory, secret), &environment, "{secret}");
    }
    // Nothing below an ignored directory is listed or counted.
    let below = SourcePath::new("node_modules/a", PathLimits::default()).unwrap();
    assert!(inventory.entry(&below).is_none());
    assert!(inventory.is_complete());
}

#[test]
fn gitignore_levels_negation_and_excluded_parents() {
    let spec = [
        (
            ".gitignore",
            file("*.log\n/dist/\nbuild\n!keep.log\nnested/*\n!nested/kept\n"),
        ),
        ("a.log", file("x")),
        ("keep.log", file("x")),
        ("dist/out.js", file("x")),
        ("src/dist/out.js", file("x")),
        ("src/build/out.js", file("x")),
        ("src/build.js", file("x")),
        ("nested/kept", file("x")),
        ("nested/dropped", file("x")),
        ("pkg/.gitignore", file("!a.log\n/local\n")),
        ("pkg/a.log", file("x")),
        ("pkg/local", file("x")),
        ("pkg/sub/local", file("x")),
        ("logs.log/.gitignore", file("!x.log\n")),
        ("logs.log/x.log", file("x")),
    ];
    let inventory = run(&Memory::new(&spec));
    assert_eq!(
        admitted(&inventory),
        [
            ".gitignore",
            "keep.log",
            "nested/kept",
            "pkg/.gitignore",
            "pkg/a.log",
            "pkg/sub/local",
            "src/build.js",
            "src/dist/out.js",
        ]
    );
    let gitignore = IgnoreReason::Gitignore;
    assert_eq!(kind(&inventory, "a.log"), &ignored(gitignore, false));
    assert_eq!(kind(&inventory, "dist"), &ignored(gitignore, true));
    assert_eq!(kind(&inventory, "src/build"), &ignored(gitignore, true));
    assert_eq!(
        kind(&inventory, "nested/dropped"),
        &ignored(gitignore, false)
    );
    assert_eq!(kind(&inventory, "pkg/local"), &ignored(gitignore, false));
    // An ignored directory is not traversed, so `!x.log` inside cannot apply.
    assert_eq!(kind(&inventory, "logs.log"), &ignored(gitignore, true));
}

#[test]
fn per_file_and_depth_limits_continue_while_cumulative_limits_stop() {
    let spec = [
        ("a/b/c/deep.txt", file("x")),
        ("big.bin", file("0123456789")),
        ("small.txt", file("12")),
        ("z.txt", file("123")),
    ];
    let mut limits = DiscoveryLimits::default();
    limits.run.max_file_bytes = 5;
    limits.run.max_directory_depth = 2;
    let result = run_with(&Memory::new(&spec), limits);
    let inventory = result.inventory.unwrap();
    assert_eq!(admitted(&inventory), ["small.txt", "z.txt"]);
    assert_eq!(
        kind(&inventory, "big.bin"),
        &EntryKind::Oversized { bytes: 10 }
    );
    assert_eq!(
        kind(&inventory, "a/b/c"),
        &EntryKind::NotTraversed(NotTraversed::DepthLimit)
    );
    assert_eq!(kind(&inventory, "a/b"), &EntryKind::Directory);
    assert_eq!(inventory.stop(), None);
    assert!(!inventory.is_complete());
    // Findings sort by severity, then location.
    assert_eq!(
        codes(&result.findings),
        [("FS1009", Some("a/b/c")), ("FS1004", Some("big.bin"))]
    );

    let mut limits = DiscoveryLimits::default();
    limits.run.max_total_bytes = 4;
    let result = run_with(&Memory::new(&spec), limits);
    let inventory = result.inventory.unwrap();
    assert_eq!(admitted(&inventory), ["a/b/c/deep.txt"]);
    assert_eq!(inventory.stop(), Some(Stop::TotalBytes));
    assert_eq!(codes(&result.findings), [("FS1012", Some("big.bin"))]);

    // The entry limit counts every name read: root (4), `a` (1), `a/b` (1).
    for (limit, expected) in [(6, None), (5, Some("a/b")), (4, Some("a")), (3, None)] {
        let mut limits = DiscoveryLimits::default();
        limits.run.max_entries = limit;
        let result = run_with(&Memory::new(&spec), limits);
        let inventory = result.inventory.unwrap();
        if limit == 6 {
            // `a/b/c` lists one more entry.
            assert_eq!(inventory.stop(), Some(Stop::Entries));
            assert_eq!(codes(&result.findings), [("FS1013", Some("a/b/c"))]);
            continue;
        }
        assert_eq!(inventory.stop(), Some(Stop::Entries), "{limit}");
        assert_eq!(codes(&result.findings), [("FS1013", expected)], "{limit}");
    }
}

#[test]
fn gitignore_limits_stop_in_canonical_order() {
    let spec = [
        (".gitignore", file("a\nb\n")),
        ("sub/.gitignore", file("c\n")),
        ("sub/file", file("x")),
    ];
    let mut limits = DiscoveryLimits::default();
    limits.ignore.max_total_bytes = 5;
    let result = run_with(&Memory::new(&spec), limits);
    let inventory = result.inventory.unwrap();
    assert_eq!(inventory.stop(), Some(Stop::IgnoreBytes));
    assert_eq!(
        codes(&result.findings),
        [("FS1014", Some("sub/.gitignore"))]
    );

    let mut limits = DiscoveryLimits::default();
    limits.ignore.max_total_bytes = 3;
    let result = run_with(&Memory::new(&spec), limits);
    assert_eq!(codes(&result.findings), [("FS1014", Some(".gitignore"))]);
    assert!(admitted(&result.inventory.unwrap()).is_empty());

    // `.gitignore` is checked against `b` (2 * 11) and `a` (2 * 11) first.
    let mut limits = DiscoveryLimits::default();
    limits.ignore.max_match_work = 43;
    let result = run_with(&Memory::new(&spec), limits);
    assert_eq!(result.inventory.unwrap().stop(), Some(Stop::IgnoreWork));
    assert_eq!(codes(&result.findings), [("FS1015", Some(".gitignore"))]);
}

#[test]
fn links_are_resolved_lexically_through_real_directories_only() {
    let spec = [
        ("assets/logo.svg", file("x")),
        ("assets/sub/icon.svg", file("x")),
        ("public/logo", link("../assets/logo.svg")),
        ("public/dir", link("../assets/")),
        ("public/root", link("..")),
        ("public/self", link("../public/self")),
        ("public/up", link("../../outside")),
        ("public/missing", link("../assets/none.svg")),
        ("public/file-as-dir", link("../assets/logo.svg/x")),
        ("public/slash-file", link("../assets/logo.svg/")),
        ("public/through", link("../public/dir/sub/icon.svg")),
        ("public/modules", link("../node_modules/pkg")),
        (
            "public/absolute",
            Node::Link(Some("/etc/passwd".to_owned())),
        ),
        ("public/unreadable", Node::Link(None)),
        ("public/raw", link("<raw>/x")),
        ("public/empty", link("")),
        ("node_modules/pkg/index.js", file("x")),
    ];
    let result = run_with(&Memory::new(&spec), DiscoveryLimits::default());
    let inventory = result.inventory.unwrap();
    let path =
        |text: &str| LinkReference::Path(SourcePath::new(text, PathLimits::default()).unwrap());
    let expectations = [
        ("public/logo", LinkTarget::Inside(path("assets/logo.svg"))),
        ("public/dir", LinkTarget::Inside(path("assets"))),
        ("public/root", LinkTarget::Inside(LinkReference::Root)),
        ("public/self", LinkTarget::Inside(path("public/self"))),
        ("public/up", LinkTarget::Escapes),
        ("public/missing", LinkTarget::Dangling),
        ("public/file-as-dir", LinkTarget::Dangling),
        ("public/slash-file", LinkTarget::Dangling),
        (
            "public/through",
            LinkTarget::Unresolved(Unresolved::ThroughLink),
        ),
        (
            "public/modules",
            LinkTarget::Unresolved(Unresolved::Untraversed),
        ),
        (
            "public/absolute",
            LinkTarget::Unresolved(Unresolved::Absolute),
        ),
        (
            "public/unreadable",
            LinkTarget::Unresolved(Unresolved::Unreadable),
        ),
        (
            "public/raw",
            LinkTarget::Unresolved(Unresolved::Unrepresentable),
        ),
        ("public/empty", LinkTarget::Dangling),
    ];
    let count = expectations.len();
    for (link_path, expected) in expectations {
        assert_eq!(
            kind(&inventory, link_path),
            &EntryKind::Link(expected),
            "{link_path}"
        );
    }
    // Links are never admitted, and their targets are admitted once.
    assert_eq!(
        admitted(&inventory),
        ["assets/logo.svg", "assets/sub/icon.svg"]
    );
    let found = codes(&result.findings);
    assert!(found.contains(&("FS1003", Some("public/up"))));
    assert!(found.contains(&("FS1005", Some("public/logo"))));
    assert!(found.contains(&("FS1006", Some("public/absolute"))));
    assert!(found.contains(&("FS1007", Some("public/missing"))));
    assert_eq!(found.len(), count);
}

#[test]
fn absent_targets_are_unresolved_when_discovery_stopped() {
    let spec = [("a/link", link("../b/file")), ("b/file", file("x"))];
    let mut limits = DiscoveryLimits::default();
    limits.run.max_entries = 3;
    let inventory = run_with(&Memory::new(&spec), limits).inventory.unwrap();
    assert_eq!(inventory.stop(), Some(Stop::Entries));
    assert_eq!(
        kind(&inventory, "a/link"),
        &EntryKind::Link(LinkTarget::Unresolved(Unresolved::Incomplete))
    );
}

#[test]
fn unrepresentable_names_are_counted_at_their_parent() {
    let spec = [
        ("<raw>", file("x")),
        ("dir/<raw>/inner", file("x")),
        ("dir/a\\b", file("x")),
        ("C:", file("x")),
        ("long/abcdefghij", file("x")),
        ("ok", file("x")),
    ];
    let mut limits = DiscoveryLimits::default();
    limits.path.max_path_bytes = 12;
    let result = run_with(&Memory::new(&spec), limits);
    let inventory = result.inventory.unwrap();
    assert_eq!(inventory.unrepresentable(), 5);
    assert!(!inventory.is_complete());
    assert_eq!(admitted(&inventory), ["ok"]);
    assert_eq!(
        codes(&result.findings),
        [
            ("FS1001", None),
            ("FS1001", None),
            ("FS1001", Some("dir")),
            ("FS1001", Some("dir")),
            ("FS1001", Some("long")),
        ]
    );
    for finding in result.findings.as_slice() {
        assert!(!finding.explanation().contains("abcdefghij"));
        assert!(!finding.explanation().contains("a\\b"));
    }

    // A file with an unrepresentable name keeps the inventory complete.
    let inventory = run(&Memory::new(&[("<raw>", file("x"))]));
    assert!(inventory.is_complete());
}

#[test]
fn excluded_and_failing_entries_are_reported() {
    let spec = [
        ("cloud", Node::Reparse),
        ("fifo", Node::Special),
        ("broken", Node::Uninspectable),
        ("locked", Node::Unreadable),
        ("secret/.gitignore", Node::UnreadableFile(4)),
        ("secret/data", file("x")),
        ("ok", file("x")),
    ];
    let result = run_with(&Memory::new(&spec), DiscoveryLimits::default());
    let inventory = result.inventory.unwrap();
    assert_eq!(admitted(&inventory), ["ok"]);
    assert_eq!(
        kind(&inventory, "cloud"),
        &EntryKind::ReparsePoint { directory: false }
    );
    assert_eq!(kind(&inventory, "fifo"), &EntryKind::Special);
    assert_eq!(kind(&inventory, "broken"), &EntryKind::Uninspectable);
    assert_eq!(
        kind(&inventory, "locked"),
        &EntryKind::NotTraversed(NotTraversed::Unreadable)
    );
    assert_eq!(
        kind(&inventory, "secret"),
        &EntryKind::NotTraversed(NotTraversed::IgnoreFileUnreadable)
    );
    assert!(!inventory.is_complete());
    let severities: Vec<(&str, Severity)> = result
        .findings
        .as_slice()
        .iter()
        .map(|finding| (finding.code().as_str(), finding.severity()))
        .collect();
    assert_eq!(
        severities,
        [
            ("FS1017", Severity::Error),
            ("FS1016", Severity::Error),
            ("FS1018", Severity::Error),
            ("FS1008", Severity::Unknown),
            ("FS1002", Severity::Info),
        ]
    );
}

#[test]
fn run_failures_leave_no_inventory() {
    let spec = [("dir/file", file("x"))];
    let mut tree = Memory::new(&spec);
    tree.changed = Some(tree.find("dir"));
    assert_eq!(
        run_with(&tree, DiscoveryLimits::default()).inventory,
        Err(DiscoveryError::SourceChanged)
    );

    let tree = Memory {
        nodes: vec![Node::Unreadable],
        ..Memory::new(&[])
    };
    assert_eq!(
        run_with(&tree, DiscoveryLimits::default()).inventory,
        Err(DiscoveryError::Root(IoFailure::PermissionDenied))
    );

    let mut findings = FindingCollector::new(FindingLimits {
        max_text_bytes: 64 * 1024,
        max_findings: 1,
    });
    let tree = Memory::new(&[("a", Node::Special), ("b", Node::Special)]);
    assert_eq!(
        walk(&tree, 0, 0, DiscoveryLimits::default(), &mut findings),
        Err(DiscoveryError::Findings(FindingError::TooManyFindings))
    );
}

#[test]
fn findings_never_echo_names_in_prose() {
    let spec = [
        ("visible-name/<raw>", file("x")),
        ("visible-name/huge", file("0123456789")),
        ("visible-name/link", link("../../escape")),
    ];
    let mut limits = DiscoveryLimits::default();
    limits.run.max_file_bytes = 1;
    let result = run_with(&Memory::new(&spec), limits);
    assert_eq!(result.findings.as_slice().len(), 3);
    for finding in result.findings.as_slice() {
        for text in [
            finding.message(),
            finding.explanation(),
            finding.remediation(),
        ] {
            assert!(
                !text.contains("visible-name") && !text.contains("escape"),
                "{text}"
            );
        }
    }
}
