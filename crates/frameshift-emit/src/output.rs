//! Emitted text with byte-region provenance across several source files.

use std::fmt;

use frameshift_core::{SourceText, Span};
use frameshift_transform::{Origin, RewrittenText};

/// Identifies one source file of an emission. Callers assign identifiers
/// deterministically, for example from the sorted list of source paths, and
/// pass the sources in the same order to every function that resolves them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(pub u32);

impl SourceId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// A source construct: a byte span in one source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    /// Source file.
    pub source: SourceId,
    /// Half-open byte span in that file. It may be empty for an insertion point.
    pub span: Span,
}

/// Where the bytes of an output region come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionOrigin {
    /// Bytes copied exactly from the anchored source range.
    Copied(Anchor),
    /// Bytes generated for the anchored source construct. This records which
    /// construct is responsible; it does not claim equivalence.
    Generated(Anchor),
}

/// A nonempty output region and its origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputRegion {
    /// Half-open output byte range.
    pub output: Span,
    /// Copied or generated origin of these bytes.
    pub origin: RegionOrigin,
}

/// Generated text whose regions partition its bytes, each mapped to a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedText {
    text: String,
    regions: Vec<OutputRegion>,
}

impl EmittedText {
    /// Converts rewritten text of one source file. Copied regions keep their
    /// source spans, and replacement text becomes generated regions anchored
    /// at the span its edit consumed.
    pub fn from_rewritten(source: SourceId, rewritten: &RewrittenText) -> Self {
        let regions = rewritten
            .provenance()
            .iter()
            .map(|region| OutputRegion {
                output: region.output,
                origin: match region.origin {
                    Origin::Source(span) => RegionOrigin::Copied(Anchor { source, span }),
                    Origin::Edit { source: span, .. } => {
                        RegionOrigin::Generated(Anchor { source, span })
                    }
                },
            })
            .collect();
        Self {
            text: rewritten.as_str().to_owned(),
            regions,
        }
    }

    /// Borrows the emitted text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the regions in output order. Together they cover every byte
    /// exactly once.
    pub fn regions(&self) -> &[OutputRegion] {
        &self.regions
    }

    /// Checks the provenance contract against the sources the anchors refer
    /// to: regions are nonempty, contiguous, and cover the text, every anchor
    /// is a valid span of its source, and every copied region equals its
    /// source bytes. Returns the first violation.
    pub fn verify(&self, sources: &[&SourceText]) -> Result<(), ProvenanceError> {
        let mut offset = 0;
        for (index, region) in self.regions.iter().enumerate() {
            if region.output.start() != offset || region.output.is_empty() {
                return Err(ProvenanceError::Gap { region: index });
            }
            let output = self
                .text
                .get(region.output.start()..region.output.end())
                .ok_or(ProvenanceError::Gap { region: index })?;
            let (anchor, copied) = match region.origin {
                RegionOrigin::Copied(anchor) => (anchor, true),
                RegionOrigin::Generated(anchor) => (anchor, false),
            };
            let source = sources
                .get(anchor.source.index())
                .and_then(|source| source.slice(anchor.span).ok())
                .ok_or(ProvenanceError::InvalidAnchor { region: index })?;
            if copied && source != output {
                return Err(ProvenanceError::CopyMismatch { region: index });
            }
            offset = region.output.end();
        }
        if offset == self.text.len() {
            Ok(())
        } else {
            Err(ProvenanceError::Gap {
                region: self.regions.len(),
            })
        }
    }
}

/// A violated provenance contract, reported by [`EmittedText::verify`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    /// The region at this index is empty or does not start where the
    /// previous one ended, or, at the region count, the text is not covered.
    Gap {
        /// Index of the offending region.
        region: usize,
    },
    /// The region's anchor names an unknown source or an invalid span.
    InvalidAnchor {
        /// Index of the offending region.
        region: usize,
    },
    /// A copied region differs from its source bytes.
    CopyMismatch {
        /// Index of the offending region.
        region: usize,
    },
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gap { region } => write!(f, "provenance gap at region {region}"),
            Self::InvalidAnchor { region } => write!(f, "invalid anchor in region {region}"),
            Self::CopyMismatch { region } => {
                write!(f, "copied region {region} differs from its source")
            }
        }
    }
}

impl std::error::Error for ProvenanceError {}

/// Explicit limits for emitting one output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitLimits {
    /// Maximum output bytes. Defaults to 64 MiB.
    pub max_output_bytes: usize,
    /// Maximum nesting of converted or printed markup and expressions.
    /// Defaults to 256.
    pub max_depth: usize,
}

