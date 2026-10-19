//! npm version ranges with node-semver's semantics (`D-070`).
//!
//! Parsing follows the documented range grammar and desugaring of
//! node-semver 7.8.5, the version that npm 11.11.0 bundles, in its default
//! mode: not loose, and without `includePrerelease`. node-semver desugars a
//! range by rewriting its text in stages; this module performs the same
//! stages on owned strings, so whitespace, prefixes, and operators that npm
//! accepts or rejects are accepted or rejected alike. Every stage is linear
//! in the text. A differential test against the oracle is recorded in
//! IMPLEMENTATION.md section 26.

use std::collections::BTreeSet;
use std::fmt;

use crate::version::{
    Identifier, MAX_VERSION_BYTES, Version, VersionError, identifiers, numeric, parse_full,
};

/// A comparison operator of a desugared comparator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Operator {
    /// `<`
    Less,
    /// `<=`
    LessOrEqual,
    /// `>`
    Greater,
    /// `>=`
    GreaterOrEqual,
    /// `=`, written without an operator.
    Equal,
}

impl Operator {
    /// Returns the spelling in canonical range text.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
            Self::Equal => "",
        }
    }
}

/// An operator and a version without build metadata.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Comparator {
    operator: Operator,
    version: Version,
}

impl Comparator {
    /// Returns the operator.
    pub const fn operator(&self) -> Operator {
        self.operator
    }

    /// Returns the version, which has no build metadata.
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns whether a version satisfies this comparator alone, by
    /// precedence and without the prerelease rule of comparator sets.
    pub fn test(&self, version: &Version) -> bool {
        let ordering = version.cmp_precedence(&self.version);
        match self.operator {
            Operator::Less => ordering.is_lt(),
            Operator::LessOrEqual => ordering.is_le(),
            Operator::Greater => ordering.is_gt(),
            Operator::GreaterOrEqual => ordering.is_ge(),
            Operator::Equal => ordering.is_eq(),
        }
    }

    fn is_null_set(&self) -> bool {
        self.operator == Operator::Less
            && self.version.tuple() == (0, 0, 0)
            && self.version.prerelease() == [Identifier::Numeric("0".into())]
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.operator.as_str())?;
        self.version.fmt_precedence(f)
    }
}

/// Why a range text was rejected. Messages never echo the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeError {
    /// A comparator does not follow the range grammar.
    Syntax,
    /// A desugared version number exceeds `2^53 - 1`.
    NumberTooLarge,
    /// A comparator's version text is longer than 256 bytes.
    TooLong,
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "range contains an invalid comparator",
            Self::NumberTooLarge => "range version number exceeds 2^53 - 1",
            Self::TooLong => "range version is longer than 256 bytes",
        })
    }
}

impl std::error::Error for RangeError {}

impl From<VersionError> for RangeError {
    fn from(error: VersionError) -> Self {
        match error {
            VersionError::Syntax => Self::Syntax,
            VersionError::NumberTooLarge => Self::NumberTooLarge,
            VersionError::TooLong => Self::TooLong,
        }
    }
}

/// A parsed npm range: comparator sets joined by `||`.
///
/// A version satisfies the range when it satisfies every comparator of at
/// least one set and, if it is a prerelease, some comparator of that set has
/// a prerelease with the same major, minor, and patch numbers. An empty set
/// is `*`, every release version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionRange {
    sets: Vec<Vec<Comparator>>,
}

impl VersionRange {
    /// Parses range text as npm does.
    pub fn parse(text: &str) -> Result<Self, RangeError> {
        let normalized = normalize_whitespace(text);
        let mut sets = Vec::new();
        for alternative in normalized.split("||") {
            sets.push(parse_set(alternative.trim_matches(' '))?);
        }
        if sets.len() > 1 {
            let first = sets[0].clone();
            sets.retain(|set| !set.first().is_some_and(Comparator::is_null_set));
            if sets.is_empty() {
                sets.push(first);
            } else if let Some(any) = sets.iter().position(Vec::is_empty) {
                sets = vec![std::mem::take(&mut sets[any])];
            }
        }
        Ok(Self { sets })
    }

