//! Process-level contracts of `frameshift inspect` (`D-075`).
//!
//! The inspect fixtures below `fixtures/discovery/` are materialized in
//! Cargo's temporary directory with the inputs that `fixture.json` creates
//! at test time (manifests naming the real `next` package, `Q-014`, and
//! symbolic links, without which a case is skipped on hosts that cannot
//! create them). Text
//! output is compared with `expected/inspection.txt`, which changes only
//! through the ignored `update_inspect_text_goldens` test, and JSON output
//! with the engine's `expected/inspection.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    JsonDialect, JsonDocument, JsonId, JsonLimits, JsonValue, parse_json,
};

const PLACEHOLDER: &str = "@ENGINE_VERSION@";

fn invoke(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/discovery")
}

fn scratch(name: &str) -> PathBuf {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cli-inspect")
        .join(name);
    if let Ok(existing) = fs::canonicalize(&directory) {
        fs::remove_dir_all(existing).unwrap();
    }
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn member<'a>(document: &'a JsonDocument<'_>, object: JsonId, key: &str) -> Option<JsonValue<'a>> {
    let Some(JsonValue::Object(object)) = document.value(object) else {
        return None;
    };
    document.value(object.get(key)?)
}

fn text<'a>(document: &'a JsonDocument<'_>, object: JsonId, key: &str) -> Option<&'a str> {
    match member(document, object, key)? {
        JsonValue::String(text) => Some(text),
        JsonValue::Null
        | JsonValue::Bool(_)
        | JsonValue::Number(_)
        | JsonValue::Array(_)
        | JsonValue::Object(_) => None,
    }
}

/// The names of the inspect cases, sorted.
fn cases() -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(fixtures()).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let metadata = fs::read_to_string(fixtures().join(&name).join("fixture.json")).unwrap();
        if metadata.contains("\"kind\": \"inspect\"") {
            names.push(name);
        }
    }
    names.sort();
    assert!(!names.is_empty());
    names
}

/// Copies the case's `source/` and creates its test-time files,
/// directories, and links in a scratch directory of its own per test, then
/// returns the root, or `None` when the host cannot create a link (Windows
/// without the privilege).
fn prepare(name: &str, test: &str) -> Option<PathBuf> {
    let case = fixtures().join(name);
    let root = scratch(&format!("{test}-{name}"));
    copy_tree(&case.join("source"), &root);
    let metadata = fs::read_to_string(case.join("fixture.json")).unwrap();
    let source = SourceText::new(metadata, TextLimits::default()).unwrap();
    let document = parse_json(&source, JsonDialect::Json, JsonLimits::default()).unwrap();
    let Some(JsonValue::Object(fields)) = document.value(document.root()) else {
        panic!("{name}: fixture.json is not an object");
    };
    let Some(created) = fields.get("createdAtTestTime") else {
        return Some(root);
    };
    let Some(JsonValue::Array(items)) = document.value(created) else {
        panic!("{name}: createdAtTestTime is not an array");
    };
    for item in items.iter() {
        let path = root.join(text(&document, item, "path").unwrap());
        match text(&document, item, "kind") {
            Some("directory") => fs::create_dir_all(path).unwrap(),
            Some("file") => {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text(&document, item, "content").unwrap()).unwrap();
            }
            Some("symlink") => {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                if let Err(error) = symlink(&path, text(&document, item, "target").unwrap()) {
                    if !cfg!(windows) {
                        panic!("{name}: {error}");
                    }
                    eprintln!("skipping {name}: {error}");
                    return None;
                }
            }
            other => panic!("{name}: unexpected input kind {other:?}"),
        }
    }
    Some(root)
}

