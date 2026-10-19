//! Discovery fixtures below `fixtures/discovery/` against their goldens (`D-062`).
//!
//! Each case of kind `discovery` copies `source/` into Cargo's temporary directory, creates the
//! inputs that `fixture.json` lists under `createdAtTestTime`, applies its
//! limit overrides, runs discovery twice, and compares a deterministic text
//! rendering of the inventory and findings with `expected/discovery.txt`.
//! The rendering is a test format, not a product document; it is replaced
//! by the `inspect` and analysis documents once they exist. Goldens change
//! only through the ignored `update_discovery_goldens` test.

mod support;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{FindingCollector, FindingLimits, FindingLocation, Severity};
use frameshift_engine::input::{
    DiscoveryLimits, EntryKind, Inventory, LinkReference, LinkTarget, NotTraversed, Stop,
    Unresolved, discover,
};
use support::Json;

/// Cargo runs integration tests in the package directory, which also holds
/// for test binaries cross-built for Linux and run in WSL (section 5 of
/// IMPLEMENTATION.md).
fn fixtures() -> PathBuf {
    PathBuf::from("../../fixtures/discovery")
}

enum Created {
    Directory(String),
    File(String, String),
    Sparse(String, u64),
    Symlink(String, String),
}

struct Case {
    name: String,
    directory: PathBuf,
    limits: DiscoveryLimits,
    created: Vec<Created>,
}

fn string(value: &Json, key: &str) -> String {
    value
        .get(key)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("missing string {key}"))
        .to_owned()
}

