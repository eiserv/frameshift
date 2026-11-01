//! `frameshift.jsonc` parsing and loading (`D-027`, `D-103`).

mod support;

use std::fs;
use std::path::PathBuf;

use frameshift_engine::config::{self, CONFIG_FILE, PackageChoice, ProjectConfig};
use frameshift_engine::limits::Limits;

fn parse(text: &str) -> Result<ProjectConfig, String> {
    config::parse(text.as_bytes().to_vec(), &Limits::default()).map_err(|e| e.to_string())
}

fn directory(label: &str) -> PathBuf {
    let path = support::temporary().join("config").join(label);
    if let Ok(existing) = fs::canonicalize(&path) {
        fs::remove_dir_all(existing).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn parses_every_key_with_comments() {
    let config = parse(
        "// Project options\n{\n  \"$schema\": \"./schema.json\",\n  \"to\": \"astro-preact\", /* the target */\n  \"package\": \"apps/web\",\n}\n",
    )
    .unwrap();
    assert!(config.target);
    let Some(PackageChoice::Member(path)) = config.package else {
        panic!("expected a member");
    };
    assert_eq!(path.as_str(), "apps/web");
    assert_eq!(parse("{}").unwrap(), ProjectConfig::default());
    assert_eq!(
        parse("{ \"package\": \".\" }").unwrap().package,
        Some(PackageChoice::Root)
    );
}

#[test]
fn rejects_everything_else_without_echoing_it() {
    for text in [
        "",
        "[]",
        "{ \"allowExec\": true }",
        "{ \"out\": \"secret-name\" }",
        "{ \"onExisting\": \"replace-managed\" }",
        "{ \"to\": \"secret-name\" }",
        "{ \"to\": 1 }",
        "{ \"$schema\": 1 }",
        "{ \"package\": 1 }",
        "{ \"package\": \"../secret-name\" }",
        "{ \"package\": \"/secret-name\" }",
        "{ \"to\": \"astro-preact\", \"to\": \"astro-preact\" }",
        "{ \"to\": \"astro-preact\"",
    ] {
        let error = parse(text).expect_err(text);
        assert!(!error.contains("secret-name"), "{text}: {error}");
    }
}

#[test]
fn loads_only_an_admitted_root_file() {
    let limits = Limits::default();
    let root = directory("load");
    assert_eq!(config::load(&root, &limits), Ok(None));
    fs::write(root.join(CONFIG_FILE), "{ \"to\": \"astro-preact\" }").unwrap();
    assert!(config::load(&root, &limits).unwrap().unwrap().target);
    fs::write(root.join(CONFIG_FILE), "{ \"unknown\": 1 }").unwrap();
    assert!(config::load(&root, &limits).is_err());
    // An ignored file is not silently treated as absent.
    fs::write(root.join(CONFIG_FILE), "{}").unwrap();
    fs::write(root.join(".gitignore"), "frameshift.jsonc\n").unwrap();
    assert!(config::load(&root, &limits).is_err());
    fs::remove_dir_all(&root).unwrap();
    assert!(config::load(&root, &limits).is_err());
}