#[cfg(unix)]
fn symlink(link: &Path, target: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink(link: &Path, target: &str) -> std::io::Result<()> {
    if link.parent().unwrap().join(target).is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

fn expected(name: &str, file: &str) -> Vec<u8> {
    fs::read_to_string(fixtures().join(name).join("expected").join(file))
        .unwrap()
        .replace(
            PLACEHOLDER,
            frameshift_engine::version::info().engine_version,
        )
        .into_bytes()
}

fn inspect(root: &Path, extra: &[&str]) -> Output {
    let mut args: Vec<&std::ffi::OsStr> = vec!["inspect".as_ref(), root.as_os_str()];
    args.extend(extra.iter().map(std::ffi::OsStr::new));
    invoke(&args)
}

#[test]
fn inspect_fixtures_match_text_and_json_goldens() {
    for name in cases() {
        let Some(root) = prepare(&name, "goldens") else {
            continue;
        };
        let output = inspect(&root, &[]);
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert!(output.stderr.is_empty(), "{name}");
        assert_eq!(output.stdout, expected(&name, "inspection.txt"), "{name}");
        let json = inspect(&root, &["--format", "json"]);
        assert_eq!(json.status.code(), Some(0), "{name}");
        assert_eq!(json.stdout, expected(&name, "inspection.json"), "{name}");
        // Repeated runs are byte-identical.
        assert_eq!(inspect(&root, &[]).stdout, output.stdout, "{name}");
    }
}

#[test]
fn package_selection_and_current_directory() {
    let root = prepare("inspect-next-workspace", "selection").unwrap();
    let output = inspect(&root, &["--package", "apps/site"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\nPackage        apps/site (fictional-site)\n"));
    assert!(!text.contains("\nPackage        apps/docs"));
    assert!(text.contains("App Router in apps/site/app (1 route file); Pages Router"));

    let output = inspect(&root, &["--package=."]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\nPackage        . (fictional-harbor)\n"));
    assert!(!text.contains("\nPackage        apps/"));

    // Without a source, the current directory is inspected.
    let from_inside = Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .args(["inspect", "--format", "json"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(from_inside.status.code(), Some(0));
    assert_eq!(
        from_inside.stdout,
        expected("inspect-next-workspace", "inspection.json")
    );
}

#[test]
fn input_failures_exit_with_three_and_an_error_document() {
    let base = scratch("failures");
    fs::write(base.join("file.txt"), "").unwrap();
    let root = prepare("inspect-next-workspace", "failures").unwrap();
    let cases: Vec<(PathBuf, &[&str])> = vec![
        (base.join("missing"), &[]),
        (base.join("file.txt"), &[]),
        (root.clone(), &["--package", "apps"]),
        (root, &["--package", "apps/missing"]),
    ];
    for (source, extra) in cases {
        let output = inspect(&source, extra);
        assert_eq!(output.status.code(), Some(3), "{source:?} {extra:?}");
        assert!(output.stdout.is_empty());
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(message.starts_with("error: ") && message.ends_with('\n'));
        // Messages name categories, never the source path.
        assert!(!message.contains("missing") && !message.contains("file.txt"));

        let mut json_args = extra.to_vec();
        json_args.extend(["--format", "json"]);
        let json = inspect(&source, &json_args);
        assert_eq!(json.status.code(), Some(3));
        let document = String::from_utf8(json.stdout).unwrap();
        assert!(document.starts_with(
            "{\n  \"kind\": \"frameshift.error\",\n  \"schemaVersion\": 1,\n  \"exitCode\": 3,\n"
        ));
        assert!(document.ends_with("\"\n}\n"));
    }
}

#[test]
fn invalid_inspect_arguments_are_usage_errors() {
    for args in [
        &["inspect", "a", "b"][..],
        &["inspect", "--package", "apps\\web"],
        &["inspect", "--package", "apps/"],
        &["inspect", "--package"],
        &["inspect", "a", "--help"],
        &["version", "--package", "."],
        &["help", "--package", "."],
    ] {
        let args: Vec<&std::ffi::OsStr> = args.iter().map(|arg| arg.as_ref()).collect();
        let output = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(output.stderr.starts_with(b"error: "), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_non_unicode_source_is_a_native_path() {
    use std::os::unix::ffi::OsStringExt;
    let source = std::ffi::OsString::from_vec(vec![b'm', 0xff]);
    let output = invoke(&["inspect".as_ref(), source.as_os_str()]);
    // The path does not exist: an input error, not a usage error.
    assert_eq!(output.status.code(), Some(3));
}

#[cfg(windows)]
#[test]
fn a_non_unicode_source_is_a_native_path() {
    use std::os::windows::ffi::OsStringExt;
    let source = std::ffi::OsString::from_wide(&[u16::from(b'm'), 0xd800]);
    let output = invoke(&["inspect".as_ref(), source.as_os_str()]);
    assert_eq!(output.status.code(), Some(3));
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-cli --test inspect -- --ignored update_inspect_text_goldens"]
fn update_inspect_text_goldens() {
    let version = frameshift_engine::version::info().engine_version;
    for name in cases() {
        let Some(root) = prepare(&name, "update") else {
            continue;
        };
        let output = inspect(&root, &[]);
        assert_eq!(output.status.code(), Some(0), "{name}");
        let first = format!("Frameshift {version}  inspect\n");
        let text = String::from_utf8(output.stdout).unwrap().replacen(
            &first,
            &format!("Frameshift {PLACEHOLDER}  inspect\n"),
            1,
        );
        let golden = fixtures()
            .join(&name)
            .join("expected")
            .join("inspection.txt");
        fs::write(golden, text).unwrap();
    }
}
