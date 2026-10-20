//! Workspace detection through the public facade, on real directories
//! created at test time (`D-022`, `D-073`).

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, PathLimits, SourcePath,
};
use frameshift_engine::input::{Snapshot, snapshot};
use frameshift_engine::limits::Limits;
use frameshift_engine::package::{
    LoadError, ManifestState, SelectionError, Workspace, WorkspaceSemantics, load_workspace,
    release_members,
};

mod support;

use support::Json;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("workspace").join(name);
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

fn load(root: &Path) -> (Snapshot, Result<Workspace, LoadError>, FindingList) {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let limits = Limits::default();
    let snapshot = snapshot(root, limits.snapshot(), &mut findings).unwrap();
    let workspace = load_workspace(&snapshot, limits.package(), &mut findings);
    (snapshot, workspace, findings.finish())
}

fn codes(findings: &FindingList) -> Vec<&str> {
    findings
        .as_slice()
        .iter()
        .map(|finding| finding.code().as_str())
        .collect()
}

fn members(workspace: &Workspace) -> Vec<&str> {
    workspace
        .members
        .iter()
        .map(|member| member.directory.as_str())
        .collect()
}

fn manifest(name: &str) -> String {
    format!(r#"{{ "name": "{name}", "version": "1.0.0" }}"#)
}

#[test]
fn npm_workspace_members_resolve_through_the_root_lockfile() {
    let root = scratch("npm");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "private": true, "packageManager": "npm@11.11.0",
             "workspaces": ["packages/*", "!packages/legacy"] }"#,
    );
    write(
        &root,
        "packages/site/package.json",
        r#"{ "name": "fictional-site", "version": "1.0.0",
             "dependencies": { "fictional-ui": "^1.0.0", "fictional-dep": "^2.1.0" } }"#,
    );
    write(&root, "packages/ui/package.json", &manifest("fictional-ui"));
    write(
        &root,
        "packages/legacy/package.json",
        &manifest("fictional-old"),
    );
    write(
        &root,
        "packages/.hidden/package.json",
        &manifest("fictional-hidden"),
    );
    write(&root, "packages/empty/readme.txt", "no manifest");
    write(
        &root,
        "package-lock.json",
        r#"{ "name": "fictional-root", "lockfileVersion": 3, "requires": true, "packages": {
             "": { "name": "fictional-root", "workspaces": ["packages/*", "!packages/legacy"] },
             "packages/site": { "name": "fictional-site", "version": "1.0.0",
               "dependencies": { "fictional-ui": "^1.0.0", "fictional-dep": "^2.1.0" } },
             "packages/ui": { "name": "fictional-ui", "version": "1.0.0" },
             "node_modules/fictional-site": { "resolved": "packages/site", "link": true },
             "node_modules/fictional-ui": { "resolved": "packages/ui", "link": true },
             "node_modules/fictional-dep": { "version": "2.3.4",
               "resolved": "https://registry.npmjs.org/fictional-dep/-/fictional-dep-2.3.4.tgz" }
           } }"#,
    );
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(codes(&findings), Vec::<&str>::new(), "{findings:?}");
    assert!(workspace.is_workspace());
    assert!(workspace.complete);
    assert_eq!(workspace.semantics, [WorkspaceSemantics::Npm]);
    assert_eq!(members(&workspace), ["packages/site", "packages/ui"]);
    let site = workspace.select(Some(&path("packages/site"))).unwrap();
    assert_eq!(site.lockfile_workspace(), "packages/site");
    assert!(matches!(site.manifest(), ManifestState::Analyzed(_)));
    let dep = site.resolve_declared("fictional-dep", "^2.1.0").unwrap();
    assert_eq!(dep.version().unwrap().to_string(), "2.3.4");
    assert_eq!(
        site.sibling("fictional-ui"),
        Some(Some(&path("packages/ui")))
    );
    assert_eq!(site.sibling("fictional-dep"), None);
    assert_eq!(
        workspace
            .select(Some(&path("packages/legacy")))
            .unwrap_err(),
        SelectionError::NotAMember
    );
}

