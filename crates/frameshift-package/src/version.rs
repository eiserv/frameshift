//! Strict SemVer 2.0 versions with npm's numeric and length bounds (`D-070`).

use std::cmp::Ordering;
use std::fmt;

/// The largest major, minor, or patch number npm accepts, `2^53 - 1`.
pub const MAX_VERSION_NUMBER: u64 = (1 << 53) - 1;

/// The longest version text npm accepts, in bytes.
pub const MAX_VERSION_BYTES: usize = 256;

/// One dot-separated prerelease identifier.
///
/// Numeric identifiers compare numerically and precede alphanumeric ones,
/// which compare by ASCII bytes. Numeric identifiers keep their exact digits,
/// so they compare exactly at any length.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Identifier {
    /// Digits without a leading zero, or `0`.
    Numeric(Box<str>),
    /// ASCII letters, digits, and hyphens with at least one non-digit.
    Alphanumeric(Box<str>),
}

impl Identifier {
    /// Returns the exact identifier text.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Numeric(text) | Self::Alphanumeric(text) => text,
        }
    }
}

impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left
                .len()
                .cmp(&right.len())
                .then_with(|| left.as_bytes().cmp(right.as_bytes())),
            (Self::Numeric(_), Self::Alphanumeric(_)) => Ordering::Less,
            (Self::Alphanumeric(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Alphanumeric(left), Self::Alphanumeric(right)) => {
                left.as_bytes().cmp(right.as_bytes())
            }
        }
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Why a version text was rejected. Messages never echo the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionError {
    /// The text is not `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]`.
    Syntax,
    /// A major, minor, or patch number exceeds [`MAX_VERSION_NUMBER`].
    NumberTooLarge,
    /// The text is longer than [`MAX_VERSION_BYTES`].
    TooLong,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "version is not valid SemVer 2.0",
            Self::NumberTooLarge => "version number exceeds 2^53 - 1",
            Self::TooLong => "version is longer than 256 bytes",
        })
    }
}

impl std::error::Error for VersionError {}

/// A SemVer 2.0 version.
///
/// Equality and [`Ord`] include build metadata, so they form a total order
/// consistent with equality; [`Version::cmp_precedence`] is SemVer
/// precedence, which ignores build metadata, and is what ranges use.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Vec<Identifier>,
    build: Vec<Box<str>>,
}

impl Version {
    /// Builds a version without prerelease or build identifiers.
    ///
    /// Returns `None` when a number exceeds [`MAX_VERSION_NUMBER`].
    pub fn new(major: u64, minor: u64, patch: u64) -> Option<Self> {
        (major <= MAX_VERSION_NUMBER && minor <= MAX_VERSION_NUMBER && patch <= MAX_VERSION_NUMBER)
            .then(|| Self {
                major,
                minor,
                patch,
                prerelease: Vec::new(),
                build: Vec::new(),
            })
    }

    /// Parses exact SemVer 2.0 text, as lockfiles and `packageManager`
    /// record versions.
    ///
    /// Unlike node-semver, no leading `v` or `=` and no surrounding
    /// whitespace are accepted. Numbers above [`MAX_VERSION_NUMBER`] and
    /// text longer than [`MAX_VERSION_BYTES`] are rejected, as npm rejects
    /// them.
    pub fn parse(text: &str) -> Result<Self, VersionError> {
        if text.len() > MAX_VERSION_BYTES {
            return Err(VersionError::TooLong);
        }
        let parsed = parse_full(text).ok_or(VersionError::Syntax)?;
        if parsed.end != text.len() {
            return Err(VersionError::Syntax);
        }
        parsed.version()
    }

    /// Returns the major number.
    pub const fn major(&self) -> u64 {
        self.major
    }

    /// Returns the minor number.
    pub const fn minor(&self) -> u64 {
        self.minor
    }

    /// Returns the patch number.
    pub const fn patch(&self) -> u64 {
        self.patch
    }

    /// Returns the prerelease identifiers, empty for a release.
    pub fn prerelease(&self) -> &[Identifier] {
        &self.prerelease
    }

    /// Returns the build metadata identifiers.
    pub fn build(&self) -> &[Box<str>] {
        &self.build
    }

