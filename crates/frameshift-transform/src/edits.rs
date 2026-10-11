use std::fmt;

use frameshift_core::{SourceText, Span, TextError};

/// Stable ordering within an operation and across operations.
///
/// Each edit in one invocation must have a unique pair. Numbers come from the
/// caller's deterministic operation ordering, never worker completion order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditOrder {
    /// Zero-based operation order.
    pub operation: u32,
    /// Zero-based edit order within that operation.
    pub edit: u32,
}

/// A replacement, deletion, or insertion against the original source bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit<'a> {
    /// Source range to replace. An empty span inserts at that offset.
    pub span: Span,
    /// Replacement text. An empty string deletes the source range.
    pub replacement: &'a str,
    /// Stable operation and edit order, also used by provenance records.
    pub order: EditOrder,
}

/// Explicit deterministic limits applied before allocating rewritten text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditLimits {
    /// Maximum number of edits. Defaults to 100,000 per invocation.
    pub max_edits: usize,
    /// Maximum output bytes. Defaults to 64 MiB.
    pub max_output_bytes: usize,
}

impl Default for EditLimits {
    fn default() -> Self {
        Self {
            max_edits: 100_000,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A rejected rewrite. No partial output is returned on error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewriteError {
    /// A byte span is outside the source or divides a UTF-8 scalar.
    InvalidSpan {
        /// Stable order of the rejected edit.
        order: EditOrder,
        /// Underlying text validation error.
        error: TextError,
    },
    /// Two edits have the same operation and edit order.
    DuplicateOrder {
        /// Repeated ordering pair.
        order: EditOrder,
    },
    /// Nonempty ranges overlap, or an insertion lies inside a replaced range.
    Conflict {
        /// Earlier edit in source order.
        earlier: EditOrder,
        /// Later conflicting edit in source order.
        later: EditOrder,
    },
    /// Edit count exceeds the configured limit.
    TooManyEdits {
        /// Actual edit count.
        count: usize,
        /// Configured edit limit.
        limit: usize,
    },
    /// Output size exceeds the configured limit or cannot fit in usize.
    OutputTooLarge {
        /// Configured output byte limit.
        limit: usize,
    },
    /// Storage for edits, text, or provenance could not be reserved.
    AllocationFailed,
    /// An internal text-range invariant was violated. Results are untrustworthy.
    InternalInvariant {
        /// Range error that exposed the defect.
        error: TextError,
    },
}

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpan { order, error } => {
                write!(
                    f,
                    "invalid span for edit {}:{}: {error}",
                    order.operation, order.edit
                )
            }
            Self::DuplicateOrder { order } => {
                write!(f, "duplicate edit order {}:{}", order.operation, order.edit)
            }
            Self::Conflict { earlier, later } => write!(
                f,
                "edit {}:{} conflicts with edit {}:{}",
                earlier.operation, earlier.edit, later.operation, later.edit
            ),
            Self::TooManyEdits { count, limit } => {
                write!(f, "edit count {count} exceeds limit {limit}")
            }
            Self::OutputTooLarge { limit } => {
                write!(f, "rewritten text exceeds output byte limit {limit}")
            }
            Self::AllocationFailed => {
                f.write_str("could not allocate rewritten text or provenance")
            }
            Self::InternalInvariant { error } => write!(f, "internal rewrite error: {error}"),
        }
    }
}

impl std::error::Error for RewriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidSpan { error, .. } | Self::InternalInvariant { error } => Some(error),
            Self::DuplicateOrder { .. }
            | Self::Conflict { .. }
            | Self::TooManyEdits { .. }
            | Self::OutputTooLarge { .. }
            | Self::AllocationFailed => None,
        }
    }
}

/// Origin of a nonempty emitted region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Unchanged bytes copied exactly from the specified source range.
    Source(Span),
    /// Replacement text associated with the consumed source range and edit.
    /// This records origin; it does not claim byte-for-byte equivalence.
    Edit {
        /// Original source range, empty for an insertion.
        source: Span,
        /// Stable order of the edit that emitted this region.
        order: EditOrder,
    },
}

/// An output region and its source or edit origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvenanceRegion {
    /// Nonempty half-open output byte range.
    pub output: Span,
    /// Source or edit responsible for these bytes.
    pub origin: Origin,
}

/// A record for every edit, including deletions that emit no bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedEdit {
    /// Stable order of the edit.
    pub order: EditOrder,
    /// Consumed source range, empty for insertions.
    pub source: Span,
    /// Emitted output range, empty for deletions.
    pub output: Span,
}

/// Successfully rewritten text with contiguous output provenance.
#[derive(Debug, PartialEq, Eq)]
pub struct RewrittenText {
    text: String,
    regions: Vec<ProvenanceRegion>,
    edits: Vec<AppliedEdit>,
}

impl RewrittenText {
    /// Borrows the emitted text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Borrows regions that partition every output byte exactly once.
    pub fn provenance(&self) -> &[ProvenanceRegion] {
        &self.regions
    }

