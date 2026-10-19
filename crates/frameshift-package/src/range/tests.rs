use super::*;

fn canonical(text: &str) -> String {
    VersionRange::parse(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error}"))
        .to_string()
}

fn version(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn satisfies(range: &str, candidate: &str) -> bool {
    VersionRange::parse(range)
        .unwrap()
        .satisfies(&version(candidate))
}

fn minimum(range: &str) -> Option<String> {
    VersionRange::parse(range)
        .unwrap()
        .min_version()
        .map(|version| version.to_string())
}

#[test]
fn desugars_documented_forms() {
    for (text, expected) in [
        ("", "*"),
        ("*", "*"),
        ("x", "*"),
        ("1", ">=1.0.0 <2.0.0-0"),
        ("1.2", ">=1.2.0 <1.3.0-0"),
        ("1.2.3", "1.2.3"),
        ("=v1.2.3", "1.2.3"),
        ("1.2.3 - 2.3.4", ">=1.2.3 <=2.3.4"),
        ("1.2 - 2.3.4", ">=1.2.0 <=2.3.4"),
        ("1.2.3 - 2.3", ">=1.2.3 <2.4.0-0"),
        ("1.2.3 - 2", ">=1.2.3 <3.0.0-0"),
        ("~1.2.3", ">=1.2.3 <1.3.0-0"),
        ("~1", ">=1.0.0 <2.0.0-0"),
        ("~0", "<1.0.0-0"),
        ("~1.2.3-beta.2", ">=1.2.3-beta.2 <1.3.0-0"),
        ("^1.2.3", ">=1.2.3 <2.0.0-0"),
        ("^0.2.3", ">=0.2.3 <0.3.0-0"),
        ("^0.0.3", ">=0.0.3 <0.0.4-0"),
        ("^0.0.3-beta", ">=0.0.3-beta <0.0.4-0"),
        ("^0.0.x", "<0.1.0-0"),
        ("^1.x", ">=1.0.0 <2.0.0-0"),
        (">1", ">=2.0.0"),
        (">1.2", ">=1.3.0"),
        ("<=1.2", "<1.3.0-0"),
        ("<1", "<1.0.0-0"),
        (">*", "<0.0.0-0"),
        (">=*", "*"),
        ("1 || 2", ">=1.0.0 <2.0.0-0||>=2.0.0 <3.0.0-0"),
        (">=1.2.3 <2", ">=1.2.3 <2.0.0-0"),
        ("1.2.3+build", "1.2.3"),
    ] {
        assert_eq!(canonical(text), expected, "{text:?}");
    }
}

#[test]
fn reproduces_node_semver_text_rules() {
    for (text, expected) in [
        // Whitespace after an operator, and JavaScript whitespace.
        (">= 1.2.3", ">=1.2.3"),
        ("> =1.2.3", ">=1.2.3"),
        ("~> 1.2", ">=1.2.0 <1.3.0-0"),
        ("^ 1.2", ">=1.2.0 <2.0.0-0"),
        ("1\u{a0}2", ">=1.0.0 <2.0.0-0 >=2.0.0 <3.0.0-0"),
        ("\u{feff}1\u{2028}", ">=1.0.0 <2.0.0-0"),
        // Prefixes, which only X-ranges and the hyphen form repeat.
        ("vv1", ">=1.0.0 <2.0.0-0"),
        ("=v=1", ">=1.0.0 <2.0.0-0"),
        ("v = 1.2 - 2", ">=1.2.0 <3.0.0-0"),
        // Build metadata, also alone and after partial versions.
        ("+build", "*"),
        ("1.2+a", ">=1.2.0 <1.3.0-0"),
        ("1 - 2 +a", ">=1.0.0 <3.0.0-0"),
        // A tilde or caret ignores components after an `x`.
        ("~1.x.3", ">=1.0.0 <2.0.0-0"),
        ("1.x.x-beta", ">=1.0.0 <2.0.0-0"),
        // `*` alternatives make the whole range `*`, and null sets fold.
        ("1.2.3-beta || *", "*"),
        ("1.2.3 <0.0.0-0", "<0.0.0-0"),
        ("<0.0.0-0 || 1", ">=1.0.0 <2.0.0-0"),
        ("* 1", ">=1.0.0 <2.0.0-0"),
        ("1.2.3 1.2.3", "1.2.3"),
        (">=0.0.0 <1", "<1.0.0-0"),
        // A star with a number behind it is removed once.
        ("1.2.*3", "1.2.3"),
        ("0.0.9007199254740991", "0.0.9007199254740991"),
    ] {
        assert_eq!(canonical(text), expected, "{text:?}");
    }
    for invalid in [
        "a",
        "latest",
        "v 1.2.3",
        "> = 1",
        "1.2.3 -2.3.4",
        "1.2.3 - 2.3.4 - 5",
        "= 1.2.3 - 2",
        "1 +a - 2",
        "1.x.3",
        "x.1",
        "1.x-beta",
        "^~1",
        "~ ~1",
        "**",
        "x|+X|",
        "1.2.3-01",
        "1.2.3-",
        "1.2.3.4",
        "01.2.3",
        "1.2.3-\u{e9}",
        "=",
        ">",
    ] {
        assert!(VersionRange::parse(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn bounds_numbers_and_lengths() {
    assert_eq!(
        VersionRange::parse("9007199254740992.0.0"),
        Err(RangeError::NumberTooLarge)
    );
    // The desugared upper bound needs one more than the largest number.
    assert_eq!(
        VersionRange::parse("1.9007199254740991"),
        Err(RangeError::NumberTooLarge)
    );
    assert_eq!(
        VersionRange::parse("~9007199254740991"),
        Err(RangeError::NumberTooLarge)
    );
    let long = format!("1.2.3-{}", "a".repeat(250));
    assert!(VersionRange::parse(&long).is_ok());
    assert_eq!(
        VersionRange::parse(&format!("{long}a")),
        Err(RangeError::TooLong)
    );
    // Bounded identifier patterns reject long ignored parts as well.
    assert!(VersionRange::parse(&format!("1.2.x-{}", "a".repeat(251))).is_ok());
    assert!(VersionRange::parse(&format!("1.2.x-{}", "a".repeat(252))).is_err());
    assert!(VersionRange::parse(&format!("~1.x.{}", "9".repeat(258))).is_err());
}

#[test]
fn applies_the_prerelease_rule() {
    assert!(satisfies("^1.2.3-beta.2", "1.2.3-beta.4"));
    assert!(!satisfies("^1.2.3-beta.2", "1.2.4-beta.2"));
    assert!(satisfies("^1.2.3-beta.2", "1.2.4"));
    assert!(!satisfies("*", "1.0.0-0"));
    assert!(satisfies(">1.2.3-alpha.3", "1.2.3-alpha.7"));
    assert!(!satisfies(">1.2.3-alpha.3", "3.4.5-alpha.9"));
    assert!(satisfies("1.2.7 || >=1.2.9 <2.0.0", "1.4.6"));
    assert!(!satisfies("1.2.7 || >=1.2.9 <2.0.0", "1.2.8"));
    assert!(satisfies("1.2.3", "1.2.3+build"));
    assert!(!satisfies("<0.0.0-0", "0.0.0-0"));
}

#[test]
fn computes_exact_minimums() {
    for (range, expected) in [
        ("*", Some("0.0.0")),
        (">=1.0.0", Some("1.0.0")),
        (">1.2.3", Some("1.2.4")),
        (">1.2.3 <1.2.4-0 || >1.2.3-beta", Some("1.2.3-beta.0")),
        (">1.2.3 <=1.2.4-0", Some("1.2.4-0")),
        (">0.0.0-0", Some("0.0.0-0.0")),
        ("<=0.0.0-0", Some("0.0.0-0")),
        ("<1.0.0-0", Some("0.0.0")),
        ("^1.2.3-beta.2", Some("1.2.3-beta.2")),
        ("<0.0.0-0", None),
        (">=1.0.0 <1.0.0", None),
        ("1.2.3 1.2.4", None),
        // A later set is satisfiable although the first set's bound is lower.
        (">=1.0.0 <0.5.0 || >=2.0.0", Some("2.0.0")),
        (">1.2.9007199254740991", Some("1.3.0")),
    ] {
        assert_eq!(minimum(range).as_deref(), expected, "{range:?}");
        if let Some(found) = expected {
            assert!(satisfies(range, found), "{range:?}");
        }
    }
}

#[test]
fn exposes_comparators() {
    let range = VersionRange::parse("~1.2 || <1.0.0-rc.1").unwrap();
    let sets: Vec<Vec<String>> = range
        .sets()
        .iter()
        .map(|set| set.iter().map(ToString::to_string).collect())
        .collect();
    assert_eq!(sets, [vec![">=1.2.0", "<1.3.0-0"], vec!["<1.0.0-rc.1"]]);
    let first = &range.sets()[0][0];
    assert_eq!(first.operator(), Operator::GreaterOrEqual);
    assert_eq!(first.version(), &version("1.2.0"));
    assert!(first.test(&version("1.2.0")));
    assert_eq!(Operator::Equal.as_str(), "");
}
