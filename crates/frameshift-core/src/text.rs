use std::fmt;

const CHECKPOINT_INTERVAL: usize = 128;

/// A half-open byte range. Offsets refer to the original source, including a BOM.
///
/// The default is the empty range at offset 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    /// Constructs a range, rejecting a start after its end.
    ///
    /// Source bounds and UTF-8 boundaries are checked by [`SourceText::slice`].
    pub fn new(start: usize, end: usize) -> Result<Self, TextError> {
        if start > end {
            return Err(TextError::ReversedSpan { start, end });
        }
        Ok(Self { start, end })
    }

    /// Returns the inclusive starting byte offset.
    pub const fn start(self) -> usize {
        self.start
    }

    /// Returns the exclusive ending byte offset.
    pub const fn end(self) -> usize {
        self.end
    }

    /// Returns the number of bytes in the range.
    pub const fn len(self) -> usize {
        self.end - self.start
    }

    /// Reports whether the range is an insertion point.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A canonical byte offset and its one-based, Unicode-scalar line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Zero-based byte offset.
    pub offset: usize,
    /// One-based line number.
    pub line: u32,
    /// One-based column counting Unicode scalar values, including tabs and BOMs.
    pub column: u32,
}

/// Explicit resource limits for immutable source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLimits {
    /// Maximum source size in bytes. Defaults to 16 MiB.
    pub max_source_bytes: usize,
}

impl Default for TextLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 16 * 1024 * 1024,
        }
    }
}

/// A rejected text operation. Diagnostics contain offsets, never source content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    /// A span starts after it ends.
    ReversedSpan {
        /// Requested starting offset.
        start: usize,
        /// Requested ending offset.
        end: usize,
    },
    /// An offset exceeds the source length.
    OutOfBounds {
        /// Requested offset.
        offset: usize,
        /// Source length in bytes.
        len: usize,
    },
    /// An offset divides a UTF-8 scalar value.
    NotCharBoundary {
        /// Requested offset.
        offset: usize,
    },
    /// Input is not UTF-8.
    InvalidUtf8 {
        /// Byte offset before which the input is valid UTF-8.
        valid_up_to: usize,
    },
    /// Input exceeds an explicit resource limit or the compact index capacity.
    SourceTooLarge {
        /// Actual source size.
        bytes: usize,
        /// Effective size limit.
        limit: usize,
    },
    /// An index allocation could not be reserved.
    AllocationFailed,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReversedSpan { start, end } => {
                write!(f, "span start {start} exceeds end {end}")
            }
            Self::OutOfBounds { offset, len } => {
                write!(f, "byte offset {offset} exceeds source length {len}")
            }
            Self::NotCharBoundary { offset } => {
                write!(f, "byte offset {offset} is not a UTF-8 boundary")
            }
            Self::InvalidUtf8 { valid_up_to } => {
                write!(f, "source is not UTF-8 at byte offset {valid_up_to}")
            }
            Self::SourceTooLarge { bytes, limit } => {
                write!(f, "source size {bytes} exceeds byte limit {limit}")
            }
            Self::AllocationFailed => f.write_str("could not allocate source position index"),
        }
    }
}

impl std::error::Error for TextError {}

#[derive(Clone, Copy, Debug)]
struct IndexEntry {
    offset: u32,
    scalars: u32,
}

/// Immutable UTF-8 text with a compact, bounded position index.
///
/// LF, CRLF, CR, U+2028, and U+2029 terminate lines. CRLF is one terminator:
/// the boundary between its two bytes is still on the preceding line.
/// Original bytes, line endings, and the BOM are never normalized.
#[derive(Debug)]
pub struct SourceText {
    text: String,
    lines: Vec<IndexEntry>,
    checkpoints: Vec<IndexEntry>,
}

