//! The `inspect` pipeline and its document through the public facade
//! (`D-075`), on real directories created at test time. Manifests naming
//! the real `next` package exist only during the test (`Q-014`).

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{PathLimits, SourcePath, SourceText, TextLimits};
use frameshift_engine::input::{DiscoveryError, Stop};
use frameshift_engine::inspect::{
    InspectError, InspectOptions, Inspection, ManifestStatus, PackageScope, inspect,
};
use frameshift_engine::limits::Limits;
use frameshift_engine::nextjs::{Router, Verdict, VersionEvidence};
use frameshift_engine::package::{SelectionError, Version};
use frameshift_engine::syntax::{JsonDialect, JsonLimits, JsonValue, parse_json};

mod support;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("inspect").join(name);
    if let Ok(existing) = fs::canonicalize(&directory) {
        fs::remove_dir_all(existing).unwrap();
    }
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn write(root: &Path, path: &str, content: &str) {
    let native = root.join(path);
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(native, content).unwrap();
}

fn path(text: &str) -> SourcePath {
    SourcePath::new(text, PathLimits::default()).unwrap()
}

fn options(packages: PackageScope) -> InspectOptions {
    InspectOptions {
        limits: Limits::default(),
        packages,
    }
}

fn codes(inspection: &Inspection) -> Vec<&str> {
    let mut codes: Vec<&str> = inspection
        .findings
        .as_slice()
        .iter()
        .map(|finding| finding.code().as_str())
        .collect();
    codes.sort_unstable();
    codes
}

/// Parses the document with the product JSON parser and returns it as a
/// test value, checking that it is valid, duplicate-free JSON.
fn document(inspection: &Inspection) -> support::Json {
    let text = inspection.document();
    assert!(text.ends_with("}\n") && !text.contains('\r'));
    let source = SourceText::new(text.clone(), TextLimits::default()).unwrap();
    let parsed = parse_json(&source, JsonDialect::Json, JsonLimits::default()).unwrap();
    assert_eq!(parsed.duplicate_keys().count(), 0);
    assert!(matches!(
        parsed.value(parsed.root()),
        Some(JsonValue::Object(_))
    ));
    support::parse(&text).unwrap()
}

fn get<'a>(value: &'a support::Json, keys: &[&str]) -> &'a support::Json {
    keys.iter().fold(value, |value, key| {
        value
            .get(key)
            .unwrap_or_else(|| panic!("missing member {key}"))
    })
}

fn text<'a>(value: &'a support::Json, keys: &[&str]) -> &'a str {
    get(value, keys).as_str().unwrap()
}

const PNPM_LOCK: &str = "lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      next:
        specifier: ^16.2.0
        version: 16.2.1
packages:
  next@16.2.1:
    resolution: {integrity: sha512-fictional}
snapshots:
  next@16.2.1: {}
";