#[test]
fn undetermined_release_disputes_members() {
    let root = scratch("disputed");
    // No packageManager and no lockfile: every release that reads the
    // workspaces field is evaluated. Bun keeps `packages/b` because the
    // negation comes first; npm and Yarn 4 drop it; Yarn 1 reads `!` as a
    // literal character.
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "private": true,
             "workspaces": ["!packages/b", "packages/*"] }"#,
    );
    write(&root, "packages/a/package.json", &manifest("fictional-a"));
    write(&root, "packages/b/package.json", &manifest("fictional-b"));
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(members(&workspace), ["packages/a"]);
    assert_eq!(workspace.undetermined, [path("packages/b")]);
    assert!(!workspace.complete);
    assert_eq!(codes(&findings), ["FS1032"]);
    assert_eq!(
        workspace.select(Some(&path("packages/b"))).unwrap_err(),
        SelectionError::Undetermined
    );
}

#[test]
fn rejected_patterns_and_hidden_entries_fail_closed() {
    let root = scratch("rejected");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "pnpm@12.14.0" }"#,
    );
    write(
        &root,
        "pnpm-workspace.yaml",
        "packages:\n  - 'apps/+(a|b)'\n",
    );
    write(&root, "apps/a/package.json", &manifest("fictional-a"));
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert!(workspace.members.is_empty());
    assert!(!workspace.complete);
    assert_eq!(codes(&findings), ["FS1030"]);

    let root = scratch("hidden");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "pnpm@10.34.7" }"#,
    );
    write(&root, "pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    write(&root, ".gitignore", "packages/generated/\n");
    write(&root, "packages/a/package.json", &manifest("fictional-a"));
    write(
        &root,
        "packages/generated/package.json",
        &manifest("fictional-g"),
    );
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(members(&workspace), ["packages/a"]);
    assert!(!workspace.complete);
    assert_eq!(codes(&findings), ["FS1031"]);
}

#[test]
fn yarn_classic_links_satisfied_siblings_instead_of_reporting_staleness() {
    let root = scratch("yarn-classic");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "private": true, "packageManager": "yarn@1.22.22",
             "workspaces": ["packages/*"] }"#,
    );
    write(
        &root,
        "packages/site/package.json",
        r#"{ "name": "fictional-site", "version": "1.0.0",
             "dependencies": { "fictional-ui": "^1.0.0", "fictional-dep": "^2.0.0" } }"#,
    );
    write(&root, "packages/ui/package.json", &manifest("fictional-ui"));
    write(
        &root,
        "yarn.lock",
        "# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n\
         # yarn lockfile v1\n\n\n\
         fictional-dep@^2.0.0:\n  version \"2.1.0\"\n  \
         resolved \"https://registry.yarnpkg.com/fictional-dep/-/fictional-dep-2.1.0.tgz\"\n",
    );
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(codes(&findings), Vec::<&str>::new(), "{findings:?}");
    assert_eq!(workspace.semantics, [WorkspaceSemantics::YarnClassic]);
    let site = workspace.select(Some(&path("packages/site"))).unwrap();
    let dep = site.resolve_declared("fictional-dep", "^2.0.0").unwrap();
    assert_eq!(dep.version().unwrap().to_string(), "2.1.0");
    assert!(site.resolve_declared("fictional-ui", "^1.0.0").is_none());
    assert_eq!(
        site.sibling("fictional-ui"),
        Some(Some(&path("packages/ui")))
    );

    // A declaration that no sibling satisfies needs a lockfile entry.
    write(
        &root,
        "packages/site/package.json",
        r#"{ "name": "fictional-site", "version": "1.0.0",
             "dependencies": { "fictional-ui": "^2.0.0" } }"#,
    );
    let (_snapshot, _workspace, findings) = load(&root);
    assert_eq!(codes(&findings), ["FS1027"]);
    assert_eq!(
        findings.as_slice()[0]
            .location()
            .file()
            .map(SourcePath::as_str),
        Some("packages/site/package.json")
    );
}

#[test]
fn yarn_berry_resolves_nested_workspaces() {
    let root = scratch("berry-nested");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "yarn@4.18.1",
             "workspaces": ["apps/*"] }"#,
    );
    write(
        &root,
        "apps/suite/package.json",
        r#"{ "name": "fictional-suite", "workspaces": ["tools/*"] }"#,
    );
    write(
        &root,
        "apps/suite/tools/cli/package.json",
        &manifest("fictional-cli"),
    );
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(codes(&findings), Vec::<&str>::new(), "{findings:?}");
    assert_eq!(members(&workspace), ["apps/suite", "apps/suite/tools/cli"]);
}

