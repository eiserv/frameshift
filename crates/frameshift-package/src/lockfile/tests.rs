use frameshift_core::{SourceText, TextLimits};
use frameshift_syntax::{JsonDialect, JsonLimits, parse_json};

use super::*;

const NPM: &str = r#"{
  "name": "fictional-root",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {
      "name": "fictional-root",
      "version": "1.0.0",
      "workspaces": ["packages/*"],
      "dependencies": { "tally": "^7.0.0", "renamed": "npm:counter@^3.0.1" },
      "devDependencies": { "evenly": "1.0.0" },
      "optionalDependencies": { "padder": "1.3.0" },
      "peerDependencies": { "tally": ">=6" }
    },
    "node_modules/@fictional/ui": { "resolved": "packages/ui", "link": true },
    "node_modules/evenly": {
      "version": "1.0.0",
      "resolved": "https://registry.example.test/evenly/-/evenly-1.0.0.tgz",
      "dev": true,
      "dependencies": { "counter": "^0.1.2" }
    },
    "node_modules/counter": { "version": "0.1.2", "dev": true, "dependencies": { "tally": "^3.0.0" } },
    "node_modules/counter/node_modules/tally": { "version": "3.0.0", "dev": true },
    "node_modules/tally": { "version": "7.0.0" },
    "node_modules/padder": { "version": "1.3.0", "optional": true },
    "node_modules/renamed": { "name": "counter", "version": "3.0.1", "dependencies": { "tally": "^6.0.0" } },
    "node_modules/renamed/node_modules/tally": { "version": "6.0.0" },
    "node_modules/from-git": { "version": "2.0.0", "resolved": "git+ssh://git@example.test/x.git#abc" },
    "node_modules/with-build": { "version": "2.0.0-dev+sha.abc", "resolved": "https://registry.example.test/w.tgz" },
    "node_modules/no-version": { "resolved": "file:../vendor/no-version" },
    "packages/ui": {
      "name": "@fictional/ui",
      "version": "0.1.0",
      "dependencies": { "tally": "^6.0.0" },
      "peerDependencies": { "react": "*" },
      "peerDependenciesMeta": { "react": { "optional": true } }
    },
    "packages/ui/node_modules/tally": { "version": "6.0.0" }
  },
  "dependencies": { "ignored": { "version": "1.0.0" } }
}"#;

const BUN: &str = r#"{
  "lockfileVersion": 2,
  "configVersion": 1,
  "workspaces": {
    "": {
      "name": "fictional-root",
      "dependencies": { "tally": "^7.0.0", "renamed": "npm:counter@^3.0.1", },
      "devDependencies": { "evenly": "1.0.0", },
      "optionalDependencies": { "padder": "1.3.0", },
      "peerDependencies": { "tally": ">=6", },
    },
    "packages/ui": {
      "name": "@fictional/ui",
      "version": "0.1.0",
      "dependencies": { "tally": "^6.0.0", },
      "peerDependencies": { "react": "*", },
      "optionalPeers": ["react",],
    },
  },
  "packages": {
    // Comments and trailing commas are JSONC.
    "@fictional/ui": ["@fictional/ui@workspace:packages/ui"],
    "evenly": ["evenly@1.0.0", "", { "dependencies": { "counter": "^0.1.2" } }, "sha512-fictional"],
    "counter": ["counter@0.1.2", "", { "dependencies": { "tally": "^3.0.0" } }, "sha512-fictional"],
    "tally": ["tally@7.0.0", "", {}, "sha512-fictional"],
    "padder": ["padder@1.3.0", "", {}, "sha512-fictional"],
    "renamed": ["counter@3.0.1", "", { "dependencies": { "tally": "^6.0.0" }, "peerDependencies": { "host": "*" }, "optionalPeers": ["host"] }, "sha512-fictional"],
    "from-git": ["from-git@github:owner/repo#abc", { "dependencies": {} }, "owner-repo-abc"],
    "@fictional/ui/tally": ["tally@6.0.0", "", {}, "sha512-fictional"],
    "counter/tally": ["tally@3.0.0", "", {}, "sha512-fictional"],
    "renamed/tally": ["tally@6.0.0", "", {}, "sha512-fictional"],
  }
}"#;

fn lockfile(kind: LockfileKind, text: &str) -> Result<Lockfile, LockfileError> {
    let source = SourceText::new(text.to_owned(), TextLimits::default()).unwrap();
    let dialect = if kind == LockfileKind::BunText {
        JsonDialect::Jsonc
    } else {
        JsonDialect::Json
    };
    let document = parse_json(&source, dialect, JsonLimits::default()).unwrap();
    Lockfile::from_json(kind, &document)
}