    /// Returns the comparator sets; an empty set means `*`.
    pub fn sets(&self) -> &[Vec<Comparator>] {
        &self.sets
    }

    /// Returns whether a version satisfies the range.
    pub fn satisfies(&self, version: &Version) -> bool {
        self.sets.iter().any(|set| {
            set.iter().all(|comparator| comparator.test(version))
                && (!version.is_prerelease()
                    || set.iter().any(|comparator| {
                        comparator.version.is_prerelease()
                            && comparator.version.tuple() == version.tuple()
                    }))
        })
    }

    /// Returns the lowest version that satisfies the range, or `None` when
    /// no version does.
    ///
    /// This is the exact minimum under the prerelease rule, among versions
    /// whose numbers are at most `2^53 - 1`. It differs from node-semver's
    /// `minVersion`, which can return a version above the minimum (for
    /// example `0.0.0` for `>0.0.0-0`) or none although a later set is
    /// satisfiable.
    pub fn min_version(&self) -> Option<Version> {
        self.sets
            .iter()
            .filter_map(|set| set_minimum(set))
            .min_by(Version::cmp_precedence)
    }
}

impl fmt::Display for VersionRange {
    /// Formats the canonical range as node-semver's `validRange` does.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sets.iter().all(Vec::is_empty) {
            return f.write_str("*");
        }
        for (index, set) in self.sets.iter().enumerate() {
            if index > 0 {
                f.write_str("||")?;
            }
            for (position, comparator) in set.iter().enumerate() {
                if position > 0 {
                    f.write_str(" ")?;
                }
                write!(f, "{comparator}")?;
            }
        }
        Ok(())
    }
}

/// The lowest version satisfying one comparator set.
fn set_minimum(set: &[Comparator]) -> Option<Version> {
    let mut lower: Option<(&Version, bool)> = None;
    let mut upper: Option<(&Version, bool)> = None;
    for comparator in set {
        let version = &comparator.version;
        let (is_lower, is_upper, inclusive) = match comparator.operator {
            Operator::Less => (false, true, false),
            Operator::LessOrEqual => (false, true, true),
            Operator::Greater => (true, false, false),
            Operator::GreaterOrEqual => (true, false, true),
            Operator::Equal => (true, true, true),
        };
        if is_lower {
            lower = Some(match lower {
                Some(current) => tighter(current, (version, inclusive), true),
                None => (version, inclusive),
            });
        }
        if is_upper {
            upper = Some(match upper {
                Some(current) => tighter(current, (version, inclusive), false),
                None => (version, inclusive),
            });
        }
    }
    let allows_prerelease = |tuple: (u64, u64, u64)| {
        set.iter().any(|comparator| {
            comparator.version.is_prerelease() && comparator.version.tuple() == tuple
        })
    };
    let zero = Identifier::Numeric("0".into());
    let candidate = match lower {
        None => {
            let base = Version::new(0, 0, 0)?;
            if allows_prerelease((0, 0, 0)) {
                base.with_prerelease(vec![zero])
            } else {
                base
            }
        }
        Some((version, true)) => version.clone(),
        Some((version, false)) if version.is_prerelease() => {
            let mut identifiers = version.prerelease().to_vec();
            identifiers.push(zero);
            version.clone().with_prerelease(identifiers)
        }
        Some((version, false)) => {
            let (major, minor, patch) = version.tuple();
            let next = Version::new(major, minor, patch + 1)
                .or_else(|| Version::new(major, minor + 1, 0))
                .or_else(|| Version::new(major + 1, 0, 0))?;
            if allows_prerelease(next.tuple()) {
                next.with_prerelease(vec![zero])
            } else {
                next
            }
        }
    };
    match upper {
        Some((bound, inclusive)) => {
            let ordering = candidate.cmp_precedence(bound);
            (ordering.is_lt() || (inclusive && ordering.is_eq())).then_some(candidate)
        }
        None => Some(candidate),
    }
}

