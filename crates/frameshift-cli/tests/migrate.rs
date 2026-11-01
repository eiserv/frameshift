//! `frameshift migrate` v0 on the fixtures below `fixtures/migrate/`
//! (`D-101`): dry-run text against `expected/dry-run.txt`, written trees
//! against `expected/output/`, exit statuses, and output safety. The text
//! goldens change only through the ignored `update_migrate_text_goldens` test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/migrate")
}

fn cases() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let key: Vec<_> = path
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_str().unwrap().to_owned())
                    .collect();
                files.insert(key.join("/"), fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

// The package.json of each case names the real next package and exists only
// at test time (Q-014); it is read from the case's fixture.json.
fn manifest(case: &str) -> String {
    let text = fs::read_to_string(fixtures().join(case).join("fixture.json")).unwrap();
    let start = text.find("\"content\": \"").unwrap() + "\"content\": \"".len();
    let end = start + text[start..].find("\"\n").unwrap();
    text[start..end].replace("\\n", "\n").replace("\\\"", "\"")
}

fn workspace(case: &str, label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "frameshift-cli-migrate-{case}-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    copy_tree(&fixtures().join(case).join("source"), &root.join("source"));
    fs::write(root.join("source/package.json"), manifest(case)).unwrap();
    root
}

fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

const MIGRATE: &[&str] = &["migrate", "source", "--to", "astro-preact", "--out", "out"];

fn dry_run(case: &str, label: &str) -> Vec<u8> {
    let root = workspace(case, label);
    let mut args = MIGRATE.to_vec();
    args.push("--dry-run");
    let output = call(&root, &args);
    assert_eq!(output.status.code(), Some(0), "{case}");
    assert!(output.stderr.is_empty(), "{case}");
    assert!(!root.join("out").exists(), "{case}: a dry run wrote output");
    fs::remove_dir_all(root).unwrap();
    output.stdout
}

#[test]
fn dry_runs_list_the_planned_files() {
    for case in cases() {
        let golden = fs::read(fixtures().join(&case).join("expected/dry-run.txt")).unwrap();
        assert_eq!(dry_run(&case, "dry"), golden, "{case}");
    }
}

#[test]
fn migration_writes_the_golden_tree_and_refuses_unsafe_outputs() {
    for case in cases() {
        let root = workspace(&case, "write");
        let before = read_tree(&root.join("source"));
        let output = call(&root, MIGRATE);
        assert_eq!(output.status.code(), Some(0), "{case}");
        assert!(output.stdout.starts_with(b"Migrated: "));
        let golden = read_tree(&fixtures().join(&case).join("expected/output"));
        assert_eq!(read_tree(&root.join("out")), golden, "{case}");
        // An existing output and an output inside the source are refused.
        for out in ["out", "source/out"] {
            let args = ["migrate", "source", "--to", "astro-preact", "--out", out];
            let output = call(&root, &args);
            assert_eq!(output.status.code(), Some(4), "{case}: {out}");
            assert!(output.stdout.is_empty());
        }
        assert_eq!(
            read_tree(&root.join("source")),
            before,
            "{case}: source changed"
        );
        let missing = call(
            &root,
            &[
                "migrate",
                "missing",
                "--to",
                "astro-preact",
                "--out",
                "other",
            ],
        );
        assert_eq!(missing.status.code(), Some(3));
        assert!(!root.join("other").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

const PLAN: &[&str] = &["plan", "source", "--to", "astro-preact"];

#[test]
fn plan_documents_approve_exactly_the_run_they_describe() {
    for case in cases() {
        let root = workspace(&case, "approve");
        let before = read_tree(&root.join("source"));
        let mut args = PLAN.to_vec();
        args.extend(["--out", "plan.json"]);
        let written = call(&root, &args);
        assert_eq!(written.status.code(), Some(0), "{case}");
        assert!(
            String::from_utf8_lossy(&written.stdout).contains("Plan document written."),
            "{case}"
        );
        // `--format json` prints the document that `--out` wrote.
        let mut json = PLAN.to_vec();
        json.extend(["--format", "json"]);
        let printed = call(&root, &json);
        assert_eq!(printed.status.code(), Some(0), "{case}");
        assert_eq!(printed.stdout, fs::read(root.join("plan.json")).unwrap());
        // An existing document or one inside the source is refused.
        assert_eq!(call(&root, &args).status.code(), Some(4), "{case}");
        let mut inside = PLAN.to_vec();
        inside.extend(["--out", "source/plan.json"]);
        assert_eq!(call(&root, &inside).status.code(), Some(4), "{case}");
        assert_eq!(read_tree(&root.join("source")), before, "{case}");

        let approved = |out: &'static str, plan: &'static str| {
            let mut args = MIGRATE[..5].to_vec();
            args.extend([out, "--plan", plan]);
            call(&root, &args)
        };
        // A changed plan document, a stale plan, and a missing one exit 3
        // and write nothing.
        let mut tampered = fs::read(root.join("plan.json")).unwrap();
        *tampered.last_mut().unwrap() = b' ';
        fs::write(root.join("tampered.json"), tampered).unwrap();
        for plan in ["tampered.json", "missing.json"] {
            let refused = approved("refused", plan);
            assert_eq!(refused.status.code(), Some(3), "{case}: {plan}");
            assert!(!root.join("refused").exists(), "{case}: {plan}");
        }
        let verified = approved("verified", "plan.json");
        assert_eq!(verified.status.code(), Some(0), "{case}");
        assert!(String::from_utf8_lossy(&verified.stdout).starts_with("Plan "));
        let golden = read_tree(&fixtures().join(&case).join("expected/output"));
        assert_eq!(read_tree(&root.join("verified")), golden, "{case}");

        fs::write(root.join("source/added.txt"), "changed").unwrap();
        let stale = approved("stale", "plan.json");
        assert_eq!(stale.status.code(), Some(3), "{case}");
        assert!(!root.join("stale").exists(), "{case}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn plan_text_lists_operations_files_and_tasks() {
    let case = "static-site";
    let root = workspace(case, "show");
    let run = |show: &'static str| {
        let mut args = PLAN.to_vec();
        args.extend(["--show", show]);
        let output = call(&root, &args);
        assert_eq!(output.status.code(), Some(0), "{show}");
        String::from_utf8(output.stdout).unwrap()
    };
    let overview = String::from_utf8(call(&root, PLAN).stdout).unwrap();
    assert!(overview.starts_with("Frameshift "), "{overview}");
    assert!(
        overview.contains("more (use --show operations)"),
        "{overview}"
    );
    assert!(run("operations").contains("layout.chain  from app/layout.tsx"));
    assert!(run("files").contains("\nsrc/pages/index.astro\n"));
    assert!(run("tasks").contains("preserved  app/contact/page.tsx: "));
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "rewrites the migrate dry-run text goldens"]
fn update_migrate_text_goldens() {
    for case in cases() {
        let text = dry_run(&case, "update");
        fs::write(fixtures().join(&case).join("expected/dry-run.txt"), text).unwrap();
    }
}
