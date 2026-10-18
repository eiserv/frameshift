//! Stable unit identity, without model entities or framework routing semantics.

use crate::identifier::{check_size, dotted};
use crate::{IdentifierError, IdentifierLimits, SourcePath};

/// Framework-neutral unit kinds from the migration reporting contract.
///
/// A kind is a counting category, not a migration or verification status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnitKind {
    /// One URL pattern and its page module.
    Route,
    /// One layout module.
    Layout,
    /// A component whose main role is providing shared state.
    Provider,
    /// One route handler's URL pattern.
    Endpoint,
    /// One component definition outside route and layout modules.
    Component,
    /// A special route file other than a page or layout.
    RouteFile,
    /// Non-component code, such as utilities or data access.
    Module,
    /// One CSS, SCSS, or CSS Module file.
    Stylesheet,
    /// One configuration concern.
    Config,
}

impl UnitKind {
    /// Returns the exact spelling used in unit IDs and future reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Route => "route",
            Self::Layout => "layout",
            Self::Provider => "provider",
            Self::Endpoint => "endpoint",
            Self::Component => "component",
            Self::RouteFile => "route-file",
            Self::Module => "module",
            Self::Stylesheet => "stylesheet",
            Self::Config => "config",
        }
    }
}

/// A deterministic `<kind>:<subject>[#<export>]` unit identity.
///
/// Subject and export bytes use uppercase percent encoding. ASCII letters,
/// digits, `-._~/[]()` stay literal; every other UTF-8 byte is encoded, including
/// `%`, `#`, `:`, spaces, controls, and non-ASCII text. Input is always original
/// text, never a pre-encoded ID. Equality, hashing, and ordering compare the
/// canonical ID bytes, without folding case or Unicode normalization.
///
/// Constructors validate identity structure only. They do not discover units,
/// interpret framework routes, validate JavaScript exports, or resolve paths.
/// Percent encoding is not general Markdown or HTML escaping.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(String);

impl UnitId {
    /// Constructs a file unit from an already validated source path.
    ///
    /// Accepts layout, provider, component, route-file, module, or stylesheet
    /// kinds. An optional export is original UTF-8, nonempty and without NUL;
    /// its language syntax and existence belong to semantic analysis.
    /// The encoded size, including kind and delimiters, is limited before
    /// allocation. No source path is normalized, rewritten, or percent-decoded.
    pub fn for_file(
        kind: UnitKind,
        path: &SourcePath,
        export: Option<&str>,
        limits: IdentifierLimits,
    ) -> Result<Self, IdentifierError> {
        match kind {
            UnitKind::Layout
            | UnitKind::Provider
            | UnitKind::Component
            | UnitKind::RouteFile
            | UnitKind::Module
            | UnitKind::Stylesheet => {}
            UnitKind::Route | UnitKind::Endpoint | UnitKind::Config => {
                return Err(IdentifierError::InvalidUnitSubject);
            }
        }
        if let Some(export) = export
            && (export.is_empty() || export.contains('\0'))
        {
            return Err(IdentifierError::InvalidExport);
        }
        build(kind, path.as_str(), export, limits)
    }

    /// Constructs a route or endpoint identity from an original URL pattern.
    ///
    /// Requires a leading slash, no backslash or NUL, and no empty interior,
    /// `.` or `..` components. `/` and a single trailing slash are preserved.
    /// Other bytes, including literal `%` and `#`, are not URL-decoded. Parameter
    /// grammar, URL encoding, and framework route semantics remain model work.
    /// Encoded size is checked before scanning pattern structure or allocating.
    pub fn for_route(
        kind: UnitKind,
        pattern: &str,
        limits: IdentifierLimits,
    ) -> Result<Self, IdentifierError> {
        match kind {
            UnitKind::Route | UnitKind::Endpoint => {}
            UnitKind::Layout
            | UnitKind::Provider
            | UnitKind::Component
            | UnitKind::RouteFile
            | UnitKind::Module
            | UnitKind::Stylesheet
            | UnitKind::Config => {
                return Err(IdentifierError::InvalidUnitSubject);
            }
        }
        encoded_size(kind, pattern, None, limits)?;
        if !pattern.starts_with('/') || pattern.contains(['\\', '\0']) {
            return Err(IdentifierError::InvalidRoutePattern);
        }
        if pattern != "/" {
            let inner = pattern.strip_suffix('/').unwrap_or(pattern);
            if inner[1..]
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            {
                return Err(IdentifierError::InvalidRoutePattern);
            }
        }
        build(kind, pattern, None, limits)
    }

