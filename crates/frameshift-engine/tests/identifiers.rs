//! Stable identifier contracts through the public engine facade.

use std::collections::BTreeSet;

use frameshift_engine::foundation::{
    FeatureId, FindingCode, IdentifierError, IdentifierLimits, PathLimits, RuleId, SourcePath,
    UnitId, UnitKind,
};

fn path(value: &str) -> SourcePath {
    SourcePath::new(value, PathLimits::default()).unwrap()
}

fn file(value: &str, export: Option<&str>) -> UnitId {
    UnitId::for_file(
        UnitKind::Component,
        &path(value),
        export,
        IdentifierLimits::default(),
    )
    .unwrap()
}

#[test]
fn finding_codes_preserve_digits_without_claiming_catalog_membership() {
    for number in 0..=9999 {
        let value = format!("FS{number:04}");
        assert_eq!(FindingCode::new(&value).unwrap().as_str(), value);
    }
    for value in [
        "",
        "FS1",
        "FS00001",
        "fs1003",
        " FS1003",
        "FS1003\n",
        "FS١٠٠٣",
        "FS1e03",
    ] {
        assert_eq!(
            FindingCode::new(value),
            Err(IdentifierError::InvalidFindingCode)
        );
    }
}

#[test]
fn catalog_ids_preserve_valid_names_and_expose_rule_parts() {
    for value in [
        "nextjs-to-astro-preact/route.static-page",
        "custom-source-to-custom-target/config.package-manifest",
        "a-to-b/x.y1",
    ] {
        let id = RuleId::new(value, IdentifierLimits::default()).unwrap();
        assert_eq!(id.as_str(), value);
        assert_eq!(format!("{}/{}", id.migration_path(), id.name()), value);
    }
    for value in [
        "nextjs.api.link",
        "react.hook.use-state",
        "react.hook.use-state.browser",
        "a.b.c1",
    ] {
        assert_eq!(
            FeatureId::new(value, IdentifierLimits::default())
                .unwrap()
                .as_str(),
            value
        );
    }
}

#[test]
fn rejects_noncanonical_catalog_ids_without_repair() {
    for value in [
        "",
        "route.static-page",
        "a/b.c",
        "-to-b/x.y",
        "a-to-/x.y",
        "a-to-b-to-c/x.y",
        "a-to-b/x.y/z",
        "a-to-b/x",
        "a-to-b/x.y.z",
        "a-to-b/.y",
        "a-to-b/x.",
        "a-to-b/X.y",
        "a-to-b/x.y-",
        "a-to-b/x.y--z",
        "a-to-b/x.1y",
        "a-to-b/x.é",
        "a-to-b/x.y%23",
        "a-to-b/x.y\n",
    ] {
        assert_eq!(
            RuleId::new(value, IdentifierLimits::default()),
            Err(IdentifierError::InvalidRuleId),
            "{value:?}"
        );
    }
    for value in [
        "",
        "nextjs.link",
        "nextjs..link",
        ".api.link",
        "nextjs.api.link.",
        "Nextjs.api.link",
        "nextjs.api.link/",
        "nextjs.api.link-",
        "nextjs.api.link--x",
        "nextjs.api.0link",
        "nextjs.api.línk",
        "nextjs.api.link\u{1b}",
    ] {
        assert_eq!(
            FeatureId::new(value, IdentifierLimits::default()),
            Err(IdentifierError::InvalidFeatureId),
            "{value:?}"
        );
    }
}

#[test]
fn limits_precede_syntax_and_are_inclusive() {
    let limits = IdentifierLimits { max_bytes: 5 };
    assert!(FeatureId::new("a.b.c", limits).is_ok());
    assert_eq!(
        FeatureId::new("a.b.cd", limits),
        Err(IdentifierError::TooLong { bytes: 6, limit: 5 })
    );
    assert_eq!(
        FeatureId::new("INVALID", limits),
        Err(IdentifierError::TooLong { bytes: 7, limit: 5 })
    );
    assert_eq!(
        RuleId::new("INVALID", limits),
        Err(IdentifierError::TooLong { bytes: 7, limit: 5 })
    );
    assert!(RuleId::new("a-to-b/x.y", IdentifierLimits { max_bytes: 10 }).is_ok());
    assert_eq!(
        RuleId::new("a-to-b/x.y", IdentifierLimits { max_bytes: 9 }),
        Err(IdentifierError::TooLong {
            bytes: 10,
            limit: 9
        })
    );
    assert_eq!(
        FeatureId::new("", IdentifierLimits { max_bytes: 0 }),
        Err(IdentifierError::InvalidFeatureId)
    );
}

