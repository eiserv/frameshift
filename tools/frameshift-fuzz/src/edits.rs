//! The span edit target (`D-044`).
//!
//! The input encodes a source text, a list of edits, and optionally small
//! limits (see [`Case::decode`]). Spans may lie outside the source, divide a
//! scalar, overlap, or repeat an ordering pair, so the target exercises both
//! rewrites and every rejection. A rewrite must partition its output into
//! provenance regions that reproduce the source and the replacements, account
//! for every source byte exactly once, and record every edit. Every outcome
//! must be independent of the order in which the edits are passed, as
//! section 3 of `docs/IMPLEMENTATION.md` specifies.

use frameshift_engine::{
    Edit, EditLimits, EditOrder, Origin, RewriteError, RewrittenText, SourceText, Span, TextLimits,
    apply_edits,
};

/// A decoded input of the edit target.
#[derive(Debug, PartialEq, Eq)]
pub struct Case {
    source: String,
    edits: Vec<(Span, String, EditOrder)>,
    limits: EditLimits,
}

/// One edit to encode: start, length, operation order, edit order, and
/// replacement. Values beyond the encoding's ranges are reduced.
pub type Encoded<'a> = (usize, usize, u32, u32, &'a str);

impl Case {
    /// Decodes an input. Byte 0 holds the edit count in its low four bits and
    /// selects small limits with bit 4. Byte 1 is the source length, followed
    /// by the source. With small limits, two bytes give the maximum edit
    /// count (modulo 16) and half the maximum output size. Each edit takes a
    /// start (modulo the source length plus two), a length (modulo 8),
    /// operation and edit orders (modulo 4), and a replacement length (modulo
    /// 8) followed by the replacement. Missing bytes read as zero, and text
    /// is decoded lossily, so replacement characters can move later spans
    /// into the middle of a scalar.
    pub fn decode(input: &[u8]) -> Self {
        let mut bytes = Reader { input, position: 0 };
        let header = bytes.byte();
        let count = usize::from(header & 0x0F);
        let length = usize::from(bytes.byte());
        let source = String::from_utf8_lossy(bytes.take(length)).into_owned();
        let limits = if header & 0x10 == 0 {
            EditLimits::default()
        } else {
            EditLimits {
                max_edits: usize::from(bytes.byte() % 16),
                max_output_bytes: usize::from(bytes.byte()) * 2,
            }
        };
        let edits = (0..count)
            .map(|_| {
                let start = usize::from(bytes.byte()) % (source.len() + 2);
                let length = usize::from(bytes.byte() % 8);
                let order = EditOrder {
                    operation: u32::from(bytes.byte() % 4),
                    edit: u32::from(bytes.byte() % 4),
                };
                let size = usize::from(bytes.byte() % 8);
                let replacement = String::from_utf8_lossy(bytes.take(size)).into_owned();
                let span = Span::new(start, start + length).unwrap_or_else(|_| {
                    unreachable!("a span from a start and a length is never reversed")
                });
                (span, replacement, order)
            })
            .collect();
        Self {
            source,
            edits,
            limits,
        }
    }

    /// Encodes a case that [`Case::decode`] reads back, for seeds and tests.
    pub fn encode(source: &str, edits: &[Encoded<'_>], limits: Option<(u8, u8)>) -> Vec<u8> {
        let small = if limits.is_some() { 0x10 } else { 0 };
        let mut bytes = vec![(edits.len() as u8 & 0x0F) | small, source.len() as u8];
        bytes.extend_from_slice(&source.as_bytes()[..source.len().min(255)]);
        if let Some((max_edits, half_output)) = limits {
            bytes.extend([max_edits, half_output]);
        }
        for &(start, length, operation, edit, replacement) in edits {
            bytes.extend([
                start as u8,
                length as u8,
                operation as u8,
                edit as u8,
                replacement.len() as u8,
            ]);
            bytes.extend_from_slice(replacement.as_bytes());
        }
        bytes
    }
}

struct Reader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> u8 {
        let byte = self.input.get(self.position).copied().unwrap_or(0);
        self.position += 1;
        byte
    }

    fn take(&mut self, length: usize) -> &'a [u8] {
        let start = self.position.min(self.input.len());
        let end = (self.position + length).min(self.input.len());
        self.position += length;
        &self.input[start..end]
    }
}

/// Decodes an input, applies its edits in both orders, and checks the result.
pub fn run(input: &[u8]) -> Result<(), String> {
    let case = Case::decode(input);
    let source = SourceText::new(case.source.clone(), TextLimits::default())
        .map_err(|error| format!("source text: {error:?}"))?;
    let edits: Vec<Edit<'_>> = case
        .edits
        .iter()
        .map(|(span, replacement, order)| Edit {
            span: *span,
            replacement,
            order: *order,
        })
        .collect();
    let result = apply_edits(&source, &edits, case.limits);
    let reversed: Vec<Edit<'_>> = edits.iter().rev().copied().collect();
    if apply_edits(&source, &reversed, case.limits) != result {
        return Err(format!("reversing the edits changed the result {result:?}"));
    }
    if edits.len() > case.limits.max_edits
        && result
            != Err(RewriteError::TooManyEdits {
                count: edits.len(),
                limit: case.limits.max_edits,
            })
    {
        return Err(format!(
            "too many edits were not rejected first: {result:?}"
        ));
    }
    match result {
        Ok(rewritten) => rewrite(&case, &edits, &rewritten),
        Err(
            RewriteError::InvalidSpan { .. }
            | RewriteError::DuplicateOrder { .. }
            | RewriteError::Conflict { .. }
            | RewriteError::TooManyEdits { .. }
            | RewriteError::OutputTooLarge { .. },
        ) => Ok(()),
        Err(error @ (RewriteError::AllocationFailed | RewriteError::InternalInvariant { .. })) => {
            Err(format!("unexpected rejection {error:?}"))
        }
    }
}