/// Picks the stricter of two lower or two upper bounds; at equal versions
/// an exclusive bound is stricter.
fn tighter<'a>(
    current: (&'a Version, bool),
    other: (&'a Version, bool),
    lower: bool,
) -> (&'a Version, bool) {
    let ordering = other.0.cmp_precedence(current.0);
    let replace = if ordering.is_eq() {
        current.1 && !other.1
    } else {
        ordering.is_gt() == lower
    };
    if replace { other } else { current }
}

/// Parses one `||` alternative into its comparator set.
fn parse_set(alternative: &str) -> Result<Vec<Comparator>, RangeError> {
    let stripped = strip_build(alternative);
    let expanded = hyphen(&stripped).unwrap_or(stripped);
    let trimmed = caret_trim(&tilde_trim(&comparator_trim(&expanded)));
    let mut texts = Vec::new();
    for token in trimmed.split(' ') {
        let desugared = replace_stars(&desugar(token));
        texts.extend(desugared.split(' ').map(str::to_owned));
    }
    let mut comparators = Vec::new();
    for text in &texts {
        // Empty texts and `>=0.0.0` are `*`, which a set drops when it has
        // other comparators; a set of only `*` is the empty set.
        if !text.is_empty() && text != ">=0.0.0" {
            comparators.push(comparator(text)?);
        }
    }
    if let Some(null) = comparators
        .iter()
        .find(|comparator| comparator.is_null_set())
    {
        return Ok(vec![null.clone()]);
    }
    let mut seen = BTreeSet::new();
    comparators.retain(|comparator| seen.insert(comparator.clone()));
    Ok(comparators)
}

/// Parses a desugared comparator: an operator, an optional `v`, and an
/// exact version.
fn comparator(text: &str) -> Result<Comparator, RangeError> {
    let (operator_end, operator) = operator(text);
    let version = &text[operator_end..];
    let plain = version.strip_prefix('v').unwrap_or(version);
    let full = parse_full(plain).ok_or(RangeError::Syntax)?;
    if full.end != plain.len() {
        return Err(RangeError::Syntax);
    }
    if version.len() > MAX_VERSION_BYTES {
        return Err(RangeError::TooLong);
    }
    let version = full.version()?.without_build();
    Ok(Comparator { operator, version })
}

/// Reads `[<>]?=?` greedily.
fn operator(text: &str) -> (usize, Operator) {
    let bytes = text.as_bytes();
    let mut at = usize::from(matches!(bytes.first(), Some(b'<' | b'>')));
    if bytes.get(at) == Some(&b'=') {
        at += 1;
    }
    let operator = match &text[..at] {
        "<" => Operator::Less,
        "<=" => Operator::LessOrEqual,
        ">" => Operator::Greater,
        ">=" => Operator::GreaterOrEqual,
        _ => Operator::Equal,
    };
    (at, operator)
}

/// The longest numeric identifier node-semver's bounded patterns match.
const MAX_NUMBER_DIGITS: usize = 257;
/// The most digits before the first non-digit of an alphanumeric
/// prerelease identifier.
const MAX_LEADING_DIGITS: usize = 256;
/// The most characters after the first non-digit of an alphanumeric
/// prerelease identifier.
const MAX_IDENTIFIER_TAIL: usize = 250;

/// The parts of an X-range: `[v= ]*`, then up to three components, and an
/// optional prerelease after a third component.
struct Partial<'a> {
    /// The whole matched text, prefix included.
    raw: &'a str,
    major: Component<'a>,
    minor: Component<'a>,
    patch: Component<'a>,
    prerelease: Option<&'a str>,
}

#[derive(Clone, Copy)]
enum Component<'a> {
    /// `x`, `X`, `*`, or absent.
    Any,
    Number(&'a str),
}

impl<'a> Component<'a> {
    fn is_any(self) -> bool {
        matches!(self, Self::Any)
    }

    fn digits(self) -> &'a str {
        match self {
            Self::Number(digits) => digits,
            Self::Any => "0",
        }
    }

    /// Adds one; a number beyond `2^53 - 1` yields digits that fail the
    /// version bound later, as node-semver's floating-point sum does.
    fn plus_one(self) -> String {
        match crate::version::number(self.digits()) {
            Some(value) => (value + 1).to_string(),
            None => "9007199254740992".to_owned(),
        }
    }
}