fn cases() -> Vec<Case> {
    let mut names: Vec<String> = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert!(!names.is_empty());
    names
        .into_iter()
        .filter_map(|name| {
            let directory = fixtures().join(&name);
            let text = fs::read_to_string(directory.join("fixture.json")).unwrap();
            let metadata = support::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
            // Package cases have their own harness (`package_fixtures.rs`).
            if metadata.get("kind").and_then(Json::as_str) == Some("package") {
                return None;
            }
            for key in metadata.keys() {
                assert!(
                    [
                        "$schema",
                        "kind",
                        "description",
                        "limits",
                        "createdAtTestTime"
                    ]
                    .contains(&key),
                    "{name}: unknown key {key}"
                );
            }
            assert_eq!(
                metadata.get("kind").and_then(Json::as_str),
                Some("discovery")
            );
            assert!(!string(&metadata, "description").is_empty());
            let mut limits = DiscoveryLimits::default();
            if let Some(overrides) = metadata.get("limits") {
                for key in overrides.keys() {
                    let value = overrides.get(key).and_then(Json::as_u64).unwrap();
                    let size = || usize::try_from(value).unwrap();
                    match key {
                        "run.max-files" => limits.run.max_files = size(),
                        "run.max-file-bytes" => limits.run.max_file_bytes = value,
                        "run.max-total-bytes" => limits.run.max_total_bytes = value,
                        "run.max-directory-depth" => limits.run.max_directory_depth = size(),
                        "run.max-entries" => limits.run.max_entries = size(),
                        "path.max-path-bytes" => limits.path.max_path_bytes = size(),
                        "path.max-components" => limits.path.max_components = size(),
                        "ignore.max-total-bytes" => limits.ignore.max_total_bytes = value,
                        "ignore.max-match-work" => limits.ignore.max_match_work = value,
                        other => panic!("{name}: unknown limit {other}"),
                    }
                }
            }
            let mut created = Vec::new();
            if let Some(Json::Array(items)) = metadata.get("createdAtTestTime") {
                for item in items {
                    let path = string(item, "path");
                    created.push(match item.get("kind").and_then(Json::as_str) {
                        Some("directory") => Created::Directory(path),
                        Some("file") => Created::File(path, string(item, "content")),
                        Some("sparse-file") => {
                            Created::Sparse(path, item.get("bytes").and_then(Json::as_u64).unwrap())
                        }
                        Some("symlink") => Created::Symlink(path, string(item, "target")),
                        other => panic!("{name}: unknown input kind {other:?}"),
                    });
                }
            }
            Some(Case {
                name,
                directory,
                limits,
                created,
            })
        })
        .collect()
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    let mut entries: Vec<_> = fs::read_dir(from).unwrap().map(Result::unwrap).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type().unwrap();
        let target = to.join(entry.file_name());
        assert!(!kind.is_symlink(), "committed fixtures contain no links");
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Prepares the case's tree, or returns `None` when the host cannot create
/// one of its inputs (symbolic links on Windows without the privilege).
fn prepare(case: &Case) -> Option<PathBuf> {
    let root = support::temporary()
        .join("discovery-fixtures")
        .join(&case.name);
    if let Ok(existing) = fs::canonicalize(&root) {
        fs::remove_dir_all(existing).unwrap();
    }
    copy_tree(&case.directory.join("source"), &root);
    for input in &case.created {
        match input {
            Created::Directory(path) => fs::create_dir_all(root.join(path)).unwrap(),
            Created::File(path, content) => {
                let native = root.join(path);
                fs::create_dir_all(native.parent().unwrap()).unwrap();
                fs::write(native, content).unwrap();
            }
            Created::Sparse(path, bytes) => {
                let native = root.join(path);
                fs::create_dir_all(native.parent().unwrap()).unwrap();
                fs::File::create(native).unwrap().set_len(*bytes).unwrap();
            }
            Created::Symlink(path, target) => {
                let native = root.join(path);
                fs::create_dir_all(native.parent().unwrap()).unwrap();
                if let Err(error) = symlink(&native, target) {
                    if !cfg!(windows) {
                        panic!("{}: {error}", case.name);
                    }
                    eprintln!("skipping {}: {error}", case.name);
                    return None;
                }
            }
        }
    }
    Some(root)
}

#[cfg(unix)]
fn symlink(link: &Path, target: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(link: &Path, target: &str) -> std::io::Result<()> {
    if link.parent().unwrap().join(target).is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// Escapes backslashes and control characters so that every path and line
/// renders on one line.
fn escape(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            character if character.is_control() => {
                write!(out, "\\u{{{:x}}}", u32::from(character)).unwrap();
            }
            character => out.push(character),
        }
    }
    out
}

fn kind(kind: &EntryKind) -> String {
    match kind {
        EntryKind::Directory => "directory".to_owned(),
        EntryKind::Ignored { reason, directory } => {
            let what = if *directory { "directory" } else { "entry" };
            format!("ignored {} {what}", reason.as_str())
        }
        EntryKind::Oversized { bytes } => format!("oversized {bytes}"),
        EntryKind::Link(target) => match target {
            LinkTarget::Inside(LinkReference::Root) => "link inside root".to_owned(),
            LinkTarget::Inside(LinkReference::Path(path)) => {
                format!("link inside {}", escape(path.as_str()))
            }
            LinkTarget::Escapes => "link escapes".to_owned(),
            LinkTarget::Dangling => "link dangling".to_owned(),
            LinkTarget::Unresolved(reason) => format!(
                "link unresolved {}",
                match reason {
                    Unresolved::Absolute => "absolute",
                    Unresolved::ThroughLink => "through-link",
                    Unresolved::Untraversed => "untraversed",
                    Unresolved::Unreadable => "unreadable",
                    Unresolved::Unrepresentable => "unrepresentable",
                    Unresolved::Incomplete => "incomplete",
                }
            ),
        },
        EntryKind::ReparsePoint { directory } => format!("reparse-point directory={directory}"),
        EntryKind::Special => "special".to_owned(),
        EntryKind::NotTraversed(reason) => format!(
            "not-traversed {}",
            match reason {
                NotTraversed::DepthLimit => "depth-limit",
                NotTraversed::Unreadable => "unreadable",
                NotTraversed::IgnoreFileUnreadable => "ignore-file-unreadable",
            }
        ),
        EntryKind::Uninspectable => "uninspectable".to_owned(),
    }
}

fn render(inventory: &Inventory, findings: &frameshift_engine::foundation::FindingList) -> String {
    let mut out = String::new();
    let stop = match inventory.stop() {
        None => "none",
        Some(Stop::Files) => "files",
        Some(Stop::TotalBytes) => "total-bytes",
        Some(Stop::Entries) => "entries",
        Some(Stop::IgnoreBytes) => "ignore-bytes",
        Some(Stop::IgnoreWork) => "ignore-work",
    };
    writeln!(out, "complete: {}", inventory.is_complete()).unwrap();
    writeln!(out, "stop: {stop}").unwrap();
    writeln!(out, "unrepresentable: {}", inventory.unrepresentable()).unwrap();
    writeln!(out, "files:").unwrap();
    for (id, path) in inventory.files().iter() {
        let size = inventory.file_size(id).unwrap();
        writeln!(out, "  {size} {}", escape(path.as_str())).unwrap();
    }
    writeln!(out, "entries:").unwrap();
    for entry in inventory.entries() {
        writeln!(
            out,
            "  {}: {}",
            escape(entry.path().as_str()),
            kind(entry.kind())
        )
        .unwrap();
    }
    writeln!(out, "findings:").unwrap();
    for (id, finding) in findings.iter() {
        let severity = match finding.severity() {
            Severity::Error => "error",
            Severity::Blocker => "blocker",
            Severity::Unknown => "unknown",
            Severity::Review => "review",
            Severity::Info => "info",
        };
        let location = match finding.location() {
            FindingLocation::Project => "project".to_owned(),
            FindingLocation::File(path) => escape(path.as_str()),
            FindingLocation::Text(text) => escape(text.file().as_str()),
        };
        writeln!(
            out,
            "  {id} {severity} {} {location}: {}",
            finding.code().as_str(),
            escape(finding.message())
        )
        .unwrap();
        writeln!(out, "    {}", escape(finding.explanation())).unwrap();
    }
    out
}

fn produce(case: &Case, root: &Path) -> String {
    let mut first = FindingCollector::new(FindingLimits::default());
    let inventory = discover(root, case.limits, &mut first).unwrap();
    let mut second = FindingCollector::new(FindingLimits::default());
    let again = discover(root, case.limits, &mut second).unwrap();
    let (first, second) = (first.finish(), second.finish());
    assert_eq!(inventory, again, "{}", case.name);
    assert_eq!(first, second, "{}", case.name);
    render(&inventory, &first)
}

fn golden(case: &Case) -> PathBuf {
    case.directory.join("expected").join("discovery.txt")
}

#[test]
fn discovery_fixtures_match_goldens() {
    let mut compared = 0;
    for case in cases() {
        let Some(root) = prepare(&case) else {
            continue;
        };
        let actual = produce(&case, &root);
        let expected =
            fs::read(golden(&case)).unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(
            actual,
            String::from_utf8(expected).unwrap(),
            "{}: run the ignored update_discovery_goldens test after reviewing the change",
            case.name
        );
        compared += 1;
    }
    assert!(compared >= 5);
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-engine --offline --locked --test discovery_fixtures -- --ignored update_discovery_goldens"]
fn update_discovery_goldens() {
    for case in cases() {
        // A case this host cannot prepare keeps its golden; update it on a
        // host that permits its inputs.
        let Some(root) = prepare(&case) else {
            continue;
        };
        let path = golden(&case);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, produce(&case, &root)).unwrap();
    }
}

#[test]
fn fixture_metadata_reader_is_strict() {
    assert!(support::parse("{\"a\": 1, \"a\": 2}").is_err());
    assert!(support::parse("{\"a\": 1.5}").is_err());
    assert!(support::parse("[01]").is_err());
    assert!(support::parse("\"tab\there\"").is_err());
    assert!(support::parse("{} x").is_err());
    let value = support::parse("{\"k\": [true, null, \"\\u00e9\\n\"]}").unwrap();
    assert_eq!(
        value.get("k"),
        Some(&Json::Array(vec![
            Json::Bool(true),
            Json::Null,
            Json::String("\u{e9}\n".to_owned())
        ]))
    );
}
