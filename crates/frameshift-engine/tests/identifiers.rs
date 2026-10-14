//! Stable identifier contracts through the public engine facade.

use std::collections::BTreeSet;

use frameshift_engine::foundation::{
    FeatureId, FindingCode, IdentifierError, IdentifierLimits, RuleId,
};

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