/// Matches the whole text as an X-range.
fn partial(text: &str) -> Option<Partial<'_>> {
    let bytes = text.as_bytes();
    let mut at = bytes
        .iter()
        .take_while(|byte| matches!(byte, b'v' | b'=' | b' '))
        .count();
    let mut components = [Component::Any; 3];
    let mut count = 0;
    while count < 3 {
        if count > 0 {
            if bytes.get(at) != Some(&b'.') {
                break;
            }
            at += 1;
        }
        let (component, next) = component(text, at)?;
        components[count] = component;
        at = next;
        count += 1;
    }
    let mut prerelease = None;
    if count == 3 && bytes.get(at) == Some(&b'-') {
        let end = bounded_prerelease(bytes, at + 1)?;
        prerelease = Some(&text[at + 1..end]);
        at = end;
    }
    (at == text.len()).then_some(Partial {
        raw: text,
        major: components[0],
        minor: components[1],
        patch: components[2],
        prerelease,
    })
}

fn component(text: &str, at: usize) -> Option<(Component<'_>, usize)> {
    match text.as_bytes().get(at) {
        Some(b'x' | b'X' | b'*') => Some((Component::Any, at + 1)),
        _ => {
            let ((start, end), next) = numeric(text.as_bytes(), at)?;
            (end - start <= MAX_NUMBER_DIGITS).then(|| (Component::Number(&text[start..end]), next))
        }
    }
}

/// Matches dot-separated prerelease identifiers up to the end of the text
/// with node-semver's bounded identifier patterns, returning their end.
fn bounded_prerelease(bytes: &[u8], start: usize) -> Option<usize> {
    let end = identifiers(bytes, start, true)?;
    if end != bytes.len() {
        return None;
    }
    let valid = bytes[start..end]
        .split(|byte| *byte == b'.')
        .all(|identifier| {
            let digits = identifier
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits == identifier.len() {
                digits <= MAX_NUMBER_DIGITS
            } else {
                digits <= MAX_LEADING_DIGITS && identifier.len() - digits - 1 <= MAX_IDENTIFIER_TAIL
            }
        });
    valid.then_some(end)
}

/// Expands `A - B` when the whole alternative has that form: at most one
/// space before `A` and after `B`, and exactly one space around `-`.
fn hyphen(alternative: &str) -> Option<String> {
    let text = alternative.strip_prefix(' ').unwrap_or(alternative);
    let split = text.find(" - ")?;
    let from = partial(&text[..split])?;
    let rest = &text[split + 3..];
    let to = partial(rest.strip_suffix(' ').unwrap_or(rest))?;
    let lower = if from.major.is_any() {
        String::new()
    } else if from.minor.is_any() {
        format!(">={}.0.0", from.major.digits())
    } else if from.patch.is_any() {
        format!(">={}.{}.0", from.major.digits(), from.minor.digits())
    } else {
        format!(">={}", from.raw)
    };
    let upper = if to.major.is_any() {
        String::new()
    } else if to.minor.is_any() {
        format!("<{}.0.0-0", to.major.plus_one())
    } else if to.patch.is_any() {
        format!("<{}.{}.0-0", to.major.digits(), to.minor.plus_one())
    } else if let Some(prerelease) = to.prerelease {
        format!(
            "<={}.{}.{}-{prerelease}",
            to.major.digits(),
            to.minor.digits(),
            to.patch.digits()
        )
    } else {
        format!("<={}", to.raw)
    };
    Some(format!("{lower} {upper}").trim_matches(' ').to_owned())
}

/// Removes up to one space between an operator and the version that
/// follows it, trying the alternatives in the order a backtracking
/// matcher would.
fn comparator_trim(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < bytes.len() {
        if let Some((operator_end, version_start, identifier_end)) = trim_match(bytes, at) {
            out.push_str(&text[at..operator_end]);
            out.push_str(&text[version_start..identifier_end]);
            at = identifier_end;
        } else {
            at = push_char(&mut out, text, at);
        }
    }
    out
}

