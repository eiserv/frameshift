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
fn remote_images_require_review_and_static_imports_have_a_proven_mapping() {
    let root = fixture("image-sources");
    fs::write(root.join("package-lock.json"), format!(
        "{{\"lockfileVersion\":3,\"packages\":{{\"\":{{\"name\":\"fictional-site\",\"dependencies\":{{\"{0}\":\"16.2.0\"}}}},\"node_modules/{0}\":{{\"version\":\"16.2.0\"}}}}}}", "next"
    )).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("assets/photo.png"), "fictional image bytes").unwrap();
    for (config, source, code) in [
        (
            "{images:{unoptimized:true}}",
            "import Image from 'next/image';export default function Page(){return <Image src='/photo.png'/>}",
            0,
        ),
        (
            "{images:{unoptimized:true}}",
            "import Image from 'next/image';export default function Page(){return <Image src='https://example.test/photo.png'/>}",
            1,
        ),
        (
            "{}",
            "import Image from 'next/image';import photo from '../assets/photo.png';export default function Page(){return <Image src={photo}/>}",
            0,
        ),
    ] {
        fs::write(
            root.join("next.config.mjs"),
            format!("export default {config}"),
        )
        .unwrap();
        fs::write(root.join("app/page.tsx"), source).unwrap();
        let output = call(&root, &["analyze", "--to=astro-preact", "--fail-on=review"]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{source}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn independent_dynamic_routes_do_not_trigger_fail_on_review() {
    let root = fixture("dynamic-route");
    fs::write(root.join("package-lock.json"), format!(
        "{{\"lockfileVersion\":3,\"packages\":{{\"\":{{\"name\":\"fictional-site\",\"dependencies\":{{\"{0}\":\"16.2.0\"}}}},\"node_modules/{0}\":{{\"version\":\"16.2.0\"}}}}}}", "next"
    )).unwrap();
    fs::create_dir_all(root.join("app/[slug]")).unwrap();
    for (generator, code) in [
        (
            "export function generateStaticParams(){return [{slug:'a'}]}",
            0,
        ),
        (
            "export function generateStaticParams({params}){return [{slug:params.slug}]}",
            1,
        ),
        (
            "const slug='a';export function generateStaticParams(){return [{slug}]}",
            1,
        ),
    ] {
        fs::write(
            root.join("app/[slug]/page.tsx"),
            format!("{generator}\nexport default function Page(){{return <p/>}}"),
        )
        .unwrap();
        let output = call(&root, &["analyze", "--to=astro-preact", "--fail-on=review"]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{generator}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn excluded_computed_links_do_not_trigger_base_path_review() {
    let root = fixture("base-path-excluded-links");
    fs::write(root.join("package-lock.json"), format!(
        "{{\"lockfileVersion\":3,\"packages\":{{\"\":{{\"name\":\"fictional-site\",\"dependencies\":{{\"{0}\":\"16.2.0\"}}}},\"node_modules/{0}\":{{\"version\":\"16.2.0\"}}}}}}", "next"
    )).unwrap();
    fs::write(
        root.join("next.config.mjs"),
        "export default {basePath:'/docs'}",
    )
    .unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(root.join("tests/links.spec.tsx"), "import Link from 'next/link';const destination='/about';export function Widget(){return <Link href={destination}/>}").unwrap();
    for (page, expected) in [
        ("export default function Page(){return <p/>}", 0),
        (
            "import {Widget} from '../tests/links.spec';export default function Page(){return <Widget/>}",
            1,
        ),
    ] {
        fs::write(root.join("app/page.tsx"), page).unwrap();
        let output = call(&root, &["analyze", "--to=astro-preact", "--fail-on=review"]);
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn literal_object_links_do_not_trigger_fail_on_review() {
    let root = fixture("link-object");
    fs::write(root.join("package-lock.json"), format!(
        "{{\"lockfileVersion\":3,\"packages\":{{\"\":{{\"name\":\"fictional-site\",\"dependencies\":{{\"{0}\":\"16.2.0\"}}}},\"node_modules/{0}\":{{\"version\":\"16.2.0\"}}}}}}", "next"
    )).unwrap();
    for (attributes, code) in [
        ("href={{pathname:'/about',query:{q:'hello',page:2}}}", 0),
        ("href={{pathname: destination}}", 1),
        ("href={{pathname:'/about'}} prefetch={false}", 1),
    ] {
        fs::write(root.join("app/page.tsx"), format!(
            "import Link from 'next/link';export default function Page(){{return <Link {attributes}>About</Link>}}"
        )).unwrap();
        let output = call(&root, &["analyze", "--to=astro-preact", "--fail-on=review"]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{attributes}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
fn assessment_rule_metadata_excludes_out_of_scope_features() {
    use frameshift_engine::catalog::{FEATURES, PATH, Status, V1};
    let root = fixture("rule-metadata");
    for (args, unsupported_only) in [
        (vec!["rules"], false),
        (vec!["rules", "--unsupported"], true),
    ] {
        let output = call(&root, &args);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        let actual: Vec<_> = text
            .lines()
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        let expected: Vec<_> = FEATURES.iter().filter(|f| matches!(f.v1, V1::Assessed(statuses) if !unsupported_only || statuses.contains(&Status::Unsupported)))
            .map(|f| format!("{PATH}/assessment.{}", f.id.replace('.', "-"))).collect();
        assert_eq!(actual, expected);
    }
    for feature in FEATURES.iter().filter(|f| f.v1 == V1::OutOfScope) {
        let id = format!("{PATH}/assessment.{}", feature.id.replace('.', "-"));
        for args in [
            vec!["rules", "show", id.as_str()],
            vec!["explain", id.as_str()],
        ] {
            let output = call(&root, &args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
        }
        let output = call(&root, &["explain", feature.id]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Outside V1 application assessment"));
        assert!(!text.contains("Assessment rule version"));
    }
}
#[test]
fn explanations_and_help_are_available_without_a_source_project() {
    let root = fixture("explain");
    for args in [
        vec!["explain", "FS3101"],
        vec!["explain", "FS3102"],
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
