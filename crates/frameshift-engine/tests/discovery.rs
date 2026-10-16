//! Discovery contracts on real directories created at test time (`D-064`).
//!
//! Inputs that cannot be committed (symbolic links, very large files,
//! sockets, and names that are not Unicode) are created here, below Cargo's
//! temporary directory, never in the repository.

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, PathLimits, SourcePath,
};
use frameshift_engine::input::{
    DiscoveryError, DiscoveryLimits, EntryKind, IgnoreReason, Inventory, IoFailure, discover,
};
use frameshift_engine::limits::Limits;

mod support;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("discovery").join(name);
    // A verbatim path also removes names that Win32 paths would rewrite.
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

fn run(root: &Path, limits: DiscoveryLimits) -> (Result<Inventory, DiscoveryError>, FindingList) {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let inventory = discover(root, limits, &mut findings);
    (inventory, findings.finish())
}

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn admitted(inventory: &Inventory) -> Vec<&str> {
    inventory
        .files()
        .as_slice()
        .iter()
        .map(SourcePath::as_str)
        .collect()
}

fn kind<'a>(inventory: &'a Inventory, text: &str) -> &'a EntryKind {
    inventory
        .entry(&path(text))
        .unwrap_or_else(|| panic!("{text} not recorded"))
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

/// Every entry below `root` with its content or link target, without following links.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let native = entry.path();
            let file_type = fs::symlink_metadata(&native).unwrap().file_type();
            let content = if file_type.is_symlink() {
                fs::read_link(&native)
                    .unwrap()
                    .into_os_string()
                    .into_encoded_bytes()
            } else if file_type.is_dir() {
                pending.push(native.clone());
                b"<dir>".to_vec()
            } else {
                fs::read(&native).unwrap()
            };
            result.push((native, content));
        }
    }
    result.sort();
    result
}

#[test]
fn discovery_is_read_only_repeatable_and_free_of_absolute_paths() {
    let root = scratch("read-only");
    write(
        &root,
        "app/page.tsx",
        b"export default function Page() {}\n",
    );
    write(
        &root,
        "app/layout.tsx",
        b"export default function Layout() {}\n",
    );
    write(&root, "package.json", b"{}\n");
    write(&root, ".gitignore", b"/out/\n*.log\n");
    write(&root, "out/index.html", b"<html></html>\n");
    write(&root, "debug.log", b"log\n");
    write(&root, ".env", b"FICTIONAL_SECRET=1\n");
    write(&root, ".env.example", b"FICTIONAL_SECRET=\n");
    write(&root, "node_modules/pkg/index.js", b"module.exports = 1;\n");
    write(&root, ".git/HEAD", b"ref: refs/heads/main\n");
    let before = snapshot(&root);

    let (first, first_findings) = run(&root, DiscoveryLimits::default());
    let (second, second_findings) = run(&root, DiscoveryLimits::default());
    assert_eq!(first, second);
    assert_eq!(first_findings, second_findings);
    assert_eq!(snapshot(&root), before);

    let inventory = first.unwrap();
    assert!(inventory.is_complete());
    assert_eq!(inventory.stop(), None);
    assert_eq!(
        admitted(&inventory),
        [
            ".env.example",
            ".gitignore",
            "app/layout.tsx",
            "app/page.tsx",
            "package.json"
        ]
    );
    let sizes: Vec<u64> = inventory
        .files()
        .iter()
        .map(|(id, _)| inventory.file_size(id).unwrap())
        .collect();
    assert_eq!(sizes, [18, 12, 36, 34, 3]);
    let ignored = |reason, directory| EntryKind::Ignored { reason, directory };
    assert_eq!(kind(&inventory, ".git"), &ignored(IgnoreReason::Vcs, true));
    assert_eq!(
        kind(&inventory, ".env"),
        &ignored(IgnoreReason::Environment, false)
    );
    assert_eq!(
        kind(&inventory, "node_modules"),
        &ignored(IgnoreReason::Dependencies, true)
    );
    assert_eq!(
        kind(&inventory, "out"),
        &ignored(IgnoreReason::Gitignore, true)
    );
    assert_eq!(
        kind(&inventory, "debug.log"),
        &ignored(IgnoreReason::Gitignore, false)
    );
    assert_eq!(kind(&inventory, "app"), &EntryKind::Directory);
    assert!(first_findings.as_slice().is_empty());

    // Findings name paths relative to the root only.
    write(&root, "huge.bin", &[0; 100]);
    let mut limits = DiscoveryLimits::default();
    limits.run.max_file_bytes = 50;
    let (_, findings) = run(&root, limits);
    assert_eq!(codes(&findings), [("FS1004", Some("huge.bin"))]);
    let absolute = root.to_string_lossy().into_owned();
    let marker = root.file_name().unwrap().to_string_lossy().into_owned();
    for finding in findings.as_slice() {
        for text in [
            finding.message(),
            finding.explanation(),
            finding.remediation(),
        ] {
            assert!(
                !text.contains(&absolute) && !text.contains(&marker),
                "{text}"
            );
        }
    }
}

