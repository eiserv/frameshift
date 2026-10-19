//! npm versions, ranges, dependency specifiers, and `packageManager`
//! (`D-069`, `D-070`).
//!
//! node-semver itself is the oracle for range semantics; it runs outside
//! the repository (IMPLEMENTATION.md section 26). This target checks the
//! contracts that hold without it: repetition, a canonical text that parses
//! to the same range, satisfaction against an independent precedence model
//! with the prerelease rule, exact minimums, version round trips, and the
//! consistency of specifiers and `packageManager` values with ranges and
//! versions.

use std::cmp::Ordering;

use frameshift_engine::package::{
    Comparator, DependencySpec, Operator, PackageManagerSpec, Version, VersionRange,
};

use crate::targets;

/// Checks the whole input and each of its lines.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = targets::text(input)? else {
        return Ok(());
    };
    check(text)?;
    for line in text.lines().take(16) {
        check(line)?;
    }
    Ok(())
}

fn check(text: &str) -> Result<(), String> {
    let parsed = VersionRange::parse(text);
    if VersionRange::parse(text) != parsed {
        return Err("range parsing is not repeatable".to_owned());
    }
    if let Ok(range) = &parsed {
        check_range(range)?;
    }
    if let Ok(version) = Version::parse(text) {
        if version.to_string() != text {
            return Err(format!("version {text:?} prints as {version}"));
        }
        if VersionRange::parse(text).map(|range| range.satisfies(&version)) == Ok(false)
            && !version.is_prerelease()
        {
            return Err(format!(
                "version {text:?} does not satisfy itself as a range"
            ));
        }
    }
    let spec = DependencySpec::parse(text);
    if DependencySpec::parse(text) != spec {
        return Err("specifier classification is not repeatable".to_owned());
    }
    if let DependencySpec::Range(range) = &spec
        && parsed.as_ref() != Ok(range)
    {
        return Err("a range specifier differs from the range".to_owned());
    }
    if let Ok(manager) = PackageManagerSpec::parse(text) {
        let mut spelling = format!("{}@{}", manager.kind.name(), manager.version);
        if let Some((algorithm, digest)) = &manager.hash {
            spelling.push_str(&format!("+{algorithm}.{digest}"));
        }
        if spelling != text || !manager.version.build().is_empty() {
            return Err(format!("packageManager {text:?} reads as {spelling:?}"));
        }
    }
    Ok(())
}

fn check_range(range: &VersionRange) -> Result<(), String> {
    let canonical = range.to_string();
    match VersionRange::parse(&canonical) {
        Ok(again) if again == *range => {}
        other => return Err(format!("canonical {canonical:?} reparses as {other:?}")),
    }
    let probes = probes(range);
    let minimum = range.min_version();
    if let Some(minimum) = &minimum
        && !satisfies(range, minimum)
    {
        return Err(format!(
            "minimum {minimum} of {canonical:?} does not satisfy it"
        ));
    }
    for probe in &probes {
        let model = satisfies(range, probe);
        if range.satisfies(probe) != model {
            return Err(format!(
                "{canonical:?} on {probe}: product disagrees with model {model}"
            ));
        }
        if model {
            match &minimum {
                Some(minimum) if precedence(minimum, probe) != Ordering::Greater => {}
                other => {
                    return Err(format!(
                        "{canonical:?}: {probe} satisfies below minimum {other:?}"
                    ));
                }
            }
        }
    }
    for left in &probes {
        for right in &probes {
            if left.cmp_precedence(right) != precedence(left, right) {
                return Err(format!(
                    "precedence of {left} and {right} disagrees with the model"
                ));
            }
        }
    }
    Ok(())
}

/// Versions at and next to every comparator bound, and some fixed ones.
fn probes(range: &VersionRange) -> Vec<Version> {
    let mut texts: Vec<String> = ["0.0.0-0", "0.0.0", "0.1.0", "1.0.0-alpha", "1.0.0", "2.0.0"]
        .iter()
        .map(|text| (*text).to_owned())
        .collect();
    for comparator in range.sets().iter().flatten().take(16) {
        let version = comparator.version();
        let (major, minor, patch) = (version.major(), version.minor(), version.patch());
        texts.push(version.to_string());
        texts.push(format!("{major}.{minor}.{patch}"));
        texts.push(format!("{major}.{minor}.{patch}-0"));
        texts.push(format!("{major}.{minor}.{}", patch.saturating_add(1)));
        texts.push(format!("{major}.{minor}.{}-0", patch.saturating_add(1)));
        if version.is_prerelease() {
            texts.push(format!("{version}.0"));
            texts.push(format!("{major}.{minor}.{patch}-zz"));
        }
    }
    if let Some(minimum) = range.min_version() {
        texts.push(minimum.to_string());
    }
    texts
        .iter()
        .filter_map(|text| Version::parse(text).ok())
        .collect()
}

/// The prerelease rule over comparators tested with the model's precedence.
fn satisfies(range: &VersionRange, version: &Version) -> bool {
    range.sets().iter().any(|set| {
        set.iter().all(|comparator| holds(comparator, version))
            && (!version.is_prerelease()
                || set.iter().any(|comparator| {
                    let bound = comparator.version();
                    bound.is_prerelease()
                        && (bound.major(), bound.minor(), bound.patch())
                            == (version.major(), version.minor(), version.patch())
                }))
    })
}

fn holds(comparator: &Comparator, version: &Version) -> bool {
    let ordering = precedence(version, comparator.version());
    match comparator.operator() {
        Operator::Less => ordering == Ordering::Less,
        Operator::LessOrEqual => ordering != Ordering::Greater,
        Operator::Greater => ordering == Ordering::Greater,
        Operator::GreaterOrEqual => ordering != Ordering::Less,
        Operator::Equal => ordering == Ordering::Equal,
    }
}

/// SemVer precedence from the printed text, independent of the product's
/// identifier representation.
fn precedence(left: &Version, right: &Version) -> Ordering {
    let split = |version: &Version| {
        let text = version.to_string();
        let core = text.split('+').next().unwrap_or_default().to_owned();
        let (main, pre) = match core.split_once('-') {
            Some((main, pre)) => (main.to_owned(), Some(pre.to_owned())),
            None => (core, None),
        };
        let numbers: Vec<u64> = main
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect();
        (numbers, pre)
    };
    let (left_numbers, left_pre) = split(left);
    let (right_numbers, right_pre) = split(right);
    left_numbers
        .cmp(&right_numbers)
        .then_with(|| match (left_pre, right_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => {
                let mut left = left.split('.');
                let mut right = right.split('.');
                loop {
                    match (left.next(), right.next()) {
                        (None, None) => return Ordering::Equal,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (Some(a), Some(b)) => {
                            let numeric =
                                |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
                            let ordering = match (numeric(a), numeric(b)) {
                                (true, true) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
                                (true, false) => Ordering::Less,
                                (false, true) => Ordering::Greater,
                                (false, false) => a.cmp(b),
                            };
                            if ordering != Ordering::Equal {
                                return ordering;
                            }
                        }
                    }
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_cases_hold() {
        for input in [
            "^1.2.3-beta.2 || >=2 <3.0.0-rc.1",
            "1.x.x-beta",
            "> =1.2.3",
            "npm:counter@~2",
            "pnpm@10.24.0+sha512.0a1b",
            "1.2.3-0a.9.x",
            "*",
            "",
            ">0.0.0-0",
        ] {
            run(input.as_bytes()).unwrap_or_else(|error| panic!("{input:?}: {error}"));
        }
    }
}
