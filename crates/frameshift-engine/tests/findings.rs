//! Finding contracts through the public engine foundation facade.

use frameshift_engine::foundation::{
    FeatureId, Finding, FindingCode, FindingCollector, FindingContext, FindingError, FindingLimits,
    FindingLocation, FindingText, IdentifierLimits, PathLimits, RuleId, Severity, SourcePath,
    SourceText, Span, TextError, TextLimits, TextLocation, UnitId, UnitKind,
};

fn path(value: &str) -> SourcePath {
    SourcePath::new(value, PathLimits::default()).unwrap()
}

fn source(value: &str) -> SourceText {
    SourceText::new(value.to_owned(), TextLimits::default()).unwrap()
}

fn text_location(file: &str, text: &str, start: usize, end: usize) -> FindingLocation {
    FindingLocation::Text(
        TextLocation::new(path(file), &source(text), Span::new(start, end).unwrap()).unwrap(),
    )
}

fn finding(severity: Severity, location: FindingLocation, message: &str) -> Finding {
    record(
        severity,
        location,
        "FS3001",
        FindingContext::default(),
        message,
        "Why",
        "Fix",
    )
}

fn record(
    severity: Severity,
    location: FindingLocation,
    code: &str,
    context: FindingContext,
    message: &str,
    explanation: &str,
    remediation: &str,
) -> Finding {
    Finding::new(
        FindingCode::new(code).unwrap(),
        severity,
        location,
        context,
        FindingText {
            message,
            explanation,
            remediation,
        },
        FindingLimits::default(),
    )
    .unwrap()
}

#[test]
fn severities_have_explicit_reporting_order_and_spelling() {
    let expected = [
        Severity::Error,
        Severity::Blocker,
        Severity::Unknown,
        Severity::Review,
        Severity::Info,
    ];
    assert_eq!(
        expected.map(Severity::as_str),
        ["error", "blocker", "unknown", "review", "info"]
    );
    for pair in expected.windows(2) {
        assert!(pair[0] < pair[1]);
    }
}

#[test]
fn locations_derive_unicode_bom_crlf_and_exclusive_end_positions() {
    let input = source("\u{feff}é\t🦀\r\nz");
    let location = TextLocation::new(path("src/é.tsx"), &input, Span::new(3, 12).unwrap()).unwrap();
    assert_eq!(location.file().as_str(), "src/é.tsx");
    assert_eq!(location.span(), Span::new(3, 12).unwrap());
    assert_eq!(
        (
            location.start().offset,
            location.start().line,
            location.start().column
        ),
        (3, 1, 2)
    );
    assert_eq!(
        (
            location.end().offset,
            location.end().line,
            location.end().column
        ),
        (12, 2, 1)
    );
    let inside_crlf = TextLocation::new(path("x"), &input, Span::new(11, 11).unwrap()).unwrap();
    assert_eq!(
        (inside_crlf.start().line, inside_crlf.start().column),
        (1, 6)
    );
    for text in ["", "é", "x\n", "x\u{2028}y\u{2029}"] {
        let input = source(text);
        let eof = TextLocation::new(
            path("x"),
            &input,
            Span::new(text.len(), text.len()).unwrap(),
        )
        .unwrap();
        assert_eq!(eof.start(), input.position(text.len()).unwrap());
        assert_eq!(eof.start(), eof.end());
    }
}

#[test]
fn locations_reject_invalid_scalar_boundaries_and_source_bounds() {
    let input = source("é");
    for (start, end, expected) in [
        (1, 2, TextError::NotCharBoundary { offset: 1 }),
        (0, 1, TextError::NotCharBoundary { offset: 1 }),
        (0, 3, TextError::OutOfBounds { offset: 3, len: 2 }),
    ] {
        assert_eq!(
            TextLocation::new(path("x"), &input, Span::new(start, end).unwrap()),
            Err(expected)
        );
    }
    assert_eq!(FindingLocation::Project.file(), None);
    assert_eq!(
        FindingLocation::File(path("invalid-encoding.ts")).file(),
        Some(&path("invalid-encoding.ts"))
    );
}