fn rewrite(case: &Case, edits: &[Edit<'_>], rewritten: &RewrittenText) -> Result<(), String> {
    let output = rewritten.as_str();
    if output.len() > case.limits.max_output_bytes {
        return Err(format!("{} output bytes exceed the limit", output.len()));
    }
    let find = |order: EditOrder| {
        edits
            .iter()
            .find(|edit| edit.order == order)
            .ok_or_else(|| format!("no edit has the order {order:?}"))
    };
    let slice = |span: Span| {
        output
            .get(span.start()..span.end())
            .ok_or_else(|| format!("{span:?} is outside the output"))
    };
    let mut consumed = Vec::new();
    let mut end = 0;
    for region in rewritten.provenance() {
        if region.output.start() != end || region.output.is_empty() {
            return Err(format!("{region:?} does not continue the output at {end}"));
        }
        end = region.output.end();
        let emitted = slice(region.output)?;
        let expected = match region.origin {
            Origin::Source(span) => {
                consumed.push(span);
                case.source.get(span.start()..span.end())
            }
            Origin::Edit { source, order } => {
                let edit = find(order)?;
                (edit.span == source).then_some(edit.replacement)
            }
        };
        if expected != Some(emitted) {
            return Err(format!("{region:?} emitted {emitted:?}, not {expected:?}"));
        }
    }
    if end != output.len() {
        return Err(format!(
            "provenance ends at {end} of {} bytes",
            output.len()
        ));
    }
    let applied = rewritten.applied_edits();
    let mut orders: Vec<EditOrder> = applied.iter().map(|edit| edit.order).collect();
    orders.sort_unstable();
    orders.dedup();
    if applied.len() != edits.len() || orders.len() != edits.len() {
        return Err(format!(
            "{} edits recorded for {} edits",
            applied.len(),
            edits.len()
        ));
    }
    for record in applied {
        let edit = find(record.order)?;
        if record.source != edit.span || slice(record.output)? != edit.replacement {
            return Err(format!("{record:?} does not match {edit:?}"));
        }
        if !record.source.is_empty() {
            consumed.push(record.source);
        }
    }
    // Copied and replaced source ranges tile the source exactly once.
    consumed.sort_unstable_by_key(|span| (span.start(), span.end()));
    let mut covered = 0;
    for span in consumed {
        if span.start() != covered {
            return Err(format!(
                "{span:?} does not continue the source at {covered}"
            ));
        }
        covered = span.end();
    }
    if covered != case.source.len() {
        return Err(format!(
            "source accounting ends at {covered} of {} bytes",
            case.source.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_cases_decode_back() {
        let encoded = Case::encode(
            "let é = 1;",
            &[(4, 2, 0, 1, "name"), (11, 0, 3, 3, "")],
            Some((5, 40)),
        );
        let case = Case::decode(&encoded);
        assert_eq!(case.source, "let é = 1;");
        assert_eq!(
            case.limits,
            EditLimits {
                max_edits: 5,
                max_output_bytes: 80
            }
        );
        let order = |operation, edit| EditOrder { operation, edit };
        assert_eq!(
            case.edits,
            [
                (Span::new(4, 6).unwrap(), "name".to_owned(), order(0, 1)),
                (Span::new(11, 11).unwrap(), String::new(), order(3, 3)),
            ]
        );
        // Truncated input reads zeros, and a missing source is empty.
        assert_eq!(
            Case::decode(&[0x02]).edits,
            [
                (Span::new(0, 0).unwrap(), String::new(), order(0, 0)),
                (Span::new(0, 0).unwrap(), String::new(), order(0, 0)),
            ]
        );
    }

    #[test]
    fn rewrites_and_rejections_satisfy_the_contracts() {
        for (edits, limits) in [
            (&[(4, 1, 0, 0, "value"), (0, 0, 1, 0, "/* a */ ")][..], None),
            (
                &[(2, 2, 0, 0, ""), (2, 0, 0, 1, "x"), (4, 0, 0, 2, "y")],
                None,
            ),
            (&[(1, 3, 0, 0, "a"), (2, 1, 0, 1, "b")], None),
            (&[(1, 0, 2, 2, "a"), (3, 0, 2, 2, "b")], None),
            (&[(5, 1, 0, 0, "")], None),
            (&[(0, 1, 0, 0, "é")], Some((0, 10))),
            (&[(0, 1, 0, 0, "longer")], Some((4, 2))),
        ] {
            let input = Case::encode("abcdé", edits, limits);
            run(&input).unwrap_or_else(|error| panic!("{edits:?}: {error}"));
        }
    }
}