#[test]
fn package_manager_findings() {
    // pnpm reads only pnpm-workspace.yaml (FS1034); pnpm 13 is not modeled
    // (FS1037), so all modeled pnpm releases are evaluated.
    let root = scratch("ignored-declaration");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "pnpm@13.0.0",
             "workspaces": ["packages/*"] }"#,
    );
    write(&root, "pnpm-workspace.yaml", "packages: [\"packages/*\"]\n");
    write(&root, "packages/a/package.json", &manifest("fictional-a"));
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert_eq!(codes(&findings), ["FS1034", "FS1037"]);
    assert_eq!(
        workspace.semantics,
        [
            WorkspaceSemantics::PnpmV10,
            WorkspaceSemantics::PnpmV11,
            WorkspaceSemantics::PnpmV12
        ]
    );
    assert_eq!(members(&workspace), ["packages/a"]);

    // Bun rejects a member without a name, and a listed directory without a
    // package.json (FS1035).
    let root = scratch("bun-rejects");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "bun@1.4.3",
             "workspaces": ["packages/*", "tools/missing"] }"#,
    );
    write(
        &root,
        "packages/a/package.json",
        r#"{ "version": "1.0.0" }"#,
    );
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert!(workspace.members.is_empty());
    assert_eq!(codes(&findings), ["FS1035", "FS1035"]);

    // pnpm rejects a packages field that is not a sequence of strings.
    let root = scratch("pnpm-invalid");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "pnpm@10.34.7" }"#,
    );
    write(&root, "pnpm-workspace.yaml", "packages:\n  - 12\n");
    let (_snapshot, workspace, findings) = load(&root);
    assert!(!workspace.unwrap().complete);
    assert_eq!(codes(&findings), ["FS1035"]);
}

#[test]
fn case_dependent_patterns_are_reported() {
    let root = scratch("case");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "packageManager": "npm@11.11.0",
             "workspaces": ["Packages/*"] }"#,
    );
    write(&root, "packages/a/package.json", &manifest("fictional-a"));
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert!(workspace.members.is_empty());
    assert_eq!(codes(&findings), ["FS1033"]);
}

#[test]
fn a_root_without_workspaces_is_a_single_package() {
    let root = scratch("single");
    write(&root, "package.json", &manifest("fictional-root"));
    write(&root, "packages/a/package.json", &manifest("fictional-a"));
    let (_snapshot, workspace, findings) = load(&root);
    let workspace = workspace.unwrap();
    assert!(!workspace.is_workspace());
    assert!(workspace.complete);
    assert!(workspace.members.is_empty());
    assert!(codes(&findings).is_empty());
    let root_package = workspace.select(None).unwrap();
    assert_eq!(root_package.lockfile_workspace(), "");
    assert!(root_package.lockfile().is_none());
}

/// The releases of each modeled semantics in the oracle results.
fn tools(release: WorkspaceSemantics) -> &'static [&'static str] {
    match release {
        WorkspaceSemantics::Npm => &["npm"],
        WorkspaceSemantics::YarnClassic => &["yarn1"],
        WorkspaceSemantics::YarnBerryV2 => &["berry2", "berry3"],
        WorkspaceSemantics::YarnBerryV4 => &["berry4"],
        WorkspaceSemantics::PnpmV10 => &["pnpm10"],
        WorkspaceSemantics::PnpmV11 => &["pnpm11"],
        WorkspaceSemantics::PnpmV12 => &["pnpm12"],
        WorkspaceSemantics::Bun => &["bun140", "bun143"],
    }
}