fn resolved(lockfile: &Lockfile, workspace: &str, name: &str) -> Option<String> {
    lockfile
        .resolve(workspace, name)
        .map(|package| match &package.resolution {
            Resolution::Registry(version) => format!("{}@{version}", package.name),
            Resolution::Link(target) => format!("link:{target}"),
            Resolution::Workspace(path) => format!("workspace:{path}"),
            Resolution::Other(text) => format!("other:{text}"),
        })
}

fn names(list: &[Declared]) -> Vec<&str> {
    list.iter().map(|declared| &*declared.name).collect()
}

#[test]
fn reads_npm_lockfiles() {
    let lockfile = lockfile(LockfileKind::NpmPackageLock, NPM).unwrap();
    assert_eq!(lockfile.kind(), LockfileKind::NpmPackageLock);
    assert_eq!(lockfile.version(), 3);
    let paths: Vec<&str> = lockfile
        .workspaces()
        .iter()
        .map(|workspace| &*workspace.path)
        .collect();
    assert_eq!(paths, ["", "packages/ui"]);
    let root = lockfile.workspace("").unwrap();
    assert_eq!(
        names(root.declared(DependencyKind::Production)),
        ["renamed", "tally"]
    );
    assert_eq!(
        names(root.declared(DependencyKind::Development)),
        ["evenly"]
    );
    assert_eq!(names(root.declared(DependencyKind::Optional)), ["padder"]);
    assert_eq!(names(root.declared(DependencyKind::Peer)), ["tally"]);
    for (workspace, name, expected) in [
        ("", "tally", Some("tally@7.0.0")),
        ("", "renamed", Some("counter@3.0.1")),
        ("", "evenly", Some("evenly@1.0.0")),
        ("", "@fictional/ui", Some("link:packages/ui")),
        ("packages/ui", "tally", Some("tally@6.0.0")),
        ("packages/ui", "evenly", Some("evenly@1.0.0")),
        (
            "",
            "from-git",
            Some("other:git+ssh://git@example.test/x.git#abc"),
        ),
        ("", "no-version", Some("other:file:../vendor/no-version")),
        ("", "with-build", Some("with-build@2.0.0-dev+sha.abc")),
        ("", "missing", None),
    ] {
        assert_eq!(
            resolved(&lockfile, workspace, name).as_deref(),
            expected,
            "{workspace} {name}"
        );
    }
    assert_eq!(
        lockfile
            .package("node_modules/with-build")
            .unwrap()
            .resolution,
        Resolution::Registry(Version::parse("2.0.0-dev+sha.abc").unwrap())
    );
    let counter = lockfile.package("node_modules/counter").unwrap();
    assert!(counter.dev && !counter.optional);
    assert_eq!(
        lockfile
            .resolve_from(counter, "tally")
            .unwrap()
            .version()
            .unwrap()
            .to_string(),
        "3.0.0"
    );
    let renamed = lockfile.package("node_modules/renamed").unwrap();
    assert_eq!(
        lockfile
            .resolve_from(renamed, "tally")
            .unwrap()
            .key
            .as_ref(),
        "node_modules/renamed/node_modules/tally"
    );
    assert!(lockfile.package("node_modules/padder").unwrap().optional);
    let ui = lockfile.workspace("packages/ui").unwrap();
    assert_eq!(ui.name.as_deref(), Some("@fictional/ui"));
    assert_eq!(ui.version.as_deref(), Some("0.1.0"));
}

#[test]
fn reads_bun_lockfiles() {
    let lockfile = lockfile(LockfileKind::BunText, BUN).unwrap();
    assert_eq!(lockfile.version(), 2);
    let root = lockfile.workspace("").unwrap();
    assert_eq!(
        names(root.declared(DependencyKind::Production)),
        ["renamed", "tally"]
    );
    assert_eq!(
        names(root.declared(DependencyKind::Development)),
        ["evenly"]
    );
    for (workspace, name, expected) in [
        ("", "tally", Some("tally@7.0.0")),
        ("", "renamed", Some("counter@3.0.1")),
        ("", "@fictional/ui", Some("workspace:packages/ui")),
        ("packages/ui", "tally", Some("tally@6.0.0")),
        ("packages/ui", "evenly", Some("evenly@1.0.0")),
        ("", "from-git", Some("other:github:owner/repo#abc")),
        ("", "missing", None),
        ("packages/missing", "tally", None),
    ] {
        assert_eq!(
            resolved(&lockfile, workspace, name).as_deref(),
            expected,
            "{workspace} {name}"
        );
    }
    let renamed = lockfile.package("renamed").unwrap();
    assert_eq!(names(&renamed.peer_dependencies), ["host"]);
    assert_eq!(renamed.optional_peers, [Box::from("host")]);
    assert_eq!(
        lockfile
            .resolve_from(renamed, "tally")
            .unwrap()
            .key
            .as_ref(),
        "renamed/tally"
    );
    let counter = lockfile.package("counter").unwrap();
    assert_eq!(
        lockfile
            .resolve_from(counter, "tally")
            .unwrap()
            .key
            .as_ref(),
        "counter/tally"
    );
    assert_eq!(
        bun::split_names("@a/b/c/@d/e"),
        Some(vec!["@a/b", "c", "@d/e"])
    );
    assert_eq!(bun::split_names("@a"), None);
    assert_eq!(bun::split_names("a//b"), None);
}