#[test]
fn next_project_is_identified_with_its_files_and_lockfile() {
    let root = scratch("app");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "private": true, "packageManager": "pnpm@10.34.7",
             "dependencies": { "next": "^16.2.0" } }"#,
    );
    write(&root, "pnpm-lock.yaml", PNPM_LOCK);
    write(&root, "package-lock.json", "{}");
    write(&root, "next.config.mjs", "export default {};\n");
    write(&root, "src/app/layout.tsx", "");
    write(&root, "src/app/page.tsx", "");
    write(&root, "src/app/blog/page.tsx", "");
    write(&root, ".env.local", "SECRET=1\n");
    write(&root, ".env.example", "SECRET=\n");
    write(&root, "node_modules/next/package.json", "{}");
    write(&root, ".next/cache.txt", "");

    let inspection = inspect(&root, &InspectOptions::default()).unwrap();
    assert_eq!(codes(&inspection), ["FS1023"], "{:?}", inspection.findings);
    let files = inspection.files;
    assert!(files.complete);
    assert_eq!(files.stop, None);
    assert_eq!(files.admitted, 8);
    assert_eq!(files.directories, 3);
    assert_eq!(files.ignored, [0, 1, 1, 1, 0]);
    assert_eq!(files.ignored_total(), 3);
    assert_eq!(files.other_total(), 0);
    assert!(!inspection.workspace.is_workspace());
    assert_eq!(inspection.packages.len(), 1);
    let package = &inspection.packages[0];
    assert_eq!(package.directory, None);
    assert_eq!(package.name.as_deref(), Some("fictional-site"));
    assert_eq!(package.manifest, ManifestStatus::Analyzed);
    assert_eq!(package.nextjs.verdict, Verdict::NextJs);
    assert_eq!(
        package.nextjs.version,
        VersionEvidence::Locked(Version::parse("16.2.1").unwrap())
    );
    assert_eq!(package.nextjs.router(), Router::App);

    let json = document(&inspection);
    assert_eq!(text(&json, &["kind"]), "frameshift.inspection");
    assert_eq!(get(&json, &["schemaVersion"]).as_u64(), Some(1));
    assert_eq!(
        text(&json, &["engineVersion"]),
        frameshift_engine::version::info().engine_version
    );
    assert_eq!(get(&json, &["files", "admitted"]).as_u64(), Some(8));
    assert_eq!(
        get(&json, &["files", "ignored", "environment"]).as_u64(),
        Some(1)
    );
    assert_eq!(text(&json, &["packageManager", "name"]), "pnpm");
    assert_eq!(
        text(&json, &["packageManager", "source"]),
        "package-manager-field"
    );
    assert_eq!(
        text(&json, &["packageManager", "declared", "version"]),
        "10.34.7"
    );
    assert_eq!(
        text(&json, &["packageManager", "lockfile", "path"]),
        "pnpm-lock.yaml"
    );
    assert_eq!(
        get(&json, &["packageManager", "lockfile", "formatVersion"]).as_u64(),
        Some(9)
    );
    let support::Json::Array(ignored) = get(&json, &["packageManager", "ignoredLockfiles"]) else {
        panic!("ignoredLockfiles is not an array");
    };
    assert_eq!(text(&ignored[0], &["path"]), "package-lock.json");
    assert_eq!(text(&ignored[0], &["reason"]), "other-manager");
    let support::Json::Array(packages) = get(&json, &["packages"]) else {
        panic!("packages is not an array");
    };
    let nextjs = get(&packages[0], &["nextjs"]);
    assert_eq!(text(&packages[0], &["directory"]), ".");
    assert_eq!(text(nextjs, &["verdict"]), "nextjs");
    assert_eq!(text(nextjs, &["version", "value"]), "16.2.1");
    assert_eq!(text(nextjs, &["version", "evidence"]), "locked");
    assert_eq!(text(nextjs, &["config", "path"]), "next.config.mjs");
    assert_eq!(text(nextjs, &["router"]), "app");
    assert_eq!(text(nextjs, &["app", "path"]), "src/app");
    assert_eq!(get(nextjs, &["app", "routeFiles"]).as_u64(), Some(2));
    assert_eq!(get(nextjs, &["pages"]), &support::Json::Null);
    let support::Json::Array(findings) = get(&json, &["findings"]) else {
        panic!("findings is not an array");
    };
    assert_eq!(findings.len(), 1);
    assert_eq!(text(&findings[0], &["id"]), "F0001");
    assert_eq!(text(&findings[0], &["code"]), "FS1023");
    assert_eq!(text(&findings[0], &["severity"]), "review");
    assert_eq!(text(&findings[0], &["location", "scope"]), "file");

    // Inspections of an unchanged tree render identical documents.
    let again = inspect(&root, &InspectOptions::default()).unwrap();
    assert_eq!(again.document(), inspection.document());
}

