//! Stable catalog identifiers with strict spelling and bounded storage.

use std::fmt;

/// Explicit, inclusive storage limit for an identifier's canonical UTF-8 text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentifierLimits {
    /// Maximum identifier size in bytes. Defaults to 128 KiB.
    pub max_bytes: usize,
}

impl Default for IdentifierLimits {
    fn default() -> Self {
        Self {
            max_bytes: 128 * 1024,
        }
    }
}

/// A rejected identifier, without any copy of source-derived text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierError {
    /// Canonical text exceeds the explicit storage bound.
    TooLong {
        /// Required storage in bytes.
        bytes: usize,
        /// Maximum permitted storage in bytes.
        limit: usize,
    },
    /// A finding code is not `FS` followed by exactly four ASCII digits.
    InvalidFindingCode,
    /// A rule ID is not a migration path followed by `/area.name`.
    InvalidRuleId,
    /// A feature ID has fewer than three dot-separated name segments.
    InvalidFeatureId,
    /// Storage could not be reserved for the value.
    AllocationFailed,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { bytes, limit } => {
                write!(f, "identifier size {bytes} exceeds byte limit {limit}")
            }
            Self::InvalidFindingCode => f.write_str("invalid finding code"),
            Self::InvalidRuleId => f.write_str("invalid rule identifier"),
            Self::InvalidFeatureId => f.write_str("invalid feature identifier"),
            Self::AllocationFailed => f.write_str("identifier storage allocation failed"),
        }
    }
}

impl std::error::Error for IdentifierError {}

/// A syntactically valid finding code, preserving all four digits.
///
/// Catalog membership and documented meaning are separate checks. Reserved or
/// unassigned codes remain representable without advertising a product finding.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FindingCode(String);

impl FindingCode {
    /// Requires exactly `FS` and four ASCII digits, then copies fallibly.
    pub fn new(value: &str) -> Result<Self, IdentifierError> {
        if value.len() != 6
            || !value.starts_with("FS")
            || !value.as_bytes()[2..].iter().all(u8::is_ascii_digit)
        {
            return Err(IdentifierError::InvalidFindingCode);
        }
        copy(value).map(Self)
    }

    /// Borrows the exact code spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A rule's stable `<migration-path>/area.name` identity.
///
/// Name segments start with a lowercase ASCII letter, followed by lowercase
/// ASCII letters or digits, with single internal hyphens. The migration path
/// has exactly one `-to-` separator between two such names. No normalization,
/// version, registry lookup, or framework-specific validation is performed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId(String);

impl RuleId {
    /// Checks the byte limit before syntax and allocates only after validation.
    pub fn new(value: &str, limits: IdentifierLimits) -> Result<Self, IdentifierError> {
        check_size(value.len(), limits)?;
        let Some((path, rule)) = value.split_once('/') else {
            return Err(IdentifierError::InvalidRuleId);
        };
        let Some((source, target)) = path.split_once("-to-") else {
            return Err(IdentifierError::InvalidRuleId);
        };
        if !name(source) || !name(target) || target.contains("-to-") || !dotted(rule, 2, 2) {
            return Err(IdentifierError::InvalidRuleId);
        }
        copy(value).map(Self)
    }

    /// Borrows the exact validated identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Borrows the migration path before the slash.
    pub fn migration_path(&self) -> &str {
        self.0.split_once('/').expect("validated rule separator").0
    }

    /// Borrows the rule's `area.name` portion.
    pub fn name(&self) -> &str {
        self.0.split_once('/').expect("validated rule separator").1
    }
}

/// A feature's stable, namespaced, dot-separated identity.
///
/// Requires at least three lowercase ASCII name segments, using [`RuleId`]'s
/// segment grammar. Further segments support nested feature catalogs. Identity
/// is independent of framework registration and catalog versions.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeatureId(String);

impl FeatureId {
    /// Checks the byte limit before syntax and allocates only after validation.
    pub fn new(value: &str, limits: IdentifierLimits) -> Result<Self, IdentifierError> {
        check_size(value.len(), limits)?;
        if !dotted(value, 3, usize::MAX) {
            return Err(IdentifierError::InvalidFeatureId);
        }
        copy(value).map(Self)
    }

    /// Borrows the exact validated identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) fn name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_lowercase) {
        return false;
    }
    let mut hyphen = false;
    for &byte in &bytes[1..] {
        if byte == b'-' {
            if hyphen {
                return false;
            }
            hyphen = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            hyphen = false;
        } else {
            return false;
        }
    }
    !hyphen
}

pub(super) fn dotted(value: &str, min: usize, max: usize) -> bool {
    let mut count = 0;
    for segment in value.split('.') {
        if !name(segment) || count >= max {
            return false;
        }
        count += 1;
    }
    count >= min
}

pub(super) fn check_size(bytes: usize, limits: IdentifierLimits) -> Result<(), IdentifierError> {
    if bytes > limits.max_bytes {
        return Err(IdentifierError::TooLong {
            bytes,
            limit: limits.max_bytes,
        });
    }
    Ok(())
}

fn copy(value: &str) -> Result<String, IdentifierError> {
    let mut result = String::new();
    result
        .try_reserve_exact(value.len())
        .map_err(|_| IdentifierError::AllocationFailed)?;
    result.push_str(value);
    Ok(result)
}