#[test]
fn rejects_unsupported_versions_and_structures() {
    let version = |kind, text: &str| lockfile(kind, text).map(|_| ()).map_err(|error| error.kind);
    use LockfileErrorKind::{Structure, UnsupportedVersion};
    let npm = LockfileKind::NpmPackageLock;
    let bun = LockfileKind::BunText;
    for (kind, text, expected) in [
        (
            npm,
            r#"{ "lockfileVersion": 1, "dependencies": {} }"#,
            Err(UnsupportedVersion),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 4, "packages": {} }"#,
            Err(UnsupportedVersion),
        ),
        (npm, r#"{ "lockfileVersion": 2, "packages": {} }"#, Ok(())),
        (
            npm,
            r#"{ "lockfileVersion": "3", "packages": {} }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3.0, "packages": {} }"#,
            Err(Structure),
        ),
        (npm, r#"{ "lockfileVersion": 3 }"#, Err(Structure)),
        (npm, r#"[]"#, Err(Structure)),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": [] } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": { "version": 1 } } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": { "dev": "yes" } } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": { "dependencies": { "b": 1 } } } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": { "link": true } } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules//a": {} } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/@s": {} } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "a\\b": {} } }"#,
            Err(Structure),
        ),
        (
            npm,
            r#"{ "lockfileVersion": 3, "packages": { "node_modules/Old_Name": { "version": "1.0.0" } } }"#,
            Ok(()),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 0, "workspaces": {} }"#,
            Err(UnsupportedVersion),
        ),
        (bun, r#"{ "lockfileVersion": 1, "workspaces": {} }"#, Ok(())),
        (
            bun,
            r#"{ "lockfileVersion": 3, "workspaces": {} }"#,
            Err(UnsupportedVersion),
        ),
        (bun, r#"{ "lockfileVersion": 1 }"#, Err(Structure)),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": { "../x": {} } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "a": [] } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "a": ["a"] } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "a": ["a@"] } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "a": [1] } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "a": ["a@1.0.0", "", { "dependencies": [] }] } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": { "": { "optionalPeers": [1] } } }"#,
            Err(Structure),
        ),
        (
            bun,
            r#"{ "lockfileVersion": 1, "workspaces": {}, "packages": { "": ["a@1.0.0"] } }"#,
            Err(Structure),
        ),
        (LockfileKind::Pnpm, r#"{}"#, Err(UnsupportedVersion)),
    ] {
        assert_eq!(version(kind, text), expected, "{text}");
    }
    let error = lockfile(
        npm,
        r#"{ "lockfileVersion": 3, "packages": { "node_modules/a": { "version": 1 } } }"#,
    )
    .unwrap_err();
    assert_eq!(error.span, Span::new(69, 70).unwrap());
}

#[test]
fn detects_declarations_out_of_sync() {
    let manifest_text = r#"{
  "dependencies": { "tally": "^7.0.0", "renamed": "npm:counter@^3.0.1" },
  "devDependencies": { "evenly": "1.0.1" },
  "optionalDependencies": { "padder": "1.3.0" },
  "peerDependencies": { "tally": ">=6" }
}"#;
    let source = SourceText::new(manifest_text.to_owned(), TextLimits::default()).unwrap();
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default()).unwrap();
    let manifest = PackageManifest::from_json(&document).unwrap();
    for kind in [LockfileKind::NpmPackageLock, LockfileKind::BunText] {
        let text = if kind == LockfileKind::BunText {
            BUN
        } else {
            NPM
        };
        assert_eq!(
            lockfile(kind, text).unwrap().out_of_sync(&manifest),
            [DependencyKind::Development]
        );
    }
}