#[test]
fn workspace_scopes_select_the_root_and_members() {
    let root = scratch("workspace");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "private": true, "workspaces": ["apps/*", "packages/*"] }"#,
    );
    write(
        &root,
        "apps/web/package.json",
        r#"{ "name": "fictional-web", "version": "1.0.0", "dependencies": { "next": "^15.5.0" } }"#,
    );
    write(&root, "apps/web/pages/index.jsx", "");
    write(
        &root,
        "packages/ui/package.json",
        "{ \"name\": \"fictional\\tui\", \"version\": \"1.0.0\" }",
    );

    let all = inspect(&root, &options(PackageScope::All)).unwrap();
    assert!(all.workspace.is_workspace());
    let directories: Vec<Option<&str>> = all
        .packages
        .iter()
        .map(|package| package.directory.as_ref().map(SourcePath::as_str))
        .collect();
    assert_eq!(directories, [None, Some("apps/web"), Some("packages/ui")]);
    assert_eq!(all.packages[0].nextjs.verdict, Verdict::Absent);
    assert_eq!(all.packages[1].nextjs.verdict, Verdict::NextJs);
    assert_eq!(
        all.packages[1].nextjs.version,
        VersionEvidence::LowerBound(Version::parse("15.5.0").unwrap())
    );
    assert_eq!(all.packages[2].name.as_deref(), Some("fictional\tui"));
    // Without a package manager, the five modeled releases that read
    // `workspaces` are evaluated and agree; Yarn 1 needs member versions.
    assert_eq!(all.workspace.semantics.len(), 5);
    assert!(codes(&all).contains(&"FS1042"));

    let json = document(&all);
    assert_eq!(
        get(&json, &["workspace", "isWorkspace"]),
        &support::Json::Bool(true)
    );
    let support::Json::Array(members) = get(&json, &["workspace", "members"]) else {
        panic!("members is not an array");
    };
    assert_eq!(members.len(), 2);
    let support::Json::Array(packages) = get(&json, &["packages"]) else {
        panic!("packages is not an array");
    };
    assert_eq!(text(&packages[2], &["name"]), "fictional\tui");
    assert!(all.document().contains("\"fictional\\tui\""));

    let root_only = inspect(&root, &options(PackageScope::Root)).unwrap();
    assert_eq!(root_only.packages.len(), 1);
    assert_eq!(root_only.packages[0].directory, None);
    assert!(!codes(&root_only).contains(&"FS1042"));

    let member = inspect(&root, &options(PackageScope::Member(path("apps/web")))).unwrap();
    assert_eq!(member.packages.len(), 1);
    assert_eq!(member.packages[0].directory, Some(path("apps/web")));
    assert_eq!(member.packages[0].nextjs.router(), Router::Pages);

    let error = inspect(&root, &options(PackageScope::Member(path("apps")))).unwrap_err();
    assert_eq!(error, InspectError::Selection(SelectionError::NotAMember));
    assert!(!error.is_internal());
}

#[test]
fn a_project_without_a_framework_is_a_result() {
    let root = scratch("plain");
    write(&root, "index.html", "<p>fictional</p>\n");
    let inspection = inspect(&root, &InspectOptions::default()).unwrap();
    assert_eq!(codes(&inspection), Vec::<&str>::new());
    assert_eq!(inspection.packages.len(), 1);
    assert_eq!(inspection.packages[0].manifest, ManifestStatus::Absent);
    assert_eq!(inspection.packages[0].nextjs.verdict, Verdict::Absent);
    let json = document(&inspection);
    assert_eq!(
        get(&json, &["packageManager", "name"]),
        &support::Json::Null
    );
    assert_eq!(
        get(&json, &["packageManager", "lockfile"]),
        &support::Json::Null
    );
    assert_eq!(
        get(&json, &["migrationPaths"]),
        &support::Json::Array(Vec::new())
    );
}

#[test]
fn limits_make_the_inventory_incomplete_without_failing() {
    let root = scratch("limits");
    for index in 0..4 {
        write(&root, &format!("content/{index}.md"), "fictional\n");
    }
    let mut limits = Limits::default();
    limits.run.max_files = 2;
    let inspection = inspect(
        &root,
        &InspectOptions {
            limits,
            packages: PackageScope::All,
        },
    )
    .unwrap();
    assert!(!inspection.files.complete);
    assert_eq!(inspection.files.stop, Some(Stop::Files));
    assert_eq!(inspection.files.admitted, 2);
    assert_eq!(codes(&inspection), ["FS1011"]);
    let json = document(&inspection);
    assert_eq!(text(&json, &["files", "stop"]), "run.max-files");
    assert_eq!(
        get(&json, &["files", "complete"]),
        &support::Json::Bool(false)
    );
}

#[test]
fn input_failures_and_invalid_limits_are_classified() {
    let base = scratch("failures");
    let missing = inspect(&base.join("missing"), &InspectOptions::default()).unwrap_err();
    assert!(matches!(
        missing,
        InspectError::Discovery(DiscoveryError::Root(_))
    ));
    assert!(!missing.is_internal());

    write(&base, "file.txt", "");
    let file = inspect(&base.join("file.txt"), &InspectOptions::default()).unwrap_err();
    assert!(!file.is_internal());

    let mut limits = Limits::default();
    limits.run.max_files = limits.run.max_entries + 1;
    let invalid = inspect(
        &base,
        &InspectOptions {
            limits,
            packages: PackageScope::All,
        },
    )
    .unwrap_err();
    assert!(matches!(invalid, InspectError::Limits(_)));
    assert!(invalid.is_internal());
}