#[test]
fn ordering_is_bytewise_and_errors_do_not_echo_input() {
    let values = [
        "nextjs.api.link",
        "react.hook.use-state",
        "astro.directive.client-load",
        "nextjs.api.image",
    ];
    let ids: BTreeSet<_> = values
        .iter()
        .rev()
        .map(|value| FeatureId::new(value, IdentifierLimits::default()).unwrap())
        .collect();
    let mut expected = values.to_vec();
    expected.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(
        ids.iter().map(FeatureId::as_str).collect::<Vec<_>>(),
        expected
    );
    let secret = "private-value\u{1b}[31m";
    for error in [
        FindingCode::new(secret).unwrap_err(),
        FeatureId::new(secret, IdentifierLimits::default()).unwrap_err(),
        RuleId::new(secret, IdentifierLimits::default()).unwrap_err(),
    ] {
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("private-value"));
            assert!(!text.contains('\u{1b}'));
        }
    }
}

#[test]
fn unit_ids_match_the_documented_examples_and_enforce_subject_categories() {
    let limits = IdentifierLimits::default();
    for (kind, value, export, expected) in [
        (
            UnitKind::Layout,
            "app/blog/layout.tsx",
            None,
            "layout:app/blog/layout.tsx",
        ),
        (
            UnitKind::Provider,
            "components/cart/CartProvider.tsx",
            Some("CartProvider"),
            "provider:components/cart/CartProvider.tsx#CartProvider",
        ),
        (
            UnitKind::Component,
            "components/Hero.tsx",
            Some("Hero"),
            "component:components/Hero.tsx#Hero",
        ),
        (
            UnitKind::RouteFile,
            "app/loading.tsx",
            None,
            "route-file:app/loading.tsx",
        ),
        (
            UnitKind::Module,
            "lib/posts.ts",
            None,
            "module:lib/posts.ts",
        ),
        (
            UnitKind::Stylesheet,
            "app/globals.css",
            None,
            "stylesheet:app/globals.css",
        ),
    ] {
        assert_eq!(
            UnitId::for_file(kind, &path(value), export, limits)
                .unwrap()
                .as_str(),
            expected
        );
        assert_eq!(
            UnitId::for_route(kind, "/", limits),
            Err(IdentifierError::InvalidUnitSubject)
        );
    }
    for (kind, pattern, expected) in [
        (UnitKind::Route, "/", "route:/"),
        (UnitKind::Route, "/blog/[slug]", "route:/blog/[slug]"),
        (
            UnitKind::Route,
            "/docs/[[...slug]]/",
            "route:/docs/[[...slug]]/",
        ),
        (UnitKind::Endpoint, "/feed.xml", "endpoint:/feed.xml"),
    ] {
        assert_eq!(
            UnitId::for_route(kind, pattern, limits).unwrap().as_str(),
            expected
        );
        assert_eq!(
            UnitId::for_file(kind, &path("file.ts"), None, limits),
            Err(IdentifierError::InvalidUnitSubject)
        );
    }
    assert_eq!(
        UnitId::for_config("nextjs.images", limits)
            .unwrap()
            .as_str(),
        "config:nextjs.images"
    );
    assert_eq!(
        UnitId::for_file(UnitKind::Config, &path("file.ts"), None, limits),
        Err(IdentifierError::InvalidUnitSubject)
    );
    assert_eq!(
        UnitId::for_route(UnitKind::Config, "/", limits),
        Err(IdentifierError::InvalidUnitSubject)
    );
}

#[test]
fn encoding_keeps_subject_export_and_literal_percent_sequences_distinct() {
    for (value, export, expected) in [
        ("a#b", None, "component:a%23b"),
        ("a", Some("b"), "component:a#b"),
        ("a%23b", None, "component:a%2523b"),
        (
            "dir/ads:stream",
            Some("a#b%"),
            "component:dir/ads%3Astream#a%23b%25",
        ),
        (
            "é/e\u{301}/🦀.tsx",
            Some("$Hero"),
            "component:%C3%A9/e%CC%81/%F0%9F%A6%80.tsx#%24Hero",
        ),
        (
            "src/(marketing)/[...slug].tsx",
            None,
            "component:src/(marketing)/[...slug].tsx",
        ),
        ("CON.ts", None, "component:CON.ts"),
        ("a \n\u{1b}[31m", None, "component:a%20%0A%1B[31m"),
    ] {
        let id = file(value, export);
        assert_eq!(id.as_str(), expected);
        assert_eq!(id, file(value, export));
        assert!(id.as_str().is_ascii());
        assert!(!id.as_str().chars().any(char::is_control));
        assert_eq!(path(value).as_str(), value);
    }
    assert_ne!(file("é", None), file("e\u{301}", None));
    assert_ne!(file("A.ts", None), file("a.ts", None));
    let limits = IdentifierLimits::default();
    assert_eq!(
        UnitId::for_route(UnitKind::Route, "/a#b%23", limits)
            .unwrap()
            .as_str(),
        "route:/a%23b%2523"
    );
}

