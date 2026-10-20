//! Inspect fixtures below `fixtures/discovery/` against their documents
//! (`D-062`, `D-075`).
//!
//! Each case of kind `inspect` copies `source/` into Cargo's temporary
//! directory, creates the files and directories that `fixture.json` lists
//! under `createdAtTestTime` (manifests naming the real `next` package
//! exist only there, `Q-014`), inspects the root and every member twice, and
//! compares the `frameshift.inspection` document with
//! `expected/inspection.json`, in which `@ENGINE_VERSION@` stands for the
//! engine version. The CLI's tests compare its text with
//! `expected/inspection.txt`. Goldens change only through the ignored
//! `update_inspect_goldens` test.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::inspect::{InspectOptions, inspect};
use support::Json;

fn fixtures() -> PathBuf {
    PathBuf::from("../../fixtures/discovery")
}

enum Created {
    Directory(String),
    File(String, String),
}

struct Case {
    name: String,
    directory: PathBuf,
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
    let mut cases = Vec::new();
    for name in names {
        let directory = fixtures().join(&name);
        let text = fs::read_to_string(directory.join("fixture.json")).unwrap();
        let metadata = support::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        if metadata.get("kind").and_then(Json::as_str) != Some("inspect") {
            continue;
        }
        for key in metadata.keys() {
            assert!(
                ["$schema", "kind", "description", "createdAtTestTime"].contains(&key),
                "{name}: unknown key {key}"
            );
        }
        assert!(!string(&metadata, "description").is_empty());
        let mut created = Vec::new();
        if let Some(Json::Array(items)) = metadata.get("createdAtTestTime") {
            for item in items {
                let path = string(item, "path");
                created.push(match item.get("kind").and_then(Json::as_str) {
                    Some("directory") => Created::Directory(path),
                    Some("file") => Created::File(path, string(item, "content")),
                    other => panic!("{name}: input kind {other:?} is not used by inspect cases"),
                });
            }
        }
        cases.push(Case {
            name,
            directory,
            created,
        });
    }
    assert!(!cases.is_empty());
    cases
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

fn prepare(case: &Case) -> PathBuf {
    let root = support::temporary()
        .join("inspect-fixtures")
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
        }
    }
    root
}

/// The document with the engine version replaced by its placeholder, so
/// that version bumps leave the goldens unchanged.
fn produce(case: &Case) -> String {
    let root = prepare(case);
    let first = inspect(&root, &InspectOptions::default())
        .unwrap_or_else(|error| panic!("{}: {error}", case.name))
        .document();
    let second = inspect(&root, &InspectOptions::default())
        .unwrap()
        .document();
    assert_eq!(first, second, "{}: repeated inspection differs", case.name);
    let version = frameshift_engine::version::info().engine_version;
    let line = format!("\n  \"engineVersion\": \"{version}\",\n");
    assert!(first.contains(&line), "{}", case.name);
    first.replacen(&line, "\n  \"engineVersion\": \"@ENGINE_VERSION@\",\n", 1)
}

fn golden(case: &Case) -> PathBuf {
    case.directory.join("expected").join("inspection.json")
}

#[test]
fn inspect_fixtures_match_goldens() {
    for case in cases() {
        let expected = fs::read_to_string(golden(&case))
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(produce(&case), expected, "{}", case.name);
    }
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-engine --test inspect_fixtures -- --ignored update_inspect_goldens"]
fn update_inspect_goldens() {
    for case in cases() {
        let text = produce(&case);
        fs::create_dir_all(golden(&case).parent().unwrap()).unwrap();
        fs::write(golden(&case), text).unwrap();
    }
}
