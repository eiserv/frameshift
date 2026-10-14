//! Process-level contracts for the CLI's streams, statuses, and golden output.

use std::{
    path::PathBuf,
    process::{Command, Output},
};

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn expected(name: &str) -> Vec<u8> {
    let info = frameshift_engine::version::info();
    std::fs::read_to_string(fixture_path(name))
        .unwrap()
        .replace("@ENGINE_VERSION@", info.engine_version)
        .replace("@TOOLCHAIN@", info.toolchain)
        .replace(
            "@BUILD_COMMIT_TEXT@",
            info.build_commit.unwrap_or("unknown"),
        )
        .replace(
            "@BUILD_COMMIT_JSON@",
            &info
                .build_commit
                .map_or_else(|| "null".to_owned(), |commit| format!("\"{commit}\"")),
        )
        .into_bytes()
}

const GOLDENS: &[(&[&str], &str, i32)] = &[
    (&["help"], "help.txt", 0),
    (&["help", "help"], "help-help.txt", 0),
    (&["help", "version"], "help-version.txt", 0),
    (&["version"], "version.txt", 0),
    (&["version", "--format", "json"], "version.json", 0),
    (&["--version"], "short-version.txt", 0),
    (&["migrate", "--format", "json"], "usage-error.json", 2),
];

#[test]
fn command_output_matches_goldens_and_repeats_byte_for_byte() {
    for &(arguments, name, status) in GOLDENS {
        let first = invoke(arguments);
        assert_eq!(first.status.code(), Some(status), "{arguments:?}");
        assert_eq!(first.stdout, expected(name), "{arguments:?}");
        if status == 0 {
            assert!(first.stderr.is_empty(), "{arguments:?}");
        } else {
            assert_eq!(
                first.stderr,
                b"error: unknown or unavailable command; run 'frameshift help'\n"
            );
        }
        let second = invoke(arguments);
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(first.stderr, second.stderr);
        assert_eq!(first.status.code(), second.status.code());
    }
}

#[test]
fn aliases_and_global_format_position_have_identical_output() {
    for arguments in [&[][..], &["-h"], &["--help"], &["help"]] {
        let output = invoke(arguments);
        assert!(output.status.success());
        assert_eq!(output.stdout, expected("help.txt"));
        assert!(output.stderr.is_empty());
    }
    for arguments in [&["version", "-h"][..], &["version", "--help"]] {
        let output = invoke(arguments);
        assert!(output.status.success());
        assert_eq!(output.stdout, expected("help-version.txt"));
        assert!(output.stderr.is_empty());
    }
    for arguments in [
        &["--format", "json", "version"][..],
        &["--format=json", "version"],
        &["version", "--format=json"],
    ] {
        let output = invoke(arguments);
        assert!(output.status.success());
        assert_eq!(output.stdout, expected("version.json"));
        assert!(output.stderr.is_empty());
    }
    assert_eq!(
        invoke(&["version", "--format", "text"]).stdout,
        expected("version.txt")
    );
}

#[test]
fn invalid_arguments_and_planned_commands_fail_without_echoing_input() {
    let cases: &[&[&str]] = &[
        &["inspect"],
        &["analyze"],
        &["plan"],
        &["migrate"],
        &["verify"],
        &["report"],
        &["rules"],
        &["explain"],
        &["help", "migrate"],
        &["version", "extra"],
        &["help", "help", "extra"],
        &["--version", "version"],
        &["--version", "--help"],
        &["--version", "--format", "text"],
        &["--version", "--version"],
        &["version", "--help", "-h"],
        &["version", "--format"],
        &["version", "--format", "yaml"],
        &["version", "--format="],
        &["version", "--format", "text", "--format", "text"],
        &["help", "--format", "text"],
        &["--format", "text"],
        &["version", "--color", "never"],
        &["--quiet"],
        &["--"],
        &["\u{1b}[31msecret-name"],
    ];
    for &arguments in cases {
        let output = invoke(arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert!(output.stderr.starts_with(b"error: "), "{arguments:?}");
        assert!(!output.stderr.contains(&0x1b));
        assert!(
            !String::from_utf8(output.stderr)
                .unwrap()
                .contains("secret-name")
        );
    }
    for arguments in [
        &["--bad", "--format", "json", "version"][..],
        &["help", "--format=json"],
        &["--version", "--format", "json"],
        &["version", "--format", "json", "--format", "text"],
    ] {
        let output = invoke(arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            output
                .stdout
                .starts_with(b"{\n  \"kind\": \"frameshift.error\",")
        );
        assert!(output.stdout.ends_with(b"\n}\n"));
        assert!(output.stderr.starts_with(b"error: "));
    }
}

#[test]
fn runtime_environment_and_current_directory_do_not_change_version() {
    let baseline = invoke(&["version", "--format", "json"]);
    let changed = Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .env("FRAMESHIFT_BUILD_COMMIT", "malicious-runtime-override")
        .args(["version", "--format", "json"])
        .current_dir(std::env::temp_dir())
        .env("FRAMESHIFT_RUSTC_VERSION", "not the build compiler")
        .env("TZ", "Pacific/Honolulu")
        .env("LANG", "tr_TR.UTF-8")
        .env("NO_COLOR", "")
        .output()
        .unwrap();
    assert_eq!(baseline.stdout, changed.stdout);
    assert_eq!(baseline.stderr, changed.stderr);
    assert_eq!(baseline.status.code(), changed.status.code());
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_return_usage_errors_instead_of_panicking() {
    use std::os::unix::ffi::OsStringExt;
    assert_invalid_encoding_is_rejected(std::ffi::OsString::from_vec(vec![0xff]));
}

#[cfg(windows)]
#[test]
fn unpaired_surrogate_arguments_return_usage_errors_instead_of_panicking() {
    use std::os::windows::ffi::OsStringExt;
    assert_invalid_encoding_is_rejected(std::ffi::OsString::from_wide(&[0xd800]));
}

#[cfg(any(unix, windows))]
fn assert_invalid_encoding_is_rejected(invalid: std::ffi::OsString) {
    let output = Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .args(["version", "--format", "json"])
        .arg(&invalid)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output
            .stdout
            .starts_with(b"{\n  \"kind\": \"frameshift.error\",")
    );
    assert_eq!(
        output.stderr,
        b"error: command arguments must be valid UTF-8\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .args(["version", "--format"])
        .arg(invalid)
        .arg("json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"error: command arguments must be valid UTF-8\n"
    );
}

#[test]
#[ignore = "explicit golden update: cargo test -p frameshift-cli --test cli -- --ignored update_golden_files"]
fn update_golden_files() {
    let info = frameshift_engine::version::info();
    std::fs::create_dir_all(fixture_path("")).unwrap();
    for &(arguments, name, status) in GOLDENS {
        let output = invoke(arguments);
        assert_eq!(output.status.code(), Some(status));
        let mut text = String::from_utf8(output.stdout)
            .unwrap()
            .replace(info.toolchain, "@TOOLCHAIN@")
            .replace(info.engine_version, "@ENGINE_VERSION@");
        if name == "version.txt" {
            text = text.replace(
                info.build_commit.unwrap_or("unknown"),
                "@BUILD_COMMIT_TEXT@",
            );
        } else if name == "version.json" {
            let commit = info
                .build_commit
                .map_or_else(|| "null".to_owned(), |commit| format!("\"{commit}\""));
            text = text.replace(
                &format!("\"buildCommit\": {commit}"),
                "\"buildCommit\": @BUILD_COMMIT_JSON@",
            );
        }
        std::fs::write(fixture_path(name), text).unwrap();
    }
}
