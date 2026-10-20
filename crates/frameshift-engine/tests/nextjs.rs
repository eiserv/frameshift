//! Next.js detection through the public facade, on real directories created
//! at test time (`D-074`). Manifests naming the real `next` package exist
//! only during the test, never as committed fixtures, because the public
//! repository's manifests are scanned for advisories.

use std::fs;
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{
    FindingCollector, FindingLimits, FindingList, PathLimits, SourcePath,
};
use frameshift_engine::input::snapshot;
use frameshift_engine::limits::Limits;
use frameshift_engine::nextjs::{
    ConfigLoad, Detection, Router, RouterState, Verdict, VersionEvidence, detect,
};
use frameshift_engine::package::{DependencyKind, Version, load_workspace};

mod support;

fn scratch(name: &str) -> PathBuf {
    let directory = support::temporary().join("nextjs").join(name);
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

fn run(root: &Path, package: Option<&str>) -> (Detection, FindingList) {
    let mut findings = FindingCollector::new(FindingLimits::default());
    let limits = Limits::default();
    let snapshot = snapshot(root, limits.snapshot(), &mut findings).unwrap();
    let workspace = load_workspace(&snapshot, limits.package(), &mut findings).unwrap();
    let package = package.map(path);
    let selected = workspace.select(package.as_ref()).unwrap();
    let detection = detect(snapshot.inventory(), &selected, &mut findings).unwrap();
    (detection, findings.finish())
}

/// The finding codes, sorted, since the finished list orders findings by
/// severity and location first.
fn codes(findings: &FindingList) -> Vec<&str> {
    let mut codes: Vec<&str> = findings
        .as_slice()
        .iter()
        .map(|finding| finding.code().as_str())
        .collect();
    codes.sort_unstable();
    codes
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
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
fn app_router_project_with_a_locked_version() {
    let root = scratch("app-router");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "private": true, "packageManager": "pnpm@10.34.7",
             "dependencies": { "next": "^16.2.0" } }"#,
    );
    write(&root, "pnpm-lock.yaml", PNPM_LOCK);
    write(&root, "next.config.ts", "export default {};\n");
    write(&root, "app/layout.tsx", "");
    write(&root, "app/page.tsx", "");
    write(&root, "app/(marketing)/about/page.tsx", "");
    write(&root, "app/api/health/route.ts", "");
    write(&root, "app/_components/page.tsx", "");
    write(&root, "app/blog/page.mdx", "");
    let (detection, findings) = run(&root, None);
    assert_eq!(codes(&findings), Vec::<&str>::new(), "{findings:?}");
    assert_eq!(detection.verdict, Verdict::NextJs);
    assert_eq!(
        detection.dependency.as_ref().unwrap().kind,
        DependencyKind::Production
    );
    assert_eq!(
        detection.version,
        VersionEvidence::Locked(version("16.2.1"))
    );
    let config = detection.config.as_ref().unwrap();
    assert_eq!(config.path, path("next.config.ts"));
    assert_eq!(config.load, ConfigLoad::Loaded);
    let app = detection.app.as_ref().unwrap();
    assert_eq!(app.path, path("app"));
    assert_eq!(
        app.state,
        RouterState::Directory {
            routes: 3,
            complete: true
        }
    );
    assert_eq!(detection.pages, None);
    assert_eq!(detection.router(), Router::App);

    // Next.js takes `src/pages` as the pages directory when `pages` is
    // missing, and from Next.js 15 rejects it beside `app` (FS1044).
    write(&root, "src/pages/index.astro", "");
    let (detection, findings) = run(&root, None);
    assert_eq!(codes(&findings), ["FS1044"]);
    assert_eq!(detection.pages.as_ref().unwrap().path, path("src/pages"));
    assert_eq!(detection.pages.as_ref().unwrap().routes(), 0);
}

#[test]
fn workspace_member_resolves_through_the_root_lockfile() {
    let root = scratch("member");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-root", "private": true, "packageManager": "npm@11.11.0",
             "workspaces": ["apps/*"] }"#,
    );
    write(
        &root,
        "apps/web/package.json",
        r#"{ "name": "fictional-web", "devDependencies": { "next": "~15.5.0" } }"#,
    );
    write(&root, "next.config.mjs", "export default {};\n");
    write(&root, "apps/web/src/app/page.jsx", "");
    write(
        &root,
        "package-lock.json",
        r#"{ "name": "fictional-root", "lockfileVersion": 3, "requires": true, "packages": {
             "": { "name": "fictional-root", "workspaces": ["apps/*"] },
             "apps/web": { "name": "fictional-web", "devDependencies": { "next": "~15.5.0" } },
             "node_modules/fictional-web": { "resolved": "apps/web", "link": true },
             "node_modules/next": { "version": "15.5.2", "dev": true,
               "resolved": "https://registry.npmjs.org/next/-/next-15.5.2.tgz" }
           } }"#,
    );
    let (detection, findings) = run(&root, Some("apps/web"));
    assert_eq!(codes(&findings), Vec::<&str>::new(), "{findings:?}");
    assert_eq!(detection.verdict, Verdict::NextJs);
    assert_eq!(
        detection.version,
        VersionEvidence::Locked(version("15.5.2"))
    );
    // Next.js searches upward from the package directory.
    assert_eq!(
        detection.config.as_ref().unwrap().path,
        path("next.config.mjs")
    );
    let app = detection.app.as_ref().unwrap();
    assert_eq!(app.path, path("apps/web/src/app"));
    assert!(app.in_src);
}