#[test]
fn very_large_files_are_accounted_but_not_admitted() {
    let root = scratch("large");
    let limit = DiscoveryLimits::default().run.max_file_bytes;
    for (name, size) in [("at-limit.bin", limit), ("over-limit.bin", limit + 1)] {
        // Sparse extension keeps the test fast; discovery never reads the content.
        fs::File::create(root.join(name))
            .unwrap()
            .set_len(size)
            .unwrap();
    }
    let (inventory, findings) = run(&root, DiscoveryLimits::default());
    let inventory = inventory.unwrap();
    assert_eq!(admitted(&inventory), ["at-limit.bin"]);
    assert_eq!(
        kind(&inventory, "over-limit.bin"),
        &EntryKind::Oversized { bytes: limit + 1 }
    );
    assert_eq!(codes(&findings), [("FS1004", Some("over-limit.bin"))]);
    assert!(inventory.is_complete());
}

#[test]
fn roots_must_be_existing_directories() {
    let root = scratch("roots");
    let (missing, _) = run(&root.join("absent"), DiscoveryLimits::default());
    assert_eq!(missing, Err(DiscoveryError::Root(IoFailure::NotFound)));
    write(&root, "file.txt", b"x");
    let (file, findings) = run(&root.join("file.txt"), DiscoveryLimits::default());
    assert_eq!(file, Err(DiscoveryError::RootNotDirectory));
    assert!(findings.as_slice().is_empty());
}

#[test]
fn engine_limits_feed_discovery() {
    let limits = Limits::default();
    assert_eq!(limits.validate(), Ok(()));
    assert_eq!(limits.discovery(), DiscoveryLimits::default());
    let mut changed = limits;
    changed.run.max_entries = 7;
    changed.ignore.max_match_work = 9;
    changed.path.max_components = 11;
    let discovery = changed.discovery();
    assert_eq!(discovery.run.max_entries, 7);
    assert_eq!(discovery.ignore.max_match_work, 9);
    assert_eq!(discovery.path.max_components, 11);
}

