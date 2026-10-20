//! Matching parsed workspace patterns against directory paths with the
//! rules in which the modeled matchers differ (`D-073`).

use super::pattern::{Pattern, Segment, Token};

/// The unit that `?` and a class consume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    /// A UTF-16 code unit, as JavaScript regular expressions without the
    /// `u` flag read names (minimatch, picomatch).
    Utf16,
    /// A Unicode scalar value (wax, Bun).
    Scalar,
}

/// How one matcher compares a segment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rules {
    pub encoding: Encoding,
    /// Whether wildcards and `**` may match a name starting with `.`.
    pub dot: bool,
    /// Whether a class at the start of a segment may match a leading `.`
    /// even when `dot` is false, as picomatch's classes do.
    pub class_matches_leading_dot: bool,
    /// Whether a trailing `**` must match at least one segment, as Bun's
    /// matcher requires.
    pub trailing_globstar_nonempty: bool,
    /// Whether names and literals are compared after Unicode lowercase
    /// folding, for the case sensitivity check.
    pub fold_case: bool,
}

/// The matching work that a workspace evaluation may still spend.
#[derive(Debug)]
pub(crate) struct Work {
    remaining: u64,
}

/// The work limit was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Exhausted;

impl Work {
    pub(crate) const fn new(limit: u64) -> Self {
        Self { remaining: limit }
    }

    fn charge(&mut self, amount: u64) -> Result<(), Exhausted> {
        if amount > self.remaining {
            self.remaining = 0;
            return Err(Exhausted);
        }
        self.remaining -= amount;
        Ok(())
    }
}

#[derive(Clone, Debug)]
enum Unit {
    Literal(u32),
    Star,
    Any,
    Class {
        negated: bool,
        ranges: Vec<(u32, u32)>,
    },
}

#[derive(Clone, Debug)]
enum Compiled {
    Globstar,
    Alternatives(Vec<Vec<Unit>>),
}

/// A pattern prepared for one set of rules.
#[derive(Clone, Debug)]
pub(crate) struct Matcher {
    segments: Vec<Compiled>,
    rules: Rules,
    /// Whether the first segment matches the names it would not match, as
    /// Bun walks a negated pattern.
    negate_first: bool,
}

const DOT: u32 = '.' as u32;

impl Matcher {
    pub(crate) fn new(pattern: &Pattern, rules: Rules) -> Self {
        Self::from_segments(&pattern.segments, rules)
    }

    pub(crate) fn from_segments(segments: &[Segment], rules: Rules) -> Self {
        let segments = segments
            .iter()
            .map(|segment| match segment {
                Segment::Globstar => Compiled::Globstar,
                Segment::Alternatives(alternatives) => Compiled::Alternatives(
                    alternatives
                        .iter()
                        .map(|tokens| compile(tokens, rules))
                        .collect(),
                ),
            })
            .collect();
        Self {
            segments,
            rules,
            negate_first: false,
        }
    }

    /// A matcher whose first segment is inverted: Bun walks `!a/b` as the
    /// names other than `a`, then `b` (`D-073`). A dot name or a first
    /// `**` never matches.
    pub(crate) fn negated_first(pattern: &Pattern, rules: Rules) -> Self {
        Self {
            negate_first: true,
            ..Self::new(pattern, rules)
        }
    }

    /// Whether the pattern matches the whole path.
    pub(crate) fn matches(&self, path: &[Vec<u32>], work: &mut Work) -> Result<bool, Exhausted> {
        let reach = self.reach(path, work)?;
        Ok(reach[self.segments.len() * (path.len() + 1) + path.len()])
    }

    /// Whether the pattern could match the path or a path below it.
    pub(crate) fn could_match_below(
        &self,
        path: &[Vec<u32>],
        work: &mut Work,
    ) -> Result<bool, Exhausted> {
        let reach = self.reach(path, work)?;
        let width = path.len() + 1;
        Ok((0..=self.segments.len()).any(|index| reach[index * width + path.len()]))
    }