impl Default for EmitLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 64 * 1024 * 1024,
            max_depth: 256,
        }
    }
}

/// Builds an [`EmittedText`] from generated and copied pieces.
///
/// Adjacent pieces with the same origin share one region: generated pieces
/// with the same anchor, and copied pieces that are contiguous in one source.
pub struct OutputBuilder<'s> {
    sources: &'s [&'s SourceText],
    limit: usize,
    text: String,
    regions: Vec<OutputRegion>,
}

impl<'s> OutputBuilder<'s> {
    /// Starts an empty output that resolves anchors in `sources`, indexed by
    /// [`SourceId`].
    pub fn new(sources: &'s [&'s SourceText], limits: EmitLimits) -> Self {
        Self {
            sources,
            limit: limits.max_output_bytes,
            text: String::new(),
            regions: Vec::new(),
        }
    }

    /// Appends source bytes copied exactly from `anchor`.
    pub fn push_copied(&mut self, anchor: Anchor) -> Result<(), BuildError> {
        let source = self.source_slice(anchor)?;
        if source.is_empty() {
            return Ok(());
        }
        let start = self.reserve(source.len())?;
        self.text.push_str(source);
        if let Some(last) = self.regions.last_mut()
            && let RegionOrigin::Copied(previous) = &mut last.origin
            && previous.source == anchor.source
            && previous.span.end() == anchor.span.start()
        {
            previous.span = span(previous.span.start(), anchor.span.end());
            last.output = span(last.output.start(), self.text.len());
            return Ok(());
        }
        self.regions.push(OutputRegion {
            output: span(start, self.text.len()),
            origin: RegionOrigin::Copied(anchor),
        });
        Ok(())
    }

    /// Appends generated `text` for the construct at `anchor`.
    pub fn push_generated(&mut self, text: &str, anchor: Anchor) -> Result<(), BuildError> {
        self.source_slice(anchor)?;
        if text.is_empty() {
            return Ok(());
        }
        let start = self.reserve(text.len())?;
        self.text.push_str(text);
        if let Some(last) = self.regions.last_mut()
            && last.origin == RegionOrigin::Generated(anchor)
        {
            last.output = span(last.output.start(), self.text.len());
            return Ok(());
        }
        self.regions.push(OutputRegion {
            output: span(start, self.text.len()),
            origin: RegionOrigin::Generated(anchor),
        });
        Ok(())
    }

    /// Returns the bytes emitted so far.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Finishes the output.
    pub fn finish(self) -> EmittedText {
        EmittedText {
            text: self.text,
            regions: self.regions,
        }
    }

    fn source_slice(&self, anchor: Anchor) -> Result<&'s str, BuildError> {
        self.sources
            .get(anchor.source.index())
            .and_then(|source| source.slice(anchor.span).ok())
            .ok_or(BuildError::InvalidAnchor(anchor))
    }

    fn reserve(&mut self, additional: usize) -> Result<usize, BuildError> {
        let start = self.text.len();
        let end = start
            .checked_add(additional)
            .filter(|end| *end <= self.limit)
            .ok_or(BuildError::OutputTooLarge { limit: self.limit })?;
        self.text
            .try_reserve(end - start)
            .map_err(|_| BuildError::AllocationFailed)?;
        Ok(start)
    }
}

/// A rejected append to an [`OutputBuilder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// The anchor names an unknown source or a span outside it or inside a
    /// UTF-8 scalar.
    InvalidAnchor(Anchor),
    /// The output would exceed the configured byte limit.
    OutputTooLarge {
        /// Configured limit.
        limit: usize,
    },
    /// Storage for the output could not be reserved.
    AllocationFailed,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAnchor(anchor) => write!(
                f,
                "invalid anchor {}..{} in source {}",
                anchor.span.start(),
                anchor.span.end(),
                anchor.source.0
            ),
            Self::OutputTooLarge { limit } => write!(f, "output exceeds byte limit {limit}"),
            Self::AllocationFailed => f.write_str("could not allocate output"),
        }
    }
}

impl std::error::Error for BuildError {}

// Spans built here come from ordered offsets of the same text.
fn span(start: usize, end: usize) -> Span {
    Span::new(start, end).unwrap_or_else(|_| unreachable!("ordered output offsets"))
}

#[cfg(test)]
mod tests {
    use frameshift_core::TextLimits;
    use frameshift_transform::{Edit, EditLimits, EditOrder, apply_edits};