/// Writes one oracle case as the external runner does.
fn materialize(root: &Path, case: &Json) {
    let mut manifest = String::from(r#"{ "name": "fictional-root", "version": "1.0.0""#);
    if case.get("private") != Some(&Json::Bool(false)) {
        manifest.push_str(r#", "private": true"#);
    }
    if let Some(workspaces) = case.get("workspaces") {
        manifest.push_str(r#", "workspaces": "#);
        manifest.push_str(&support::render(workspaces));
    }
    manifest.push('}');
    write(root, "package.json", &manifest);
    match case.get("pnpm") {
        Some(Json::Array(patterns)) if patterns.is_empty() => {
            write(root, "pnpm-workspace.yaml", "packages: []\n");
        }
        Some(Json::Array(patterns)) => {
            let mut text = String::from("packages:\n");
            for pattern in patterns {
                text.push_str("  - ");
                text.push_str(&support::render(pattern));
                text.push('\n');
            }
            write(root, "pnpm-workspace.yaml", &text);
        }
        Some(Json::Null) => write(root, "pnpm-workspace.yaml", "{}\n"),
        _ => {}
    }
    let Some(Json::Array(tree)) = case.get("tree") else {
        panic!("case without tree");
    };
    for entry in tree {
        let entry_path = entry.get("path").and_then(Json::as_str).unwrap();
        if entry.get("kind").and_then(Json::as_str) == Some("dir") {
            fs::create_dir_all(root.join(entry_path)).unwrap();
        } else {
            let content = entry.get("content").and_then(Json::as_str).unwrap_or("");
            write(root, entry_path, content);
        }
    }
}

/// Compares every release's members with the package managers' own
/// listings of generated fictional workspaces. `FRAMESHIFT_WORKSPACE_CASES`
/// names the cases and `FRAMESHIFT_WORKSPACE_RESULTS` the listings that the
/// external runner recorded (IMPLEMENTATION.md section 30).
#[test]
#[ignore = "external reference: set FRAMESHIFT_WORKSPACE_CASES and FRAMESHIFT_WORKSPACE_RESULTS"]
fn external_workspace_oracle() {
    let cases = std::env::var("FRAMESHIFT_WORKSPACE_CASES").expect("cases file");
    let results = std::env::var("FRAMESHIFT_WORKSPACE_RESULTS").expect("results file");
    let cases = support::parse(&fs::read_to_string(cases).unwrap()).unwrap();
    let results = support::parse(&fs::read_to_string(results).unwrap()).unwrap();
    let Json::Array(cases) = cases else {
        panic!("cases are not an array");
    };
    let limits = Limits::default();
    let (mut compared, mut skipped, mut mismatches) = (0usize, 0usize, Vec::new());
    for case in &cases {
        let id = case.get("id").and_then(Json::as_str).unwrap();
        let Some(result) = results.get(id) else {
            continue;
        };
        let root = scratch(&format!("oracle-{id}"));
        materialize(&root, case);
        let mut findings = FindingCollector::new(FindingLimits::default());
        let snapshot = snapshot(&root, limits.snapshot(), &mut findings).unwrap();
        for release in WorkspaceSemantics::ALL {
            let mut release_findings = FindingCollector::new(FindingLimits::default());
            let ours = release_members(&snapshot, limits.package(), release, &mut release_findings)
                .unwrap();
            for tool in tools(release) {
                let Some(expected) = result.get(tool) else {
                    continue;
                };
                if !ours.exact {
                    skipped += 1;
                    continue;
                }
                compared += 1;
                let error = expected.get("error") == Some(&Json::Bool(true));
                let listed: Vec<&str> = match expected.get("members") {
                    Some(Json::Array(members)) => members.iter().filter_map(Json::as_str).collect(),
                    _ => Vec::new(),
                };
                let mine: Vec<&str> = ours.members.iter().map(SourcePath::as_str).collect();
                let agrees = if error {
                    ours.rejects
                } else {
                    !ours.rejects && mine == listed
                };
                if !agrees {
                    mismatches.push(format!(
                        "{id} {tool}: expected {}{listed:?}, got {}{mine:?}",
                        if error { "error " } else { "" },
                        if ours.rejects { "rejects " } else { "" },
                    ));
                }
            }
        }
        fs::remove_dir_all(fs::canonicalize(&root).unwrap()).unwrap();
    }
    println!(
        "{compared} comparisons, {skipped} skipped as not exact, {} mismatches",
        mismatches.len()
    );
    for mismatch in &mismatches {
        println!("  {mismatch}");
    }
    assert!(mismatches.is_empty());
}