    /// Returns whether the version has prerelease identifiers.
    pub fn is_prerelease(&self) -> bool {
        !self.prerelease.is_empty()
    }

    /// Compares by SemVer precedence, ignoring build metadata.
    pub fn cmp_precedence(&self, other: &Self) -> Ordering {
        self.tuple().cmp(&other.tuple()).then_with(|| {
            match (self.is_prerelease(), other.is_prerelease()) {
                (false, false) => Ordering::Equal,
                (false, true) => Ordering::Greater,
                (true, false) => Ordering::Less,
                (true, true) => self.prerelease.cmp(&other.prerelease),
            }
        })
    }

    pub(crate) const fn tuple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }

    pub(crate) fn with_prerelease(mut self, prerelease: Vec<Identifier>) -> Self {
        self.prerelease = prerelease;
        self
    }

    pub(crate) fn without_build(mut self) -> Self {
        self.build = Vec::new();
        self
    }

    /// Formats the version without build metadata, as range comparators show it.
    pub(crate) fn fmt_precedence(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (index, identifier) in self.prerelease.iter().enumerate() {
            f.write_str(if index == 0 { "-" } else { "." })?;
            f.write_str(identifier.as_str())?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_precedence(other)
            .then_with(|| self.build.cmp(&other.build))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_precedence(f)?;
        for (index, identifier) in self.build.iter().enumerate() {
            f.write_str(if index == 0 { "+" } else { "." })?;
            f.write_str(identifier)?;
        }
        Ok(())
    }
}

/// A version prefix of some text, before numeric bounds are checked.
pub(crate) struct FullMatch<'a> {
    pub(crate) major: &'a str,
    pub(crate) minor: &'a str,
    pub(crate) patch: &'a str,
    pub(crate) prerelease: Option<&'a str>,
    pub(crate) build: Option<&'a str>,
    /// The byte length of the matched prefix.
    pub(crate) end: usize,
}

impl FullMatch<'_> {
    pub(crate) fn version(&self) -> Result<Version, VersionError> {
        let number = |digits: &str| number(digits).ok_or(VersionError::NumberTooLarge);
        Ok(Version {
            major: number(self.major)?,
            minor: number(self.minor)?,
            patch: number(self.patch)?,
            prerelease: self.prerelease.map(prerelease).unwrap_or_default(),
            build: self
                .build
                .map(|build| build.split('.').map(Box::from).collect())
                .unwrap_or_default(),
        })
    }
}

/// Matches `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]` at the start of `text`,
/// as long as possible.
pub(crate) fn parse_full(text: &str) -> Option<FullMatch<'_>> {
    let bytes = text.as_bytes();
    let (major, mut at) = numeric(bytes, 0)?;
    let mut parts = [major, (0, 0), (0, 0)];
    for part in &mut parts[1..] {
        if bytes.get(at) != Some(&b'.') {
            return None;
        }
        let (range, next) = numeric(bytes, at + 1)?;
        *part = range;
        at = next;
    }
    let mut prerelease = None;
    if bytes.get(at) == Some(&b'-')
        && let Some(end) = identifiers(bytes, at + 1, true)
    {
        prerelease = Some(&text[at + 1..end]);
        at = end;
    }
    let mut build = None;
    if bytes.get(at) == Some(&b'+')
        && let Some(end) = identifiers(bytes, at + 1, false)
    {
        build = Some(&text[at + 1..end]);
        at = end;
    }
    let slice = |(start, end): (usize, usize)| &text[start..end];
    Some(FullMatch {
        major: slice(parts[0]),
        minor: slice(parts[1]),
        patch: slice(parts[2]),
        prerelease,
        build,
        end: at,
    })
}

