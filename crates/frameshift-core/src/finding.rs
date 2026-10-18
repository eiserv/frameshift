//! Bounded diagnostic records and deterministic report-local numbering.

use std::{cmp::Ordering, fmt};

use crate::{
    FeatureId, FindingCode, Position, RuleId, SourcePath, SourceText, Span, TextError, UnitId,
};

/// Diagnostic urgency, ordered from most to least severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// An operational failure or internal defect.
    Error,
    /// A recognized migration blocker.
    Blocker,
    /// A construct whose migration behavior is unknown.
    Unknown,
    /// A transformation requiring human review.
    Review,
    /// Information that does not change assessment.
    Info,
}

impl Severity {
    /// Returns the stable reporting spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Blocker => "blocker",
            Self::Unknown => "unknown",
            Self::Review => "review",
            Self::Info => "info",
        }
    }
}

/// A text range with positions derived from one immutable source snapshot.
///
/// The caller binds the path to that snapshot; this value performs no I/O and
/// does not verify a filesystem association or retain the source text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextLocation {
    file: SourcePath,
    span: Span,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

impl TextLocation {
    /// Validates bounds and scalar boundaries, including empty ranges at EOF.
    pub fn new(file: SourcePath, source: &SourceText, span: Span) -> Result<Self, TextError> {
        source.slice(span)?;
        let start = source.position(span.start())?;
        let end = source.position(span.end())?;
        Ok(Self {
            file,
            span,
            start_line: start.line,
            start_column: start.column,
            end_line: end.line,
            end_column: end.column,
        })
    }

    /// Borrows the original canonical source path.
    pub fn file(&self) -> &SourcePath {
        &self.file
    }

    /// Returns the canonical half-open byte range.
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Returns the derived one-based start position.
    pub const fn start(&self) -> Position {
        Position {
            offset: self.span.start(),
            line: self.start_line,
            column: self.start_column,
        }
    }

    /// Returns the derived one-based exclusive end position.
    pub const fn end(&self) -> Position {
        Position {
            offset: self.span.end(),
            line: self.end_line,
            column: self.end_column,
        }
    }
}

/// Diagnostic scope without fabricated text coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindingLocation {
    /// A run or project condition without a specific file.
    Project,
    /// A file condition, including unreadable or non-UTF-8 content.
    File(SourcePath),
    /// A validated range in decoded source text.
    Text(TextLocation),
}

impl FindingLocation {
    /// Returns the path for file and text scopes, or `None` for project scope.
    pub fn file(&self) -> Option<&SourcePath> {
        match self {
            Self::Project => None,
            Self::File(file) => Some(file),
            Self::Text(location) => Some(location.file()),
        }
    }

    fn range(&self) -> Option<&TextLocation> {
        match self {
            Self::Project | Self::File(_) => None,
            Self::Text(location) => Some(location),
        }
    }
}

impl Ord for FindingLocation {
    fn cmp(&self, other: &Self) -> Ordering {
        self.file()
            .cmp(&other.file())
            .then_with(|| self.range().cmp(&other.range()))
    }
}

impl PartialOrd for FindingLocation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Optional typed associations; identity does not establish catalog membership.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct FindingContext {
    /// Affected unit, if discovery has established one.
    pub unit: Option<UnitId>,
    /// Detected feature, where applicable.
    pub feature: Option<FeatureId>,
    /// Producing or blocked rule, where applicable.
    pub rule: Option<RuleId>,
}

/// Borrowed prose copied into a validated finding.
#[derive(Clone, Copy, Debug)]
pub struct FindingText<'a> {
    /// Nonblank one-line summary, without controls or Unicode line separators.
    pub message: &'a str,
    /// Nonblank explanation, without NUL. Other controls are retained verbatim.
    pub explanation: &'a str,
    /// Nonblank concrete remediation, without NUL. May span multiple lines.
    pub remediation: &'a str,
}

/// Inclusive diagnostic storage limits, independent of parser recovery limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindingLimits {
    /// Maximum UTF-8 bytes in each prose field. Defaults to 64 KiB.
    pub max_text_bytes: usize,
    /// Maximum records in a collector. Defaults to 100,000, counting duplicates.
    pub max_findings: usize,
}

impl Default for FindingLimits {
    fn default() -> Self {
        Self {
            max_text_bytes: 64 * 1024,
            max_findings: 100_000,
        }
    }
}

/// Invalid diagnostic storage or prose, without any source-derived text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingError {
    /// At least one prose field exceeds the explicit byte bound.
    TextTooLong,
    /// A prose field is blank or contains NUL, or a summary is not one line.
    InvalidText,
    /// Adding a record would exceed the collector's count bound.
    TooManyFindings,
    /// Record or prose storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for FindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TextTooLong => "finding text exceeds byte limit",
            Self::InvalidText => "invalid finding text",
            Self::TooManyFindings => "finding count exceeds limit",
            Self::AllocationFailed => "finding storage allocation failed",
        })
    }
}

