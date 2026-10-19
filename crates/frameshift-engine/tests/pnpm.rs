//! pnpm 9.0 lockfile contracts through the engine facade (`D-071`).

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::package::{DependencyKind, Lockfile, LockfileErrorKind};
use frameshift_engine::syntax::{YamlLimits, parse_yaml};

mod support;

const LOCK: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      fictional-view:
        specifier: ^1
        version: 1.1.0(fictional-peer@2.0.0)
      fictional-alias:
        specifier: npm:fictional-leaf@1
        version: fictional-leaf@1.0.0
      fictional-sibling:
        specifier: workspace:*
        version: link:packages/side
  packages/side:
    devDependencies:
      fictional-view:
        specifier: ^1
        version: 1.1.0(fictional-peer@3.0.0)
packages:
  fictional-view@1.1.0:
    resolution: {integrity: sha512-fictional}
    peerDependencies: {fictional-peer: '*'}
    peerDependenciesMeta: {fictional-peer: {optional: true}}
  fictional-leaf@1.0.0:
    resolution: {integrity: sha512-fictional}
  fictional-leaf@2.0.0:
    resolution: {integrity: sha512-fictional}
snapshots:
  fictional-view@1.1.0(fictional-peer@2.0.0):
    dependencies: {fictional-leaf: 1.0.0}
  fictional-view@1.1.0(fictional-peer@3.0.0):
    optionalDependencies: {fictional-leaf: 2.0.0}
    optional: true
  fictional-leaf@1.0.0: {}
  fictional-leaf@2.0.0: {}
";

fn read(text: &str) -> Result<Lockfile, frameshift_engine::package::LockfileError> {
    let source = SourceText::new(text.into(), TextLimits::default()).unwrap();
    Lockfile::from_yaml(&parse_yaml(&source, YamlLimits::default()).unwrap())
}

#[test]
fn importers_aliases_peer_contexts_and_transitive_versions_stay_distinct() {
    let lockfile = read(LOCK).unwrap();
    assert_eq!(lockfile.version(), 9);
    assert_eq!(
        lockfile
            .workspace("")
            .unwrap()
            .declared(DependencyKind::Production)
            .len(),
        3
    );
    let root = lockfile.resolve("", "fictional-view").unwrap();
    let sibling = lockfile.resolve("packages/side", "fictional-view").unwrap();
    assert_ne!(root.key, sibling.key);
    assert_eq!(root.version().unwrap().to_string(), "1.1.0");
    assert_eq!(
        lockfile
            .resolve_from(root, "fictional-leaf")
            .unwrap()
            .version()
            .unwrap()
            .to_string(),
        "1.0.0"
    );
    assert_eq!(
        lockfile
            .resolve_from(sibling, "fictional-leaf")
            .unwrap()
            .version()
            .unwrap()
            .to_string(),
        "2.0.0"
    );
    assert!(sibling.optional);
    assert_eq!(&*root.optional_peers[0], "fictional-peer");
    assert_eq!(
        lockfile
            .resolve("", "fictional-alias")
            .unwrap()
            .name
            .as_ref(),
        "fictional-leaf"
    );
    assert!(lockfile.resolve("", "fictional-sibling").is_none());
    assert_eq!(
        lockfile.pnpm_reference("", "fictional-sibling"),
        Some("link:packages/side")
    );
    assert!(lockfile.resolve("missing", "fictional-view").is_none());
    let self_link = read(&LOCK.replace("version: link:packages/side", "version: 'link:'")).unwrap();
    assert_eq!(
        self_link.pnpm_reference("", "fictional-sibling"),
        Some("link:")
    );
    assert!(self_link.resolve("", "fictional-sibling").is_none());
}

#[test]
fn invalid_structure_and_unknown_versions_reject_the_whole_lockfile() {
    for (old, new) in [("'9.0'", "'6.0'"), ("'9.0'", "'10.0'"), ("'9.0'", "'9.1'")] {
        assert_eq!(
            read(&LOCK.replacen(old, new, 1)).unwrap_err().kind,
            LockfileErrorKind::UnsupportedVersion
        );
    }
    for (old, new) in [
        ("importers:", "importers: []\nunused:"),
        ("specifier: ^1", "specifier: false"),
        (
            "version: fictional-leaf@1.0.0",
            "version: fictional-leaf@9.0.0",
        ),
        ("optional: true", "optional: 'true'"),
        ("fictional-leaf: 2.0.0", "fictional-leaf: 9.0.0"),
        (
            "resolution: {integrity: sha512-fictional}",
            "resolution: {integrity: 42}",
        ),
        ("specifier: ^1", "specifier: ^1\n        specifier: ^2"),
        ("importers:", "importers: !custom"),
        ("version: link:packages/side", "version: false"),
    ] {
        let text = LOCK.replacen(old, new, 1);
        let error = read(&text).unwrap_err();
        assert_eq!(error.kind, LockfileErrorKind::Structure, "{old}");
        assert!(text.is_char_boundary(error.span.start()));
        assert!(text.is_char_boundary(error.span.end()));
        assert!(!error.to_string().contains("fictional"));
    }
    assert_eq!(read("").unwrap_err().kind, LockfileErrorKind::Structure);
    assert_eq!(
        read("lockfileVersion: '9.0'\n---\n{}").unwrap_err().kind,
        LockfileErrorKind::Structure
    );
}

#[test]
fn yaml_properties_and_flow_style_preserve_package_facts() {
    let anchored = LOCK
        .replace("specifier: ^1", "specifier: !!str ^1")
        .replacen(
            "resolution: {integrity: sha512-fictional}",
            "resolution: &resolution {integrity: sha512-fictional}",
            1,
        )
        .replace(
            "resolution: {integrity: sha512-fictional}",
            "resolution: *resolution",
        );
    let plain = read(LOCK).unwrap();
    let with_properties = read(&anchored).unwrap();
    assert_eq!(plain.workspaces(), with_properties.workspaces());
    for (left, right) in plain.packages().iter().zip(with_properties.packages()) {
        let mut right = right.clone();
        right.span = left.span;
        assert_eq!(*left, right);
    }
}

#[test]
#[ignore = "requires an explicit external reference lockfile in FRAMESHIFT_PNPM_LOCKFILE"]
fn external_reference_lockfile() {
    let path =
        std::env::var_os("FRAMESHIFT_PNPM_LOCKFILE").expect("external reference lockfile path");
    let text = std::fs::read_to_string(path).unwrap();
    let lockfile = read(&text).unwrap();
    eprintln!(
        "pnpm reference: {} workspaces, {} snapshots",
        lockfile.workspaces().len(),
        lockfile.packages().len()
    );
    if let Some(path) = std::env::var_os("FRAMESHIFT_PNPM_ORACLE") {
        let text = std::fs::read_to_string(path).unwrap();
        let support::Json::Array(records) = support::parse(&text).unwrap() else {
            panic!("oracle rows");
        };
        for record in &records {
            let support::Json::Array(fields) = record else {
                panic!("oracle fields");
            };
            let scope = fields[0].as_str().unwrap();
            let context = fields[1].as_str().unwrap();
            let name = fields[2].as_str().unwrap();
            let reference = fields[3].as_str().unwrap();
            let actual = if scope == "workspace" {
                assert_eq!(lockfile.pnpm_reference(context, name), Some(reference));
                lockfile.resolve(context, name)
            } else {
                lockfile.resolve_from(lockfile.package(context).unwrap(), name)
            };
            assert_eq!(
                actual.map(|package| package.key.as_ref()),
                fields[4].as_str()
            );
        }
        eprintln!("{} oracle lookups match", records.len());
    }
}