    /// Borrows applied edits in emission order, including empty output ranges.
    pub fn applied_edits(&self) -> &[AppliedEdit] {
        &self.edits
    }
}

/// Applies validated, non-overlapping edits to immutable original source text.
///
/// Implements the text-edit contract of `docs/ARCHITECTURE.md`, section 14.2.
/// Insertions sharing an offset use operation order, then edit order. At a
/// replacement boundary, insertions precede a replacement starting there and
/// follow a replacement ending there. Insertions strictly inside a replacement
/// and overlapping nonempty ranges are rejected, including identical edits.
///
/// All edits and sizes are checked before allocating output. Input enumeration
/// order has no effect on text, provenance, or validation errors. Complexity is
/// O(source bytes + replacement bytes + edits * log(edits)). No I/O is performed.
pub fn apply_edits(
    source: &SourceText,
    edits: &[Edit<'_>],
    limits: EditLimits,
) -> Result<RewrittenText, RewriteError> {
    if edits.len() > limits.max_edits {
        return Err(RewriteError::TooManyEdits {
            count: edits.len(),
            limit: limits.max_edits,
        });
    }
    let mut ordered: Vec<&Edit<'_>> = Vec::new();
    ordered
        .try_reserve_exact(edits.len())
        .map_err(|_| RewriteError::AllocationFailed)?;
    ordered.extend(edits.iter());
    ordered.sort_unstable_by_key(|edit| edit.order);
    for pair in ordered.windows(2) {
        if pair[0].order == pair[1].order {
            return Err(RewriteError::DuplicateOrder {
                order: pair[0].order,
            });
        }
    }
    ordered.sort_unstable_by_key(|edit| (edit.span.start(), !edit.span.is_empty(), edit.order));
    let mut previous: Option<&Edit<'_>> = None;
    let mut removed_bytes = 0;
    let mut replacement_bytes = 0usize;
    for edit in &ordered {
        source
            .slice(edit.span)
            .map_err(|error| RewriteError::InvalidSpan {
                order: edit.order,
                error,
            })?;
        if let Some(earlier) = previous
            && edit.span.start() < earlier.span.end()
        {
            return Err(RewriteError::Conflict {
                earlier: earlier.order,
                later: edit.order,
            });
        }
        if !edit.span.is_empty() {
            previous = Some(edit);
        }
        removed_bytes += edit.span.len();
        replacement_bytes = replacement_bytes
            .checked_add(edit.replacement.len())
            .ok_or(RewriteError::OutputTooLarge {
                limit: limits.max_output_bytes,
            })?;
    }
    let output_bytes = (source.len() - removed_bytes)
        .checked_add(replacement_bytes)
        .filter(|&bytes| bytes <= limits.max_output_bytes)
        .ok_or(RewriteError::OutputTooLarge {
            limit: limits.max_output_bytes,
        })?;
    let mut result = RewrittenText {
        text: String::new(),
        regions: Vec::new(),
        edits: Vec::new(),
    };
    result
        .text
        .try_reserve_exact(output_bytes)
        .map_err(|_| RewriteError::AllocationFailed)?;
    let region_capacity = ordered
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(1))
        .ok_or(RewriteError::AllocationFailed)?;
    result
        .regions
        .try_reserve_exact(region_capacity)
        .map_err(|_| RewriteError::AllocationFailed)?;
    result
        .edits
        .try_reserve_exact(ordered.len())
        .map_err(|_| RewriteError::AllocationFailed)?;
    let mut cursor = 0;
    for edit in ordered {
        if cursor < edit.span.start() {
            copy_region(&mut result, source, span(cursor, edit.span.start())?)?;
        }
        let output_start = result.text.len();
        result.text.push_str(edit.replacement);
        let output = span(output_start, result.text.len())?;
        if !output.is_empty() {
            result.regions.push(ProvenanceRegion {
                output,
                origin: Origin::Edit {
                    source: edit.span,
                    order: edit.order,
                },
            });
        }
        result.edits.push(AppliedEdit {
            order: edit.order,
            source: edit.span,
            output,
        });
        cursor = edit.span.end();
    }
    if cursor < source.len() {
        copy_region(&mut result, source, span(cursor, source.len())?)?;
    }
    Ok(result)
}

fn span(start: usize, end: usize) -> Result<Span, RewriteError> {
    Span::new(start, end).map_err(|error| RewriteError::InternalInvariant { error })
}

fn copy_region(
    result: &mut RewrittenText,
    source: &SourceText,
    range: Span,
) -> Result<(), RewriteError> {
    let text = source
        .slice(range)
        .map_err(|error| RewriteError::InternalInvariant { error })?;
    let start = result.text.len();
    result.text.push_str(text);
    result.regions.push(ProvenanceRegion {
        output: span(start, result.text.len())?,
        origin: Origin::Source(range),
    });
    Ok(())
}