#[test]
fn pages_router_without_a_lockfile_uses_the_lower_bound() {
    let root = scratch("pages");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "next": "^14.2.3" } }"#,
    );
    write(&root, "pages/index.tsx", "");
    write(&root, "pages/_app.tsx", "");
    write(&root, "pages/types.d.ts", "");
    write(&root, "pages/api/hello.ts", "");
    let (detection, findings) = run(&root, None);
    assert_eq!(codes(&findings), ["FS1042", "FS1043"]);
    assert_eq!(
        detection.version,
        VersionEvidence::LowerBound(version("14.2.3"))
    );
    assert_eq!(detection.pages.as_ref().unwrap().routes(), 2);
    assert_eq!(detection.router(), Router::Pages);
}

#[test]
fn version_and_configuration_outside_what_next_supports() {
    let root = scratch("old");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "next": "13.5.6" } }"#,
    );
    write(&root, "next.config.ts", "export default {};\n");
    write(&root, "app/page.tsx", "");
    write(&root, "src/pages/index.tsx", "");
    let (detection, findings) = run(&root, None);
    // No lockfile (FS1042), outside 14 to 16 (FS1041), next.config.ts is
    // not loaded before Next.js 15 (FS1043); Next.js 13 accepts app and
    // pages in different directories.
    assert_eq!(codes(&findings), ["FS1041", "FS1042", "FS1043"]);
    assert_eq!(detection.verdict, Verdict::NextJs);
    assert_eq!(
        detection.config.as_ref().unwrap().load,
        ConfigLoad::Unsupported
    );
    assert_eq!(detection.router(), Router::Hybrid);

    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "next": "16.4.2" } }"#,
    );
    write(&root, "next.config.mts", "export default {};\n");
    let (detection, findings) = run(&root, None);
    // next.config.ts comes first in Next.js's order; app and pages in
    // different directories fail from Next.js 15 (FS1044).
    assert_eq!(codes(&findings), ["FS1042", "FS1044"]);
    assert_eq!(detection.config.as_ref().unwrap().load, ConfigLoad::Loaded);
}

#[test]
fn incomplete_and_absent_evidence() {
    let root = scratch("inconclusive");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "next": "^16.0.0" } }"#,
    );
    let (detection, findings) = run(&root, None);
    assert_eq!(detection.verdict, Verdict::Inconclusive);
    assert_eq!(codes(&findings), ["FS1040"]);

    let root = scratch("astro");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "fictional-astro": "^7.0.0" } }"#,
    );
    write(&root, "src/pages/index.astro", "");
    write(&root, "src/pages/about.ts", "");
    let (detection, findings) = run(&root, None);
    assert_eq!(detection.verdict, Verdict::Absent);
    assert!(codes(&findings).is_empty());

    let root = scratch("ignored-app");
    write(
        &root,
        "package.json",
        r#"{ "name": "fictional-site", "dependencies": { "next": "^16.0.0" } }"#,
    );
    write(&root, "next.config.js", "module.exports = {};\n");
    write(&root, ".gitignore", "app/generated/\n");
    write(&root, "app/page.tsx", "");
    write(&root, "app/generated/page.tsx", "");
    let (detection, findings) = run(&root, None);
    assert_eq!(detection.verdict, Verdict::NextJs);
    assert_eq!(codes(&findings), ["FS1042", "FS1044"]);
    assert_eq!(
        detection.app.as_ref().unwrap().state,
        RouterState::Directory {
            routes: 1,
            complete: false
        }
    );
}

/// Prints the workspace and Next.js detection of every package of the
/// project directories that `FRAMESHIFT_NEXTJS_PROJECTS` names, such as the
/// approved real-world sample of `D-043`, kept outside the repository.
#[test]
#[ignore = "external reference: set FRAMESHIFT_NEXTJS_PROJECTS to project directories"]
fn external_projects() {
    let paths = std::env::var_os("FRAMESHIFT_NEXTJS_PROJECTS")
        .expect("FRAMESHIFT_NEXTJS_PROJECTS names project directories");
    let limits = Limits::default();
    for root in std::env::split_paths(&paths) {
        let mut findings = FindingCollector::new(FindingLimits::default());
        let snapshot = snapshot(&root, limits.snapshot(), &mut findings).unwrap();
        let workspace = load_workspace(&snapshot, limits.package(), &mut findings).unwrap();
        println!(
            "{}: releases {:?}, {} members, complete {}",
            root.display(),
            workspace.semantics,
            workspace.members.len(),
            workspace.complete
        );
        let mut packages = vec![None];
        packages.extend(
            workspace
                .members
                .iter()
                .map(|member| Some(&member.directory)),
        );
        for package in packages {
            let selected = workspace.select(package).unwrap();
            let detection = detect(snapshot.inventory(), &selected, &mut findings).unwrap();
            if detection.verdict == Verdict::Absent {
                continue;
            }
            println!(
                "  {}: {:?} {:?} {:?} config {:?}",
                package.map_or(".", SourcePath::as_str),
                detection.verdict,
                detection.version,
                detection.router(),
                detection
                    .config
                    .as_ref()
                    .map(|config| (config.path.as_str(), config.load)),
            );
        }
        let findings = findings.finish();
        let mut codes: Vec<&str> = findings
            .as_slice()
            .iter()
            .map(|finding| finding.code().as_str())
            .filter(|code| code.as_bytes()[2] != b'1' || code.as_bytes()[4] >= b'3')
            .collect();
        codes.sort_unstable();
        codes.dedup();
        println!("  workspace and Next.js codes: {codes:?}");
    }
}