/// Creates a symbolic link, or returns `false` where the host does not
/// permit it (Windows without the symbolic link privilege).
fn symlink(root: &Path, link: &str, target: &str, directory: bool) -> bool {
    let native = root.join(link);
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    #[cfg(unix)]
    let result = {
        let _ = directory;
        std::os::unix::fs::symlink(target, &native)
    };
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, &native)
    } else {
        std::os::windows::fs::symlink_file(target, &native)
    };
    match result {
        Ok(()) => true,
        Err(error) if cfg!(windows) => {
            eprintln!("skipping symbolic link test: {error}");
            false
        }
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn symbolic_links_are_never_followed() {
    let base = scratch("links");
    let root = base.join("project");
    write(&base, "outside/secret.txt", b"outside\n");
    write(&root, "assets/logo.svg", b"<svg/>\n");
    write(&root, "real/.gitignore", b"hidden.txt\n");
    write(&root, "linked/hidden.txt", b"x\n");
    let links = [
        ("public/logo.svg", "../assets/logo.svg", false),
        ("public/assets", "../assets", true),
        ("public/escape", "../../outside/secret.txt", false),
        ("public/outside", "../../outside", true),
        ("public/missing", "../assets/none.svg", false),
        ("linked/.gitignore", "../real/.gitignore", false),
    ];
    for (link, target, directory) in links {
        if !symlink(&root, link, target, directory) {
            return;
        }
    }
    let before = snapshot(&base);
    let (inventory, findings) = run(&root, DiscoveryLimits::default());
    assert_eq!(snapshot(&base), before);
    let inventory = inventory.unwrap();
    // A linked `.gitignore` is not read, so `linked/hidden.txt` stays admitted;
    // nothing outside the root is listed.
    assert_eq!(
        admitted(&inventory),
        ["assets/logo.svg", "linked/hidden.txt", "real/.gitignore"]
    );
    for (link, _, _) in links {
        assert!(
            matches!(kind(&inventory, link), EntryKind::Link(_)),
            "{link}"
        );
    }
    let found = codes(&findings);
    assert!(found.contains(&("FS1003", Some("public/escape"))));
    assert!(found.contains(&("FS1003", Some("public/outside"))));
    assert!(found.contains(&("FS1005", Some("public/logo.svg"))));
    assert!(found.contains(&("FS1005", Some("public/assets"))));
    assert!(found.contains(&("FS1005", Some("linked/.gitignore"))));
    assert!(found.contains(&("FS1007", Some("public/missing"))));
    assert_eq!(found.len(), links.len());
    assert!(inventory.is_complete());
}

#[cfg(unix)]
#[test]
fn special_files_are_skipped() {
    let root = scratch("special");
    write(&root, "ok.txt", b"x");
    let _listener = std::os::unix::net::UnixListener::bind(root.join("app.sock")).unwrap();
    let (inventory, findings) = run(&root, DiscoveryLimits::default());
    let inventory = inventory.unwrap();
    assert_eq!(admitted(&inventory), ["ok.txt"]);
    assert_eq!(kind(&inventory, "app.sock"), &EntryKind::Special);
    assert_eq!(codes(&findings), [("FS1002", Some("app.sock"))]);
}

#[cfg(target_os = "linux")]
#[test]
fn names_outside_the_path_grammar_are_counted() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let root = scratch("unrepresentable");
    write(&root, "ok.txt", b"x");
    fs::write(root.join(OsStr::from_bytes(b"bad\xff.txt")), b"x").unwrap();
    fs::write(root.join("back\\slash"), b"x").unwrap();
    fs::create_dir(root.join(OsStr::from_bytes(b"dir\xfe"))).unwrap();
    let (inventory, findings) = run(&root, DiscoveryLimits::default());
    let inventory = inventory.unwrap();
    assert_eq!(admitted(&inventory), ["ok.txt"]);
    assert_eq!(inventory.unrepresentable(), 3);
    assert!(!inventory.is_complete());
    assert_eq!(codes(&findings), [("FS1001", None); 3]);
}

#[cfg(windows)]
#[test]
fn windows_names_stay_exact() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let root = scratch("windows-names");
    // Verbatim paths create names that ordinary Win32 paths would rewrite.
    let verbatim = fs::canonicalize(&root).unwrap();
    fs::create_dir(verbatim.join("dir.")).unwrap();
    fs::write(verbatim.join("dir.").join("inner.txt"), b"1").unwrap();
    fs::create_dir(verbatim.join("dir")).unwrap();
    fs::write(verbatim.join("dir").join("other.txt"), b"22").unwrap();
    fs::write(verbatim.join("CON.ts"), b"333").unwrap();
    let unpaired = OsString::from_wide(&[u16::from(b'x'), 0xD800]);
    fs::write(verbatim.join(unpaired), b"x").unwrap();
    let (inventory, findings) = run(&root, DiscoveryLimits::default());
    let inventory = inventory.unwrap();
    assert_eq!(
        admitted(&inventory),
        ["CON.ts", "dir./inner.txt", "dir/other.txt"]
    );
    assert_eq!(kind(&inventory, "dir."), &EntryKind::Directory);
    assert_eq!(inventory.unrepresentable(), 1);
    assert_eq!(codes(&findings), [("FS1001", None)]);
}