/// Matches a numeric identifier, `0` or digits without a leading zero.
pub(crate) fn numeric(bytes: &[u8], start: usize) -> Option<((usize, usize), usize)> {
    match bytes.get(start) {
        Some(b'0') => Some(((start, start + 1), start + 1)),
        Some(b'1'..=b'9') => {
            let end = start
                + bytes[start..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
            Some(((start, end), end))
        }
        _ => None,
    }
}

/// Matches dot-separated identifiers of `[0-9A-Za-z-]`. Prerelease
/// identifiers that are all digits must not have a leading zero; a run that
/// breaks this rule or ends in a dot matches only up to its last valid
/// identifier, and nothing when the first identifier is invalid.
pub(crate) fn identifiers(bytes: &[u8], start: usize, prerelease: bool) -> Option<usize> {
    let mut at = start;
    let mut end = None;
    loop {
        let length = bytes[at..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'-')
            .count();
        let identifier = &bytes[at..at + length];
        let leading_zero = prerelease
            && identifier.len() > 1
            && identifier[0] == b'0'
            && identifier.iter().all(u8::is_ascii_digit);
        if identifier.is_empty() || leading_zero {
            return end;
        }
        at += length;
        end = Some(at);
        if bytes.get(at) != Some(&b'.') {
            return end;
        }
        at += 1;
    }
}

/// Converts digits to a number within [`MAX_VERSION_NUMBER`].
pub(crate) fn number(digits: &str) -> Option<u64> {
    if digits.len() > 16 {
        return None;
    }
    digits
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= MAX_VERSION_NUMBER)
}

pub(crate) fn prerelease(text: &str) -> Vec<Identifier> {
    text.split('.')
        .map(|part| {
            if part.bytes().all(|byte| byte.is_ascii_digit()) {
                Identifier::Numeric(part.into())
            } else {
                Identifier::Alphanumeric(part.into())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_exact_semver() {
        let parsed = version("1.20.300-rc.1.x-y+build.007");
        assert_eq!(parsed.tuple(), (1, 20, 300));
        assert_eq!(parsed.to_string(), "1.20.300-rc.1.x-y+build.007");
        assert_eq!(
            parsed.prerelease(),
            [
                Identifier::Alphanumeric("rc".into()),
                Identifier::Numeric("1".into()),
                Identifier::Alphanumeric("x-y".into()),
            ]
        );
        for invalid in [
            "",
            "1",
            "1.2",
            "v1.2.3",
            "=1.2.3",
            " 1.2.3",
            "1.2.3 ",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "1.2.3-",
            "1.2.3+",
            "1.2.3-01",
            "1.2.3-a..b",
            "1.2.3-a.",
            "1.2.3+a..b",
            "1.2.3-é",
            "1.2.3.4",
            "1.2.3-a+b+c",
            "1.x.3",
        ] {
            assert_eq!(
                Version::parse(invalid),
                Err(VersionError::Syntax),
                "{invalid:?}"
            );
        }
        assert!(Version::parse("1.2.3-0a.00a.-").is_ok());
        assert!(Version::parse("1.2.3+00.01").is_ok());
    }

    #[test]
    fn bounds_numbers_and_length() {
        assert_eq!(version("9007199254740991.0.0").major(), MAX_VERSION_NUMBER);
        assert_eq!(
            Version::parse("9007199254740992.0.0"),
            Err(VersionError::NumberTooLarge)
        );
        assert_eq!(
            Version::parse("0.99999999999999999999.0"),
            Err(VersionError::NumberTooLarge)
        );
        let long = format!("1.2.3-{}", "a".repeat(MAX_VERSION_BYTES - 6));
        assert!(Version::parse(&long).is_ok());
        assert_eq!(
            Version::parse(&format!("{long}a")),
            Err(VersionError::TooLong)
        );
        assert!(Version::new(MAX_VERSION_NUMBER, 0, 0).is_some());
        assert!(Version::new(0, MAX_VERSION_NUMBER + 1, 0).is_none());
    }

    #[test]
    fn orders_by_precedence() {
        let ordered = [
            "1.0.0-0",
            "1.0.0-0.0",
            "1.0.0-1",
            "1.0.0-2",
            "1.0.0-10",
            "1.0.0-99999999999999999999",
            "1.0.0-a",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(
                version(pair[0]).cmp_precedence(&version(pair[1])),
                Ordering::Less,
                "{pair:?}"
            );
        }
        assert_eq!(
            version("1.0.0+a").cmp_precedence(&version("1.0.0+b")),
            Ordering::Equal
        );
        assert_eq!(version("1.0.0+a").cmp(&version("1.0.0+b")), Ordering::Less);
        assert_ne!(version("1.0.0+a"), version("1.0.0"));
    }
}
