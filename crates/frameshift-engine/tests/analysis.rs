//! M5 end-to-end evidence contracts on fictional, temporary source projects.
use frameshift_engine::{
    analysis::{
        self, AnalyzeOptions, FileClass, Strategy,
        stored::{StoredReport, Value},
    },
    catalog::Status,
    foundation::{PathLimits, SourcePath, UnitKind},
    limits::Limits,
};
use std::{fs, path::PathBuf};
mod support;
fn create(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = support::temporary().join(format!("analysis-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // Real framework names are introduced at test time only (Q-014).
    fs::write(root.join("package.json"),format!("{{\"name\":\"fictional-site\",\"dependencies\":{{\"{}\":\"16.2.0\",\"react\":\"19.2.0\",\"react-dom\":\"19.2.0\"}}}}","next")).unwrap();
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/layout.tsx"),
        "export default function Layout({children}){return <html><body>{children}</body></html>}",
    )
    .unwrap();
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    root
}
fn run(root: &std::path::Path) -> analysis::Analysis {
    analysis::analyze(root, &AnalyzeOptions::default()).unwrap()
}
const PAGE: &str = "export default function Page() { return <h1>Welcome</h1> }\n";
#[test]
fn manifest_scripts_preserve_production_server_and_other_next_review() {
    let root = create("scripts", &[("app/page.tsx", PAGE)]);
    for (script, expected) in [
        ("next dev", Status::Auto),
        ("next build", Status::Auto),
        ("next start", Status::Review),
        ("next lint", Status::Review),
        ("next build && next start", Status::Review),
    ] {
        fs::write(
            root.join("package.json"),
            format!(
                "{{\"dependencies\":{{\"{}\":\"16.2.0\"}},\"scripts\":{{\"test\":\"{script}\"}}}}",
                "next"
            ),
        )
        .unwrap();
        let result = run(&root);
        let manifest = result
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "npm.manifest")
            .unwrap();
        assert_eq!(manifest.status, Some(expected), "{script}");
        let owner = manifest.unit.as_ref().unwrap();
        assert_eq!(
            result
                .assessment
                .units
                .iter()
                .find(|u| u.id.as_str() == owner)
                .unwrap()
                .status,
            expected
        );
    }
}
#[test]
fn analysis_is_read_only_deterministic_and_schema_readable() {
    let root = create(
        "deterministic",
        &[
            ("app/page.tsx", PAGE),
            ("public/a.svg", "<svg/>"),
            ("tests/page.spec.ts", "throw 1"),
            ("data/catalog.csv", "a,b\n"),
        ],
    );
    let before = fs::read(root.join("app/page.tsx")).unwrap();
    let a = run(&root);
    let b = run(&root);
    assert_eq!(a.document().unwrap(), b.document().unwrap());
    assert_eq!(fs::read(root.join("app/page.tsx")).unwrap(), before);
    let stored =
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    assert_eq!(
        stored.value().get("kind").and_then(Value::text),
        Some("frameshift.analysis")
    );
    assert!(
        a.assessment
            .files
            .iter()
            .any(|f| f.path.as_str() == "public/a.svg" && f.class == FileClass::Verbatim)
    );
    assert!(
        a.assessment
            .files
            .iter()
            .any(|f| f.path.as_str() == "tests/page.spec.ts" && f.class == FileClass::OutOfScope)
    );
    let route = a
        .assessment
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Route)
        .unwrap();
    assert_eq!(route.id.as_str(), "route:/");
    assert_eq!(route.weight, 8);
    assert_eq!(route.strategy, Strategy::Astro);
    assert!(!a.document().unwrap().contains(root.to_str().unwrap()));
    assert_eq!(a.assessment.routes[0].unit, "route:/");
    let again =
        StoredReport::parse(stored.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    assert_eq!(stored, again);
}
#[test]
fn spread_fetch_arguments_cannot_establish_an_option_free_call() {
    for (arguments, expected) in [
        ("'/data'", Status::Auto),
        ("'/data', {next:{revalidate:60}}", Status::Review),
        ("...args", Status::Review),
        ("'/data', ...args", Status::Review),
        ("...['/data', {cache:'no-store'}]", Status::Review),
    ] {
        let source = format!(
            "const args=['/data'];export default function Page(){{fetch({arguments});return <p/>}}"
        );
        let root = create("fetch-spread", &[("app/page.tsx", &source)]);
        let result = run(&root);
        let occurrence = result
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.fetch")
            .unwrap();
        assert_eq!(occurrence.status, Some(expected), "{arguments}");
        let unit = result
            .assessment
            .units
            .iter()
            .find(|u| u.id.as_str() == "route:/")
            .unwrap();
        assert_eq!(unit.status, expected, "{arguments}");
        assert_eq!(result.assessment.routes[0].status, expected, "{arguments}");
    }
}
#[test]
fn request_api_finding_codes_follow_the_imported_export() {
    for (import, read, code) in [
        ("import {cookies} from 'next/headers';", "cookies", "FS3101"),
        ("import {headers} from 'next/headers';", "headers", "FS3102"),
        (
            "import {headers as read} from 'next/headers';",
            "read",
            "FS3102",
        ),
        (
            "import * as request from 'next/headers';",
            "request.headers",
            "FS3102",
        ),
        (
            "import {draftMode} from 'next/headers';",
            "draftMode",
            "FS3100",
        ),
    ] {
        let source = format!("{import}export default function Page(){{{read}();return <p/>}}");
        let root = create("request-codes", &[("app/page.tsx", &source)]);
        let result = run(&root);
        let findings: Vec<_> = result
            .findings
            .iter()
            .filter(|(_, f)| {
                f.context()
                    .feature
                    .as_ref()
                    .is_some_and(|id| id.as_str() == "nextjs.api.request")
            })
            .collect();
        assert_eq!(findings.len(), 1, "{read}");
        assert_eq!(findings[0].1.code().as_str(), code, "{read}");
        assert_eq!(
            findings[0].1.severity(),
            frameshift_engine::foundation::Severity::Blocker
        );
        assert_eq!(result.assessment.routes[0].status, Status::Unsupported);
        assert_eq!(
            findings[0].1.message(),
            analysis::knowledge::explain(code).unwrap().0
        );
        StoredReport::parse(result.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
    let root = create(
        "shadowed-headers",
        &[(
            "app/page.tsx",
            "function headers(){return 'local'}export default function Page(){headers();return <p/>}",
        )],
    );
    assert!(
        !run(&root)
            .findings
            .iter()
            .any(|(_, f)| f.code().as_str() == "FS3102")
    );
}
#[test]
fn shared_request_time_dependency_blocks_each_route_and_retains_a_witness() {
    let root = create(
        "closure",
        &[
            (
                "app/page.tsx",
                "import {read} from '../lib/request'; export default function Page(){return <p>{read()}</p>}",
            ),
            (
                "app/shop/page.tsx",
                "import {read} from '../../lib/request'; export default function Shop(){return <p>{read()}</p>}",
            ),
            (
                "lib/request.ts",
                "import {cookies} from 'next/headers'; export const read = () => cookies();",
            ),
        ],
    );
    let a = run(&root);
    assert_eq!(a.assessment.routes.len(), 2);
    for route in &a.assessment.routes {
        assert_eq!(route.status, Status::Unsupported);
        assert!(route.blocker.as_ref().unwrap().contains("lib/request.ts"));
        assert_eq!(
            route.dependency_path.last().unwrap().as_str(),
            "lib/request.ts"
        );
    }
    assert!(
        a.findings
            .iter()
            .any(|(_, f)| f.code().as_str() == "FS3101")
    );
    assert!(a.verdict().starts_with("Poor candidate"));
}
#[test]
fn unknown_evidence_does_not_disappear_and_definite_blockers_rank_above_it() {
    let root = create(
        "unknown",
        &[
            (
                "app/page.tsx",
                "import Missing from '../absent'; import {cookies} from 'next/headers'; export default function Page(){cookies();return <Missing/>}",
            ),
            ("unreached.ts", "export const ="),
        ],
    );
    let a = run(&root);
    assert!(a.assessment.units.iter().any(|u| {
        u.path
            .as_ref()
            .is_some_and(|p| p.as_str() == "unreached.ts")
            && u.status == Status::Unknown
    }));
    assert!(
        a.assessment
            .occurrences
            .iter()
            .any(|o| o.status == Some(Status::Unknown))
    );
    assert_eq!(a.assessment.routes[0].status, Status::Unsupported);
}
#[test]
fn component_ownership_partitions_bytes_and_stable_identities() {
    let code = "export function Hero(){return <h1>Hero</h1>}\nexport function Badge(){return <b>New</b>}\nexport const value=1;\n";
    let root = create(
        "units",
        &[
            (
                "app/page.tsx",
                "import {Hero,Badge} from '../ui'; export default function Page(){return <><Hero/><Badge/></>}",
            ),
            ("ui.tsx", code),
        ],
    );
    let a = run(&root);
    let units: Vec<_> = a
        .assessment
        .units
        .iter()
        .filter(|u| u.path.as_ref().is_some_and(|p| p.as_str() == "ui.tsx"))
        .collect();
    assert_eq!(
        units
            .iter()
            .filter(|u| u.kind == UnitKind::Component)
            .count(),
        2
    );
    let mut ranges: Vec<_> = units.iter().flat_map(|u| u.ranges.iter()).collect();
    ranges.sort();
    let mut end = 0;
    for range in ranges {
        assert_eq!(range.start(), end);
        end = range.end()
    }
    assert_eq!(end, code.len());
    assert!(
        units
            .iter()
            .any(|u| u.id.as_str() == "component:ui.tsx#Hero")
    );
}
#[test]
fn conversion_fixed_point_demotes_children_and_rejects_async_preact() {
    let root = create(
        "fixed-point",
        &[
            (
                "app/page.tsx",
                "import Parent from '../parent';export default function Page(){return <Parent/>}",
            ),
            (
                "parent.tsx",
                "import Child from './child'; export default class Parent {render(){return <Child/>}}",
            ),
            (
                "child.tsx",
                "export default async function Child(){return <div>Child</div>}",
            ),
        ],
    );
    let a = run(&root);
    let child = a
        .assessment
        .units
        .iter()
        .find(|u| {
            u.path.as_ref().is_some_and(|p| p.as_str() == "child.tsx")
                && u.kind == UnitKind::Component
        })
        .unwrap();
    assert_eq!(child.status, Status::Unsupported);
    assert_eq!(child.strategy, Strategy::Preserve);
    assert_eq!(a.assessment.routes[0].status, Status::Unsupported);
}
#[test]
fn declaration_only_dependency_does_not_poison_a_route() {
    let root = create("dependencies", &[("app/page.tsx", PAGE)]);
    let manifest = root.join("package.json");
    let original = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        original.replacen(
            "\"dependencies\":{",
            "\"dependencies\":{\"fictional-widget\":\"^1.0.0\",",
            1,
        ),
    )
    .unwrap();
    let a = run(&root);
    let dependency = a
        .dependencies
        .iter()
        .find(|d| d.name == "fictional-widget")
        .unwrap();
    assert!(dependency.used_in.is_empty());
    assert!(
        !a.findings
            .iter()
            .any(|(_, f)| f.code().as_str() == "FS3302")
    );
    fs::write(
        root.join("app/page.tsx"),
        "import Widget from 'fictional-widget';export default function Page(){return <Widget/>}",
    )
    .unwrap();
    let b = run(&root);
    assert_eq!(b.assessment.routes[0].status, Status::Unknown);
    assert!(
        b.findings
            .iter()
            .any(|(_, f)| f.code().as_str() == "FS3302")
    );
}
#[test]
fn computed_script_strategy_is_unknown_and_literal_worker_is_unsupported() {
    for (name, strategy, status) in [
        ("worker", "\"worker\"", Status::Unsupported),
        ("computed", "{mode}", Status::Unknown),
    ] {
        let code = format!(
            "import Script from 'next/script';export default function Page(){{return <Script src=\"/a.js\" strategy={strategy}/>}}"
        );
        let root = create(name, &[("app/page.tsx", &code)]);
        let a = run(&root);
        let o = a
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.api.script")
            .unwrap();
        assert_eq!(o.status, Some(status));
    }
}
#[test]
fn hostile_text_is_escaped_in_json_and_coordinates_are_original() {
    let root = create(
        "hostile",
        &[
            (
                "app/page.tsx",
                "// 💫\nimport {cookies} from 'next/headers';export default function Page(){cookies();return <p/>}",
            ),
            ("public/evil\u{202e}.txt", "x"),
        ],
    );
    let a = run(&root);
    let document = a.document().unwrap();
    assert!(!document.contains('\u{202e}'));
    assert!(document.contains("\\u202e"));
    let f = a
        .findings
        .iter()
        .find(|(_, f)| f.code().as_str() == "FS3101")
        .unwrap()
        .1;
    assert!(
        matches!(f.location(),frameshift_engine::foundation::FindingLocation::Text(t) if t.start().line==2)
    );
    StoredReport::parse(document.into_bytes(), &Limits::default()).unwrap();
}
#[test]
fn stored_finding_ids_follow_the_published_grammar() {
    fn rename(value: &mut Value, old: &str, new: &str) {
        match value {
            Value::String(s) => {
                if s == old {
                    *s = new.to_owned();
                }
            }
            Value::Array(a) => {
                for v in a {
                    rename(v, old, new);
                }
            }
            Value::Object(o) => {
                for v in o.values_mut() {
                    rename(v, old, new);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    let root = create("finding-id-grammar", &[("app/page.tsx", PAGE)]);
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    let first_id = original.value().get("findings").unwrap().array()[0]
        .get("id")
        .and_then(Value::text)
        .unwrap();
    for (id, valid) in [
        ("F0001", true),
        ("F12345", true),
        ("", false),
        ("x", false),
        ("F1", false),
        ("F001", false),
        ("F000A", false),
        ("F１２３４", false),
        ("F0001\n", false),
    ] {
        let mut changed = original.value().clone();
        // Rename references too, so dangling links cannot mask an ID defect.
        rename(&mut changed, first_id, id);
        assert_eq!(
            StoredReport::parse(changed.document().into_bytes(), &Limits::default()).is_ok(),
            valid,
            "{id:?}"
        );
    }
}
#[test]
fn stored_unit_statuses_match_occurrences_and_unit_level_findings() {
    for (name, source, expected) in [
        (
            "blocker",
            "import Script from 'next/script';export default function Page(){return <Script strategy='worker'/>}",
            Status::Unsupported,
        ),
        (
            "unknown",
            "export default function Page(){return <Missing/>}",
            Status::Unknown,
        ),
        (
            "review",
            "import Link from 'next/link';const destination='/about';export default function Page(){return <Link href={destination}/>}",
            Status::Review,
        ),
    ] {
        let root = create(
            &format!("stored-unit-status-{name}"),
            &[("app/page.tsx", source)],
        );
        for evidence in ["both", "occurrence", "finding"] {
            let mut analysis = run(&root);
            assert_eq!(analysis.assessment.routes[0].status, expected);
            StoredReport::parse(
                analysis.document().unwrap().into_bytes(),
                &Limits::default(),
            )
            .unwrap();
            // Recompute every dependent metric and headline from the lowered
            // statuses, retaining the original contradictory evidence.
            for unit in &mut analysis.assessment.units {
                unit.status = Status::Auto;
            }
            for route in &mut analysis.assessment.routes {
                route.status = Status::Auto;
                route.blocker = None;
                route.dependency_path.clear();
            }
            if evidence == "finding" {
                for occurrence in &mut analysis.assessment.occurrences {
                    occurrence.status = None;
                    occurrence.unit = None;
                }
            }
            let document = analysis.document().unwrap();
            let source = frameshift_engine::foundation::SourceText::from_bytes(
                document.into_bytes(),
                frameshift_engine::foundation::TextLimits::default(),
            )
            .unwrap();
            // Parse as plain JSON, since the stored reader must reject it.
            let parsed = frameshift_engine::syntax::parse_json(
                &source,
                frameshift_engine::syntax::JsonDialect::Json,
                Limits::default().json,
            )
            .unwrap();
            let mut document = source.as_str().to_owned();
            if evidence == "occurrence" {
                // All status evidence still belongs to the occurrences;
                // remove only the finding ownership associations.
                let mut edits = Vec::new();
                for id in parsed.ids() {
                    if let Some(frameshift_engine::syntax::JsonValue::Object(o)) = parsed.value(id)
                        && o.get("severity").is_some()
                        && let Some(unit) = o.get("unit")
                    {
                        edits.push(parsed.span(unit).unwrap());
                    }
                }
                edits.sort_by_key(|s| std::cmp::Reverse(s.start()));
                for span in edits {
                    document.replace_range(span.start()..span.end(), "null");
                }
            }
            assert!(
                StoredReport::parse(document.into_bytes(), &Limits::default()).is_err(),
                "{name}: {evidence}"
            );
        }
    }
}
#[test]
fn stored_analysis_reports_cannot_claim_migration_outputs() {
    fn object(v: &mut Value) -> &mut std::collections::BTreeMap<String, Value> {
        let Value::Object(o) = v else {
            panic!("object required")
        };
        o
    }
    let root = create("stored-output-claims", &[("app/page.tsx", PAGE)]);
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    for (section, field, invalid) in [
        (
            "routes",
            "outputPath",
            vec![
                None,
                Some(Value::String("src/pages/index.astro".into())),
                Some(Value::Bool(false)),
                Some(Value::Array(Vec::new())),
            ],
        ),
        (
            "files",
            "outputPaths",
            vec![
                None,
                Some(Value::Array(vec![Value::String(
                    "src/pages/index.astro".into(),
                )])),
                Some(Value::Null),
                Some(Value::String("src/pages/index.astro".into())),
            ],
        ),
    ] {
        for replacement in invalid {
            let mut value = original.value().clone();
            let Value::Array(records) = object(&mut value).get_mut(section).unwrap() else {
                panic!("records required")
            };
            if let Some(replacement) = replacement {
                object(&mut records[0]).insert(field.into(), replacement);
            } else {
                object(&mut records[0]).remove(field);
            }
            assert!(
                StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_err(),
                "{section}.{field}"
            );
        }
    }
    // Unknown additive metadata remains supported without inventing output.
    let mut value = original.value().clone();
    for section in ["routes", "files"] {
        let Value::Array(records) = object(&mut value).get_mut(section).unwrap() else {
            panic!("records required")
        };
        object(&mut records[0]).insert("futureEvidence".into(), Value::Bool(true));
    }
    StoredReport::parse(value.document().into_bytes(), &Limits::default()).unwrap();
}
#[test]
fn stored_request_evidence_requires_consistent_status_rule_and_owner() {
    fn object(v: &mut Value) -> &mut std::collections::BTreeMap<String, Value> {
        let Value::Object(o) = v else {
            panic!("object required")
        };
        o
    }
    let root = create(
        "stored-request-evidence",
        &[
            (
                "app/page.tsx",
                "import {cookies} from 'next/headers';import {read} from '../lib/request';export default function Page(){cookies();read();return <p/>}",
            ),
            (
                "lib/request.ts",
                "import {headers} from 'next/headers';export const read=()=>headers()",
            ),
            (
                "tests/request.spec.ts",
                "import {cookies} from 'next/headers';cookies()",
            ),
        ],
    );
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    let occurrences = original.value().get("occurrences").unwrap().array();
    assert!(occurrences.iter().any(|o| {
        o.get("feature").and_then(Value::text) == Some("nextjs.api.request")
            && o.get("status") == Some(&Value::Null)
    }));
    let index = occurrences
        .iter()
        .position(|o| {
            o.get("feature").and_then(Value::text) == Some("nextjs.api.request")
                && o.get("unit").and_then(Value::text) == Some("route:/")
        })
        .unwrap();
    let other = occurrences
        .iter()
        .find(|o| {
            o.get("feature").and_then(Value::text) == Some("nextjs.api.request")
                && o.get("unit")
                    .and_then(Value::text)
                    .is_some_and(|u| u != "route:/")
        })
        .unwrap()
        .get("unit")
        .unwrap()
        .clone();
    let different_rule = occurrences
        .iter()
        .find_map(|o| {
            let rule = o.get("rule")?.text()?;
            (o.get("feature").and_then(Value::text) != Some("nextjs.api.request")).then_some(rule)
        })
        .unwrap();
    for (key, bad) in [
        ("status", Value::String("auto".into())),
        ("status", Value::String("review".into())),
        ("status", Value::String("unknown".into())),
        ("status", Value::Null),
        ("rule", Value::String(different_rule.into())),
        ("rule", Value::Null),
        ("unit", other),
        ("unit", Value::Null),
    ] {
        let mut value = original.value().clone();
        let Value::Array(records) = object(&mut value).get_mut("occurrences").unwrap() else {
            panic!("occurrences required")
        };
        object(&mut records[index]).insert(key.into(), bad);
        assert!(
            StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_err(),
            "{key}"
        );
    }
    let root = create("stored-request-relabel", &[("app/page.tsx", PAGE)]);
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    for feature in [
        "nextjs.api.request",
        "nextjs.api.server-action",
        "nextjs.segment.dynamic",
    ] {
        for status in ["auto", "unsupported"] {
            let mut value = original.value().clone();
            let Value::Array(records) = object(&mut value).get_mut("occurrences").unwrap() else {
                panic!("occurrences required")
            };
            let occurrence = records
                .iter_mut()
                .find(|o| {
                    o.get("unit").and_then(Value::text) == Some("route:/")
                        && o.get("status").and_then(Value::text) == Some("auto")
                })
                .unwrap();
            object(occurrence).insert("feature".into(), Value::String(feature.into()));
            object(occurrence).insert("status".into(), Value::String(status.into()));
            let rule = format!(
                "nextjs-to-astro-preact/assessment.{}",
                feature.replace('.', "-")
            );
            object(occurrence).insert("rule".into(), Value::String(rule.clone()));
            let Value::Array(rules) = object(&mut value).get_mut("rules").unwrap() else {
                panic!("rules required")
            };
            rules.push(Value::Object(std::collections::BTreeMap::from([
                ("id".into(), Value::String(rule)),
                ("phase".into(), Value::String("assessment".into())),
                ("version".into(), Value::Number("1".into())),
            ])));
            object(object(&mut value).get_mut("summary").unwrap()).insert(
                "verdict".into(),
                Value::String("Poor candidate for a static migration: most routes depend on request-time features.".into()),
            );
            assert!(
                StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_err(),
                "{feature}: {status}"
            );
        }
    }
}
#[test]
fn stored_verdicts_are_derived_from_route_and_request_evidence() {
    const REQUEST: &str = "import {cookies} from 'next/headers';export default function Page(){cookies();return <p/>}";
    const MANUAL: &str = "import Script from 'next/script';export default function Page(){return <Script strategy='worker'/>}";
    type Case = (
        &'static str,
        &'static [(&'static str, &'static str)],
        &'static str,
    );
    let cases: &[Case] = &[
        ("automatic", &[("app/page.tsx", PAGE)], "Good candidate:"),
        ("empty", &[], "Suitability is uncertain:"),
        (
            "unknown",
            &[(
                "app/page.tsx",
                "export default function Page(){return <Missing/>}",
            )],
            "Suitability is uncertain:",
        ),
        (
            "manual",
            &[("app/page.tsx", MANUAL)],
            "Good candidate with manual work:",
        ),
        ("request", &[("app/page.tsx", REQUEST)], "Poor candidate"),
        (
            "dynamic",
            &[(
                "app/page.tsx",
                "export const dynamic='force-dynamic';export default function Page(){return <p/>}",
            )],
            "Poor candidate",
        ),
        (
            "server-action",
            &[
                (
                    "app/page.tsx",
                    "import {save} from '../lib/action';export default function Page(){return <form action={save}/>}",
                ),
                (
                    "lib/action.ts",
                    "'use server';export async function save(){}",
                ),
            ],
            "Poor candidate",
        ),
        (
            "tie",
            &[("app/page.tsx", REQUEST), ("app/other/page.tsx", PAGE)],
            "Good candidate with manual work:",
        ),
        (
            "majority",
            &[
                ("app/page.tsx", REQUEST),
                ("app/other/page.tsx", REQUEST),
                ("app/static/page.tsx", PAGE),
            ],
            "Poor candidate",
        ),
        (
            "unreached-request",
            &[
                ("app/page.tsx", PAGE),
                (
                    "lib/request.ts",
                    "import {cookies} from 'next/headers';export const read=()=>cookies()",
                ),
            ],
            "Good candidate:",
        ),
        (
            "shared-request",
            &[
                (
                    "app/page.tsx",
                    "import {read} from '../lib/request';export default function Page(){return <p>{read()}</p>}",
                ),
                (
                    "app/other/page.tsx",
                    "import {read} from '../../lib/request';export default function Page(){return <p>{read()}</p>}",
                ),
                (
                    "lib/request.ts",
                    "import {cookies} from 'next/headers';export const read=()=>cookies()",
                ),
            ],
            "Poor candidate",
        ),
    ];
    let mut originals = Vec::new();
    let mut verdicts = std::collections::BTreeSet::new();
    for &(name, files, prefix) in cases {
        let root = create(&format!("stored-verdict-{name}"), files);
        let analysis = run(&root);
        assert!(
            analysis.verdict().starts_with(prefix),
            "{name}: {}",
            analysis.verdict()
        );
        verdicts.insert(analysis.verdict().to_owned());
        let original = StoredReport::parse(
            analysis.document().unwrap().into_bytes(),
            &Limits::default(),
        )
        .unwrap();
        originals.push((name, original));
    }
    assert_eq!(verdicts.len(), 4);
    verdicts.insert("Unsubstantiated suitability claim".into());
    for (name, original) in originals {
        let expected = original
            .value()
            .get("summary")
            .unwrap()
            .get("verdict")
            .unwrap()
            .text()
            .unwrap();
        for verdict in &verdicts {
            let mut value = original.value().clone();
            let Value::Object(top) = &mut value else {
                panic!("object required")
            };
            let Value::Object(summary) = top.get_mut("summary").unwrap() else {
                panic!("summary required")
            };
            summary.insert("verdict".into(), Value::String(verdict.clone()));
            assert_eq!(
                StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_ok(),
                verdict == expected,
                "{name}: {verdict}"
            );
        }
    }
}
#[test]
fn stored_routes_match_route_and_endpoint_units_exactly_once() {
    fn object(v: &mut Value) -> &mut std::collections::BTreeMap<String, Value> {
        let Value::Object(o) = v else {
            panic!("object required")
        };
        o
    }
    let root = create(
        "stored-route-population",
        &[
            ("app/page.tsx", PAGE),
            (
                "app/api/route.ts",
                "export function GET(request){return new Response(request.url)}",
            ),
        ],
    );
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    let routes = original.value().get("routes").unwrap().array();
    assert_eq!(routes.len(), 2);
    assert!(
        routes
            .iter()
            .any(|r| r.get("status").and_then(Value::text) == Some("auto"))
    );
    assert!(
        routes
            .iter()
            .any(|r| r.get("status").and_then(Value::text) == Some("unsupported"))
    );
    for index in 0..routes.len() {
        for duplicate in [false, true] {
            let mut value = original.value().clone();
            let mut changed = routes.to_vec();
            if duplicate {
                changed.push(changed[index].clone());
            } else {
                changed.remove(index);
            }
            // Keep summary counts consistent so they cannot mask the defect.
            let readiness = object(object(&mut value).get_mut("summary").unwrap())
                .get_mut("routeReadiness")
                .unwrap();
            let Value::Array(rows) = readiness else {
                panic!("readiness required")
            };
            for row in rows {
                let status = row.get("status").and_then(Value::text).unwrap();
                let count = changed
                    .iter()
                    .filter(|r| r.get("status").and_then(Value::text) == Some(status))
                    .count();
                object(row).insert("count".into(), Value::Number(count.to_string()));
            }
            object(&mut value).insert("routes".into(), Value::Array(changed));
            assert!(
                StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_err(),
                "route {index}, duplicate {duplicate}"
            );
        }
    }
}
#[test]
fn stored_reports_require_physical_units_in_their_own_files() {
    fn object(v: &mut Value) -> &mut std::collections::BTreeMap<String, Value> {
        let Value::Object(o) = v else {
            panic!("object required")
        };
        o
    }
    fn files(v: &mut Value) -> &mut Vec<Value> {
        let Value::Array(a) = object(v).get_mut("files").unwrap() else {
            panic!("files required")
        };
        a
    }
    let root = create("file-ownership", &[("app/page.tsx", PAGE)]);
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    for damage in ["move", "missing", "extra", "missing-file"] {
        let mut value = original.value().clone();
        for file in files(&mut value) {
            let path = file.get("path").and_then(Value::text).unwrap().to_owned();
            if path == "app/page.tsx" && damage != "extra" {
                object(file).insert("units".into(), Value::Array(Vec::new()));
                object(file).insert("class".into(), Value::String("verbatim".into()));
            }
            if path == "app/layout.tsx" && matches!(damage, "move" | "extra") {
                let Value::Array(ids) = object(file).get_mut("units").unwrap() else {
                    panic!("units required")
                };
                ids.push(Value::String("route:/".into()));
            }
        }
        if damage == "missing-file" {
            files(&mut value)
                .retain(|f| f.get("path").and_then(Value::text) != Some("app/page.tsx"));
        }
        assert!(
            StoredReport::parse(value.document().into_bytes(), &Limits::default()).is_err(),
            "{damage}"
        );
    }
    // Logical configuration concerns can span inherited files. They do not
    // acquire an exclusive physical owner from one occurrence's path.
    let mut value = original.value().clone();
    for file in files(&mut value) {
        if file.get("path").and_then(Value::text) == Some("app/layout.tsx") {
            let Value::Array(ids) = object(file).get_mut("units").unwrap() else {
                panic!("units required")
            };
            ids.push(Value::String("config:npm.manifest".into()));
        }
    }
    StoredReport::parse(value.document().into_bytes(), &Limits::default()).unwrap();
}
#[test]
fn stored_reports_validate_nested_records_and_relationships() {
    fn object(value: &mut Value) -> &mut std::collections::BTreeMap<String, Value> {
        let Value::Object(o) = value else {
            panic!("object required")
        };
        o
    }
    fn first<'a>(value: &'a mut Value, section: &str) -> &'a mut Value {
        let Value::Array(a) = object(value).get_mut(section).unwrap() else {
            panic!("array required")
        };
        &mut a[0]
    }
    let root = create(
        "nested-reader",
        &[
            ("app/page.tsx", PAGE),
            ("next.config.mjs", "export default {output:'export'}"),
        ],
    );
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    // Every published field in these records is required, including nullable
    // fields. A missing field must not become an empty renderer fallback.
    for section in [
        "dependencies",
        "config",
        "rules",
        "tasks",
        "limits",
        "occurrences",
    ] {
        let mut baseline = original.value().clone();
        let keys: Vec<_> = object(first(&mut baseline, section))
            .keys()
            .cloned()
            .collect();
        for key in keys {
            // Config's unit is an optional additive association.
            if section == "config" && key == "unit" {
                continue;
            }
            let mut damaged = original.value().clone();
            object(first(&mut damaged, section)).remove(&key);
            assert!(
                StoredReport::parse(damaged.document().into_bytes(), &Limits::default()).is_err(),
                "{section}.{key}"
            );
        }
    }
    for (section, key, bad) in [
        ("tasks", "finding", Value::String("F9999".into())),
        ("tasks", "instruction", Value::Bool(true)),
        ("dependencies", "class", Value::String("guessed".into())),
        (
            "dependencies",
            "usedIn",
            Value::Array(vec![Value::String("../outside.ts".into())]),
        ),
        ("dependencies", "fields", Value::Array(vec![Value::Null])),
        ("dependencies", "resolved", Value::Bool(false)),
        ("config", "unit", Value::String("module:missing.ts".into())),
        ("config", "status", Value::String("complete".into())),
        ("rules", "phase", Value::String("execution".into())),
        ("rules", "version", Value::Number("2".into())),
        ("limits", "value", Value::Number("-1".into())),
        ("occurrences", "rule", Value::String("missing/rule".into())),
        (
            "occurrences",
            "location",
            Value::Object(std::collections::BTreeMap::from([
                ("scope".into(), Value::String("file".into())),
                ("path".into(), Value::String("a/../outside.ts".into())),
            ])),
        ),
    ] {
        let mut damaged = original.value().clone();
        object(first(&mut damaged, section)).insert(key.into(), bad);
        assert!(
            StoredReport::parse(damaged.document().into_bytes(), &Limits::default()).is_err(),
            "{section}.{key}"
        );
    }
    let mut additive = original.value().clone();
    for section in [
        "dependencies",
        "config",
        "rules",
        "tasks",
        "limits",
        "occurrences",
    ] {
        object(first(&mut additive, section)).insert("futureEvidence".into(), Value::Bool(true));
    }
    let accepted =
        StoredReport::parse(additive.document().into_bytes(), &Limits::default()).unwrap();
    assert_eq!(accepted.value(), &additive);
}
#[test]
fn stored_unit_ids_require_canonical_spelling_and_record_consistency() {
    fn rename(value: &mut Value, old: &str, new: &str) {
        match value {
            Value::String(s) => {
                if s == old {
                    *s = new.into();
                }
            }
            Value::Array(a) => {
                for v in a {
                    rename(v, old, new);
                }
            }
            Value::Object(o) => {
                for v in o.values_mut() {
                    rename(v, old, new);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    let root = create(
        "unit-id-grammar",
        &[
            (
                "app/page.tsx",
                "import Cafe from '../components/Café';export default function Page(){return <Cafe/>}",
            ),
            (
                "components/Café.tsx",
                "export default function Café(){return <p>Café</p>}",
            ),
        ],
    );
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    let component = original
        .value()
        .get("units")
        .unwrap()
        .array()
        .iter()
        .find(|u| u.get("kind").and_then(Value::text) == Some("component"))
        .unwrap()
        .get("id")
        .and_then(Value::text)
        .unwrap();
    assert!(component.contains("%C3%A9"));
    for (old, new) in [
        ("route:/", "x"),
        ("route:/", "endpoint:/"),
        ("route:/", "route:%2f"),
        ("route:/", "route:%2F"),
        ("route:/", "route:/#name"),
        ("route:/", "route:/other"),
        ("route:/", "route:/a//b"),
        ("route:/", "route:/%FF"),
        ("layout:app/layout.tsx", "layout:app/other.tsx"),
        ("layout:app/layout.tsx", "layout:app/%6Cayout.tsx"),
        ("layout:app/layout.tsx", "layout:app/layout.tsx#"),
        ("layout:app/layout.tsx", "layout:app/layout.tsx#%00"),
        (component, "component:components/Caf%c3%a9.tsx#Caf%c3%a9"),
        (component, "component:components/Café.tsx#Café"),
    ] {
        let mut changed = original.value().clone();
        rename(&mut changed, old, new);
        assert_ne!(changed, *original.value(), "{old}");
        assert!(
            StoredReport::parse(changed.document().into_bytes(), &Limits::default()).is_err(),
            "{new}"
        );
    }
    let mut limits = Limits::default();
    limits.identifier.max_bytes = original
        .value()
        .get("units")
        .unwrap()
        .array()
        .iter()
        .map(|u| u.get("id").and_then(Value::text).unwrap().len())
        .max()
        .unwrap();
    StoredReport::parse(original.document().unwrap().into_bytes(), &limits).unwrap();
    let mut changed = original.value().clone();
    rename(
        &mut changed,
        component,
        &format!("{component}{}", "x".repeat(limits.identifier.max_bytes)),
    );
    assert!(StoredReport::parse(changed.document().into_bytes(), &limits).is_err());
}
#[test]
fn stored_unit_weights_match_kind_and_required_client_flag() {
    let root = create("stored-unit-weights", &[("app/page.tsx", PAGE)]);
    for kind in [
        UnitKind::Route,
        UnitKind::Layout,
        UnitKind::Provider,
        UnitKind::Endpoint,
        UnitKind::Component,
        UnitKind::RouteFile,
        UnitKind::Module,
        UnitKind::Stylesheet,
        UnitKind::Config,
    ] {
        for client in [false, true] {
            let mut analysis = run(&root);
            // Report rendering recomputes every summary from these weights,
            // so an inconsistent total cannot mask missing per-unit checks.
            let index = analysis
                .assessment
                .units
                .iter()
                .position(|u| u.kind == UnitKind::Route)
                .unwrap();
            let unit = &mut analysis.assessment.units[index];
            unit.kind = kind;
            unit.client = client;
            let identifier = match kind {
                UnitKind::Route | UnitKind::Endpoint => {
                    frameshift_engine::foundation::UnitId::for_route(
                        kind,
                        "/",
                        Limits::default().identifier,
                    )
                    .unwrap()
                }
                UnitKind::Config => frameshift_engine::foundation::UnitId::for_config(
                    "test.weight",
                    Limits::default().identifier,
                )
                .unwrap(),
                UnitKind::Layout
                | UnitKind::Provider
                | UnitKind::Component
                | UnitKind::RouteFile
                | UnitKind::Module
                | UnitKind::Stylesheet => frameshift_engine::foundation::UnitId::for_file(
                    kind,
                    unit.path.as_ref().unwrap(),
                    None,
                    Limits::default().identifier,
                )
                .unwrap(),
            };
            if !matches!(kind, UnitKind::Route | UnitKind::Endpoint) {
                analysis.assessment.routes.clear();
            }
            let expected = analysis::metrics::weight(kind, client);
            for weight in [2, 3, 4, 5, 8] {
                analysis.assessment.units[index].weight = weight;
                let document = analysis
                    .document()
                    .unwrap()
                    .replace("\"route:/\"", &format!("\"{}\"", identifier.as_str()));
                assert_eq!(
                    StoredReport::parse(document.into_bytes(), &Limits::default()).is_ok(),
                    weight == expected,
                    "{kind:?}, client={client}, weight={weight}"
                );
            }
        }
    }
    let original = StoredReport::parse(
        run(&root).document().unwrap().into_bytes(),
        &Limits::default(),
    )
    .unwrap();
    for flag in [
        None,
        Some(Value::Null),
        Some(Value::Number("0".into())),
        Some(Value::String("false".into())),
    ] {
        let mut changed = original.value().clone();
        let Value::Object(root) = &mut changed else {
            unreachable!()
        };
        let Value::Array(units) = root.get_mut("units").unwrap() else {
            unreachable!()
        };
        let Value::Object(unit) = &mut units[0] else {
            unreachable!()
        };
        if let Some(flag) = flag {
            unit.insert("client".into(), flag);
        } else {
            unit.remove("client");
        }
        assert!(StoredReport::parse(changed.document().into_bytes(), &Limits::default()).is_err());
    }
}
#[test]
fn stored_report_reemission_enforces_the_retained_byte_bound() {
    let root = create("stored-output-bound", &[("app/page.tsx", PAGE)]);
    let original = run(&root).document().unwrap();
    let compact_extra = format!(
        "{},\"extra\":{}[{}]{} }}",
        original.trim_end().strip_suffix('}').unwrap(),
        "[".repeat(100),
        vec!["null"; 200].join(","),
        "]".repeat(100),
    );
    let expanded = StoredReport::parse(compact_extra.clone().into_bytes(), &Limits::default())
        .unwrap()
        .document()
        .unwrap();
    assert!(expanded.len() > compact_extra.len());
    for bound in [compact_extra.len(), expanded.len() - 1, expanded.len()] {
        let mut limits = Limits::default();
        limits.analysis.max_report_bytes = bound;
        let stored = StoredReport::parse(compact_extra.clone().into_bytes(), &limits).unwrap();
        if bound == expanded.len() {
            assert_eq!(stored.document().unwrap(), expanded);
        } else {
            assert_eq!(
                stored.document().unwrap_err().kind(),
                analysis::AnalyzeErrorKind::Limit
            );
            assert_eq!(
                stored
                    .projection_document(stored.value())
                    .unwrap_err()
                    .kind(),
                analysis::AnalyzeErrorKind::Limit
            );
        }
    }
    let value = Value::Object(std::collections::BTreeMap::from([
        (
            "\u{202e}é".into(),
            Value::String("\"\\\n\t\u{0080}💫".into()),
        ),
        (
            "array".into(),
            Value::Array(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Null,
                Value::Number("-1.2e3".into()),
                Value::Array(vec![]),
                Value::Object(Default::default()),
            ]),
        ),
    ]));
    let expected = value.document();
    assert_eq!(value.document_bounded(expected.len()).unwrap(), expected);
    assert!(value.document_bounded(expected.len() - 1).is_err());
}
#[test]
fn stored_reports_enforce_the_local_occurrence_bound() {
    let root = create("stored-occurrence-limit", &[("app/page.tsx", PAGE)]);
    let document = run(&root).document().unwrap();
    let stored = StoredReport::parse(document.clone().into_bytes(), &Limits::default()).unwrap();
    let count = stored.value().get("occurrences").unwrap().array().len();
    assert!(count > 0);
    let mut limits = Limits::default();
    limits.analysis.max_occurrences = count;
    StoredReport::parse(document.clone().into_bytes(), &limits).unwrap();
    limits.analysis.max_occurrences = count - 1;
    assert!(StoredReport::parse(document.into_bytes(), &limits).is_err());
}
#[test]
fn stored_reports_reject_tampering_duplicate_keys_dangling_units_and_versions() {
    let root = create("reader", &[("app/page.tsx", PAGE)]);
    let original = run(&root).document().unwrap();
    for damaged in [
        original.replacen("\"schemaVersion\": 1", "\"schemaVersion\": 2", 1),
        original.replacen(
            "\"schemaVersion\": 1",
            "\"schemaVersion\": 1,\"schemaVersion\":1",
            1,
        ),
        original.replacen("\"totalWeight\": 12", "\"totalWeight\": 999", 1),
        original.replace("\"closure\": [", "\"closure\": [\"module:missing.ts\", "),
    ] {
        if damaged != original {
            assert!(StoredReport::parse(damaged.into_bytes(), &Limits::default()).is_err())
        }
    }
    let mut limits = Limits::default();
    limits.analysis.max_report_bytes = 10;
    assert!(StoredReport::parse(original.into_bytes(), &limits).is_err());
}
#[test]
fn root_relative_workspace_selection_is_explicit() {
    let root = create(
        "workspace",
        &[
            ("apps/site/package.json", "{\"name\":\"fictional-member\"}"),
            ("apps/site/app/page.tsx", PAGE),
        ],
    );
    fs::write(root.join("package-lock.json"), "{}").unwrap();
    let mut manifest = fs::read_to_string(root.join("package.json")).unwrap();
    manifest = manifest.replacen("{", "{\"workspaces\":[\"apps/*\"],", 1);
    fs::write(root.join("package.json"), manifest).unwrap();
    fs::remove_dir_all(root.join("app")).unwrap();
    assert!(analysis::analyze(&root, &AnalyzeOptions::default()).is_err());
    let options = AnalyzeOptions {
        package: Some(SourcePath::new("apps/site", PathLimits::default()).unwrap()),
        ..Default::default()
    };
    let a = analysis::analyze(&root, &options).unwrap();
    assert!(
        a.assessment
            .files
            .iter()
            .all(|f| f.path.as_str().starts_with("apps/site/"))
    );
}

/// Reproducible, read-only M5 measurement on the approved D-043 checkouts.
/// FRAMESHIFT_ANALYSIS_DIRS is a semicolon-separated list of external roots.
#[test]
#[ignore = "requires the approved pinned M5 corpus outside the repository"]
fn external_analysis_sample() {
    use frameshift_engine::{
        foundation::{FindingCollector, FindingLimits},
        input::snapshot,
        package::load_workspace,
    };
    use std::collections::{BTreeMap, BTreeSet};
    let roots = std::env::var("FRAMESHIFT_ANALYSIS_DIRS").expect("set FRAMESHIFT_ANALYSIS_DIRS");
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut increment = |key: String, n: u64| *counts.entry(key).or_default() += n;
    for root in roots.split(';').filter(|s| !s.is_empty()) {
        let root = std::path::Path::new(root);
        let mut limits = Limits::default();
        limits.run.max_files = 400_000;
        limits.run.max_entries = 800_000;
        limits.run.max_total_bytes = 4 << 30;
        let mut findings = FindingCollector::new(FindingLimits::default());
        let snapshot = snapshot(root, limits.snapshot(), &mut findings).unwrap();
        let workspace = load_workspace(&snapshot, limits.package(), &mut findings).ok();
        let files: BTreeSet<_> = snapshot
            .inventory()
            .files()
            .as_slice()
            .iter()
            .map(SourcePath::as_str)
            .collect();
        let mut packages = Vec::new();
        for path in &files {
            let Some(package) = path.strip_suffix("package.json") else {
                continue;
            };
            if ["app/", "src/app/"].iter().any(|app| {
                let prefix = format!("{package}{app}");
                files
                    .range(prefix.as_str()..)
                    .take_while(|f| f.starts_with(&prefix))
                    .any(|file| {
                        ["/page.tsx", "/page.ts", "/page.jsx", "/page.js"]
                            .iter()
                            .any(|suffix| file.ends_with(suffix))
                    })
            }) {
                packages.push(package.to_owned());
            }
        }
        for package in packages {
            let member = package
                .strip_suffix('/')
                .map(|p| SourcePath::new(p, PathLimits::default()).unwrap());
            let at_workspace = workspace
                .as_ref()
                .is_some_and(|w| w.select(member.as_ref()).is_ok());
            let options = AnalyzeOptions {
                package: if at_workspace { member } else { None },
                limits,
            };
            let directory = if at_workspace {
                root.to_path_buf()
            } else {
                root.join(&package)
            };
            let result = analysis::analyze(&directory, &options);
            let a = match result {
                Ok(a) => a,
                Err(e) => {
                    increment(format!("error {e}"), 1);
                    continue;
                }
            };
            increment("packages".into(), 1);
            for unit in &a.assessment.units {
                increment(format!("kind {} count", unit.kind.as_str()), 1);
                increment(format!("kind {} weight", unit.kind.as_str()), unit.weight);
                increment(format!("status {} count", unit.status.as_str()), 1);
                increment(
                    format!("status {} weight", unit.status.as_str()),
                    unit.weight,
                );
                increment(format!("strategy {}", unit.strategy.as_str()), 1);
            }
            for route in &a.assessment.routes {
                increment(format!("route {}", route.status.as_str()), 1);
            }
            for file in &a.assessment.files {
                increment(format!("file {}", file.class.as_str()), 1);
            }
            let document = a.document().unwrap();
            StoredReport::parse(document.into_bytes(), &limits).unwrap();
            assert!(a.assessment.files.windows(2).all(|p| p[0].path < p[1].path));
        }
    }
    for (key, count) in counts {
        println!("{key}: {count}");
    }
}
/// Executes only the explicit, externally installed compatibility references,
/// never the real-world corpus. Install scripts must be disabled at setup.
#[test]
#[ignore = "requires pinned compatibility packages outside the repository"]
fn external_knowledge_fixtures() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let directory =
        std::env::var("FRAMESHIFT_COMPAT_MODULES").expect("set FRAMESHIFT_COMPAT_MODULES");
    let script = r#"
const path=require('node:path');
const assert=require('node:assert/strict');
const root=process.env.FRAMESHIFT_COMPAT_MODULES;
const load=(name)=>require(path.join(root,name));
for(const [name,version] of [['clsx','2.1.1'],['classnames','2.5.1'],['preact','10.29.8'],['preact-render-to-string','6.6.1']]) {
  assert.equal(load(name+'/package.json').version,version);
}
const {h}=load('preact');const render=load('preact-render-to-string');
for(const name of ['clsx','classnames']) {
  const classes=load(name);
  for(const [args,expected] of [
    [['a',false,null,undefined,0,'b'],'a b'],
    [[['a',['b',{c:true,d:false}]],{e:1,f:0}],'a b c e'],
    [[{active:true,hidden:false},'theme'],'active theme'],
    [[1,2,'x'],'1 2 x']
  ]) {
    assert.equal(classes(...args),expected);
    assert.equal(render(h('div',{className:classes(...args)},'fixture')),`<div class="${expected}">fixture</div>`);
  }
}
console.log('knowledge fixtures: clsx 2.1.1 and classnames 2.5.1 with preact 10.29.8 passed');
"#;
    let mut child = Command::new("node")
        .arg("-")
        .env("FRAMESHIFT_COMPAT_MODULES", directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8(result.stdout).unwrap());
}

#[test]
fn assessment_limits_fail_explicitly_and_report_byte_boundary_is_inclusive() {
    let root = create("limits", &[("app/page.tsx", PAGE)]);
    let normal = run(&root);
    let bytes = normal.document().unwrap().len();
    for (name, limit) in [("units", 0), ("occurrences", 0), ("dependencies", 0)] {
        let mut options = AnalyzeOptions::default();
        match name {
            "units" => options.limits.analysis.max_units = limit,
            "occurrences" => options.limits.analysis.max_occurrences = limit,
            "dependencies" => options.limits.analysis.max_dependency_work = limit as u64,
            _ => unreachable!(),
        }
        assert!(analysis::analyze(&root, &options).is_err(), "{name}");
    }
    let mut options = AnalyzeOptions::default();
    options.limits.analysis.max_report_bytes = bytes;
    let exact = analysis::analyze(&root, &options)
        .unwrap()
        .document()
        .unwrap()
        .len();
    options.limits.analysis.max_report_bytes = exact;
    assert_eq!(
        analysis::analyze(&root, &options)
            .unwrap()
            .document()
            .unwrap()
            .len(),
        exact
    );
    options.limits.analysis.max_report_bytes = exact - 1;
    assert!(
        analysis::analyze(&root, &options)
            .unwrap()
            .document()
            .is_err()
    );
}

#[test]
fn island_props_keep_unknown_types_and_promises_visible() {
    for (name, prop, feature, status) in [
        (
            "plain",
            "data={{name:'x',items:[1,2]}}",
            "boundary.props",
            Status::Auto,
        ),
        (
            "jsx",
            "icon={<b>Icon</b>}",
            "boundary.jsx-props",
            Status::Review,
        ),
        (
            "dynamic",
            "data={unknownValue}",
            "boundary.dynamic-props",
            Status::Review,
        ),
        (
            "promise",
            "data={Promise.resolve(1)}",
            "boundary.promise-props",
            Status::Unsupported,
        ),
    ] {
        let page = format!(
            "import Counter from '../counter';export default function Page(){{return <Counter {prop}/>}}"
        );
        let root = create(
            name,
            &[
                ("app/page.tsx", &page),
                (
                    "counter.tsx",
                    "'use client';export default function Counter(props){return <p>Counter</p>}",
                ),
            ],
        );
        let a = run(&root);
        assert!(
            a.assessment
                .occurrences
                .iter()
                .any(|o| o.occurrence.feature.id == feature && o.status == Some(status)),
            "{name}"
        );
    }
}

#[test]
fn request_reading_get_is_a_blocker_while_an_unused_parameter_is_static() {
    for (name, source, status) in [
        (
            "request-read",
            "export function GET(request){return new Response(request.url)}",
            Status::Unsupported,
        ),
        (
            "unused-request",
            "export function GET(request){return new Response('fixed')}",
            Status::Review,
        ),
        (
            "destructured-request",
            "export const GET=({url})=>new Response(url)",
            Status::Unsupported,
        ),
    ] {
        let root = create(
            name,
            &[("app/page.tsx", PAGE), ("app/data/route.ts", source)],
        );
        let a = run(&root);
        let unit = a
            .assessment
            .units
            .iter()
            .find(|u| u.id.as_str() == "endpoint:/data")
            .unwrap();
        assert_eq!(unit.status, status, "{name}");
    }
}

#[test]
fn every_shipped_input_finding_has_an_explanation() {
    for source in [
        include_str!("../../frameshift-input/src/walk.rs"),
        include_str!("../../frameshift-input/src/snapshot.rs"),
        include_str!("../../frameshift-package/src/load.rs"),
        include_str!("../../frameshift-package/src/workspace.rs"),
        include_str!("../../frameshift-package/src/workspace/listing.rs"),
        include_str!("../../frameshift-nextjs/src/detect.rs"),
    ] {
        for (offset, _) in source.match_indices("FS") {
            if let Some(code) = source.get(offset..offset + 6)
                && frameshift_engine::foundation::FindingCode::new(code).is_ok()
            {
                assert!(analysis::knowledge::explain(code).is_some(), "{code}");
            }
        }
    }
}

fn golden_document() -> String {
    let root = create(
        "golden",
        &[
            (
                "app/page.tsx",
                "import Counter from '../counter';export default function Page(){return <Counter label=\"Count\"/>}",
            ),
            (
                "counter.tsx",
                "'use client';import {useState} from 'react';export default function Counter({label}){const [n,set]=useState(0);return <button onClick={()=>set(n+1)}>{label}: {n}</button>}",
            ),
            (
                "app/private/page.tsx",
                "import {cookies} from 'next/headers';export default function Private(){cookies();return <p>Private</p>}",
            ),
            ("public/logo.svg", "fictional asset"),
        ],
    );
    run(&root).document().unwrap().replace(
        frameshift_engine::version::info().engine_version,
        "@ENGINE_VERSION@",
    )
}
#[test]
fn analysis_golden_matches_the_complete_evidence_document() {
    assert_eq!(
        golden_document(),
        fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/analysis.json")
        )
        .unwrap()
    );
}
#[test]
#[ignore = "explicit golden update"]
fn update_analysis_golden() {
    fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/analysis.json"),
        golden_document(),
    )
    .unwrap();
}

#[test]
fn a_direct_children_slot_has_no_value_dependent_conversion_assumption() {
    let root = create("slot", &[("app/page.tsx", PAGE)]);
    let a = run(&root);
    let layout = a
        .assessment
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Layout)
        .unwrap();
    assert_eq!(layout.strategy, Strategy::Astro);
    assert_eq!(layout.status, Status::Auto);
}

#[test]
fn duplicate_route_patterns_are_source_evidence_rather_than_an_engine_error() {
    let root = create(
        "route-collision",
        &[("app/(one)/page.tsx", PAGE), ("app/(two)/page.tsx", PAGE)],
    );
    let a = run(&root);
    assert_eq!(a.assessment.routes.len(), 1);
    assert_eq!(a.assessment.routes[0].status, Status::Unknown);
    StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
}

#[test]
fn convention_strategy_uses_its_entry_and_combines_owned_helpers_in_any_order() {
    let helper = "function Helper(){const text='Hello';return <p>{text}</p>}";
    for (name, path, entry, helper, expected) in [
        (
            "page",
            "app/page.tsx",
            "export default function Page(){return <Helper/>}",
            helper,
            Strategy::Astro,
        ),
        (
            "async-page",
            "app/page.tsx",
            "export default async function Page(){return <Helper/>}",
            helper,
            Strategy::Astro,
        ),
        (
            "layout",
            "app/layout.tsx",
            "export default function Layout({children}){return <html><body><Helper/>{children}</body></html>}",
            helper,
            Strategy::Astro,
        ),
        (
            "preact-page",
            "app/page.tsx",
            "export default function Page(){const text='Hello';return <Helper text={text}/>}",
            "function Helper({text}){return <p>{text}</p>}",
            Strategy::PreactStatic,
        ),
        (
            "unmapped-helper",
            "app/page.tsx",
            "export default function Page(){return <Helper/>}",
            "async function Helper(){const text='Hello';return <p>{text}</p>}",
            Strategy::Preserve,
        ),
    ] {
        let mut results = Vec::new();
        for source in [format!("{entry}{helper}"), format!("{helper}{entry}")] {
            let root = create(name, &[("app/page.tsx", PAGE), (path, &source)]);
            let analysis = run(&root);
            let unit = analysis
                .assessment
                .units
                .iter()
                .find(|u| u.path.as_ref().is_some_and(|p| p.as_str() == path))
                .unwrap();
            assert_eq!(unit.strategy, expected, "{name}");
            assert_eq!(
                unit.status,
                if expected == Strategy::Preserve {
                    Status::Unsupported
                } else {
                    Status::Auto
                },
                "{name}"
            );
            results.push((
                unit.strategy,
                unit.status,
                analysis.assessment.routes[0].status,
            ));
            StoredReport::parse(
                analysis.document().unwrap().into_bytes(),
                &Limits::default(),
            )
            .unwrap();
        }
        assert_eq!(results[0], results[1], "{name}");
    }
}
#[test]
fn implicit_layout_rendering_participates_in_conversion_demotion() {
    let root = create(
        "implicit-layout",
        &[
            (
                "app/layout.tsx",
                "import {Children} from 'react';export default function Layout({children}){return <html><body>{Children.toArray(children)}</body></html>}",
            ),
            (
                "app/page.tsx",
                "export default async function Page(){return <p>Page</p>}",
            ),
        ],
    );
    let a = run(&root);
    assert_eq!(
        a.assessment
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Route)
            .unwrap()
            .status,
        Status::Unsupported
    );
}
#[test]
fn image_assessment_requires_local_source_provenance_and_astro_context() {
    for (config, imports, attrs, body, expected) in [
        (
            "{images:{unoptimized:true}}",
            "",
            "src='/photo.png'",
            "return <Image SRC alt='' width={10} height={10}/>",
            Status::Auto,
        ),
        (
            "{images:{unoptimized:true}}",
            "",
            "src='https://example.test/photo.png'",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{images:{unoptimized:true}}",
            "",
            "src='//example.test/photo.png'",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{images:{unoptimized:true}}",
            "import photo from '../assets/photo.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Auto,
        ),
        (
            "{}",
            "import photo from '../assets/photo.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Auto,
        ),
        (
            "{}",
            "import {default as photo} from '../assets/photo.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Auto,
        ),
        (
            "{images:{unoptimized:true}}",
            "const photo='/photo.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "import photo from '../assets/photo.png';",
            "src={photo}",
            "const value=1;return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "import photo from '../assets/photo.png';",
            "src={photo} fill",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "import photo from '../assets/photo.png';",
            "src={photo} placeholder='blur'",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "import photo from '../assets/photo.png';",
            "src={photo} {...props}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{images:{loader:'custom',loaderFile:'./loader.ts'}}",
            "import photo from '../assets/photo.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{images:{unoptimized:true}}",
            "",
            "src='/photo.png' unoptimized={false}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{images:{unoptimized:true}}",
            "",
            "src='/photo.png' unoptimized={setting}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "",
            "src='/photo.png' unoptimized",
            "return <Image SRC/>",
            Status::Auto,
        ),
        (
            "{}",
            "import photo from '../assets/missing.png';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Review,
        ),
        (
            "{}",
            "import photo from '../assets/data.csv';",
            "src={photo}",
            "return <Image SRC/>",
            Status::Review,
        ),
    ] {
        let source = format!(
            "import Image from 'next/image';{imports}export default function Page(){{{}}}",
            body.replace("SRC", attrs)
        );
        let config = format!("export default {config}");
        let root = create(
            "image-provenance",
            &[
                ("app/page.tsx", &source),
                ("next.config.mjs", &config),
                ("assets/photo.png", "fictional image bytes"),
                ("assets/data.csv", "a,b"),
            ],
        );
        let a = run(&root);
        let image = a
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.api.image")
            .unwrap();
        assert_eq!(image.status, Some(expected), "{source}; {config}");
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
    for source in [
        "'use client';import Image from 'next/image';import photo from '../assets/photo.png';export default function Page(){return <Image src={photo}/>}",
        "import Image from 'next/image';import photo from '../assets/photo.png';export default function Page(){return <Helper/>}function Helper(){const n=1;return <Image src={photo}/>}",
        "import Image from 'next/image';import photo from '../assets/photo.png';export default function Page(){const photo='/photo.png';return <Image src={photo}/>}",
    ] {
        let root = create(
            "image-renderer",
            &[
                ("app/page.tsx", source),
                ("next.config.mjs", "export default {}"),
                ("assets/photo.png", "fictional image bytes"),
            ],
        );
        let a = run(&root);
        assert_eq!(
            a.assessment
                .occurrences
                .iter()
                .find(|o| o.occurrence.feature.id == "nextjs.api.image")
                .unwrap()
                .status,
            Some(Status::Review),
            "{source}"
        );
    }
}
#[test]
fn runtime_reachability_overrides_test_like_file_names() {
    for path in [
        "app/lib/session.spec.ts",
        "tests/session.ts",
        "app/__tests__/session.ts",
    ] {
        let source = format!(
            "import {{session}} from '../{}';export default function Page(){{return <p>{{session()}}</p>}}",
            path.strip_suffix(".ts").unwrap()
        );
        let root = create(
            "runtime-test-name",
            &[
                ("app/page.tsx", &source),
                (
                    path,
                    "import {cookies} from 'next/headers';export function session(){return cookies()}",
                ),
                (
                    "tests/unused.spec.ts",
                    "import {cookies} from 'next/headers';cookies();",
                ),
            ],
        );
        let a = run(&root);
        let file = a
            .assessment
            .files
            .iter()
            .find(|f| f.path.as_str() == path)
            .unwrap();
        assert_eq!(file.class, FileClass::Units, "{path}");
        let request = a
            .assessment
            .occurrences
            .iter()
            .find(|o| {
                o.occurrence.feature.id == "nextjs.api.request"
                    && o.occurrence
                        .location
                        .path()
                        .is_some_and(|p| p.as_str() == path)
            })
            .unwrap();
        assert_eq!(request.status, Some(Status::Unsupported), "{path}");
        assert!(request.unit.is_some());
        assert_eq!(a.assessment.routes[0].status, Status::Unsupported);
        assert!(
            a.assessment.routes[0]
                .closure
                .contains(request.unit.as_ref().unwrap())
        );
        assert_eq!(
            a.assessment
                .files
                .iter()
                .find(|f| f.path.as_str() == "tests/unused.spec.ts")
                .unwrap()
                .class,
            FileClass::OutOfScope
        );
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
}
#[test]
fn independent_single_segment_enumeration_is_automatic() {
    for (segment, generator, page, expected) in [
        (
            "[slug]",
            "export function generateStaticParams(){return [{slug:'a'}]}",
            "export default function Page(){return <p/>}",
            Status::Auto,
        ),
        (
            "[slug]",
            "export async function generateStaticParams(){const slug='a';return [{slug}]}",
            "export default function Page({params}){return <p>{params.slug}</p>}",
            Status::Auto,
        ),
        (
            "[slug]",
            "export const generateStaticParams=()=>[{slug:'a'}]",
            "export default function Page(){return <p/>}",
            Status::Auto,
        ),
        (
            "[slug]",
            "const enumerate=()=>[{slug:'a'}];export {enumerate as generateStaticParams}",
            "export default function Page(){return <p/>}",
            Status::Auto,
        ),
        (
            "[slug]",
            "export function generateStaticParams({params}){return [{slug:params.slug}]}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "const slug='a';export function generateStaticParams(){return [{slug}]}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return arguments[0]}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return this.params}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return eval('[]')}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function* generateStaticParams(){yield {slug:'a'}}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return [{slug:'a'}]}",
            "export default function Page({params}){return <p>{JSON.stringify(params)}</p>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return [{slug:'a'}]}",
            "export default function Page({params}){return <p>{params.other}</p>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return [{slug:'a'}]}",
            "export default function Page(props){return <p>{props.params.slug}</p>}",
            Status::Review,
        ),
        (
            "[slug]",
            "export function generateStaticParams(){return [{slug:'a'}]}",
            "export default async function Page({params}){const {slug}=await params;return <p>{slug}</p>}",
            Status::Auto,
        ),
        (
            "[...slug]",
            "export function generateStaticParams(){return [{slug:['a','b']}]}",
            "export default async function Page({params}){const {slug}=await params;return <p>{slug}</p>}",
            Status::Auto,
        ),
        (
            "[...slug]",
            "export function generateStaticParams(){return [{slug:['a']}]}",
            "export default function Page({params}){return <p>{params.slug}</p>}",
            Status::Review,
        ),
        (
            "[[...slug]]",
            "export function generateStaticParams(){return [{slug:['a']}]}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
        (
            "[parent]/[slug]",
            "export function generateStaticParams(){return [{parent:'p',slug:'a'}]}",
            "export default function Page(){return <p/>}",
            Status::Review,
        ),
    ] {
        let path = format!("app/{segment}/page.tsx");
        let source = format!("{generator}\n{page}");
        let root = create("single-enumeration", &[(path.as_str(), source.as_str())]);
        let a = run(&root);
        let occurrences: Vec<_> = a
            .assessment
            .occurrences
            .iter()
            .filter(|o| {
                [
                    "nextjs.route.dynamic",
                    "nextjs.route.catch-all",
                    "nextjs.route.optional-catch-all",
                ]
                .contains(&o.occurrence.feature.id)
            })
            .collect();
        assert!(!occurrences.is_empty(), "{segment}: {generator}");
        for o in occurrences {
            assert_eq!(o.status, Some(expected), "{segment}: {generator} {page}");
        }
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
    let root = create(
        "layout-enumeration",
        &[
            (
                "app/[slug]/layout.tsx",
                "export function generateStaticParams(){return [{slug:'a'}]}export default function Layout({children}){return <section>{children}</section>}",
            ),
            (
                "app/[slug]/page.tsx",
                "export default function Page(){return <p/>}",
            ),
        ],
    );
    let a = run(&root);
    assert!(
        a.assessment
            .occurrences
            .iter()
            .filter(|o| o.occurrence.feature.id == "nextjs.route.dynamic")
            .all(|o| o.status == Some(Status::Auto))
    );
}
#[test]
fn base_path_ignores_excluded_links_and_retains_runtime_reachable_tests() {
    for path in ["tests/links.spec.tsx", "vitest.config.tsx", "lib/links.tsx"] {
        for reached in [false, true] {
            let import = format!("../{path}");
            let page = if reached {
                format!(
                    "import {{Widget}} from '{import}';export default function Page(){{return <Widget/>}}"
                )
            } else {
                PAGE.to_owned()
            };
            let root = create(
                &format!("base-path-scope-{}-{reached}", path.replace('/', "-")),
                &[
                    ("app/page.tsx", &page),
                    (
                        path,
                        "import Link from 'next/link';const destination='/about';export function Widget(){return <Link href={destination}/>}",
                    ),
                    ("next.config.mjs", "export default {basePath:'/docs'}"),
                ],
            );
            let a = run(&root);
            let excluded = !reached && path != "lib/links.tsx";
            let expected = if excluded {
                Status::Auto
            } else {
                Status::Review
            };
            let base = a
                .assessment
                .occurrences
                .iter()
                .find(|o| o.occurrence.feature.id == "nextjs.config.base-path")
                .unwrap();
            assert_eq!(base.status, Some(expected), "{path}: {reached}");
            assert_eq!(
                a.assessment
                    .units
                    .iter()
                    .find(|u| Some(u.id.as_str()) == base.unit.as_deref())
                    .unwrap()
                    .status,
                expected
            );
            let link = a
                .assessment
                .occurrences
                .iter()
                .find(|o| o.occurrence.feature.id == "nextjs.api.link")
                .unwrap();
            assert_eq!(link.status, (!excluded).then_some(Status::Review));
            assert_eq!(
                a.assessment
                    .files
                    .iter()
                    .find(|f| f.path.as_str() == path)
                    .unwrap()
                    .class,
                if excluded {
                    FileClass::OutOfScope
                } else {
                    FileClass::Units
                }
            );
            StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
        }
    }
}
#[test]
fn base_path_uses_only_binding_proven_next_link_sites() {
    for (body, expected) in [
        ("return <a href={destination}>Native</a>", Status::Auto),
        (
            "function Widget(props){return <p/>} return <Widget {...props}/>",
            Status::Auto,
        ),
        (
            "function Link(props){return <a/>} return <Link href={destination}/>",
            Status::Auto,
        ),
        (
            "return <><a href={destination}/><NextLink href='/about'/></>",
            Status::Auto,
        ),
        ("return <NextLink href={destination}/>", Status::Review),
        ("return <NextLink {...props}/>", Status::Review),
    ] {
        let source =
            format!("import NextLink from 'next/link';export default function Page(){{{body}}}");
        let root = create(
            "base-path-links",
            &[
                ("app/page.tsx", &source),
                ("next.config.mjs", "export default {basePath:'/docs'}"),
            ],
        );
        let a = run(&root);
        let base = a
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.config.base-path")
            .unwrap();
        assert_eq!(base.status, Some(expected), "{body}");
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
}
#[test]
fn link_literal_objects_preserve_mapping_and_base_path_assessments() {
    for (attributes, expected) in [
        ("href=\"/about\"", Status::Auto),
        ("href={{pathname:'/about'}}", Status::Auto),
        (
            "href={(({pathname:'/about',query:{q:'a b',page:2,tag:['one','two']},hash:'section'}) as const)}",
            Status::Auto,
        ),
        ("href={{pathname: destination}}", Status::Review),
        ("href={{...destination}}", Status::Review),
        ("href={{['pathname']:'/about'}}", Status::Review),
        ("href={{get pathname(){return '/about'}}}", Status::Review),
        ("href={{pathname: makePath()}}", Status::Review),
        ("href={{pathname:'/about'}} {...props}", Status::Review),
        (
            "href={{pathname:'/about'}} prefetch={false}",
            Status::Review,
        ),
        ("href={{pathname:'/about'}} replace", Status::Review),
        ("href={{pathname:'/about'}} scroll={false}", Status::Review),
        ("href={true}", Status::Review),
    ] {
        let source = format!(
            "import Link from 'next/link';export default function Page(){{return <Link {attributes}>About</Link>}}"
        );
        let root = create(
            "link-literal-object",
            &[
                ("app/page.tsx", &source),
                ("next.config.mjs", "export default {basePath:'/docs'}"),
            ],
        );
        let a = run(&root);
        let occurrence = a
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.api.link")
            .unwrap();
        assert_eq!(occurrence.status, Some(expected), "{attributes}");
        let owner = occurrence.unit.as_ref().unwrap();
        assert_eq!(
            a.assessment
                .units
                .iter()
                .find(|u| u.id.as_str() == owner)
                .unwrap()
                .status,
            expected,
            "{attributes}"
        );
        assert_eq!(a.assessment.routes[0].status, expected, "{attributes}");
        let base = a
            .assessment
            .occurrences
            .iter()
            .find(|o| o.occurrence.feature.id == "nextjs.config.base-path")
            .unwrap();
        let computed_href = attributes.contains("destination")
            || attributes.contains("makePath")
            || attributes.contains("['pathname']")
            || attributes.contains("get pathname")
            || attributes.contains("...props")
            || attributes.contains("true");
        assert_eq!(
            base.status,
            Some(if computed_href {
                Status::Review
            } else {
                Status::Auto
            }),
            "{attributes}"
        );
        StoredReport::parse(a.document().unwrap().into_bytes(), &Limits::default()).unwrap();
    }
}
#[test]
fn a_proven_next_link_mapping_does_not_block_async_astro_conversion() {
    let root = create(
        "async-link",
        &[(
            "app/page.tsx",
            "import Link from 'next/link';export default async function Page(){return <Link href=\"/about\">About</Link>}",
        )],
    );
    let a = run(&root);
    let page = a
        .assessment
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Route)
        .unwrap();
    assert_eq!(page.strategy, Strategy::Astro);
    assert_ne!(page.status, Status::Unsupported);
}
