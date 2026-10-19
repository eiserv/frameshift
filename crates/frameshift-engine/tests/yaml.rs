//! YAML syntax contracts and an opt-in external conformance-suite runner.

use frameshift_engine::foundation::{SourceText, TextLimits};
use frameshift_engine::syntax::{
    YamlErrorKind, YamlId, YamlLimits, YamlScalarStyle, YamlStream, YamlValue, parse_yaml,
};

fn parse(text: &str) -> YamlStream {
    parse_yaml(
        &SourceText::new(text.to_owned(), TextLimits::default()).unwrap(),
        YamlLimits::default(),
    )
    .unwrap()
}

#[test]
fn graph_preserves_aliases_tags_styles_and_documents() {
    let stream = parse(
        "%YAML 1.2\n---\n&a { self: *a, ? [a, b]: !!str 9 }\n...\n---\n>+\n  folded\n  text\n\n",
    );
    assert_eq!(stream.documents().len(), 2);
    let root = stream.documents()[0].root;
    assert_eq!(stream.node(root).unwrap().anchor.as_deref(), Some("a"));
    assert!(
        stream
            .nodes()
            .iter()
            .any(|node| node.value == YamlValue::Alias(root))
    );
    assert!(
        stream
            .nodes()
            .iter()
            .any(|node| node.tag.as_deref() == Some("tag:yaml.org,2002:str"))
    );
    assert_eq!(
        stream.scalar(stream.documents()[1].root),
        Some("folded text\n\n")
    );
}

