//! Package fixtures below `fixtures/discovery/` against their goldens
//! (`D-062`, `D-069`).
//!
//! Each case of kind `package` copies `source/` into Cargo's temporary
//! directory, applies its limit overrides, takes a snapshot, loads every
//! listed package directory, and compares a deterministic text rendering of
//! the package models, version resolutions, and findings with
//! `expected/package.txt`. The rendering is a test format, not a product
//! document; it is replaced by the `inspect` document once it exists.
//! Goldens change only through the ignored `update_package_goldens` test.

mod support;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, FindingLocation, PathLimits, Severity, SourcePath,
};
use frameshift_engine::input::snapshot;
use frameshift_engine::limits::Limits;
use frameshift_engine::package::{
    DependencyKind, DependencySpec, DetectionSource, IgnoredReason, LockedPackage, Lockfile,
    LockfileState, ManifestState, ModuleType, PackageDirectory, Resolution, load_package,
};
use support::Json;

fn fixtures() -> PathBuf {
    PathBuf::from("../../fixtures/discovery")
}

struct Case {
    name: String,
    directory: PathBuf,
    limits: Limits,
    directories: Vec<String>,
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
        if metadata.get("kind").and_then(Json::as_str) != Some("package") {
            continue;
        }
        for key in metadata.keys() {
            assert!(
                ["$schema", "kind", "description", "limits", "directories"].contains(&key),
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
                    "package.max-lockfile-bytes" => limits.package.max_lockfile_bytes = value,
                    "text.max-source-bytes" => limits.text.max_source_bytes = size(),
                    "json.max-depth" => limits.json.max_depth = size(),
                    "json.max-nodes" => limits.json.max_nodes = size(),
                    "yaml.max-depth" => limits.yaml.max_depth = size(),
                    "yaml.max-nodes" => limits.yaml.max_nodes = size(),
                    other => panic!("{name}: unknown limit {other}"),
                }
            }
        }
        let directories = match metadata.get("directories") {
            Some(Json::Array(items)) => items
                .iter()
                .map(|item| item.as_str().unwrap().to_owned())
                .collect(),
            None => vec![String::new()],
            Some(other) => panic!("{name}: directories {other:?}"),
        };
        cases.push(Case {
            name,
            directory,
            limits,
            directories,
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
        .join("package-fixtures")
        .join(&case.name);
    if let Ok(existing) = fs::canonicalize(&root) {
        fs::remove_dir_all(existing).unwrap();
    }
    copy_tree(&case.directory.join("source"), &root);
    root
}

fn spec(spec: &DependencySpec) -> String {
    match spec {
        DependencySpec::Range(range) => format!("range {range}"),
        DependencySpec::Tag(tag) => format!("tag {tag}"),
        DependencySpec::Alias { name, target } => format!("alias {name} {}", self::spec(target)),
        DependencySpec::Workspace(text) => format!("workspace {text}"),
        DependencySpec::Catalog(name) => format!("catalog {name}"),
        DependencySpec::Local => "local".to_owned(),
        DependencySpec::Git => "git".to_owned(),
        DependencySpec::Url => "url".to_owned(),
        DependencySpec::OtherProtocol => "other-protocol".to_owned(),
        DependencySpec::Invalid => "invalid".to_owned(),
    }
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

fn render_directory(out: &mut String, loaded: &PackageDirectory) {
    let directory = loaded
        .directory
        .as_ref()
        .map_or("(root)", SourcePath::as_str);
    writeln!(out, "directory {directory}").unwrap();
    let lockfile: Option<&Lockfile> = match &loaded.lockfile {
        LockfileState::Parsed(lockfile) => Some(lockfile),
        LockfileState::None | LockfileState::NotParsed(_) => None,
    };
    match &loaded.manifest {
        ManifestState::Absent => writeln!(out, "  manifest absent").unwrap(),
        ManifestState::NotAnalyzed => writeln!(out, "  manifest not analyzed").unwrap(),
        ManifestState::Analyzed(manifest) => {
            let text = |field: &Option<frameshift_engine::package::Field<Box<str>>>| {
                field
                    .as_ref()
                    .map_or("-".to_owned(), |field| field.value.to_string())
            };
            writeln!(
                out,
                "  manifest name={} version={} parsed-version={} private={} type={}",
                text(&manifest.name),
                text(&manifest.version),
                manifest
                    .parsed_version()
                    .map_or("-".to_owned(), |version| version.to_string()),
                manifest
                    .private
                    .as_ref()
                    .map_or("-".to_owned(), |field| field.value.to_string()),
                match manifest.module_type.as_ref().map(|field| field.value) {
                    None => "-",
                    Some(ModuleType::Module) => "module",
                    Some(ModuleType::CommonJs) => "commonjs",
                }
            )
            .unwrap();
            if let Some(workspaces) = &manifest.workspaces {
                let patterns: Vec<&str> = workspaces
                    .value
                    .iter()
                    .map(|element| &*element.value)
                    .collect();
                writeln!(out, "  workspaces {}", patterns.join(" ")).unwrap();
            }
            for kind in DependencyKind::ALL {
                for entry in manifest.dependencies_of(kind) {
                    let dependency = &entry.value;
                    let minimum = dependency
                        .spec
                        .range()
                        .and_then(|range| range.min_version())
                        .map_or("-".to_owned(), |version| version.to_string());
                    let resolved = lockfile.map_or("no lockfile".to_owned(), |lockfile| {
                        locked(lockfile.resolve("", &entry.key))
                    });
                    writeln!(
                        out,
                        "  {} {} {:?}: {}; min {minimum}; {resolved}",
                        kind.key(),
                        entry.key,
                        &*dependency.text,
                        spec(&dependency.spec)
                    )
                    .unwrap();
                }
            }
            let other: Vec<&str> = manifest.other.iter().map(|member| &*member.key).collect();
            if !other.is_empty() {
                writeln!(out, "  other {}", other.join(" ")).unwrap();
            }
        }
    }
    match &loaded.package_manager {
        None => writeln!(out, "  packageManager -").unwrap(),
        Some(spec) => writeln!(
            out,
            "  packageManager {} {}{}",
            spec.kind.name(),
            spec.version,
            spec.hash
                .as_ref()
                .map_or(String::new(), |(algorithm, _)| format!(" hash {algorithm}"))
        )
        .unwrap(),
    }
    let detection = &loaded.detection;
    writeln!(
        out,
        "  detected {} from {} lockfile {}",
        detection.manager.map_or("-", |manager| manager.name()),
        match detection.source {
            None => "-",
            Some(DetectionSource::Field) => "field",
            Some(DetectionSource::Lockfile) => "lockfile",
        },
        detection.lockfile.map_or("-", |kind| kind.file_name())
    )
    .unwrap();
    for ignored in &detection.ignored {
        writeln!(
            out,
            "  ignored {} {}",
            ignored.kind.file_name(),
            match ignored.reason {
                IgnoredReason::OtherManager => "other-manager",
                IgnoredReason::Superseded => "superseded",
                IgnoredReason::Ambiguous => "ambiguous",
            }
        )
        .unwrap();
    }
    match &loaded.lockfile {
        LockfileState::None => writeln!(out, "  lockfile none").unwrap(),
        LockfileState::NotParsed(kind) => {
            writeln!(out, "  lockfile not parsed {}", kind.file_name()).unwrap();
        }
        LockfileState::Parsed(lockfile) => {
            writeln!(
                out,
                "  lockfile {} version {}: {} workspaces, {} packages",
                lockfile.kind().file_name(),
                lockfile.version(),
                lockfile.workspaces().len(),
                lockfile.packages().len()
            )
            .unwrap();
            for workspace in lockfile
                .workspaces()
                .iter()
                .filter(|workspace| !workspace.path.is_empty())
            {
                for declared in workspace.declared(DependencyKind::Production) {
                    writeln!(
                        out,
                        "  workspace {} {} {:?}: {}",
                        workspace.path,
                        declared.name,
                        &*declared.spec,
                        locked(lockfile.resolve(&workspace.path, &declared.name))
                    )
                    .unwrap();
                }
            }
        }
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
    let mut out = String::new();
    for directory in &case.directories {
        let path = (!directory.is_empty())
            .then(|| SourcePath::new(directory, PathLimits::default()).unwrap());
        let loaded = load_package(
            &snapshot,
            path.as_ref(),
            case.limits.package(),
            &mut findings,
        )
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        render_directory(&mut out, &loaded);
    }
    snapshot.report_unreadable(&mut findings).unwrap();
    render_findings(&mut out, &findings.finish());
    out
}

fn golden(case: &Case) -> PathBuf {
    case.directory.join("expected").join("package.txt")
}

#[test]
fn package_fixtures_match_goldens() {
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
            "{}: run the ignored update_package_goldens test after reviewing the change",
            case.name
        );
    }
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-engine --offline --locked --test package_fixtures -- --ignored update_package_goldens"]
fn update_package_goldens() {
    for case in cases() {
        let root = prepare(&case);
        let path = golden(&case);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, produce(&case, &root)).unwrap();
    }
}
