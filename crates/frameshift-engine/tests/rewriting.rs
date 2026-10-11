//! Tests of the public engine text API, independent of parser or migration rules.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, Origin, RewriteError, RewrittenText, SourceText, Span, TextError,
    TextLimits, apply_edits,
};

fn source(text: &str) -> SourceText {
    SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
}

fn edit(start: usize, end: usize, replacement: &str, operation: u32, order: u32) -> Edit<'_> {
    Edit {
        span: Span::new(start, end).unwrap(),
        replacement,
        order: EditOrder {
            operation,
            edit: order,
        },
    }
}

fn rewrite(source: &SourceText, edits: &[Edit<'_>]) -> Result<RewrittenText, RewriteError> {
    apply_edits(source, edits, EditLimits::default())
}

fn assert_provenance(source: &SourceText, result: &RewrittenText, edits: &[Edit<'_>]) {
    let mut cursor = 0;
    let mut accounted_source = Vec::new();
    for region in result.provenance() {
        assert_eq!(region.output.start(), cursor);
        assert!(!region.output.is_empty());
        let emitted = &result.as_str()[region.output.start()..region.output.end()];
        match region.origin {
            Origin::Source(range) => {
                assert_eq!(emitted, source.slice(range).unwrap());
                accounted_source.push(range);
            }
            Origin::Edit {
                source: range,
                order,
            } => {
                let edit = edits.iter().find(|edit| edit.order == order).unwrap();
                assert_eq!(range, edit.span);
                assert_eq!(emitted, edit.replacement);
            }
        }
        cursor = region.output.end();
    }
    assert_eq!(cursor, result.as_str().len());
    assert_eq!(result.applied_edits().len(), edits.len());
    for applied in result.applied_edits() {
        let edit = edits
            .iter()
            .find(|edit| edit.order == applied.order)
            .unwrap();
        assert_eq!(applied.source, edit.span);
        if !applied.source.is_empty() {
            accounted_source.push(applied.source);
        }
        assert_eq!(
            &result.as_str()[applied.output.start()..applied.output.end()],
            edit.replacement
        );
    }
    accounted_source.sort_unstable();
    let mut cursor = 0;
    for range in accounted_source {
        assert_eq!(range.start(), cursor);
        cursor = range.end();
    }
    assert_eq!(cursor, source.len());
}

#[test]
fn empty_edit_set_is_a_byte_exact_round_trip() {
    for text in [
        "",
        "\u{feff}// comment\r\nconst x = 'é🦀';\r\n",
        "x\u{2028}y\u{2029}",
    ] {
        let source = source(text);
        let result = rewrite(&source, &[]).unwrap();
        assert_eq!(result.as_str().as_bytes(), text.as_bytes());
        assert_provenance(&source, &result, &[]);
    }
}

#[test]
fn preserves_untouched_comments_formatting_and_line_endings() {
    let text = "\u{feff}// Keep this comment\r\nconst label = 'é🦀'; // same\r\n";
    let source = source(text);
    let start = text.find("label").unwrap();
    let edits = [edit(start, start + 5, "title", 0, 0)];
    let result = rewrite(&source, &edits).unwrap();
    assert_eq!(
        result.as_str(),
        "\u{feff}// Keep this comment\r\nconst title = 'é🦀'; // same\r\n"
    );
    assert_eq!(source.as_str(), text);
    assert_provenance(&source, &result, &edits);
}

#[test]
fn orders_shared_insertions_and_adjacent_replacements_at_boundaries() {
    let source = source("abcde");
    let edits = [
        edit(1, 1, "2", 2, 0),
        edit(1, 1, "1b", 1, 1),
        edit(1, 1, "1a", 1, 0),
        edit(1, 3, "X", 0, 0),
        edit(3, 3, "!", 0, 1),
        edit(3, 4, "Y", 0, 2),
        edit(5, 5, "?", 3, 0),
    ];
    let result = rewrite(&source, &edits).unwrap();
    assert_eq!(result.as_str(), "a1a1b2X!Ye?");
    assert_provenance(&source, &result, &edits);
}

#[test]
fn records_deletions_and_inserts_into_empty_source() {
    let source = source("abc");
    let edits = [edit(0, 3, "", 0, 0)];
    let result = rewrite(&source, &edits).unwrap();
    assert_eq!(result.as_str(), "");
    assert!(result.provenance().is_empty());
    assert_eq!(result.applied_edits()[0].output, Span::new(0, 0).unwrap());
    assert_provenance(&source, &result, &edits);

    let empty = SourceText::new(String::new(), TextLimits::default()).unwrap();
    let edits = [edit(0, 0, "é🦀", 0, 0)];
    let result = rewrite(&empty, &edits).unwrap();
    assert_eq!(result.as_str(), "é🦀");
    assert_provenance(&empty, &result, &edits);
}

#[test]
fn rejects_overlaps_identical_replacements_and_insertions_inside_replacements() {
    let source = source("abcdef");
    for edits in [
        [edit(1, 4, "X", 0, 0), edit(2, 5, "Y", 1, 0)],
        [edit(1, 4, "X", 0, 0), edit(1, 4, "X", 1, 0)],
        [edit(1, 4, "X", 0, 0), edit(2, 2, "Y", 1, 0)],
        [edit(1, 5, "X", 0, 0), edit(2, 3, "Y", 1, 0)],
    ] {
        assert_eq!(
            rewrite(&source, &edits),
            Err(RewriteError::Conflict {
                earlier: edits[0].order,
                later: edits[1].order
            })
        );
        assert_eq!(
            rewrite(&source, &[edits[1], edits[0]]),
            rewrite(&source, &edits)
        );
    }
    assert_eq!(source.as_str(), "abcdef");
}

#[test]
fn rejects_duplicate_order_even_at_distinct_offsets() {
    let source = source("abc");
    let edits = [edit(0, 0, "X", 0, 0), edit(3, 3, "Y", 0, 0)];
    assert_eq!(
        rewrite(&source, &edits),
        Err(RewriteError::DuplicateOrder {
            order: edits[0].order
        })
    );
}

#[test]
fn rejects_invalid_spans_instead_of_slicing_or_panicking() {
    let source = source("é🦀");
    for (start, end, error) in [
        (1, 1, TextError::NotCharBoundary { offset: 1 }),
        (0, 3, TextError::NotCharBoundary { offset: 3 }),
        (7, 7, TextError::OutOfBounds { offset: 7, len: 6 }),
        (
            6,
            usize::MAX,
            TextError::OutOfBounds {
                offset: usize::MAX,
                len: 6,
            },
        ),
    ] {
        let edit = edit(start, end, "X", 0, 0);
        assert_eq!(
            rewrite(&source, &[edit]),
            Err(RewriteError::InvalidSpan {
                order: edit.order,
                error
            })
        );
    }
    let edits = [edit(0, 2, "E", 0, 0), edit(2, 6, "C", 1, 0)];
    let result = rewrite(&source, &edits).unwrap();
    assert_eq!(result.as_str(), "EC");
    assert_provenance(&source, &result, &edits);
}

#[test]
fn applies_count_and_output_limits_before_emission() {
    let source = source("abc");
    let insertion = edit(0, 0, "X", 0, 0);
    assert_eq!(
        apply_edits(
            &source,
            &[insertion],
            EditLimits {
                max_edits: 0,
                max_output_bytes: 4
            }
        ),
        Err(RewriteError::TooManyEdits { count: 1, limit: 0 })
    );
    assert_eq!(
        apply_edits(
            &source,
            &[insertion],
            EditLimits {
                max_edits: 1,
                max_output_bytes: 3
            }
        ),
        Err(RewriteError::OutputTooLarge { limit: 3 })
    );
    assert_eq!(
        apply_edits(
            &source,
            &[],
            EditLimits {
                max_edits: 0,
                max_output_bytes: 2
            }
        ),
        Err(RewriteError::OutputTooLarge { limit: 2 })
    );
    let result = apply_edits(
        &source,
        &[insertion],
        EditLimits {
            max_edits: 1,
            max_output_bytes: 4,
        },
    )
    .unwrap();
    assert_eq!(result.as_str(), "Xabc");
    // Limits describe final output, so a later deletion may offset an insertion.
    let edits = [edit(0, 0, "123", 0, 0), edit(0, 3, "", 1, 0)];
    assert_eq!(
        apply_edits(
            &source,
            &edits,
            EditLimits {
                max_edits: 2,
                max_output_bytes: 3
            }
        )
        .unwrap()
        .as_str(),
        "123"
    );
}

#[test]
fn all_input_permutations_produce_identical_text_and_provenance() {
    let source = source("abc");
    let edits = [
        edit(0, 0, "<", 0, 0),
        edit(1, 2, "B", 1, 0),
        edit(3, 3, "1", 2, 0),
        edit(3, 3, "2", 2, 1),
    ];
    let expected = rewrite(&source, &edits).unwrap();
    assert_eq!(expected.as_str(), "<aBc12");
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    if a != b && a != c && a != d && b != c && b != d && c != d {
                        let permutation = [edits[a], edits[b], edits[c], edits[d]];
                        let actual = rewrite(&source, &permutation).unwrap();
                        assert_eq!(actual, expected);
                        assert_provenance(&source, &actual, &permutation);
                    }
                }
            }
        }
    }
}

#[test]
fn range_pairs_have_exhaustive_conflict_and_provenance_checks() {
    let source = source("abcd");
    for start_a in 0..=4 {
        for end_a in start_a..=4 {
            for start_b in 0..=4 {
                for end_b in start_b..=4 {
                    let edits = [
                        edit(start_a, end_a, "X", 0, 0),
                        edit(start_b, end_b, "Y", 1, 0),
                    ];
                    let a = start_a..end_a;
                    let b = start_b..end_b;
                    let overlaps = if a.is_empty() {
                        start_b < start_a && start_a < end_b
                    } else if b.is_empty() {
                        start_a < start_b && start_b < end_a
                    } else {
                        start_a.max(start_b) < end_a.min(end_b)
                    };
                    let result = rewrite(&source, &edits);
                    assert_eq!(result, rewrite(&source, &[edits[1], edits[0]]));
                    if overlaps {
                        assert!(matches!(result, Err(RewriteError::Conflict { .. })));
                    } else {
                        assert_provenance(&source, &result.unwrap(), &edits);
                    }
                }
            }
        }
    }
}
