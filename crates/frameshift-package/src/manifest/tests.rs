use frameshift_core::{SourceText, TextLimits};
use frameshift_syntax::{JsonDialect, JsonLimits, parse_json};

use super::*;

fn manifest(text: &str) -> PackageManifest {
    let source = SourceText::new(text.to_owned(), TextLimits::default()).unwrap();
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default()).unwrap();
    PackageManifest::from_json(&document).unwrap()
}

fn slice(text: &str, span: Span) -> &str {
    &text[span.start()..span.end()]
}

#[test]
fn models_identification_fields() {
    let text = r#"{
  "name": "lumen-site",
  "version": "1.2.3-rc.1",
  "private": true,
  "type": "module",
  "packageManager": "pnpm@10.14.0",
  "engines": { "node": ">=20" },
  "workspaces": ["apps/*", "packages/*"],
  "scripts": { "dev": "next dev", "build": "next build" },
  "dependencies": { "next": "16.2.1", "react": "^19.0.0", "ui": "workspace:*" },
  "devDependencies": { "typescript": "~6.0.3" },
  "peerDependencies": { "react-dom": ">=19" },
  "optionalDependencies": { "fsevents": "*" },
  "peerDependenciesMeta": { "react-dom": { "optional": true }, "other": {} },
  "description": "fictional"
}"#;
    let manifest = manifest(text);
    assert!(manifest.issues.is_empty(), "{:?}", manifest.issues);
    assert_eq!(manifest.name.as_ref().unwrap().value.as_ref(), "lumen-site");
    let version = manifest.version.as_ref().unwrap();
    assert_eq!(slice(text, version.span), "\"1.2.3-rc.1\"");
    assert_eq!(slice(text, version.key_span), "\"version\"");
    assert_eq!(manifest.parsed_version().unwrap().to_string(), "1.2.3-rc.1");
    assert!(manifest.private.as_ref().unwrap().value);
    assert_eq!(
        manifest.module_type.as_ref().unwrap().value,
        ModuleType::Module
    );
    assert_eq!(
        manifest.package_manager.as_ref().unwrap().value.as_ref(),
        "pnpm@10.14.0"
    );
    assert_eq!(manifest.engines[0].value.as_ref(), ">=20");
    let workspaces = &manifest.workspaces.as_ref().unwrap().value;
    assert_eq!(workspaces.len(), 2);
    assert_eq!(slice(text, workspaces[1].span), "\"packages/*\"");
    assert_eq!(manifest.scripts.len(), 2);
    let names: Vec<&str> = manifest
        .dependencies
        .iter()
        .map(|entry| &*entry.key)
        .collect();
    assert_eq!(names, ["next", "react", "ui"]);
    assert_eq!(
        manifest.dependencies[0]
            .value
            .spec
            .range()
            .unwrap()
            .to_string(),
        "16.2.1"
    );
    assert_eq!(
        manifest.dependencies[2].value.spec,
        DependencySpec::Workspace("*".into())
    );
    assert_eq!(
        manifest.dependencies_of(DependencyKind::Development).len(),
        1
    );
    assert_eq!(manifest.dependencies_of(DependencyKind::Peer).len(), 1);
    assert_eq!(manifest.dependencies_of(DependencyKind::Optional).len(), 1);
    let optional: Vec<(&str, bool)> = manifest
        .optional_peers
        .iter()
        .map(|entry| (&*entry.key, entry.value))
        .collect();
    assert_eq!(optional, [("react-dom", true), ("other", false)]);
    assert_eq!(manifest.other.len(), 1);
    assert_eq!(&*manifest.other[0].key, "description");
}

