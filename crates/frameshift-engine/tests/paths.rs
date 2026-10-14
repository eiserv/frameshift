//! Public facade contracts for canonical source and portable output path values.

use std::collections::{BTreeSet, HashSet};

use frameshift_engine::foundation::{OutputPath, PathError, PathLimits, SourcePath};

fn source(value: &str) -> SourcePath {
    SourcePath::new(value, PathLimits::default()).unwrap()
}

fn output(value: &str) -> Result<OutputPath, PathError> {
    OutputPath::new(value, PathLimits::default())
}

#[test]
fn preserves_source_names_without_claiming_portability() {
    for value in [
        "src/CON.ts",
        "aux",
        "trailing.",
        "directory /file ",
        "dir/ads:stream",
        "quotes\"<>|?*",
        "src/line\nbreak",
        "é/🦀.tsx",
        "e\u{301}/file",
        ".gitignore",
        "src/[...slug]/page.tsx",
        "src/(marketing)/page.tsx",
        "%2e%2e/file",
    ] {
        let path = source(value);
        assert_eq!(path.as_str().as_bytes(), value.as_bytes());
        assert_eq!(path.components().collect::<Vec<_>>().join("/"), value);
        assert_eq!(
            SourcePath::from_bytes(value.as_bytes(), PathLimits::default()),
            Ok(path)
        );
    }
    assert_ne!(source("é"), source("e\u{301}"));
    assert_ne!(source("Page.tsx"), source("page.tsx"));
}

#[test]
fn rejects_ambiguous_structure_without_repairing_it() {
    for (value, error) in [
        ("", PathError::Empty),
        ("/file", PathError::Absolute),
        ("//server/share", PathError::Absolute),
        ("C:file", PathError::DrivePrefix),
        ("z:/file", PathError::DrivePrefix),
        ("\\file", PathError::Backslash { offset: 0 }),
        ("\\\\server\\share", PathError::Backslash { offset: 0 }),
        ("\\\\?\\C:\\file", PathError::Backslash { offset: 0 }),
        ("dir\\file", PathError::Backslash { offset: 3 }),
        ("é/\\file", PathError::Backslash { offset: 3 }),
        ("dir/", PathError::EmptyComponent { offset: 4 }),
        ("dir//file", PathError::EmptyComponent { offset: 4 }),
        (".", PathError::DotComponent { offset: 0 }),
        ("..", PathError::DotComponent { offset: 0 }),
        ("./file", PathError::DotComponent { offset: 0 }),
        ("dir/../file", PathError::DotComponent { offset: 4 }),
        ("é/./file", PathError::DotComponent { offset: 3 }),
        ("dir/\0file", PathError::Nul { offset: 4 }),
    ] {
        assert_eq!(
            SourcePath::new(value, PathLimits::default()),
            Err(error),
            "{value:?}"
        );
        assert_eq!(output(value), Err(error), "{value:?}");
    }
    assert_eq!(source(".../file..name").as_str(), ".../file..name");
}

#[test]
fn limits_are_inclusive_and_checked_before_decoding_or_structure() {
    let limits = PathLimits {
        max_path_bytes: 4,
        max_components: 2,
    };
    assert_eq!(SourcePath::new("é/a", limits).unwrap().as_str(), "é/a");
    assert_eq!(
        SourcePath::new("é/ab", limits),
        Err(PathError::TooLong { bytes: 5, limit: 4 })
    );
    for result in [
        SourcePath::from_bytes(&[0xff; 5], limits).map(|_| ()),
        OutputPath::from_bytes(&[0xff; 5], limits).map(|_| ()),
    ] {
        assert_eq!(result, Err(PathError::TooLong { bytes: 5, limit: 4 }));
    }
    let limits = PathLimits {
        max_path_bytes: 100,
        max_components: 2,
    };
    assert_eq!(
        SourcePath::new("é/a", limits).unwrap().components().count(),
        2
    );
    assert_eq!(
        SourcePath::new("é/a/b", limits),
        Err(PathError::TooManyComponents {
            offset: 5,
            limit: 2
        })
    );
    assert_eq!(
        SourcePath::new("a/b/..", limits),
        Err(PathError::TooManyComponents {
            offset: 4,
            limit: 2
        })
    );
    let zero = PathLimits {
        max_path_bytes: 0,
        max_components: 0,
    };
    assert_eq!(SourcePath::new("", zero), Err(PathError::Empty));
    assert_eq!(
        SourcePath::new("a", zero),
        Err(PathError::TooLong { bytes: 1, limit: 0 })
    );
    assert_eq!(
        SourcePath::new(
            "a",
            PathLimits {
                max_path_bytes: 1,
                max_components: 0
            }
        ),
        Err(PathError::TooManyComponents {
            offset: 0,
            limit: 0
        })
    );
}

