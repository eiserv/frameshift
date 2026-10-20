//! Yarn classic lockfile syntax through the engine facade, and an opt-in
//! differential runner against Yarn 1.22.22's own parser (`D-072`).

mod support;

use std::collections::BTreeMap;

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    YarnDocument, YarnErrorKind, YarnId, YarnLimits, YarnValue, parse_yarn_lockfile,
};
use support::Json;

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

#[test]
fn facade_parses_lockfiles_and_reports_positions() {
    let text = "# yarn lockfile v1\n\n\"left@^1\", left@1.x:\n  version \"1.4.0\"\n";
    let source = source(text);
    let document = parse_yarn_lockfile(&source, YarnLimits::default()).unwrap();
    let root = document
        .value(document.root())
        .unwrap()
        .as_object()
        .unwrap();
    let member = root.member(0).unwrap();
    let keys: Vec<&str> = member.keys().map(|key| key.text()).collect();
    assert_eq!(keys, ["left@^1", "left@1.x"]);
    let entry = document.value(member.value()).unwrap().as_object().unwrap();
    let version = entry.get("version").unwrap();
    assert_eq!(document.value(version), Some(YarnValue::String("1.4.0")));
    assert_eq!(
        source.slice(document.span(version).unwrap()),
        Ok("\"1.4.0\"")
    );
    let error =
        parse_yarn_lockfile(&self::source("a:\n   b c\n"), YarnLimits::default()).unwrap_err();
    assert_eq!(error.kind(), YarnErrorKind::Indentation);
}

/// A comparable value: Yarn's object semantics, where the last occurrence
/// of a key wins and keys that share a member share its value.
#[derive(Debug, PartialEq, Eq)]
enum Canon {
    String(String),
    Number(String),
    Bool(bool),
    Object(BTreeMap<String, Canon>),
}

/// Converts a parsed value, or `None` for a digit run that JavaScript
/// numbers cannot represent exactly.
fn canon(document: &YarnDocument<'_>, id: YarnId) -> Option<Canon> {
    Some(match document.value(id).unwrap() {
        YarnValue::String(text) => Canon::String(text.to_owned()),
        YarnValue::Number(digits) => {
            let significant = digits.trim_start_matches('0');
            if significant.len() > 15 {
                return None;
            }
            Canon::Number(if significant.is_empty() {
                "0".to_owned()
            } else {
                significant.to_owned()
            })
        }
        YarnValue::Bool(value) => Canon::Bool(value),
        YarnValue::Object(object) => {
            let mut map = BTreeMap::new();
            for member in object.members() {
                for key in member.keys() {
                    map.insert(key.text().to_owned(), canon(document, member.value())?);
                }
            }
            Canon::Object(map)
        }
    })
}

fn expected(json: &Json) -> Canon {
    let Json::Object(members) = json else {
        panic!("canonical value {json:?}");
    };
    let [(kind, value)] = members.as_slice() else {
        panic!("canonical value {json:?}");
    };
    match (kind.as_str(), value) {
        ("s", Json::String(text)) => Canon::String(text.clone()),
        ("n", Json::String(text)) => Canon::Number(text.clone()),
        ("b", Json::Bool(value)) => Canon::Bool(*value),
        ("o", Json::Array(pairs)) => Canon::Object(
            pairs
                .iter()
                .map(|pair| {
                    let Json::Array(items) = pair else {
                        panic!("canonical pair {pair:?}");
                    };
                    let [Json::String(key), value] = items.as_slice() else {
                        panic!("canonical pair {pair:?}");
                    };
                    (key.clone(), expected(value))
                })
                .collect(),
        ),
        _ => panic!("canonical value {json:?}"),
    }
}

/// Compares each case of JSON lines written by the oracle script, outside
/// the repository, with this parser: `[input, canonical or null, flag]`.
/// The flag `ok` requires the same values; `error` (Yarn's parser fails),
/// `conflict` (Yarn would merge both sides), `truncated` (Yarn would ignore
/// content after top-level indentation), and `surrogate` (an unpaired
/// surrogate escape) require a rejection.
#[test]
#[ignore = "external oracle: set FRAMESHIFT_YARN_ORACLE to the oracle's JSON lines files"]
fn external_yarn_classic_oracle() {
    let paths = std::env::var_os("FRAMESHIFT_YARN_ORACLE")
        .expect("FRAMESHIFT_YARN_ORACLE names the oracle's JSON lines files");
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped = 0;
    for path in std::env::split_paths(&paths) {
        let text = std::fs::read_to_string(&path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let case =
                support::parse(line).unwrap_or_else(|error| panic!("line {number}: {error}"));
            let Json::Array(items) = case else {
                panic!("line {number}: not an array");
            };
            let [Json::String(input), canonical, Json::String(flag)] = items.as_slice() else {
                panic!("line {number}: not [input, canonical, flag]");
            };
            let source = SourceText::new(input.clone(), TextLimits::default()).unwrap();
            let result = parse_yarn_lockfile(&source, YarnLimits::default());
            match flag.as_str() {
                "ok" => {
                    let document = result.unwrap_or_else(|error| {
                        panic!("line {number}: rejected ({error}) what Yarn accepts: {input:?}")
                    });
                    match canon(&document, document.root()) {
                        Some(actual) => assert_eq!(
                            actual,
                            expected(canonical),
                            "line {number}: values differ for {input:?}"
                        ),
                        None => skipped += 1,
                    }
                }
                "error" | "conflict" | "truncated" | "surrogate" => assert!(
                    result.is_err(),
                    "line {number}: accepted a case flagged {flag}: {input:?}"
                ),
                other => panic!("line {number}: unknown flag {other}"),
            }
            *tally.entry(flag.clone()).or_default() += 1;
        }
    }
    println!("cases by flag: {tally:?}; long digit runs not compared: {skipped}");
}