    /// `reach[i * (n + 1) + j]` tells whether the first `i` pattern
    /// segments match the first `j` path segments.
    fn reach(&self, path: &[Vec<u32>], work: &mut Work) -> Result<Vec<bool>, Exhausted> {
        let width = path.len() + 1;
        let count = self.segments.len();
        work.charge(((count + 1) * width) as u64)?;
        let mut reach = vec![false; (count + 1) * width];
        reach[0] = true;
        for index in 0..count {
            let last = index + 1 == count;
            for position in 0..=path.len() {
                if !reach[index * width + position] {
                    continue;
                }
                match &self.segments[index] {
                    Compiled::Globstar if self.negate_first && index == 0 => {}
                    Compiled::Globstar => {
                        let nonempty = last && self.rules.trailing_globstar_nonempty;
                        if !nonempty {
                            reach[(index + 1) * width + position] = true;
                        }
                        let mut end = position;
                        while end < path.len() && (self.rules.dot || !starts_with_dot(&path[end])) {
                            end += 1;
                            reach[(index + 1) * width + end] = true;
                        }
                    }
                    Compiled::Alternatives(alternatives) => {
                        if position < path.len() {
                            let name = &path[position];
                            let mut matched = false;
                            for units in alternatives {
                                if self.segment(units, name, work)? {
                                    matched = true;
                                    break;
                                }
                            }
                            if self.negate_first && index == 0 {
                                matched = !matched && !starts_with_dot(name);
                            }
                            if matched {
                                reach[(index + 1) * width + position + 1] = true;
                            }
                        }
                    }
                }
            }
        }
        Ok(reach)
    }

    fn segment(&self, units: &[Unit], name: &[u32], work: &mut Work) -> Result<bool, Exhausted> {
        if starts_with_dot(name) && !self.rules.dot {
            match units.first() {
                Some(Unit::Literal(_)) | None => {}
                Some(Unit::Class { .. }) if self.rules.class_matches_leading_dot => {}
                Some(Unit::Star | Unit::Any | Unit::Class { .. }) => return Ok(false),
            }
        }
        glob(units, name, work)
    }
}

fn starts_with_dot(name: &[u32]) -> bool {
    name.first() == Some(&DOT)
}

fn compile(tokens: &[Token], rules: Rules) -> Vec<Unit> {
    let mut compiled = Vec::new();
    for token in tokens {
        match token {
            Token::Literal(character) => {
                let mut buffer = [0u8; 4];
                let text = character.encode_utf8(&mut buffer);
                compiled.extend(units(text, rules).into_iter().map(Unit::Literal));
            }
            Token::Star => compiled.push(Unit::Star),
            Token::Any => compiled.push(Unit::Any),
            Token::Class { negated, ranges } => {
                let mut converted = Vec::new();
                for &(low, high) in ranges {
                    let (low, high) = if rules.fold_case && low.is_ascii_uppercase() {
                        (low.to_ascii_lowercase(), high.to_ascii_lowercase())
                    } else {
                        (low, high)
                    };
                    converted.push((low as u32, high as u32));
                }
                compiled.push(Unit::Class {
                    negated: *negated,
                    ranges: converted,
                });
            }
        }
    }
    compiled
}

/// Converts a name into the units that `rules` compare.
pub(crate) fn units(text: &str, rules: Rules) -> Vec<u32> {
    let folded;
    let text = if rules.fold_case {
        folded = text.to_lowercase();
        folded.as_str()
    } else {
        text
    };
    match rules.encoding {
        Encoding::Utf16 => text.encode_utf16().map(u32::from).collect(),
        Encoding::Scalar => text.chars().map(u32::from).collect(),
    }
}

/// Matches one segment with `*` backtracking, which is linear in practice
/// and at most quadratic in the segment length.
fn glob(units: &[Unit], name: &[u32], work: &mut Work) -> Result<bool, Exhausted> {
    let (mut token, mut position) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while position < name.len() {
        work.charge(1)?;
        if let Some(unit) = units.get(token) {
            let unit_matches = match unit {
                Unit::Star => {
                    star = Some((token, position));
                    token += 1;
                    continue;
                }
                Unit::Any => true,
                Unit::Literal(expected) => *expected == name[position],
                Unit::Class { negated, ranges } => {
                    let inside = ranges
                        .iter()
                        .any(|&(low, high)| (low..=high).contains(&name[position]));
                    inside != *negated
                }
            };
            if unit_matches {
                token += 1;
                position += 1;
                continue;
            }
        }
        match star {
            Some((star_token, star_position)) => {
                token = star_token + 1;
                position = star_position + 1;
                star = Some((star_token, star_position + 1));
            }
            None => return Ok(false),
        }
    }
    while matches!(units.get(token), Some(Unit::Star)) {
        token += 1;
    }
    Ok(token == units.len())
}
