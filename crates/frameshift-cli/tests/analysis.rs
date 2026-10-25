//! CLI streams, statuses, read-only analysis, report projections, and output safety.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "frameshift-cli-analysis-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("package.json"),
        format!(
            "{{\"name\":\"fictional-site\",\"dependencies\":{{\"{}\":\"16.2.0\"}}}}",
            "next"
        ),
    )
    .unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Page(){return <p>Hello</p>}",
    )
    .unwrap();
    fs::write(
        root.join("app/layout.tsx"),
        "export default function Layout({children}){return <html><body>{children}</body></html>}",
    )
    .unwrap();
    root
}
fn call(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_frameshift"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn analysis_json_exit_conditions_and_stored_report_projections() {
    let root = fixture("streams");
    let original = fs::read(root.join("app/page.tsx")).unwrap();
    let a = call(&root, &["analyze", "--to", "astro-preact", "--format=json"]);
    assert_eq!(
        a.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&a.stderr)
    );
    assert!(a.stderr.is_empty());
    let b = call(&root, &["--format=json", "analyze", "--to=astro-preact"]);
    assert_eq!(a.stdout, b.stdout);
    let document =
        std::env::temp_dir().join(format!("frameshift-cli-report-{}.json", std::process::id()));
    let _ = fs::remove_file(&document);
    let written = call(
        &root,
        &[
            "analyze",
            "--to",
            "astro-preact",
            "--report",
            document.to_str().unwrap(),
        ],
    );
    assert!(written.status.success());
    let json_output = call(&root, &["analyze", "--to=astro-preact", "--format=json"]);
    assert_eq!(json_output.stdout, fs::read(&document).unwrap());
    let report = call(
        &root,
        &["report", document.to_str().unwrap(), "--section", "routes"],
    );
    assert!(report.status.success());
    assert!(String::from_utf8_lossy(&report.stdout).contains("route:/"));
    let filter = call(
        &root,
        &[
            "report",
            document.to_str().unwrap(),
            "--section",
            "units",
            "--route",
            "/",
            "--format",
            "json",
        ],
    );
    assert!(filter.status.success());
    assert!(filter.stdout.starts_with(b"["));
    let unchanged = fs::read(&document).unwrap();
    let refused = call(
        &root,
        &[
            "analyze",
            "--to",
            "astro-preact",
            "--report",
            document.to_str().unwrap(),
        ],
    );
    assert_eq!(refused.status.code(), Some(4));
    assert_eq!(fs::read(&document).unwrap(), unchanged);
    fs::write(
        root.join("app/page.tsx"),
        "'use client';export default function Page(){return <p>Hello</p>}",
    )
    .unwrap();
    let failed = call(
        &root,
        &["analyze", "--to", "astro-preact", "--fail-on", "review"],
    );
    assert_eq!(failed.status.code(), Some(1));
    fs::write(root.join("app/page.tsx"), &original).unwrap();
    assert!(!failed.stdout.is_empty());
    assert_eq!(fs::read(root.join("app/page.tsx")).unwrap(), original);
    let _ = fs::remove_file(document);
}
#[test]
fn fail_on_review_includes_findings_without_a_counting_unit() {
    let root = fixture("unowned-review");
    let report = call(&root, &["analyze", "--to=astro-preact", "--format=json"]);
    assert!(report.status.success());
    let parsed = frameshift_engine::analysis::stored::StoredReport::parse(
        report.stdout,
        &frameshift_engine::limits::Limits::default(),
    )
    .unwrap();
    let value = parsed.value();
    assert!(
        value
            .get("units")
            .unwrap()
            .array()
            .iter()
            .all(|u| u.get("status").and_then(|s| s.text()) == Some("auto"))
    );
    assert!(value.get("findings").unwrap().array().iter().any(|f| {
        f.get("code").and_then(|s| s.text()) == Some("FS1042") && f.get("unit").is_none()
    }));
    let failed = call(&root, &["analyze", "--to=astro-preact", "--fail-on=review"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(!failed.stdout.is_empty());
    let unmatched = call(
        &root,
        &[
            "analyze",
            "--to=astro-preact",
            "--fail-on=unsupported,unknown,blocked-routes",
        ],
    );
    assert_eq!(unmatched.status.code(), Some(0));
}
#[test]
fn blockers_are_successful_assessments_until_fail_on_requests_failure() {
    let root = fixture("blocker");
    fs::write(root.join("app/page.tsx"),"import {cookies} from 'next/headers';export default function Page(){cookies();return <p/>}").unwrap();
    for (args, code) in [
        (vec!["analyze", "--to", "astro-preact"], 0),
        (
            vec![
                "analyze",
                "--to",
                "astro-preact",
                "--fail-on=blocked-routes",
            ],
            1,
        ),
        (
            vec![
                "analyze",
                "--to",
                "astro-preact",
                "--fail-on=unsupported",
                "--format=json",
            ],
            1,
        ),
    ] {
        let result = call(&root, &args);
        assert_eq!(result.status.code(), Some(code));
        assert!(!result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}
#[test]
fn invalid_options_and_reports_fail_without_echoing_untrusted_arguments() {
    let root = fixture("invalid");
    for args in [
        vec!["analyze"],
        vec!["analyze", "--to", "hostile\u{1b}[31m"],
        vec!["analyze", "--to=astro-preact", "--fail-on=unknown,unknown"],
        vec!["analyze", "--to=astro-preact", "--all", "--all"],
        vec!["analyze", "--to=astro-preact", "--section=units"],
        vec!["explain", "hostile\u{1b}[31m"],
        vec!["report", "missing.json", "--severity=hostile"],
        vec!["rules", "--format=json"],
        vec!["help", "analyze", "extra"],
    ] {
        let result = call(&root, &args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(!result.stderr.contains(&27));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("hostile"));
    }
    fs::write(
        root.join("bad.json"),
        "{\"kind\":\"frameshift.analysis\",\"schemaVersion\":999}",
    )
    .unwrap();
    let result = call(&root, &["report", "bad.json", "--format=json"]);
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stdout).contains("frameshift.error"));
}
#[test]
fn explanations_and_help_are_available_without_a_source_project() {
    let root = fixture("explain");
    for args in [
        vec!["explain", "FS3101"],
        vec!["explain", "nextjs.api.request"],
        vec![
            "explain",
            "nextjs-to-astro-preact/assessment.nextjs-api-request",
        ],
        vec!["rules"],
        vec!["help", "analyze"],
        vec!["analyze", "--help"],
        vec!["--help", "report"],
    ] {
        let result = call(&root, &args);
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!result.stdout.is_empty());
        assert!(result.stderr.is_empty())
    }
}
#[test]
fn markdown_and_terminal_output_do_not_interpret_source_controls() {
    let root = fixture("markdown");
    fs::write(
        root.join("app/page.tsx"),
        "import X from '../missing';export default function Page(){return <X/>}",
    )
    .unwrap();
    let json = call(&root, &["analyze", "--to=astro-preact", "--format=json"]);
    assert!(json.status.success());
    fs::write(root.join("report.json"), json.stdout).unwrap();
    let markdown = call(&root, &["report", "report.json", "--format=markdown"]);
    assert!(markdown.status.success());
    let text = String::from_utf8(markdown.stdout).unwrap();
    assert!(text.starts_with("# Frameshift analysis"));
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("```text"));
}

#[test]
fn explicit_report_output_cannot_add_files_inside_the_source_root() {
    let root = fixture("containment");
    let result = call(
        &root,
        &["analyze", "--to=astro-preact", "--report=new-report.json"],
    );
    assert_eq!(result.status.code(), Some(4));
    assert!(!root.join("new-report.json").exists());
}
