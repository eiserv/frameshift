//! Package model contracts through the public facade, on real directories
//! created at test time (`D-069`, `D-070`).

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, PathLimits, SourcePath, TextLimits,
};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::package::{
    DependencySpec, LoadError, LockfileKind, LockfileState, ManifestState, PackageDirectory,
    PackageManagerKind, Version, VersionRange, load_package,
};

mod support;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("package").join(name);
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

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn load(
    root: &Path,
    directory: Option<&str>,
    limits: Limits,
) -> (Snapshot, Result<PackageDirectory, LoadError>, FindingList) {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let snapshot = snapshot(root, limits.snapshot(), &mut findings).unwrap();
    let directory = directory.map(path);
    let loaded = load_package(
        &snapshot,
        directory.as_ref(),
        limits.package(),
        &mut findings,
    );
    let findings = findings.finish();
    (snapshot, loaded, findings)
}

fn codes(findings: &FindingList) -> Vec<&str> {
    findings
        .as_slice()
        .iter()
        .map(|finding| finding.code().as_str())
        .collect()
}

fn is_read(snapshot: &Snapshot, file: &str) -> bool {
    let id = snapshot.inventory().files().find(&path(file)).unwrap();
    snapshot.is_read(id)
}

#[test]
fn versions_and_ranges_through_the_facade() {
    let range = VersionRange::parse("^16.2.0 || 15.5.x").unwrap();
    assert_eq!(range.to_string(), ">=16.2.0 <17.0.0-0||>=15.5.0 <15.6.0-0");
    assert!(range.satisfies(&Version::parse("16.3.1").unwrap()));
    assert!(!range.satisfies(&Version::parse("17.0.0-canary.1").unwrap()));
    assert_eq!(range.min_version().unwrap().to_string(), "15.5.0");
    assert!(matches!(
        DependencySpec::parse("npm:next@canary"),
        DependencySpec::Alias { .. }
    ));
}

#[test]
fn reads_only_the_manifest_and_the_selected_lockfile() {
    let root = scratch("reads-only-needed");
    write(
        &root,
        "package.json",
        br#"{ "name": "fictional", "packageManager": "npm@11.0.0", "dependencies": { "next": "^16.0.0" } }"#,
    );
    write(
        &root,
        "package-lock.json",
        br#"{ "lockfileVersion": 3, "packages": { "": { "dependencies": { "next": "^16.0.0" } }, "node_modules/next": { "version": "16.0.2" } } }"#,
    );
    write(&root, "yarn.lock", b"# yarn lockfile v1\n");
    write(
        &root,
        "app/page.tsx",
        b"export default function Page() {}\n",
    );
    let (snapshot, loaded, findings) = load(&root, None, Limits::default());
    let loaded = loaded.unwrap();
    assert_eq!(codes(&findings), ["FS1023"]);
    assert_eq!(loaded.detection.manager, Some(PackageManagerKind::Npm));
    let LockfileState::Parsed(lockfile) = &loaded.lockfile else {
        panic!("parsed lockfile expected");
    };
    assert_eq!(
        lockfile
            .resolve("", "next")
            .unwrap()
            .version()
            .unwrap()
            .to_string(),
        "16.0.2"
    );
    assert!(is_read(&snapshot, "package.json"));
    assert!(is_read(&snapshot, "package-lock.json"));
    assert!(!is_read(&snapshot, "yarn.lock"));
    assert!(!is_read(&snapshot, "app/page.tsx"));
}

#[test]
fn ignored_and_absent_files_are_not_package_files() {
    let root = scratch("ignored");
    write(&root, ".gitignore", b"package-lock.json\n");
    write(&root, "package-lock.json", b"not even JSON");
    write(&root, "web/package.json", br#"{ "name": "web" }"#);
    let (_, loaded, findings) = load(&root, None, Limits::default());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.manifest, ManifestState::Absent);
    assert_eq!(loaded.detection.manager, None);
    assert_eq!(loaded.lockfile, LockfileState::None);
    assert!(findings.as_slice().is_empty());
    let (_, loaded, _) = load(&root, Some("web"), Limits::default());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.directory, Some(path("web")));
    let ManifestState::Analyzed(manifest) = &loaded.manifest else {
        panic!("manifest expected");
    };
    assert_eq!(manifest.name.as_ref().unwrap().value.as_ref(), "web");
}