#[test]
fn path_precedes_scope_and_ranges_in_location_order() {
    let expected = vec![
        FindingLocation::Project,
        FindingLocation::File(path("a")),
        text_location("a", "abc", 0, 0),
        text_location("a", "abc", 0, 1),
        text_location("a", "abc", 1, 2),
        FindingLocation::File(path("b")),
        text_location("b", "abc", 0, 1),
        FindingLocation::File(path("é")),
    ];
    let mut shuffled = expected.clone();
    shuffled.reverse();
    shuffled.sort();
    assert_eq!(shuffled, expected);
    assert_ne!(
        FindingLocation::File(path("é")),
        FindingLocation::File(path("e\u{301}"))
    );
    assert_ne!(
        FindingLocation::File(path("A")),
        FindingLocation::File(path("a"))
    );
    // Equal byte spans from distinct snapshots still compare all derived coordinates.
    let a = text_location("a", "a\nb", 2, 3);
    let b = text_location("a", "abc", 2, 3);
    assert_ne!(a, b);
    assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
}

#[test]
fn record_keeps_typed_context_and_prose_without_source_snippets() {
    let limits = IdentifierLimits::default();
    let context = FindingContext {
        unit: Some(
            UnitId::for_file(UnitKind::Component, &path("src/a.tsx"), Some("A"), limits).unwrap(),
        ),
        feature: Some(FeatureId::new("component.island", limits).unwrap()),
        rule: Some(RuleId::new("nextjs-to-astro-preact/component.island", limits).unwrap()),
    };
    let location = text_location("src/a.tsx", "abc", 1, 2);
    let record = record(
        Severity::Review,
        location.clone(),
        "FS9999",
        context.clone(),
        "Review é",
        "Reason\r\n\u{1b}[31m<script>",
        "Fix\nthis",
    );
    assert_eq!(record.code().as_str(), "FS9999");
    assert_eq!(record.severity(), Severity::Review);
    assert_eq!(record.context(), &context);
    assert_eq!(record.location(), &location);
    assert_eq!(record.message(), "Review é");
    assert_eq!(record.explanation(), "Reason\r\n\u{1b}[31m<script>");
    assert_eq!(record.remediation(), "Fix\nthis");
}

fn construct(text: FindingText<'_>, limit: usize) -> Result<Finding, FindingError> {
    Finding::new(
        FindingCode::new("FS1001").unwrap(),
        Severity::Error,
        FindingLocation::Project,
        FindingContext::default(),
        text,
        FindingLimits {
            max_text_bytes: limit,
            max_findings: 0,
        },
    )
}

#[test]
fn prose_limits_count_utf8_bytes_before_content_checks() {
    let text = FindingText {
        message: "é",
        explanation: "ok",
        remediation: "ok",
    };
    assert!(construct(text, 2).is_ok());
    assert_eq!(construct(text, 1), Err(FindingError::TextTooLong));
    for text in [
        FindingText {
            message: "",
            explanation: "long",
            remediation: "ok",
        },
        FindingText {
            message: "long",
            explanation: "",
            remediation: "ok",
        },
        FindingText {
            message: "ok",
            explanation: "",
            remediation: "long",
        },
    ] {
        assert_eq!(construct(text, 3), Err(FindingError::TextTooLong));
    }
    assert_eq!(
        construct(
            FindingText {
                message: "",
                explanation: "",
                remediation: ""
            },
            0
        ),
        Err(FindingError::InvalidText)
    );
}