impl std::error::Error for FindingError {}

/// Immutable diagnostic evidence without a report-local number or source snippet.
///
/// Ordering compares severity, location, code, context (unit, feature, rule),
/// message, explanation, and remediation, in that order. Every field participates.
/// Prose and paths require context-specific escaping before rendering.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    severity: Severity,
    location: FindingLocation,
    code: FindingCode,
    context: FindingContext,
    message: String,
    explanation: String,
    remediation: String,
}

impl Finding {
    /// Checks every prose byte limit before content validation, then copies fallibly.
    ///
    /// Typed associations and location are moved in. Their constructors' own limits
    /// remain authoritative; `max_findings` applies only to a collector.
    pub fn new(
        code: FindingCode,
        severity: Severity,
        location: FindingLocation,
        context: FindingContext,
        text: FindingText<'_>,
        limits: FindingLimits,
    ) -> Result<Self, FindingError> {
        let fields = [text.message, text.explanation, text.remediation];
        if fields
            .iter()
            .any(|field| field.len() > limits.max_text_bytes)
        {
            return Err(FindingError::TextTooLong);
        }
        if fields
            .iter()
            .any(|field| field.trim().is_empty() || field.contains('\0'))
            || text
                .message
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
        {
            return Err(FindingError::InvalidText);
        }
        Ok(Self {
            code,
            severity,
            location,
            context,
            message: copy(text.message)?,
            explanation: copy(text.explanation)?,
            remediation: copy(text.remediation)?,
        })
    }

    /// Returns the diagnostic severity.
    pub const fn severity(&self) -> Severity {
        self.severity
    }
    /// Borrows the scope and optional validated text coordinates.
    pub fn location(&self) -> &FindingLocation {
        &self.location
    }
    /// Borrows the stable code, without claiming catalog membership.
    pub fn code(&self) -> &FindingCode {
        &self.code
    }
    /// Borrows optional unit, feature, and rule associations.
    pub fn context(&self) -> &FindingContext {
        &self.context
    }
    /// Borrows the one-line summary.
    pub fn message(&self) -> &str {
        &self.message
    }
    /// Borrows the explanation verbatim.
    pub fn explanation(&self) -> &str {
        &self.explanation
    }
    /// Borrows the remediation verbatim.
    pub fn remediation(&self) -> &str {
        &self.remediation
    }
}

fn copy(value: &str) -> Result<String, FindingError> {
    let mut text = String::new();
    text.try_reserve_exact(value.len())
        .map_err(|_| FindingError::AllocationFailed)?;
    text.push_str(value);
    Ok(text)
}

/// A one-based identity within one finalized finding list, never a persistent key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FindingId(usize);

impl FindingId {
    /// Returns the one-based report-local ordinal.
    pub const fn number(self) -> usize {
        self.0
    }
}

impl fmt::Display for FindingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "F{:04}", self.0)
    }
}

/// Bounded append-only diagnostic storage before deterministic finalization.
#[derive(Debug)]
pub struct FindingCollector {
    entries: Vec<Finding>,
    limits: FindingLimits,
}

impl FindingCollector {
    /// Starts empty without allocating.
    pub const fn new(limits: FindingLimits) -> Self {
        Self {
            entries: Vec::new(),
            limits,
        }
    }

    /// Revalidates prose size and count before reserving storage; failure leaves
    /// existing evidence unchanged. Duplicates count as separate records.
    pub fn push(&mut self, finding: Finding) -> Result<(), FindingError> {
        if self.entries.len() >= self.limits.max_findings {
            return Err(FindingError::TooManyFindings);
        }
        if [
            finding.message(),
            finding.explanation(),
            finding.remediation(),
        ]
        .iter()
        .any(|field| field.len() > self.limits.max_text_bytes)
        {
            return Err(FindingError::TextTooLong);
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| FindingError::AllocationFailed)?;
        self.entries.push(finding);
        Ok(())
    }

    /// Sorts in place without allocation in O(n log n) worst-case comparisons.
    /// Exact duplicates remain, with indistinguishable records receiving successive IDs.
    pub fn finish(mut self) -> FindingList {
        self.entries.sort_unstable();
        FindingList {
            entries: self.entries,
        }
    }
}

/// Immutable, completely ordered evidence with reproducible report-local IDs.
#[derive(Debug, PartialEq, Eq)]
pub struct FindingList {
    entries: Vec<Finding>,
}

impl FindingList {
    /// Borrows records in their canonical order.
    pub fn as_slice(&self) -> &[Finding] {
        &self.entries
    }
    /// Iterates numbered records, starting at `F0001` and expanding beyond four digits.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (FindingId, &Finding)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, finding)| (FindingId(index + 1), finding))
    }
}
