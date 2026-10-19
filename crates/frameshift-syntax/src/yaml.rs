//! YAML 1.2 syntax, with a flat graph and unexpanded aliases (`D-071`).
//!
//! Scalar values are decoded text, not host-language objects. Tags are
//! preserved, never executed. Schema resolution belongs to the consumer.

mod parser;

use std::fmt;

use frameshift_core::{SourceText, Span};

/// Explicit bounds for one YAML stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YamlLimits {
    /// Maximum collection nesting, default 128. The implementation ceiling
    /// is 128, including when callers request a larger bound.
    pub max_depth: usize,
    /// Maximum nodes, including keys and alias occurrences, default 4,194,304.
    pub max_nodes: usize,
}

impl Default for YamlLimits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_nodes: 4_194_304,
        }
    }
}

/// A node identity in one stream's flat arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct YamlId(usize);

impl YamlId {
    /// Returns the arena position.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// The presentation style of a scalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YamlScalarStyle {
    /// An unquoted scalar, including an empty node.
    Plain,
    /// A single-quoted scalar.
    SingleQuoted,
    /// A double-quoted scalar.
    DoubleQuoted,
    /// A literal block scalar (`|`).
    Literal,
    /// A folded block scalar (`>`).
    Folded,
}

/// YAML syntax without implicit schema conversions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YamlValue {
    /// Decoded scalar text with its original style.
    Scalar {
        /// The decoded text; numeric spelling is never rounded.
        text: Box<str>,
        /// The source style.
        style: YamlScalarStyle,
    },
    /// Sequence elements in source order.
    Sequence {
        /// Node identities.
        elements: Vec<YamlId>,
        /// Whether the source used flow style.
        flow: bool,
    },
    /// Mapping entries in source order, including duplicate and complex keys.
    Mapping {
        /// Key and value identities.
        entries: Vec<(YamlId, YamlId)>,
        /// Whether the source used flow style.
        flow: bool,
    },
    /// An alias occurrence referring to a preceding anchor, potentially a
    /// containing collection. It is never copied or expanded.
    Alias(YamlId),
}

/// One syntax node. A span includes any anchor or tag properties.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlNode {
    /// The source range, at UTF-8 boundaries.
    pub span: Span,
    /// The decoded tag URI, or the non-specific `!` tag, when explicit.
    pub tag: Option<Box<str>>,
    /// The anchor name when present.
    pub anchor: Option<Box<str>>,
    /// The node's contents.
    pub value: YamlValue,
}

/// One document of a YAML stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YamlDocument {
    /// The document root, including an empty scalar for an empty document.
    pub root: YamlId,
    /// An explicit `---` marker was present.
    pub explicit_start: bool,
    /// An explicit `...` marker was present.
    pub explicit_end: bool,
}

/// A complete YAML stream, represented without recursive ownership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlStream {
    nodes: Vec<YamlNode>,
    documents: Vec<YamlDocument>,
}

impl YamlStream {
    /// Returns documents in source order; an empty stream has none.
    pub fn documents(&self) -> &[YamlDocument] {
        &self.documents
    }
    /// Returns all syntax nodes in arena order.
    pub fn nodes(&self) -> &[YamlNode] {
        &self.nodes
    }
    /// Returns every identity in arena order, with constant-time indexing.
    pub fn identities(&self) -> impl ExactSizeIterator<Item = YamlId> + DoubleEndedIterator {
        (0..self.nodes.len()).map(YamlId)
    }
    /// Looks up an identity. Use identities only in their originating stream.
    pub fn node(&self, id: YamlId) -> Option<&YamlNode> {
        self.nodes.get(id.0)
    }
    /// Returns decoded scalar text without following aliases or interpreting tags.
    pub fn scalar(&self, id: YamlId) -> Option<&str> {
        match &self.node(id)?.value {
            YamlValue::Scalar { text, .. } => Some(text),
            YamlValue::Sequence { .. } | YamlValue::Mapping { .. } | YamlValue::Alias(_) => None,
        }
    }
}

/// Why a YAML stream was rejected. No variant includes source content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YamlErrorKind {
    /// The input does not follow the syntax grammar.
    Syntax,
    /// A forbidden raw character or invalid Unicode escape.
    Character,
    /// A tag, anchor, escape, or directive is malformed.
    Property,
    /// An alias refers to no preceding anchor of this document.
    UnknownAlias,
    /// A collection exceeds the effective nesting bound.
    DepthLimit,
    /// A node would exceed the node bound.
    NodeLimit,
    /// Storage could not be reserved.
    AllocationFailed,
}

/// A rejected stream with a source range, never source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YamlError {
    /// The failure category.
    pub kind: YamlErrorKind,
    /// The offending source range, or an empty range at end of input.
    pub span: Span,
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.kind {
            YamlErrorKind::Syntax => "invalid YAML syntax",
            YamlErrorKind::Character => "invalid YAML character or Unicode escape",
            YamlErrorKind::Property => "invalid YAML property or directive",
            YamlErrorKind::UnknownAlias => "YAML alias has no preceding anchor",
            YamlErrorKind::DepthLimit => {
                "YAML collection nesting exceeds yaml.max-depth (implementation ceiling 128)"
            }
            YamlErrorKind::NodeLimit => "YAML nodes exceed yaml.max-nodes",
            YamlErrorKind::AllocationFailed => "YAML storage allocation failed",
        };
        write!(
            f,
            "{reason} at bytes {}..{}",
            self.span.start(),
            self.span.end()
        )
    }
}

impl std::error::Error for YamlError {}

/// Parses a complete UTF-8 YAML 1.2 stream. The text limit is enforced by
/// `SourceText`; aliases stay as references and are never expanded. Parsing
/// performs no I/O and never constructs objects from tags.
pub fn parse_yaml(source: &SourceText, limits: YamlLimits) -> Result<YamlStream, YamlError> {
    parser::parse(source.as_str(), limits)
}