#[test]
fn unit_identity_is_injective_for_adversarial_subject_and_export_combinations() {
    let mut values = BTreeSet::new();
    let fragments = [
        "a", "#", "%", "%23", ":", "é", "e\u{301}", "\n", "\u{1b}", " ", "🦀", "/", "[",
    ];
    for first in fragments {
        for second in fragments {
            let subject = format!("s{first}x{second}t");
            if let Ok(subject) = SourcePath::new(&subject, PathLimits::default()) {
                assert!(values.insert(file(subject.as_str(), None)));
                for export in fragments {
                    assert!(values.insert(file(subject.as_str(), Some(export))));
                }
            }
        }
    }
    let mut ids: Vec<_> = values.iter().map(UnitId::as_str).collect();
    let original = ids.clone();
    ids.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(ids, original);
}

#[test]
fn unit_limits_count_encoded_bytes_and_delimiters_before_allocation() {
    let subject = path("é");
    let limits = IdentifierLimits { max_bytes: 18 };
    assert_eq!(
        UnitId::for_file(UnitKind::Component, &subject, Some("x"), limits)
            .unwrap()
            .as_str(),
        "component:%C3%A9#x"
    );
    assert_eq!(
        UnitId::for_file(UnitKind::Component, &subject, Some("#"), limits),
        Err(IdentifierError::TooLong {
            bytes: 20,
            limit: 18
        })
    );
    assert_eq!(
        UnitId::for_file(
            UnitKind::Component,
            &subject,
            None,
            IdentifierLimits { max_bytes: 15 }
        ),
        Err(IdentifierError::TooLong {
            bytes: 16,
            limit: 15
        })
    );
    assert_eq!(
        UnitId::for_route(UnitKind::Route, "BAD", IdentifierLimits { max_bytes: 0 }),
        Err(IdentifierError::TooLong { bytes: 9, limit: 0 })
    );
    assert_eq!(
        UnitId::for_config("BAD", IdentifierLimits { max_bytes: 0 }),
        Err(IdentifierError::TooLong {
            bytes: 10,
            limit: 0
        })
    );
    let text = "a".repeat(IdentifierLimits::default().max_bytes - 7);
    let boundary = SourcePath::new(
        &text,
        PathLimits {
            max_path_bytes: text.len(),
            max_components: 1,
        },
    )
    .unwrap();
    assert!(
        UnitId::for_file(
            UnitKind::Module,
            &boundary,
            None,
            IdentifierLimits::default()
        )
        .is_ok()
    );
}

#[test]
fn unit_subjects_reject_ambiguous_structure_and_invalid_exports() {
    let limits = IdentifierLimits::default();
    for value in [
        "", "blog", "//", "//blog", "/a//b", "/a//", "/./b", "/a/..", "/a\\b", "/a\0b",
    ] {
        assert_eq!(
            UnitId::for_route(UnitKind::Route, value, limits),
            Err(IdentifierError::InvalidRoutePattern),
            "{value:?}"
        );
    }
    for concern in [
        "",
        "images",
        "nextjs..images",
        "nextjs.Images",
        "nextjs.images#x",
        "nextjs.images\u{1b}",
    ] {
        assert_eq!(
            UnitId::for_config(concern, limits),
            Err(IdentifierError::InvalidConcern)
        );
    }
    for export in ["", "a\0b"] {
        assert_eq!(
            UnitId::for_file(UnitKind::Component, &path("file.ts"), Some(export), limits),
            Err(IdentifierError::InvalidExport)
        );
    }
    for error in [
        IdentifierError::InvalidUnitSubject,
        IdentifierError::InvalidRoutePattern,
        IdentifierError::InvalidConcern,
        IdentifierError::InvalidExport,
    ] {
        assert!(!error.to_string().chars().any(char::is_control));
    }
}