#[test]
fn rejects_blank_nul_and_multiline_summaries_without_echoing_input() {
    for bad in [
        "",
        " ",
        "\u{2003}",
        "secret\0",
        "secret\n",
        "secret\r",
        "secret\t",
        "secret\u{1b}",
        "secret\u{85}",
        "secret\u{2028}",
        "secret\u{2029}",
    ] {
        let error = construct(
            FindingText {
                message: bad,
                explanation: "Why",
                remediation: "Fix",
            },
            100,
        )
        .unwrap_err();
        assert_eq!(error, FindingError::InvalidText);
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
    for bad in ["", " \r\n", "secret\0"] {
        for text in [
            FindingText {
                message: "ok",
                explanation: bad,
                remediation: "ok",
            },
            FindingText {
                message: "ok",
                explanation: "ok",
                remediation: bad,
            },
        ] {
            assert_eq!(construct(text, 100), Err(FindingError::InvalidText));
        }
    }
}

#[test]
fn every_record_field_breaks_ties_in_the_documented_order() {
    let limits = IdentifierLimits::default();
    let context = FindingContext::default();
    let unit = FindingContext {
        unit: Some(UnitId::for_config("nextjs.images", limits).unwrap()),
        ..context.clone()
    };
    let feature = FindingContext {
        feature: Some(FeatureId::new("nextjs.fetch", limits).unwrap()),
        ..context.clone()
    };
    let rule = FindingContext {
        rule: Some(RuleId::new("a-to-b/area.name", limits).unwrap()),
        ..context.clone()
    };
    let make = |code: &str,
                context: FindingContext,
                message: &str,
                explanation: &str,
                remediation: &str| {
        record(
            Severity::Info,
            FindingLocation::Project,
            code,
            context,
            message,
            explanation,
            remediation,
        )
    };
    let base = make("FS1000", context.clone(), "a", "a", "a");
    let variations = [
        make("FS1000", context.clone(), "a", "a", "b"),
        make("FS1000", context.clone(), "a", "b", "a"),
        make("FS1000", context.clone(), "b", "a", "a"),
        make("FS1000", rule, "a", "a", "a"),
        make("FS1000", feature, "a", "a", "a"),
        make("FS1000", unit, "a", "a", "a"),
        make("FS1001", context, "a", "a", "a"),
    ];
    let mut previous = &base;
    for next in &variations {
        assert!(previous < next);
        previous = next;
    }
}

#[test]
fn all_input_permutations_assign_identical_ids_and_preserve_duplicates() {
    let input = [
        finding(Severity::Info, FindingLocation::Project, "info"),
        finding(Severity::Error, FindingLocation::File(path("b")), "error b"),
        finding(Severity::Error, FindingLocation::File(path("a")), "error a"),
        finding(Severity::Error, FindingLocation::File(path("a")), "error a"),
    ];
    let mut expected = None;
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let indices = [a, b, c, d];
                    if (0..4).any(|i| (i + 1..4).any(|j| indices[i] == indices[j])) {
                        continue;
                    }
                    let mut collector = FindingCollector::new(FindingLimits::default());
                    for index in indices {
                        collector.push(input[index].clone()).unwrap();
                    }
                    let list = collector.finish();
                    assert_eq!(list.as_slice().len(), 4);
                    let numbered: Vec<_> = list
                        .iter()
                        .map(|(id, f)| (id.to_string(), id.number(), f.message().to_owned()))
                        .collect();
                    assert_eq!(
                        numbered.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
                        ["F0001", "F0002", "F0003", "F0004"]
                    );
                    assert_eq!(
                        numbered.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(),
                        ["error a", "error a", "error b", "info"]
                    );
                    if let Some(expected) = &expected {
                        assert_eq!(&numbered, expected);
                    } else {
                        expected = Some(numbered);
                    }
                }
            }
        }
    }
}

#[test]
fn collector_limits_fail_explicitly_without_losing_existing_evidence() {
    let item = finding(Severity::Info, FindingLocation::Project, "ok");
    let mut zero = FindingCollector::new(FindingLimits {
        max_findings: 0,
        max_text_bytes: 0,
    });
    assert_eq!(zero.push(item.clone()), Err(FindingError::TooManyFindings));
    assert_eq!(zero.finish().iter().len(), 0);
    let mut collector = FindingCollector::new(FindingLimits {
        max_findings: 1,
        max_text_bytes: 3,
    });
    let oversized = finding(Severity::Info, FindingLocation::Project, "long");
    assert_eq!(collector.push(oversized), Err(FindingError::TextTooLong));
    collector.push(item.clone()).unwrap();
    assert_eq!(
        collector.push(item.clone()),
        Err(FindingError::TooManyFindings)
    );
    let list = collector.finish();
    assert_eq!(list.as_slice(), &[item]);
    assert_eq!(list.iter().next().unwrap().0.number(), 1);
}

#[test]
fn numbering_expands_beyond_four_digits_without_truncation() {
    let mut collector = FindingCollector::new(FindingLimits::default());
    let item = finding(Severity::Info, FindingLocation::Project, "ok");
    for _ in 0..10_001 {
        collector.push(item.clone()).unwrap();
    }
    let list = collector.finish();
    assert_eq!(list.iter().nth(9998).unwrap().0.to_string(), "F9999");
    assert_eq!(list.iter().nth(9999).unwrap().0.to_string(), "F10000");
    assert_eq!(list.iter().last().unwrap().0.to_string(), "F10001");
}