impl SourceText {
    /// Takes ownership of source bytes after checking size and UTF-8 encoding.
    pub fn from_bytes(bytes: Vec<u8>, limits: TextLimits) -> Result<Self, TextError> {
        check_size(bytes.len(), limits)?;
        let text = String::from_utf8(bytes).map_err(|error| TextError::InvalidUtf8 {
            valid_up_to: error.utf8_error().valid_up_to(),
        })?;
        Self::new(text, limits)
    }

    /// Takes ownership of UTF-8 text and builds the line and scalar indexes.
    ///
    /// Construction is linear in source size. Each position lookup scans at
    /// most 128 scalar values after binary searches in the two indexes.
    pub fn new(text: String, limits: TextLimits) -> Result<Self, TextError> {
        check_size(text.len(), limits)?;
        let line_count = text
            .char_indices()
            .filter(|&(offset, ch)| is_line_end(&text, offset, ch))
            .count()
            + 1;
        let mut lines = Vec::new();
        lines
            .try_reserve_exact(line_count)
            .map_err(|_| TextError::AllocationFailed)?;
        let mut checkpoints = Vec::new();
        checkpoints
            .try_reserve_exact(text.len() / CHECKPOINT_INTERVAL + 1)
            .map_err(|_| TextError::AllocationFailed)?;
        let first = IndexEntry {
            offset: 0,
            scalars: 0,
        };
        lines.push(first);
        for (scalars, (offset, ch)) in text.char_indices().enumerate() {
            if scalars % CHECKPOINT_INTERVAL == 0 {
                checkpoints.push(IndexEntry {
                    offset: offset as u32,
                    scalars: scalars as u32,
                });
            }
            if is_line_end(&text, offset, ch) {
                lines.push(IndexEntry {
                    offset: (offset + ch.len_utf8()) as u32,
                    scalars: scalars as u32 + 1,
                });
            }
        }
        if checkpoints.is_empty() {
            checkpoints.push(first);
        }
        Ok(Self {
            text,
            lines,
            checkpoints,
        })
    }

    /// Borrows the original text, without normalization.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the source size in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Reports whether the source contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Validates and borrows a half-open source range.
    pub fn slice(&self, span: Span) -> Result<&str, TextError> {
        self.validate_offset(span.start)?;
        self.validate_offset(span.end)?;
        Ok(&self.text[span.start..span.end])
    }

    /// Validates an offset, allowing EOF but rejecting partial UTF-8 scalars.
    pub fn validate_offset(&self, offset: usize) -> Result<(), TextError> {
        if offset > self.text.len() {
            return Err(TextError::OutOfBounds {
                offset,
                len: self.text.len(),
            });
        }
        if !self.text.is_char_boundary(offset) {
            return Err(TextError::NotCharBoundary { offset });
        }
        Ok(())
    }

    /// Resolves a validated byte offset to a one-based line and scalar column.
    pub fn position(&self, offset: usize) -> Result<Position, TextError> {
        self.validate_offset(offset)?;
        let line_index = self
            .lines
            .partition_point(|entry| entry.offset as usize <= offset)
            - 1;
        let line = self.lines[line_index];
        let checkpoint_index = self
            .checkpoints
            .partition_point(|entry| entry.offset as usize <= offset)
            - 1;
        let checkpoint = self.checkpoints[checkpoint_index];
        let scalars = checkpoint.scalars
            + self.text[checkpoint.offset as usize..offset]
                .chars()
                .count() as u32;
        Ok(Position {
            offset,
            line: line_index as u32 + 1,
            column: scalars - line.scalars + 1,
        })
    }
}

fn is_line_end(text: &str, offset: usize, ch: char) -> bool {
    match ch {
        '\n' | '\u{2028}' | '\u{2029}' => true,
        '\r' => text.as_bytes().get(offset + 1) != Some(&b'\n'),
        _ => false,
    }
}

fn check_size(bytes: usize, limits: TextLimits) -> Result<(), TextError> {
    // Keep both EOF positions and one-based columns representable by u32.
    let limit = limits.max_source_bytes.min(u32::MAX as usize - 1);
    if bytes > limit {
        return Err(TextError::SourceTooLarge { bytes, limit });
    }
    Ok(())
}