#[test]
fn rejects_invalid_utf8_without_lossy_conversion() {
    for (bytes, valid_up_to) in [
        (&[0xff][..], 0),
        (&b"dir/\xc0\xaf"[..], 4),
        (&b"dir/\xed\xa0\x80"[..], 4),
        (&b"dir/\xf0\x9f"[..], 4),
    ] {
        let error = PathError::InvalidUtf8 { valid_up_to };
        assert_eq!(
            SourcePath::from_bytes(bytes, PathLimits::default()),
            Err(error)
        );
        assert_eq!(
            OutputPath::from_bytes(bytes, PathLimits::default()),
            Err(error)
        );
    }
}

#[test]
fn output_checks_each_component_and_keeps_valid_names_exact() {
    for name in [
        "src/pages/[...slug].astro",
        ".frameshift/SUMMARY.md",
        "é/🦀.tsx",
        "e\u{301}.txt",
        "CONtent.ts",
        "xNUL",
        "COM0",
        "COM10",
        "LPT0",
        "LPT10",
        ".CON",
        "file.COM1",
        "a..b",
        "a b.ts",
        "com⁴.txt",
        "nulled/file",
    ] {
        let path = output(name).unwrap();
        assert_eq!(path.as_str(), name);
        assert_eq!(path.as_source(), &source(name));
        assert_eq!(
            OutputPath::from_bytes(name.as_bytes(), PathLimits::default()),
            Ok(path)
        );
    }
    for (name, error) in [
        ("dir/nul.txt", PathError::OutputDevice { offset: 4 }),
        ("CON/sub/file", PathError::OutputDevice { offset: 0 }),
        ("é/end.", PathError::OutputEnding { offset: 6 }),
        ("dir /file", PathError::OutputEnding { offset: 3 }),
        ("dir/ads:stream", PathError::OutputCharacter { offset: 7 }),
        ("dir/é\n.txt", PathError::OutputControl { offset: 6 }),
    ] {
        assert!(SourcePath::new(name, PathLimits::default()).is_ok());
        assert_eq!(output(name), Err(error), "{name:?}");
    }
    // Structural errors precede portable-name errors, even in a later component.
    assert_eq!(output("CON/.."), Err(PathError::DotComponent { offset: 4 }));
}

#[test]
fn rejects_reserved_devices_with_extensions_and_ascii_case_variants() {
    let mut devices = vec![
        "CON".to_owned(),
        "PRN".to_owned(),
        "AUX".to_owned(),
        "NUL".to_owned(),
        "CONIN$".to_owned(),
        "CONOUT$".to_owned(),
    ];
    for prefix in ["COM", "LPT"] {
        for digit in ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"] {
            devices.push(format!("{prefix}{digit}"));
        }
    }
    for device in devices {
        for spelling in [&device, &device.to_ascii_lowercase()] {
            for suffix in ["", ".ts", ".tar.gz", " .txt"] {
                let name = format!("src/{spelling}{suffix}/child");
                assert_eq!(source(&name).as_str(), name);
                assert_eq!(
                    output(&name),
                    Err(PathError::OutputDevice { offset: 4 }),
                    "{name:?}"
                );
            }
        }
    }
}