/// Returns the end of the operator, the start of the version, and the end
/// of its first identifier character.
fn trim_match(bytes: &[u8], at: usize) -> Option<(usize, usize, usize)> {
    let spaces = |position: usize| -> &'static [usize] {
        if bytes.get(position) == Some(&b' ') {
            &[1, 0]
        } else {
            &[0]
        }
    };
    for &leading in spaces(at) {
        let start = at + leading;
        let mut operators = Vec::with_capacity(4);
        let angle = usize::from(matches!(bytes.get(start), Some(b'<' | b'>')));
        for angle in (0..=angle).rev() {
            let equals = usize::from(bytes.get(start + angle) == Some(&b'='));
            for equals in (0..=equals).rev() {
                operators.push(angle + equals);
            }
        }
        for operator in operators {
            let operator_end = start + operator;
            for &gap in spaces(operator_end) {
                let version_start = operator_end + gap;
                let identifier = version_start
                    + bytes[version_start..]
                        .iter()
                        .take_while(|byte| matches!(byte, b'v' | b'=' | b' '))
                        .count();
                if matches!(
                    bytes.get(identifier),
                    Some(b'0'..=b'9' | b'x' | b'X' | b'*')
                ) {
                    return Some((operator_end, version_start, identifier + 1));
                }
            }
        }
    }
    None
}

/// Joins `~` and `~>` with the following text over one space, dropping
/// the `>`.
fn tilde_trim(text: &str) -> String {
    lone_trim(text, b'~', true)
}

/// Joins `^` with the following text over one space.
fn caret_trim(text: &str) -> String {
    lone_trim(text, b'^', false)
}

fn lone_trim(text: &str, marker: u8, arrow: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == marker {
            let mut next = at + 1;
            if arrow && bytes.get(next) == Some(&b'>') && bytes.get(next + 1) == Some(&b' ') {
                next += 1;
            }
            if bytes.get(next) == Some(&b' ') {
                out.push(char::from(marker));
                at = next + 1;
                continue;
            }
        }
        at = push_char(&mut out, text, at);
    }
    out
}

fn push_char(out: &mut String, text: &str, at: usize) -> usize {
    let length = text[at..].chars().next().map_or(1, char::len_utf8);
    out.push_str(&text[at..at + length]);
    at + length
}

/// Desugars one token: a caret, tilde, or X-range, or returns it unchanged.
fn desugar(token: &str) -> String {
    if let Some(rest) = token.strip_prefix('^') {
        return partial(rest).map_or_else(|| token.to_owned(), |partial| caret(&partial));
    }
    if let Some(rest) = token.strip_prefix('~') {
        let rest = rest
            .strip_prefix('>')
            .filter(|rest| partial(rest).is_some())
            .unwrap_or(rest);
        return partial(rest).map_or_else(|| token.to_owned(), |partial| tilde(&partial));
    }
    let (operator_end, _) = operator(token);
    partial(&token[operator_end..])
        .and_then(|partial| x_range(&token[..operator_end], &partial))
        .unwrap_or_else(|| token.to_owned())
}

fn caret(partial: &Partial<'_>) -> String {
    let (major, minor, patch) = (partial.major, partial.minor, partial.patch);
    let (m, n) = (major.digits(), minor.digits());
    if major.is_any() {
        String::new()
    } else if minor.is_any() {
        format!(">={m}.0.0 <{}.0.0-0", major.plus_one())
    } else if patch.is_any() {
        if m == "0" {
            format!(">={m}.{n}.0 <{m}.{}.0-0", minor.plus_one())
        } else {
            format!(">={m}.{n}.0 <{}.0.0-0", major.plus_one())
        }
    } else {
        let p = patch.digits();
        let lower = match partial.prerelease {
            Some(prerelease) => format!(">={m}.{n}.{p}-{prerelease}"),
            None => format!(">={m}.{n}.{p}"),
        };
        if m != "0" {
            format!("{lower} <{}.0.0-0", major.plus_one())
        } else if n != "0" {
            format!("{lower} <{m}.{}.0-0", minor.plus_one())
        } else {
            format!("{lower} <{m}.{n}.{}-0", patch.plus_one())
        }
    }
}

