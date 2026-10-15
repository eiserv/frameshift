//! Catalog identifier acceptance and independent unit encoding contracts.

use frameshift_engine::foundation::{
    FeatureId, FindingCode, IdentifierError, IdentifierLimits, PathLimits, RuleId, SourcePath,
    UnitId, UnitKind,
};

/// Checks catalog grammar, unit encoding, limits, and repeated outcomes.
pub fn run(input: &[u8]) -> Result<(), String> {
    let Ok(text) = std::str::from_utf8(input) else {
        return Ok(());
    };
    let finding = FindingCode::new(text);
    let finding_valid = text.starts_with("FS")
        && text.chars().count() == 6
        && text.chars().skip(2).all(|ch| ch.is_ascii_digit());
    if finding.is_ok() != finding_valid
        || finding != FindingCode::new(text)
        || finding.as_ref().is_ok_and(|id| id.as_str() != text)
    {
        return Err("finding code disagrees with grammar or repetition".to_owned());
    }
    for max_bytes in [128 * 1024, input.len(), input.len() / 2] {
        let limits = IdentifierLimits { max_bytes };
        let feature = FeatureId::new(text, limits);
        check_catalog(
            feature.map(|id| id.as_str().to_owned()),
            FeatureId::new(text, limits).map(|id| id.as_str().to_owned()),
            text,
            dotted(text, 2, usize::MAX),
            limits,
        )?;
        let rule = RuleId::new(text, limits);
        if let Ok(rule) = &rule
            && format!("{}/{}", rule.migration_path(), rule.name()) != text
        {
            return Err("rule parts changed the original spelling".to_owned());
        }
        check_catalog(
            rule.map(|id| id.as_str().to_owned()),
            RuleId::new(text, limits).map(|id| id.as_str().to_owned()),
            text,
            rule_valid(text),
            limits,
        )?;

        let encoded = encode(text);
        let route_expected = format!("route:{encoded}");
        check_unit(
            UnitId::for_route(UnitKind::Route, text, limits),
            UnitId::for_route(UnitKind::Route, text, limits),
            &route_expected,
            route_valid(text),
            limits,
        )?;
        let config_expected = format!("config:{encoded}");
        check_unit(
            UnitId::for_config(text, limits),
            UnitId::for_config(text, limits),
            &config_expected,
            dotted(text, 2, usize::MAX),
            limits,
        )?;

        let (subject, export) = match text.split_once('\0') {
            Some((subject, export)) => (subject, Some(export)),
            None => (text, None),
        };
        if let Ok(path) = SourcePath::new(subject, PathLimits::default()) {
            let mut expected = format!("component:{}", encode(subject));
            if let Some(export) = export {
                expected.push('#');
                expected.push_str(&encode(export));
            }
            let valid_export =
                export.is_none_or(|value| !value.is_empty() && !value.contains('\0'));
            check_unit(
                UnitId::for_file(UnitKind::Component, &path, export, limits),
                UnitId::for_file(UnitKind::Component, &path, export, limits),
                &expected,
                valid_export,
                limits,
            )?;
            if path.as_str() != subject {
                return Err("unit construction changed source spelling".to_owned());
            }
        }
    }
    Ok(())
}

fn check_catalog(
    actual: Result<String, IdentifierError>,
    repeated: Result<String, IdentifierError>,
    text: &str,
    syntax: bool,
    limits: IdentifierLimits,
) -> Result<(), String> {
    if actual != repeated
        || actual.is_ok() != (syntax && text.len() <= limits.max_bytes)
        || actual.as_ref().is_ok_and(|value| value != text)
    {
        return Err("catalog identifier disagrees with grammar or repetition".to_owned());
    }
    if text.len() > limits.max_bytes
        && actual
            != Err(IdentifierError::TooLong {
                bytes: text.len(),
                limit: limits.max_bytes,
            })
    {
        return Err("catalog byte limit did not precede syntax".to_owned());
    }
    Ok(())
}

fn check_unit(
    actual: Result<UnitId, IdentifierError>,
    repeated: Result<UnitId, IdentifierError>,
    expected: &str,
    syntax: bool,
    limits: IdentifierLimits,
) -> Result<(), String> {
    if actual != repeated
        || actual.is_ok() != (syntax && expected.len() <= limits.max_bytes)
        || actual
            .as_ref()
            .is_ok_and(|id| id.as_str() != expected || !id.as_str().is_ascii())
    {
        return Err("unit identity disagrees with independent encoding or repetition".to_owned());
    }
    if syntax
        && expected.len() > limits.max_bytes
        && actual
            != Err(IdentifierError::TooLong {
                bytes: expected.len(),
                limit: limits.max_bytes,
            })
    {
        return Err("unit limit did not count all encoded bytes".to_owned());
    }
    Ok(())
}

fn segment(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase())
        && text.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        })
}

fn dotted(text: &str, min: usize, max: usize) -> bool {
    let parts: Vec<_> = text.split('.').collect();
    (min..=max).contains(&parts.len()) && parts.into_iter().all(segment)
}

fn rule_valid(text: &str) -> bool {
    let pieces: Vec<_> = text.split('/').collect();
    if pieces.len() != 2 {
        return false;
    }
    let path: Vec<_> = pieces[0].split("-to-").collect();
    path.len() == 2
        && !path[0].ends_with("-to")
        && !path[1].starts_with("to-")
        && path.into_iter().all(segment)
        && dotted(pieces[1], 2, 2)
}

fn route_valid(text: &str) -> bool {
    if !text.starts_with('/') || text.contains(['\\', '\0']) {
        return false;
    }
    if text == "/" {
        return true;
    }
    let parts: Vec<_> = text.split('/').skip(1).collect();
    parts.iter().enumerate().all(|(index, part)| {
        !matches!(*part, "." | "..") && (!part.is_empty() || index + 1 == parts.len())
    }) && parts.first().is_some_and(|part| !part.is_empty())
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || "-._~/[]()".bytes().any(|safe| safe == byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identifiers_and_unit_fragments_satisfy_independent_contracts() {
        let parts = [
            "a", "b1", "-", "#", "%23", "é", "\0", "\u{1b}", "", "/", "..", "-to-", "a--b", "to-b",
            "a-to",
        ];
        for first in parts {
            for last in parts {
                for text in [
                    format!("{first}.{last}.x"),
                    format!("{first}-to-{last}/x.y"),
                    format!("/{first}/{last}"),
                    format!("s{first}\0{last}"),
                ] {
                    run(text.as_bytes()).unwrap();
                }
            }
        }
        for byte in 0..=255 {
            run(&[b'a', b'/', byte]).unwrap();
        }
        for number in 0..=9999 {
            run(format!("FS{number:04}").as_bytes()).unwrap();
        }
    }
}