#[test]
fn rejects_output_punctuation_controls_and_trailing_spaces_or_dots() {
    for ch in ['<', '>', ':', '"', '|', '?', '*'] {
        assert_eq!(
            output(&format!("é/x{ch}y")),
            Err(PathError::OutputCharacter { offset: 4 })
        );
    }
    for code in (1..=31).chain(0x7f..=0x9f) {
        let ch = char::from_u32(code).unwrap();
        let name = format!("é/x{ch}y");
        assert_eq!(source(&name).as_str(), name);
        assert_eq!(output(&name), Err(PathError::OutputControl { offset: 4 }));
    }
    for name in ["a ", "a.", "a .", "a. "] {
        assert_eq!(
            output(name),
            Err(PathError::OutputEnding {
                offset: name.len() - 1
            })
        );
    }
}

#[test]
fn output_component_size_bounds_bytes_without_restricting_source_names() {
    for name in ["a".repeat(255), format!("{}a", "é".repeat(127))] {
        assert_eq!(output(&name).unwrap().as_str(), name);
    }
    for name in ["a".repeat(256), "é".repeat(128), "🦀".repeat(64)] {
        let nested = format!("ok/{name}/file");
        assert_eq!(source(&nested).as_str(), nested);
        assert_eq!(
            output(&nested),
            Err(PathError::OutputComponentTooLong {
                offset: 3,
                bytes: name.len()
            })
        );
    }
    let long = std::iter::repeat_n("a".repeat(200), 200)
        .collect::<Vec<_>>()
        .join("/");
    assert!(matches!(output(&long), Err(PathError::TooLong { .. })));
    let limits = PathLimits {
        max_path_bytes: long.len(),
        max_components: 200,
    };
    assert_eq!(OutputPath::new(&long, limits).unwrap().as_str(), long);
    let deep = std::iter::repeat_n("a", 256).collect::<Vec<_>>().join("/");
    assert!(output(&deep).is_ok());
    assert_eq!(
        output(&format!("{deep}/b")),
        Err(PathError::TooManyComponents {
            offset: 512,
            limit: 256
        })
    );
}

#[test]
fn strengthening_and_weakening_preserve_storage_and_spelling() {
    let path = source("src/é.ts");
    let pointer = path.as_str().as_ptr();
    let path = OutputPath::try_from(path).unwrap();
    assert_eq!(path.as_str().as_ptr(), pointer);
    let path = path.into_source();
    assert_eq!(path.as_str().as_ptr(), pointer);
    assert_eq!(
        OutputPath::try_from(source("CON.ts")),
        Err(PathError::OutputDevice { offset: 0 })
    );
    let text = "a".repeat(33_000);
    let limits = PathLimits {
        max_path_bytes: text.len(),
        max_components: 1,
    };
    let path = SourcePath::new(&text, limits).unwrap();
    assert_eq!(
        OutputPath::try_from(path),
        Err(PathError::OutputComponentTooLong {
            offset: 0,
            bytes: text.len()
        })
    );
}

#[test]
fn ordered_collections_use_original_bytes_and_exact_identity() {
    let names = ["é", "e\u{301}", "z", "A", "a", "src/a", "src/A", "🦀"];
    let mut expected = names.to_vec();
    expected.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let paths: BTreeSet<_> = names.iter().rev().map(|name| source(name)).collect();
    assert_eq!(
        paths.iter().map(SourcePath::as_str).collect::<Vec<_>>(),
        expected
    );
    let outputs: BTreeSet<_> = names.iter().map(|name| output(name).unwrap()).collect();
    assert_eq!(
        outputs.iter().map(OutputPath::as_str).collect::<Vec<_>>(),
        expected
    );
    let hashes: HashSet<_> = names
        .iter()
        .flat_map(|name| [source(name), source(name)])
        .collect();
    assert_eq!(hashes.len(), names.len());
    assert_eq!(paths.first().unwrap().components().next_back(), Some("A"));
}

#[test]
fn errors_do_not_echo_source_names_or_terminal_controls() {
    let secret = "private-name-123";
    let name = format!("{secret}/\u{1b}[31m");
    let error = output(&name).unwrap_err();
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(secret));
        assert!(!text.contains('\u{1b}'));
    }
}