    /// Constructs a config identity from at least two catalog name segments.
    ///
    /// Uses the same lowercase ASCII segment grammar as [`crate::FeatureId`],
    /// permitting concerns such as `nextjs.images` without inventing a feature.
    /// Checks the complete ID size before syntax or allocation.
    pub fn for_config(concern: &str, limits: IdentifierLimits) -> Result<Self, IdentifierError> {
        encoded_size(UnitKind::Config, concern, None, limits)?;
        if !dotted(concern, 2, usize::MAX) {
            return Err(IdentifierError::InvalidConcern);
        }
        build(UnitKind::Config, concern, None, limits)
    }

    /// Borrows the canonical identity, independently of source path spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns an upper bound on the size of a file unit identity without an
    /// export, for any source path of at most `path_bytes` UTF-8 bytes.
    ///
    /// The bound assumes the longest file kind and that every path byte is
    /// percent-encoded, which a non-ASCII path attains. It saturates at
    /// `usize::MAX`. Callers reconcile [`IdentifierLimits`] with path limits
    /// through it before constructing identities.
    pub fn max_file_id_bytes(path_bytes: usize) -> usize {
        let kind = FILE_KINDS
            .iter()
            .fold(0, |longest, kind| longest.max(kind.as_str().len()));
        (kind + 1).saturating_add(path_bytes.saturating_mul(3))
    }
}

// The kinds that `UnitId::for_file` accepts.
const FILE_KINDS: [UnitKind; 6] = [
    UnitKind::Layout,
    UnitKind::Provider,
    UnitKind::Component,
    UnitKind::RouteFile,
    UnitKind::Module,
    UnitKind::Stylesheet,
];

fn literal(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~/[]()".contains(&byte)
}

fn encoded_size(
    kind: UnitKind,
    subject: &str,
    export: Option<&str>,
    limits: IdentifierLimits,
) -> Result<usize, IdentifierError> {
    let mut bytes = kind.as_str().len() + 1;
    // Incremental accounting cannot overflow: each UTF-8 input length is at
    // most isize::MAX; saturation still exceeds any allocatable byte bound.
    for byte in subject
        .bytes()
        .chain(export.into_iter().flat_map(str::bytes))
    {
        bytes = bytes.saturating_add(if literal(byte) { 1 } else { 3 });
    }
    bytes = bytes.saturating_add(usize::from(export.is_some()));
    check_size(bytes, limits)?;
    Ok(bytes)
}

fn build(
    kind: UnitKind,
    subject: &str,
    export: Option<&str>,
    limits: IdentifierLimits,
) -> Result<UnitId, IdentifierError> {
    let bytes = encoded_size(kind, subject, export, limits)?;
    let mut text = String::new();
    text.try_reserve_exact(bytes)
        .map_err(|_| IdentifierError::AllocationFailed)?;
    text.push_str(kind.as_str());
    text.push(':');
    encode(subject, &mut text);
    if let Some(export) = export {
        text.push('#');
        encode(export, &mut text);
    }
    Ok(UnitId(text))
}

fn encode(value: &str, text: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value.bytes() {
        if literal(byte) {
            text.push(char::from(byte));
        } else {
            text.push('%');
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FILE_KINDS, UnitId, UnitKind};
    use crate::{IdentifierLimits, PathLimits, SourcePath};

    #[test]
    fn file_kinds_are_exactly_the_kinds_the_file_constructor_accepts() {
        let path = SourcePath::new("a", PathLimits::default()).unwrap();
        for kind in [
            UnitKind::Route,
            UnitKind::Layout,
            UnitKind::Provider,
            UnitKind::Endpoint,
            UnitKind::Component,
            UnitKind::RouteFile,
            UnitKind::Module,
            UnitKind::Stylesheet,
            UnitKind::Config,
        ] {
            let accepted = UnitId::for_file(kind, &path, None, IdentifierLimits::default()).is_ok();
            assert_eq!(accepted, FILE_KINDS.contains(&kind), "{kind:?}");
        }
    }
}