fn tilde(partial: &Partial<'_>) -> String {
    let (major, minor, patch) = (partial.major, partial.minor, partial.patch);
    let (m, n) = (major.digits(), minor.digits());
    if major.is_any() {
        String::new()
    } else if minor.is_any() {
        format!(">={m}.0.0 <{}.0.0-0", major.plus_one())
    } else if patch.is_any() {
        format!(">={m}.{n}.0 <{m}.{}.0-0", minor.plus_one())
    } else {
        let p = patch.digits();
        let upper = minor.plus_one();
        match partial.prerelease {
            Some(prerelease) => format!(">={m}.{n}.{p}-{prerelease} <{m}.{upper}.0-0"),
            None => format!(">={m}.{n}.{p} <{m}.{upper}.0-0"),
        }
    }
}

/// Desugars an X-range with an operator. `None` leaves the token unchanged:
/// a version without `x`, or a number after an `x`.
fn x_range(operator: &str, partial: &Partial<'_>) -> Option<String> {
    let (major, minor, patch) = (partial.major, partial.minor, partial.patch);
    let number_after_any = (major.is_any() && !minor.is_any())
        || (minor.is_any() && matches!(patch, Component::Number(_)));
    let any_minor = major.is_any() || minor.is_any();
    if number_after_any || !(any_minor || patch.is_any()) {
        return None;
    }
    let operator = if operator == "=" { "" } else { operator };
    let (m, n) = (major.digits(), minor.digits());
    Some(if major.is_any() {
        if operator == ">" || operator == "<" {
            "<0.0.0-0".to_owned()
        } else {
            "*".to_owned()
        }
    } else if operator.is_empty() {
        if any_minor {
            format!(">={m}.0.0 <{}.0.0-0", major.plus_one())
        } else {
            format!(">={m}.{n}.0 <{m}.{}.0-0", minor.plus_one())
        }
    } else {
        let mut major_text = m.to_owned();
        let mut minor_text = if any_minor {
            "0".to_owned()
        } else {
            n.to_owned()
        };
        let operator = match operator {
            ">" | "<=" => {
                if any_minor {
                    major_text = major.plus_one();
                } else {
                    minor_text = minor.plus_one();
                }
                if operator == ">" { ">=" } else { "<" }
            }
            other => other,
        };
        let suffix = if operator == "<" { "-0" } else { "" };
        format!("{operator}{major_text}.{minor_text}.0{suffix}")
    })
}

/// Removes the first `*`, with an operator and one space directly before
/// it; later stars remain and fail as comparators.
fn replace_stars(text: &str) -> String {
    let text = text.trim_matches(' ');
    let bytes = text.as_bytes();
    for at in 0..bytes.len() {
        let mut end = at + usize::from(matches!(bytes[at], b'<' | b'>'));
        if bytes.get(end) == Some(&b'=') {
            end += 1;
        }
        if bytes.get(end) == Some(&b' ') {
            end += 1;
        }
        if bytes.get(end) == Some(&b'*') {
            return format!("{}{}", &text[..at], &text[end + 1..]);
        }
    }
    text.to_owned()
}

/// Removes every `+` followed by build identifiers from an alternative,
/// wherever it occurs: node-semver ignores build metadata in ranges, also
/// after partial versions, and `+build` alone is `*`.
fn strip_build(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'+'
            && let Some(end) = identifiers(bytes, at + 1, false)
        {
            at = end;
            continue;
        }
        at = push_char(&mut out, text, at);
    }
    out
}

/// Trims JavaScript whitespace and replaces each inner run with one space.
fn normalize_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, word) in text
        .split(is_js_whitespace)
        .filter(|word| !word.is_empty())
        .enumerate()
    {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// JavaScript's `WhiteSpace` and `LineTerminator`, which `trim` and `\s` use.
fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests;
