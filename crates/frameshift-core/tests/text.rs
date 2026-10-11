//! Contract tests for immutable source text and canonical positions.

use frameshift_core::{Position, SourceText, Span, TextError, TextLimits};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

#[test]
fn preserves_bom_crlf_and_unicode_bytes() {
    let text = "\u{feff}é\t🦀\r\nz\r終\u{2028}x\u{2029}";
    let source = SourceText::from_bytes(text.as_bytes().to_vec(), TextLimits::default()).unwrap();
    assert_eq!(source.as_str().as_bytes(), text.as_bytes());
    assert_eq!(
        source.slice(Span::new(0, text.len()).unwrap()).unwrap(),
        text
    );
    assert_eq!(source.slice(Span::new(3, 5).unwrap()).unwrap(), "é");
    assert_eq!(
        source.position(10).unwrap(),
        Position {
            offset: 10,
            line: 1,
            column: 5
        }
    );
    assert_eq!(
        source.position(11).unwrap(),
        Position {
            offset: 11,
            line: 1,
            column: 6
        }
    );
    assert_eq!(
        source.position(12).unwrap(),
        Position {
            offset: 12,
            line: 2,
            column: 1
        }
    );
    assert_eq!(
        source.position(14).unwrap(),
        Position {
            offset: 14,
            line: 3,
            column: 1
        }
    );
    assert_eq!(source.position(text.len()).unwrap().line, 5);
}

#[test]
fn supports_empty_text_eof_and_trailing_newline() {
    let empty = source("");
    assert!(empty.is_empty());
    assert_eq!(
        empty.position(0).unwrap(),
        Position {
            offset: 0,
            line: 1,
            column: 1
        }
    );
    let source = source("a\n");
    assert_eq!(
        source.position(2).unwrap(),
        Position {
            offset: 2,
            line: 2,
            column: 1
        }
    );
    assert_eq!(source.slice(Span::new(2, 2).unwrap()).unwrap(), "");
}

#[test]
fn rejects_invalid_ranges_and_partial_scalars() {
    assert_eq!(
        Span::new(3, 2),
        Err(TextError::ReversedSpan { start: 3, end: 2 })
    );
    let source = source("é");
    assert_eq!(
        source.position(1),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        source.slice(Span::new(0, 1).unwrap()),
        Err(TextError::NotCharBoundary { offset: 1 })
    );
    assert_eq!(
        source.position(3),
        Err(TextError::OutOfBounds { offset: 3, len: 2 })
    );
    assert_eq!(
        source.slice(Span::new(2, 3).unwrap()),
        Err(TextError::OutOfBounds { offset: 3, len: 2 })
    );
}

#[test]
fn rejects_encoding_and_size_errors_without_exposing_content() {
    let error = SourceText::from_bytes(vec![b'a', 0xff], TextLimits::default()).unwrap_err();
    assert_eq!(error, TextError::InvalidUtf8 { valid_up_to: 1 });
    let limits = TextLimits {
        max_source_bytes: 2,
    };
    assert!(SourceText::new("é".to_owned(), limits).is_ok());
    assert_eq!(
        SourceText::new("abc".to_owned(), limits).unwrap_err(),
        TextError::SourceTooLarge { bytes: 3, limit: 2 }
    );
    assert_eq!(
        SourceText::from_bytes(vec![0xff; 3], limits).unwrap_err(),
        TextError::SourceTooLarge { bytes: 3, limit: 2 }
    );
}

#[test]
fn indexed_positions_match_a_scalar_reference_on_long_mixed_lines() {
    let text = format!(
        "{}\r\n{}\n{}",
        "é🦀a\t".repeat(300),
        "x".repeat(600),
        "z\r".repeat(300)
    );
    let source = source(&text);
    let mut line = 1;
    let mut column = 1;
    for (offset, ch) in text.char_indices() {
        assert_eq!(
            source.position(offset).unwrap(),
            Position {
                offset,
                line,
                column
            }
        );
        if ch == '\n' || (ch == '\r' && text.as_bytes().get(offset + 1) != Some(&b'\n')) {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    assert_eq!(
        source.position(text.len()).unwrap(),
        Position {
            offset: text.len(),
            line,
            column
        }
    );
}