#[test]
fn models_entry_point_and_override_fields() {
    let text = r#"{
  "exports": { ".": { "import": "./a.mjs", "default": ["./b.js", null] }, "./x": null },
  "main": "index.js",
  "module": "index.mjs",
  "browser": { "./node.js": "./web.js", "fs": false },
  "bin": { "tool": "bin/tool.js" },
  "files": ["dist"],
  "browserslist": { "production": ["> 0.5%"], "development": "last 1 chrome version" },
  "overrides": { "a": "1.0.0", "b": { ".": "2.0.0", "c": "$c" } },
  "resolutions": { "d": "3.0.0" },
  "pnpm": { "overrides": { "e": "4.0.0" }, "neverBuiltDependencies": [] },
  "workspaces": { "packages": ["libs/*"], "nohoist": ["**/x"] }
}"#;
    let manifest = manifest(text);
    assert!(manifest.issues.is_empty(), "{:?}", manifest.issues);
    let Exports::Subpaths(subpaths) = &manifest.exports.as_ref().unwrap().value else {
        panic!("subpaths expected");
    };
    assert_eq!(subpaths.len(), 2);
    let Exports::Conditions(conditions) = &subpaths[0].value else {
        panic!("conditions expected");
    };
    assert_eq!(&*conditions[1].key, "default");
    assert!(
        matches!(&conditions[1].value, Exports::Array(elements) if elements[1].value == Exports::Null)
    );
    assert_eq!(subpaths[1].value, Exports::Null);
    assert_eq!(manifest.main.as_ref().unwrap().value.as_ref(), "index.js");
    assert_eq!(
        manifest.module.as_ref().unwrap().value.as_ref(),
        "index.mjs"
    );
    let Browser::Map(browser) = &manifest.browser.as_ref().unwrap().value else {
        panic!("map expected");
    };
    assert_eq!(browser[1].value, BrowserTarget::Disabled);
    assert!(
        matches!(&manifest.bin.as_ref().unwrap().value, Bin::Map(entries) if entries.len() == 1)
    );
    assert_eq!(manifest.files.as_ref().unwrap().value.len(), 1);
    let Browserslist::Environments(environments) = &manifest.browserslist.as_ref().unwrap().value
    else {
        panic!("environments expected");
    };
    assert_eq!(
        environments[1].value[0].value.as_ref(),
        "last 1 chrome version"
    );
    let Override::Nested(nested) = &manifest.overrides[1].value else {
        panic!("nested expected");
    };
    assert_eq!(nested[1].value, Override::Spec("$c".into()));
    assert_eq!(manifest.resolutions[0].value.as_ref(), "3.0.0");
    assert_eq!(manifest.pnpm_overrides[0].value.as_ref(), "4.0.0");
    assert_eq!(
        manifest.workspaces.as_ref().unwrap().value[0]
            .value
            .as_ref(),
        "libs/*"
    );
}

#[test]
fn reports_unexpected_types_and_values() {
    let text = r#"{
  "name": 7,
  "type": "esm",
  "dependencies": { "a": 1, "b": "^1", "c": "1.2.3beta", "d": "has space" },
  "devDependencies": [],
  "workspaces": ["ok", 3],
  "exports": { ".": "./a.js", "import": "./b.js" },
  "peerDependenciesMeta": { "x": { "optional": "yes" } },
  "browser": 1,
  "packageManager": false
}"#;
    let manifest = manifest(text);
    let issues: Vec<(ManifestField, IssueKind, &str)> = manifest
        .issues
        .iter()
        .map(|issue| (issue.field, issue.kind, slice(text, issue.span)))
        .collect();
    assert_eq!(
        issues,
        [
            (ManifestField::Name, IssueKind::UnexpectedType, "7"),
            (ManifestField::Type, IssueKind::InvalidValue, "\"esm\""),
            (ManifestField::Dependencies, IssueKind::UnexpectedType, "1"),
            (
                ManifestField::Dependencies,
                IssueKind::InvalidValue,
                "\"1.2.3beta\""
            ),
            (
                ManifestField::Dependencies,
                IssueKind::InvalidValue,
                "\"has space\""
            ),
            (
                ManifestField::DevDependencies,
                IssueKind::UnexpectedType,
                "[]"
            ),
            (ManifestField::Workspaces, IssueKind::UnexpectedType, "3"),
            (
                ManifestField::Exports,
                IssueKind::InvalidValue,
                "{ \".\": \"./a.js\", \"import\": \"./b.js\" }"
            ),
            (
                ManifestField::PeerDependenciesMeta,
                IssueKind::UnexpectedType,
                "\"yes\""
            ),
            (ManifestField::Browser, IssueKind::UnexpectedType, "1"),
            (
                ManifestField::PackageManager,
                IssueKind::UnexpectedType,
                "false"
            ),
        ]
    );
    assert!(manifest.name.is_none());
    assert!(manifest.module_type.is_none());
    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(manifest.workspaces.as_ref().unwrap().value.len(), 1);
    assert!(manifest.exports.is_none());
}

#[test]
fn uses_the_last_duplicate_and_rejects_non_objects() {
    let manifest =
        manifest(r#"{ "name": "first", "dependencies": { "a": "1", "a": "2" }, "name": "last" }"#);
    assert_eq!(manifest.name.unwrap().value.as_ref(), "last");
    assert_eq!(manifest.dependencies.len(), 1);
    assert_eq!(manifest.dependencies[0].value.text.as_ref(), "2");
    let source = SourceText::new("[]".to_owned(), TextLimits::default()).unwrap();
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default()).unwrap();
    assert_eq!(
        PackageManifest::from_json(&document),
        Err(ManifestError::NotAnObject {
            span: Span::new(0, 2).unwrap()
        })
    );
    assert_eq!(ManifestField::ALL.len(), 23);
    for field in ManifestField::ALL {
        assert_eq!(ManifestField::from_key(field.key()), Some(field));
    }
}