    use super::*;

    fn source(text: &str) -> SourceText {
        SourceText::new(text.to_owned(), TextLimits::default()).unwrap()
    }

    fn anchor(source: u32, start: usize, end: usize) -> Anchor {
        Anchor {
            source: SourceId(source),
            span: Span::new(start, end).unwrap(),
        }
    }

    #[test]
    fn regions_merge_and_partition_the_output() {
        let first = source("const a = 1;\nconst b = 2;\n");
        let second = source("<p>x</p>");
        let sources = [&first, &second];
        let mut builder = OutputBuilder::new(&sources, EmitLimits::default());
        builder.push_generated("---\n", anchor(1, 0, 8)).unwrap();
        builder.push_copied(anchor(0, 0, 13)).unwrap();
        builder.push_copied(anchor(0, 13, 26)).unwrap();
        builder.push_generated("---\n", anchor(1, 0, 8)).unwrap();
        builder.push_generated("", anchor(1, 0, 0)).unwrap();
        builder.push_copied(anchor(1, 3, 3)).unwrap();
        let emitted = builder.finish();
        assert_eq!(emitted.as_str(), "---\nconst a = 1;\nconst b = 2;\n---\n");
        assert_eq!(emitted.regions().len(), 3);
        assert_eq!(
            emitted.regions()[1].origin,
            RegionOrigin::Copied(anchor(0, 0, 26))
        );
        assert_eq!(emitted.verify(&sources), Ok(()));
    }

    #[test]
    fn invalid_anchors_and_limits_are_rejected() {
        let only = source("é");
        let sources = [&only];
        let limits = EmitLimits {
            max_output_bytes: 3,
            ..EmitLimits::default()
        };
        let mut builder = OutputBuilder::new(&sources, limits);
        assert_eq!(
            builder.push_copied(anchor(0, 0, 1)),
            Err(BuildError::InvalidAnchor(anchor(0, 0, 1)))
        );
        assert_eq!(
            builder.push_generated("x", anchor(1, 0, 0)),
            Err(BuildError::InvalidAnchor(anchor(1, 0, 0)))
        );
        builder.push_generated("ab", anchor(0, 0, 2)).unwrap();
        assert_eq!(
            builder.push_copied(anchor(0, 0, 2)),
            Err(BuildError::OutputTooLarge { limit: 3 })
        );
        assert_eq!(builder.as_str(), "ab");
    }

    #[test]
    fn rewritten_text_keeps_copied_and_edited_regions() {
        let original = source("'use client';\n\nexport const x = 1;\n");
        let delete = Edit {
            span: Span::new(0, 15).unwrap(),
            replacement: "",
            order: EditOrder {
                operation: 0,
                edit: 0,
            },
        };
        let rename = Edit {
            span: Span::new(28, 29).unwrap(),
            replacement: "y",
            order: EditOrder {
                operation: 0,
                edit: 1,
            },
        };
        let rewritten = apply_edits(&original, &[delete, rename], EditLimits::default()).unwrap();
        let emitted = EmittedText::from_rewritten(SourceId(0), &rewritten);
        assert_eq!(emitted.as_str(), "export const y = 1;\n");
        assert_eq!(
            emitted.regions()[1].origin,
            RegionOrigin::Generated(anchor(0, 28, 29))
        );
        assert_eq!(emitted.verify(&[&original]), Ok(()));
        let other = source("export const z = 1;\n");
        assert!(emitted.verify(&[&other]).is_err());
    }

    #[test]
    fn verify_reports_gaps_mismatches_and_unknown_sources() {
        let original = source("abc");
        let tampered = EmittedText {
            text: "abd".to_owned(),
            regions: vec![OutputRegion {
                output: Span::new(0, 3).unwrap(),
                origin: RegionOrigin::Copied(anchor(0, 0, 3)),
            }],
        };
        assert_eq!(
            tampered.verify(&[&original]),
            Err(ProvenanceError::CopyMismatch { region: 0 })
        );
        assert_eq!(
            tampered.verify(&[]),
            Err(ProvenanceError::InvalidAnchor { region: 0 })
        );
        let short = EmittedText {
            text: "abc".to_owned(),
            regions: vec![OutputRegion {
                output: Span::new(0, 2).unwrap(),
                origin: RegionOrigin::Generated(anchor(0, 0, 3)),
            }],
        };
        assert_eq!(
            short.verify(&[&original]),
            Err(ProvenanceError::Gap { region: 1 })
        );
    }
}