#[test]
fn undecodable_and_oversized_manifests_are_not_analyzed() {
    let root = scratch("encoding");
    write(&root, "package.json", b"{ \"name\": \"caf\xE9\" }");
    let (_, loaded, findings) = load(&root, None, Limits::default());
    assert_eq!(loaded.unwrap().manifest, ManifestState::NotAnalyzed);
    assert_eq!(codes(&findings), ["FS2002"]);

    let root = scratch("oversized");
    write(
        &root,
        "package.json",
        br#"{ "name": "fictional-and-long" }"#,
    );
    let limits = Limits {
        text: TextLimits {
            max_source_bytes: 10,
        },
        ..Limits::default()
    };
    let (_, loaded, findings) = load(&root, None, limits);
    assert_eq!(loaded.unwrap().manifest, ManifestState::NotAnalyzed);
    assert_eq!(codes(&findings), ["FS2003"]);
}

#[test]
fn findings_never_echo_package_content() {
    let root = scratch("no-echo");
    let secret = "zz-secret-marker";
    let manifest = format!(
        r#"{{ "name": "{secret}", "name": 1, "type": "{secret}", "packageManager": "{secret}@1",
            "dependencies": {{ "{secret}": "{secret} {secret}" }} }}"#
    );
    write(&root, "package.json", manifest.as_bytes());
    write(
        &root,
        "bun.lock",
        format!(r#"{{ "lockfileVersion": 1, "workspaces": {{ "{secret}/../x": {{}} }} }}"#)
            .as_bytes(),
    );
    let (_, loaded, findings) = load(&root, None, Limits::default());
    let loaded = loaded.unwrap();
    assert_eq!(
        loaded.lockfile,
        LockfileState::NotParsed(LockfileKind::BunText)
    );
    assert_eq!(
        codes(&findings),
        ["FS1026", "FS1020", "FS1020", "FS1022", "FS1020", "FS1021"]
    );
    for finding in findings.as_slice() {
        for text in [
            finding.message(),
            finding.explanation(),
            finding.remediation(),
        ] {
            assert!(!text.contains(secret), "{text}");
        }
    }
}

#[test]
fn engine_limits_bridge_to_the_loader() {
    let limits = Limits::default().package();
    assert_eq!(limits.package.max_lockfile_bytes, 64 * 1024 * 1024);
    assert_eq!(limits.text, TextLimits::default());
    assert_eq!(
        limits.yaml,
        frameshift_engine::syntax::YamlLimits::default()
    );
}

#[test]
fn pnpm_loader_reads_only_the_selected_snapshot_files() {
    let root = scratch("pnpm-selected");
    write(&root, "package.json", br#"{"name":"fictional","packageManager":"pnpm@10.20.0","peerDependencies":{"fictional-host":"^1"}}"#);
    write(
        &root,
        "pnpm-lock.yaml",
        b"lockfileVersion: '9.0'\nimporters:\n  .: {}\n",
    );
    write(&root, "package-lock.json", b"not opened");
    write(&root, "source.ts", b"throw new Error('never executed');");
    let (snapshot, loaded, findings) = load(&root, None, Limits::default());
    assert!(matches!(loaded.unwrap().lockfile, LockfileState::Parsed(_)));
    assert_eq!(codes(&findings), ["FS1023"]);
    assert!(is_read(&snapshot, "package.json"));
    assert!(is_read(&snapshot, "pnpm-lock.yaml"));
    assert!(!is_read(&snapshot, "package-lock.json"));
    assert!(!is_read(&snapshot, "source.ts"));
    let mut limits = Limits::default();
    limits.yaml.max_nodes = 1;
    let (_, loaded, findings) = load(&root, None, limits);
    assert!(matches!(
        loaded.unwrap().lockfile,
        LockfileState::NotParsed(LockfileKind::Pnpm)
    ));
    assert!(codes(&findings).contains(&"FS2001"));
    let mut limits = Limits::default();
    limits.package.max_lockfile_bytes = 1;
    let (snapshot, _, findings) = load(&root, None, limits);
    assert!(codes(&findings).contains(&"FS1028"));
    assert!(!is_read(&snapshot, "pnpm-lock.yaml"));
}
