//! Yarn classic lockfile syntax and model contracts (`D-072`): determinism,
//! spans, bounds, duplicate keys, and a write and reparse round trip.

use std::collections::BTreeMap;

use frameshift_engine::package::Lockfile;
use frameshift_engine::syntax::{
    YarnDocument, YarnErrorKind, YarnId, YarnLimits, YarnValue, parse_yarn_lockfile,
};

/// Yarn's object semantics: the last occurrence of a key wins.
#[derive(Debug, PartialEq, Eq)]
enum Canon {
    Scalar(String),
    Object(BTreeMap<String, Canon>),
}

fn canon(document: &YarnDocument<'_>, id: YarnId) -> Result<Canon, String> {
    Ok(match document.value(id).ok_or("unknown Yarn value")? {
        YarnValue::String(text) => Canon::Scalar(quote(text)),
        YarnValue::Number(digits) => Canon::Scalar(digits.to_owned()),
        YarnValue::Bool(value) => Canon::Scalar(value.to_string()),
        YarnValue::Object(object) => {
            let mut map = BTreeMap::new();
            for member in object.members() {
                for key in member.keys() {
                    map.insert(key.text().to_owned(), canon(document, member.value())?);
                }
            }
            Canon::Object(map)
        }
    })
}

/// Quotes a string as a JSON string. Quotes and backslashes are written as
/// `\u` escapes, because Yarn ends a quoted string early at a quote after
/// three backslashes, which `\\\"` would spell.
fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' | '\\' | '\u{0}'..='\u{1F}' => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

/// Writes a value in the layout that Yarn's stringifier uses, with every
/// key quoted.
fn write(object: &BTreeMap<String, Canon>, depth: usize, out: &mut String) {
    for (key, value) in object {
        out.push_str(&"  ".repeat(depth));
        out.push_str(&quote(key));
        match value {
            Canon::Scalar(text) => {
                out.push(' ');
                out.push_str(text);
                out.push('\n');
            }
            Canon::Object(nested) => {
                out.push_str(":\n");
                write(nested, depth + 1, out);
            }
        }
    }
}

pub fn run(input: &[u8]) -> Result<(), String> {
    let Some(text) = crate::targets::text(input)? else {
        return Ok(());
    };
    let source = crate::targets::source(text)?;
    let limits = YarnLimits::default();
    let parsed = parse_yarn_lockfile(&source, limits);
    if parsed != parse_yarn_lockfile(&source, limits) {
        return Err("Yarn parse is not deterministic".into());
    }
    let document = match parsed {
        Err(error) => {
            source
                .slice(error.span())
                .map_err(|_| "Yarn error span is invalid")?;
            return Ok(());
        }
        Ok(document) => document,
    };
    let mut pending = vec![document.root()];
    let mut values = 0usize;
    let mut keys = 0usize;
    while let Some(id) = pending.pop() {
        values += 1;
        let span = document.span(id).ok_or("Yarn value has no span")?;
        source
            .slice(span)
            .map_err(|_| "Yarn value span is invalid")?;
        if let Some(YarnValue::Object(object)) = document.value(id) {
            for member in object.members() {
                source
                    .slice(member.span())
                    .map_err(|_| "Yarn member span is invalid")?;
                if member.key_count() == 0 {
                    return Err("Yarn member has no key".into());
                }
                for key in member.keys() {
                    keys += 1;
                    let slice = source
                        .slice(key.span())
                        .map_err(|_| "Yarn key span is invalid")?;
                    if key.text().is_empty()
                        || !(slice.contains(key.text()) || slice.starts_with('"'))
                    {
                        return Err("Yarn key text does not match its span".into());
                    }
                    if object.get(key.text()).is_none() {
                        return Err("Yarn key lookup failed".into());
                    }
                }
                pending.push(member.value());
            }
        }
    }
    if values + keys != document.node_count() {
        return Err("Yarn node count is not the number of values and keys".into());
    }
    let bounded = parse_yarn_lockfile(
        &source,
        YarnLimits {
            max_nodes: document.node_count() - 1,
            ..limits
        },
    );
    if !matches!(bounded, Err(error) if matches!(error.kind(), YarnErrorKind::NodeLimit { .. })) {
        return Err("Yarn node bound is ineffective".into());
    }
    let lockfile = Lockfile::from_yarn_classic(&document);
    if lockfile != Lockfile::from_yarn_classic(&document) {
        return Err("Yarn classic model is not deterministic".into());
    }
    match lockfile {
        Ok(lockfile) => crate::package::check_lockfile(&lockfile, None)?,
        Err(error) => {
            source
                .slice(error.span)
                .map_err(|_| "Yarn classic model error span is invalid")?;
        }
    }
    // Writing the values in Yarn's layout and parsing them again gives the
    // same values, unless the written text spells all three merge conflict
    // markers, which escapes in the input can produce.
    let Canon::Object(root) = canon(&document, document.root())? else {
        return Err("Yarn root is not an object".into());
    };
    let mut written = String::new();
    write(&root, 0, &mut written);
    if !(written.contains("<<<<<<<") && written.contains("=======") && written.contains(">>>>>>>"))
    {
        let rewritten = crate::targets::source(&written)?;
        let reparsed = parse_yarn_lockfile(&rewritten, limits)
            .map_err(|error| format!("written lockfile was rejected: {error}"))?;
        if canon(&reparsed, reparsed.root())? != Canon::Object(root) {
            return Err("written lockfile reads differently".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_cases_hold() {
        for input in [
            "# yarn lockfile v1\n\n\"a@^1\", a@1.x:\n  version \"1.0.0\"\n  dependencies:\n    b \"^2\"\n\nb@^2:\n  version \"2.0.0\"\n",
            "a 1\nb true\nc:\nd: \"x\" e y\n",
            "a b\na c\n",
            "a:\n   b c\n",
            "",
        ] {
            run(input.as_bytes()).unwrap_or_else(|error| panic!("{input:?}: {error}"));
        }
    }
}