#[test]
fn errors_and_limits_do_not_echo_content() {
    for text in [
        "*SECRET",
        "a: [SECRET",
        "a: \"\\qSECRET\"",
        "a: &SECRET b\nc: *MISSING",
    ] {
        let source = SourceText::new(text.to_owned(), TextLimits::default()).unwrap();
        let error = parse_yaml(&source, YamlLimits::default()).unwrap_err();
        assert!(source.slice(error.span).is_ok());
        assert!(!error.to_string().contains("SECRET"));
    }
    let source = SourceText::new("[[[]]]".to_owned(), TextLimits::default()).unwrap();
    assert_eq!(
        parse_yaml(
            &source,
            YamlLimits {
                max_depth: 2,
                ..YamlLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        YamlErrorKind::DepthLimit
    );
    assert_eq!(
        parse_yaml(
            &source,
            YamlLimits {
                max_nodes: 2,
                ..YamlLimits::default()
            }
        )
        .unwrap_err()
        .kind,
        YamlErrorKind::NodeLimit
    );
    assert_eq!(parse("&a [*a, *a, *a]").nodes().len(), 4);
}

#[test]
fn json_compatible_quoted_characters_and_exact_scalar_values() {
    for ch in ['\u{7f}', '\u{9d}', '\u{fffe}', '\u{ffff}'] {
        let stream = parse(&format!("[\"fictional{ch}\", '{ch}']"));
        assert!(stream.nodes().iter().any(
            |node| matches!(&node.value, YamlValue::Scalar { text, .. } if text.contains(ch))
        ));
        let source = SourceText::new(format!("plain{ch}"), TextLimits::default()).unwrap();
        assert_eq!(
            parse_yaml(&source, YamlLimits::default()).unwrap_err().kind,
            YamlErrorKind::Character
        );
    }
    for text in [
        "{ 'label': \"line\\t\n  folded\" }",
        "? [fictional, keys]\n: &mapping {self: *mapping}\n",
        "- !!str\n- {value: &empty}\n- *empty\n",
    ] {
        let lf = events(&parse(text));
        assert_eq!(lf, events(&parse(&text.replace('\n', "\r\n"))));
        assert_eq!(lf, events(&parse(&text.replace('\n', "\r"))));
    }
}

#[test]
fn colon_prefixed_flow_keys_bom_and_surrogate_pairs_preserve_meaning() {
    assert!(events(&parse("{:fictional}")).contains("=VAL ::fictional\n=VAL :\n"));
    assert!(events(&parse("{a:b}")).contains("=VAL :a:b\n=VAL :\n"));
    assert!(events(&parse("[:]")).contains("+MAP {}\n=VAL :\n=VAL :\n-MAP"));
    assert_eq!(
        events(&parse("\u{feff}first: one\nsecond: two\n")),
        events(&parse("first: one\nsecond: two\n"))
    );
    assert_eq!(
        events(&parse(
            "\u{feff}---\nfirst: one\n...\n\u{feff}---\nsecond: two\n"
        )),
        events(&parse("---\nfirst: one\n...\n---\nsecond: two\n"))
    );
    assert!(events(&parse("%YAML 1.42\n---\nfictional\n")).contains("=VAL :fictional\n"));
    let pair = parse("\"\\uD83D\\uDE00\"");
    assert_eq!(pair.scalar(pair.documents()[0].root), Some("\u{1f600}"));
    assert_eq!(parse("first\n\u{feff}---\nsecond\n").documents().len(), 2);
    for text in [
        "a: b\n\u{feff}c: d\n",
        "---\n\u{feff}fictional\n",
        "\"\\uD83D\"",
        "\"\\uDE00\"",
    ] {
        let source = SourceText::new(text.into(), TextLimits::default()).unwrap();
        assert!(parse_yaml(&source, YamlLimits::default()).is_err());
    }
}

#[test]
fn long_flow_lines_and_lookahead_keep_all_values_and_locations() {
    let input = format!(
        "[{}]\n",
        std::iter::repeat_n("--- ", 20_000)
            .collect::<Vec<_>>()
            .join(",")
    );
    let stream = parse(&input);
    let YamlValue::Sequence { elements, .. } =
        &stream.node(stream.documents()[0].root).unwrap().value
    else {
        panic!("flow sequence");
    };
    assert_eq!(elements.len(), 20_000);
    for id in elements {
        assert_eq!(stream.scalar(*id), Some("---"));
    }
    let multiline = parse("[{fictional: one\n ,other: two},\n three]\n");
    assert_eq!(multiline.documents().len(), 1);
    for node in multiline.nodes() {
        assert!(node.span.start() <= node.span.end());
    }
}

fn events(stream: &YamlStream) -> String {
    fn node(stream: &YamlStream, id: YamlId, output: &mut String) {
        let node = stream.node(id).unwrap();
        let mut props = String::new();
        if let Some(anchor) = &node.anchor {
            props.push_str(&format!(" &{anchor}"));
        }
        if let Some(tag) = &node.tag {
            props.push_str(&format!(" <{tag}>"));
        }
        match &node.value {
            YamlValue::Scalar { text, style } => {
                let style = match style {
                    YamlScalarStyle::Plain => ':',
                    YamlScalarStyle::SingleQuoted => '\'',
                    YamlScalarStyle::DoubleQuoted => '"',
                    YamlScalarStyle::Literal => '|',
                    YamlScalarStyle::Folded => '>',
                };
                let escaped = text
                    .replace('\\', "\\\\")
                    .replace('\0', "\\0")
                    .replace('\u{7}', "\\a")
                    .replace('\u{8}', "\\b")
                    .replace('\t', "\\t")
                    .replace('\n', "\\n")
                    .replace('\u{b}', "\\v")
                    .replace('\u{c}', "\\f")
                    .replace('\r', "\\r")
                    .replace('\u{1b}', "\\e");
                output.push_str(&format!("=VAL{props} {style}{escaped}\n"));
            }
            YamlValue::Alias(target) => output.push_str(&format!(
                "=ALI *{}\n",
                stream.node(*target).unwrap().anchor.as_deref().unwrap()
            )),
            YamlValue::Sequence { elements, flow } => {
                output.push_str(&format!("+SEQ{}{props}\n", if *flow { " []" } else { "" }));
                for element in elements {
                    node_event(stream, *element, output);
                }
                output.push_str("-SEQ\n");
            }
            YamlValue::Mapping { entries, flow } => {
                output.push_str(&format!("+MAP{}{props}\n", if *flow { " {}" } else { "" }));
                for (key, value) in entries {
                    node_event(stream, *key, output);
                    node_event(stream, *value, output);
                }
                output.push_str("-MAP\n");
            }
        }
    }
    fn node_event(stream: &YamlStream, id: YamlId, output: &mut String) {
        node(stream, id, output);
    }
    let mut output = String::from("+STR\n");
    for document in stream.documents() {
        output.push_str(if document.explicit_start {
            "+DOC ---\n"
        } else {
            "+DOC\n"
        });
        node(stream, document.root, &mut output);
        output.push_str(if document.explicit_end {
            "-DOC ...\n"
        } else {
            "-DOC\n"
        });
    }
    output.push_str("-STR\n");
    output
}

#[test]
#[ignore = "requires FRAMESHIFT_YAML_TEST_SUITE pointing to the external data branch"]
fn external_yaml_test_suite() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FRAMESHIFT_YAML_TEST_SUITE").expect("external YAML suite path"),
    );
    let mut paths = vec![root.clone()];
    let mut cases = Vec::new();
    while let Some(path) = paths.pop() {
        if path.join("in.yaml").is_file() {
            cases.push(path);
        } else {
            for entry in std::fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() && entry.file_name() != ".git" {
                    paths.push(entry.path());
                }
            }
        }
    }
    cases.sort();
    let mut failures = Vec::new();
    let mut passed = 0;
    for case in &cases {
        let text = std::fs::read_to_string(case.join("in.yaml")).unwrap();
        let source = SourceText::new(text, TextLimits::default()).unwrap();
        let result = parse_yaml(&source, YamlLimits::default());
        let invalid = case.join("error").is_file();
        let label = case.strip_prefix(&root).unwrap().display();
        match result {
            Err(_) if invalid => passed += 1,
            Err(error) => failures.push(format!("{label}: rejected {error}")),
            Ok(_) if invalid => failures.push(format!("{label}: accepted invalid input")),
            Ok(stream) => {
                let expected = std::fs::read_to_string(case.join("test.event")).unwrap();
                let actual = events(&stream);
                if expected.trim_end_matches('\n') == actual.trim_end_matches('\n') {
                    passed += 1;
                } else {
                    let mismatch = expected
                        .lines()
                        .zip(actual.lines())
                        .position(|(a, b)| a != b)
                        .unwrap_or(expected.lines().count().min(actual.lines().count()));
                    failures.push(format!(
                        "{label}: event {mismatch}: expected {:?}, got {:?}",
                        expected.lines().nth(mismatch),
                        actual.lines().nth(mismatch)
                    ));
                }
            }
        }
    }
    eprintln!(
        "{passed}/{} YAML cases passed\n{}",
        cases.len(),
        failures.join("\n")
    );
    assert!(failures.is_empty(), "YAML conformance failures");
}
