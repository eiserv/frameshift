//! Workspace fixtures below `fixtures/discovery/` against their goldens
//! (`D-062`, `D-073`).
//!
//! Each case of kind `workspace` copies `source/` into Cargo's temporary
//! directory, applies its limit overrides, takes a snapshot, loads the
//! workspace at its root, and compares a deterministic text rendering of
//! the evaluated releases, the members, their resolutions through the root
//! lockfile, and the findings with `expected/workspace.txt`. The rendering
//! is a test format, not a product document. Goldens change only through
//! the ignored `update_workspace_goldens` test.

mod support;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, FindingLocation, Severity,
};
use frameshift_engine::input::snapshot;
use frameshift_engine::limits::Limits;
use frameshift_engine::package::{
    DeclarationFile, DependencyKind, LockedPackage, ManifestState, Resolution, SelectedPackage,
    Workspace, load_workspace,
};
use support::Json;

fn fixtures() -> PathBuf {
    PathBuf::from("../../fixtures/discovery")
}

struct Case {
    name: String,
    directory: PathBuf,
    limits: Limits,
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
        if metadata.get("kind").and_then(Json::as_str) != Some("workspace") {
            continue;
        }
        for key in metadata.keys() {
            assert!(
                ["$schema", "kind", "description", "limits"].contains(&key),
                "{name}: unknown key {key}"
            );
        }
        assert!(
            !metadata
                .get("description")
                .and_then(Json::as_str)
                .unwrap()
                .is_empty()
        );
        let mut limits = Limits::default();
        if let Some(overrides) = metadata.get("limits") {
            for key in overrides.keys() {
                let value = overrides.get(key).and_then(Json::as_u64).unwrap();
                let size = || usize::try_from(value).unwrap();
                match key {
                    "workspace.max-patterns" => limits.workspace.max_patterns = size(),
                    "workspace.max-pattern-bytes" => limits.workspace.max_pattern_bytes = size(),
                    "workspace.max-alternatives" => limits.workspace.max_alternatives = size(),
                    "workspace.max-match-work" => limits.workspace.max_match_work = value,
                    other => panic!("{name}: unknown limit {other}"),
                }
            }
        }
        cases.push(Case {
            name,
            directory,
            limits,
        });
    }
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
        .join("workspace-fixtures")
        .join(&case.name);
    if let Ok(existing) = fs::canonicalize(&root) {
        fs::remove_dir_all(existing).unwrap();
    }
    copy_tree(&case.directory.join("source"), &root);
    root
}

fn locked(package: Option<&LockedPackage>) -> String {
    match package {
        None => "unresolved".to_owned(),
        Some(package) => match &package.resolution {
            Resolution::Registry(version) => format!("{}@{version}", package.name),
            Resolution::Link(target) => format!("link {target}"),
            Resolution::Workspace(path) => format!("workspace {path}"),
            Resolution::Other(text) => format!("{} other {text}", package.name),
        },
    }
}

fn render_package(out: &mut String, label: &str, selected: &SelectedPackage<'_>) {
    writeln!(
        out,
        "package {label} lockfile-workspace {:?}",
        selected.lockfile_workspace()
    )
    .unwrap();
    let ManifestState::Analyzed(manifest) = selected.manifest() else {
        writeln!(out, "  manifest not analyzed").unwrap();
        return;
    };
    for kind in DependencyKind::ALL {
        for entry in manifest.dependencies_of(kind) {
            let sibling = match selected.sibling(&entry.key) {
                None => String::new(),
                Some(None) => "; sibling (root)".to_owned(),
                Some(Some(directory)) => format!("; sibling {}", directory.as_str()),
            };
            writeln!(
                out,
                "  {} {} {:?}: {}{sibling}",
                kind.key(),
                entry.key,
                &*entry.value.text,
                if selected.lockfile().is_some() {
                    locked(selected.resolve_declared(&entry.key, &entry.value.text))
                } else {
                    "no lockfile".to_owned()
                }
            )
            .unwrap();
        }
    }
}

fn render_workspace(out: &mut String, workspace: &Workspace) {
    let releases: Vec<&str> = workspace
        .semantics
        .iter()
        .map(|release| release.as_str())
        .collect();
    writeln!(out, "releases {}", releases.join(" ")).unwrap();
    let declarations: Vec<&str> = workspace
        .declarations
        .iter()
        .map(|declaration| match declaration {
            DeclarationFile::PackageJson => "package.json",
            DeclarationFile::PnpmWorkspace => "pnpm-workspace.yaml",
        })
        .collect();
    writeln!(out, "declarations {}", declarations.join(" ")).unwrap();
    writeln!(out, "complete {}", workspace.complete).unwrap();
    for member in &workspace.members {
        writeln!(
            out,
            "member {} name={}",
            member.directory.as_str(),
            member.name().unwrap_or("-")
        )
        .unwrap();
    }
    for directory in &workspace.undetermined {
        writeln!(out, "undetermined {}", directory.as_str()).unwrap();
    }
    render_package(out, "(root)", &workspace.select(None).unwrap());
    for member in &workspace.members {
        let selected = workspace.select(Some(&member.directory)).unwrap();
        render_package(out, member.directory.as_str(), &selected);
    }
}

fn render_findings(out: &mut String, findings: &FindingList) {
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
            FindingLocation::File(path) => path.as_str().to_owned(),
            FindingLocation::Text(text) => format!(
                "{}:{}:{}",
                text.file().as_str(),
                text.start().line,
                text.start().column
            ),
        };
        writeln!(
            out,
            "  {id} {severity} {} {location}: {}",
            finding.code().as_str(),
            finding.message()
        )
        .unwrap();
        writeln!(out, "    {}", finding.explanation()).unwrap();
    }
}

fn produce(case: &Case, root: &Path) -> String {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(root, case.limits.snapshot(), &mut findings).unwrap();
    let workspace = load_workspace(&snapshot, case.limits.package(), &mut findings)
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    let mut out = String::new();
    render_workspace(&mut out, &workspace);
    snapshot.report_unreadable(&mut findings).unwrap();
    render_findings(&mut out, &findings.finish());
    out
}

fn golden(case: &Case) -> PathBuf {
    case.directory.join("expected").join("workspace.txt")
}

#[test]
fn workspace_fixtures_match_goldens() {
    let cases = cases();
    assert!(cases.len() >= 6);
    for case in cases {
        let root = prepare(&case);
        let actual = produce(&case, &root);
        assert_eq!(
            actual,
            produce(&case, &root),
            "{}: not repeatable",
            case.name
        );
        let expected =
            fs::read(golden(&case)).unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(
            actual,
            String::from_utf8(expected).unwrap(),
            "{}: run the ignored update_workspace_goldens test after reviewing the change",
            case.name
        );
    }
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-engine --offline --locked --test workspace_fixtures -- --ignored update_workspace_goldens"]
fn update_workspace_goldens() {
    for case in cases() {
        let root = prepare(&case);
        let path = golden(&case);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, produce(&case, &root)).unwrap();
    }
}
