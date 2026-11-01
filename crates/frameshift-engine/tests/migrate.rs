//! Migrate v0 fixtures below `fixtures/migrate/` (`D-062`, `D-101`).
//!
//! Each case copies `source/` into Cargo's temporary directory, creates the
//! files that `fixture.json` lists under `createdAtTestTime` (manifests
//! naming the real `next` package exist only there, `Q-014`), plans twice,
//! and compares the plan listing with `expected/plan.txt` and every output
//! file with `expected/output/`. Execution then writes the output next to
//! the source, and the test checks that the written tree equals the plan,
//! that the source is byte-identical afterwards, and that nothing else was
//! created. Goldens change only through the ignored `update_migrate_goldens`
//! test.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::migrate::{self, MigrateErrorKind, MigrateOptions, Plan};
use support::Json;

fn fixtures() -> PathBuf {
    PathBuf::from("../../fixtures/migrate")
}

struct Case {
    name: String,
    directory: PathBuf,
    created: Vec<(String, String)>,
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
        assert_eq!(metadata.get("kind").and_then(Json::as_str), Some("migrate"));
        for key in metadata.keys() {
            assert!(
                ["$schema", "kind", "description", "createdAtTestTime"].contains(&key),
                "{name}: unknown key {key}"
            );
        }
        let mut created = Vec::new();
        if let Some(Json::Array(items)) = metadata.get("createdAtTestTime") {
            for item in items {
                assert_eq!(item.get("kind").and_then(Json::as_str), Some("file"));
                created.push((
                    item.get("path").and_then(Json::as_str).unwrap().to_owned(),
                    item.get("content")
                        .and_then(Json::as_str)
                        .unwrap()
                        .to_owned(),
                ));
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

/// Every file below `root` with its bytes, keyed by slash-separated path.
fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                let key: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap().to_owned())
                    .collect();
                files.insert(key.join("/"), fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut files);
    }
    files
}

/// A fresh temporary workspace with the case's source in `source/`.
fn prepare(case: &Case, label: &str) -> PathBuf {
    let workspace = support::temporary()
        .join("migrate-fixtures")
        .join(format!("{}-{label}", case.name));
    if let Ok(existing) = fs::canonicalize(&workspace) {
        fs::remove_dir_all(existing).unwrap();
    }
    let source = workspace.join("source");
    copy_tree(&case.directory.join("source"), &source);
    for (path, content) in &case.created {
        let native = source.join(path);
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(native, content).unwrap();
    }
    workspace
}

fn listing(plan: &Plan) -> String {
    let mut text = String::new();
    for operation in &plan.operations {
        text.push_str(&format!(
            "{:<10} {} {} {}{}\n",
            operation.kind.as_str(),
            operation.output.as_str(),
            operation.rule,
            operation.source.as_ref().map_or("-", |s| s.as_str()),
            if operation.review { " review" } else { "" },
        ));
    }
    text.push('\n');
    for task in &plan.tasks {
        text.push_str(&format!(
            "task {:?} {}: {}\n",
            task.kind,
            task.source.as_ref().map_or("-", |s| s.as_str()),
            task.text
        ));
    }
    text
}

fn outputs(plan: &Plan) -> BTreeMap<String, Vec<u8>> {
    plan.operations
        .iter()
        .map(|o| (o.output.as_str().to_owned(), o.bytes().to_vec()))
        .collect()
}

fn run(case: &Case, label: &str) -> (String, BTreeMap<String, Vec<u8>>) {
    let workspace = prepare(case, label);
    let source = workspace.join("source");
    let first = migrate::plan(&source, &MigrateOptions::default())
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    let second = migrate::plan(&source, &MigrateOptions::default()).unwrap();
    assert_eq!(first, second, "{}: repeated plans differ", case.name);
    fs::remove_dir_all(workspace).unwrap();
    (listing(&first), outputs(&first))
}

#[test]
fn migrate_fixtures_match_their_goldens() {
    for case in cases() {
        let (listing, files) = run(&case, "golden");
        let expected = case.directory.join("expected");
        assert_eq!(
            fs::read_to_string(expected.join("plan.txt")).unwrap(),
            listing,
            "{}: plan listing",
            case.name
        );
        let golden = read_tree(&expected.join("output"));
        assert_eq!(
            golden.keys().collect::<Vec<_>>(),
            files.keys().collect::<Vec<_>>(),
            "{}: output paths",
            case.name
        );
        for (path, bytes) in &files {
            assert!(
                golden.get(path) == Some(bytes),
                "{}: {path} differs from its golden",
                case.name
            );
        }
    }
}

#[test]
fn execution_writes_exactly_the_plan_and_never_touches_the_source() {
    for case in cases() {
        let workspace = prepare(&case, "execute");
        let source = workspace.join("source");
        let before = read_tree(&source);
        let plan = migrate::plan(&source, &MigrateOptions::default()).unwrap();
        let out = workspace.join("out");
        let destination = migrate::check_output(&source, &out).unwrap();
        migrate::execute(&plan, &destination).unwrap();
        assert_eq!(read_tree(&out), outputs(&plan), "{}", case.name);
        assert_eq!(read_tree(&source), before, "{}: source changed", case.name);
        let mut entries: Vec<_> = fs::read_dir(&workspace)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        assert_eq!(entries, ["out", "source"], "{}: stray entries", case.name);
        // A second run refuses the existing output and writes nothing.
        let error = migrate::check_output(&source, &out).unwrap_err();
        assert_eq!(error.kind(), MigrateErrorKind::Output);
        let error = migrate::check_output(&source, &source.join("out")).unwrap_err();
        assert_eq!(error.kind(), MigrateErrorKind::Output);
        assert_eq!(read_tree(&source), before);
        fs::remove_dir_all(workspace).unwrap();
    }
}

#[test]
#[ignore = "rewrites the migrate fixture goldens"]
fn update_migrate_goldens() {
    for case in cases() {
        let (listing, files) = run(&case, "update");
        let expected = case.directory.join("expected");
        if expected.exists() {
            fs::remove_dir_all(&expected).unwrap();
        }
        fs::create_dir_all(&expected).unwrap();
        fs::write(expected.join("plan.txt"), listing).unwrap();
        for (path, bytes) in files {
            let native = expected.join("output").join(&path);
            fs::create_dir_all(native.parent().unwrap()).unwrap();
            fs::write(native, bytes).unwrap();
        }
    }
}
