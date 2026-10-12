//! Development-only generator for Frameshift's pinned character reference tables.
//!
//! It reads local copies of the WHATWG HTML named character references
//! (`entities.json`) and the three XHTML 1.0 character entity sets
//! (`xhtml-lat1.ent`, `xhtml-symbol.ent`, `xhtml-special.ent`) and writes, or
//! with `--check` compares, the tables compiled into `frameshift-syntax`. It
//! performs no network access, and no product crate depends on it. Changing
//! the sources is a deliberate decision, recorded in `docs/DECISIONS.md`.

use std::{collections::BTreeMap, env, fs, process::ExitCode};

/// The number of named references in the WHATWG list, which the HTML
/// Standard declares static.
const HTML_COUNT: usize = 2231;

/// The XHTML 1.0 entity files in command-line order, with their entity counts.
const XHTML_FILES: [(&str, usize); 3] = [
    ("xhtml-lat1.ent", 96),
    ("xhtml-symbol.ent", 124),
    ("xhtml-special.ent", 33),
];

const USAGE: &str = "usage: frameshift-entity-gen [--check] <entities.json> <xhtml-lat1.ent> \
                     <xhtml-symbol.ent> <xhtml-special.ent> <entity_tables.rs>";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (check, paths) = match args.as_slice() {
        [flag, rest @ ..] if flag == "--check" && rest.len() == 5 => (true, rest),
        rest if rest.len() == 5 => (false, rest),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(check, paths) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("frameshift-entity-gen: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(check: bool, paths: &[String]) -> Result<(), String> {
    let html = read(&paths[0])?;
    let xhtml = [read(&paths[1])?, read(&paths[2])?, read(&paths[3])?];
    let output = &paths[4];
    let generated = generate(&html, [&xhtml[0], &xhtml[1], &xhtml[2]])?;
    if check {
        let current =
            fs::read_to_string(output).map_err(|error| format!("cannot read {output}: {error}"))?;
        if current != generated {
            return Err(format!(
                "{output} differs from the tables generated from the given sources"
            ));
        }
        return Ok(());
    }
    fs::write(output, generated).map_err(|error| format!("cannot write {output}: {error}"))
}

fn read(path: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8"))
}

/// Parses, validates, and renders the tables. Pure, so output is a function of
/// the four input texts only.
fn generate(html: &str, xhtml: [&str; 3]) -> Result<String, String> {
    let references = html_references(html)?;
    let mut entities = BTreeMap::new();
    for ((file, expected), text) in XHTML_FILES.into_iter().zip(xhtml) {
        for (name, code_point) in xhtml_entities(text, file, expected)? {
            if entities.insert(name.clone(), code_point).is_some() {
                return Err(format!("{file}: {name} is declared in two entity sets"));
            }
        }
    }
    Ok(render(
        html.len(),
        xhtml.map(str::len),
        &references,
        &entities,
    ))
}

/// Reads `entities.json`, whose layout is one reference per line:
/// `  "&name;": { "codepoints": [n], "characters": "\uXXXX" },`.
/// Anything else is rejected rather than interpreted.
fn html_references(text: &str) -> Result<BTreeMap<String, Vec<char>>, String> {
    let body = text
        .strip_prefix("{\n")
        .and_then(|rest| rest.strip_suffix("\n}\n"))
        .ok_or("entities.json must consist of `{`, one entry per line, and `}`")?;
    let lines: Vec<&str> = body.split('\n').collect();
    let mut references = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 2;
        let last = index + 1 == lines.len();
        let (name, code_points) = html_reference(line, last)
            .map_err(|message| format!("entities.json line {number}: {message}"))?;
        if references.insert(name.clone(), code_points).is_some() {
            return Err(format!("entities.json line {number}: duplicate {name}"));
        }
    }
    if references.len() != HTML_COUNT {
        return Err(format!(
            "entities.json has {} references, expected {HTML_COUNT}",
            references.len()
        ));
    }
    // Every legacy name without a semicolon also exists with one.
    for (name, value) in &references {
        if !name.ends_with(';') && references.get(&format!("{name};")) != Some(value) {
            return Err(format!("entities.json: {name} has no matching {name};"));
        }
    }
    Ok(references)
}

fn html_reference(line: &str, last: bool) -> Result<(String, Vec<char>), String> {
    let rest = line
        .strip_prefix("  \"&")
        .ok_or("expected an indented `\"&name\"` key")?;
    let (name, rest) = rest.split_once('"').ok_or("unterminated name")?;
    let stem = name.strip_suffix(';').unwrap_or(name);
    if stem.is_empty() || !stem.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!("invalid name {name:?}"));
    }
    let rest = rest
        .strip_prefix(": { \"codepoints\": [")
        .ok_or("expected a codepoints array")?;
    let (numbers, rest) = rest
        .split_once(']')
        .ok_or("unterminated codepoints array")?;
    let code_points = numbers
        .split(", ")
        .map(|number| {
            number
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("invalid code point {number:?}"))
        })
        .collect::<Result<Vec<char>, String>>()?;
    if !(1..=2).contains(&code_points.len()) {
        return Err("a reference must have one or two code points".to_owned());
    }
    let rest = rest
        .strip_prefix(", \"characters\": \"")
        .ok_or("expected a characters string")?;
    let terminator = if last { "\" }" } else { "\" }," };
    let escaped = rest
        .strip_suffix(terminator)
        .ok_or("unexpected text after the characters string")?;
    if decode_json_escapes(escaped)? != code_points {
        return Err(format!("characters of {name} differ from its code points"));
    }
    Ok((name.to_owned(), code_points))
}

/// Decodes a JSON string body that consists only of `\uXXXX` escapes, joining
/// surrogate pairs.
fn decode_json_escapes(text: &str) -> Result<Vec<char>, String> {
    let mut units = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest
            .strip_prefix("\\u")
            .and_then(|digits| digits.get(..4))
            .ok_or_else(|| format!("characters must be escaped code units: {text:?}"))?;
        let unit = u16::from_str_radix(digits, 16)
            .map_err(|_| format!("invalid escape digits {digits:?}"))?;
        units.push(unit);
        rest = &rest[6..];
    }
    char::decode_utf16(units)
        .collect::<Result<Vec<char>, _>>()
        .map_err(|_| format!("unpaired surrogate in {text:?}"))
}

/// Reads one XHTML 1.0 entity set. Declarations start in the first column as
/// `<!ENTITY name "&#n;">`, or `"&#38;#n;">` for `&` and `<`; indented text
/// is part of comments.
fn xhtml_entities(text: &str, file: &str, expected: usize) -> Result<Vec<(String, char)>, String> {
    for phrase in [
        "Portions (C) International Organization for Standardization 1986",
        "provided this notice is included in all copies.",
    ] {
        if !text.contains(phrase) {
            return Err(format!("{file} lacks the ISO 8879 notice"));
        }
    }
    let mut entities = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(rest) = line.strip_prefix("<!ENTITY ") else {
            continue;
        };
        let number = index + 1;
        let (name, rest) = rest
            .split_once([' ', '\t'])
            .ok_or_else(|| format!("{file} line {number}: expected a name"))?;
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(format!("{file} line {number}: invalid name {name:?}"));
        }
        let value = rest.trim_start_matches([' ', '\t']);
        let digits = value
            .strip_prefix("\"&#38;#")
            .or_else(|| value.strip_prefix("\"&#"))
            .and_then(|value| value.split_once(";\">"))
            .map(|(digits, _)| digits)
            .ok_or_else(|| format!("{file} line {number}: expected a numeric value"))?;
        let code_point = digits
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| format!("{file} line {number}: invalid code point {digits:?}"))?;
        entities.push((name.to_owned(), code_point));
    }
    if entities.len() != expected {
        return Err(format!(
            "{file} declares {} entities, expected {expected}",
            entities.len()
        ));
    }
    Ok(entities)
}

fn render(
    html_bytes: usize,
    xhtml_bytes: [usize; 3],
    references: &BTreeMap<String, Vec<char>>,
    entities: &BTreeMap<String, char>,
) -> String {
    let legacy = references
        .keys()
        .filter(|name| !name.ends_with(';'))
        .count();
    let pairs = references.values().filter(|value| value.len() == 2).count();
    let mut out = String::new();
    out.push_str("// @generated by tools/frameshift-entity-gen. Do not edit by hand.\n");
    out.push_str(&format!(
        "// Source: entities.json, {html_bytes} bytes, the named character references of\n"
    ));
    out.push_str("// the WHATWG HTML Standard (https://html.spec.whatwg.org/entities.json).\n");
    out.push_str("// Copyright WHATWG (Apple, Google, Mozilla, Microsoft). Licensed under the\n");
    out.push_str("// Creative Commons Attribution 4.0 International License.\n");
    out.push_str("//\n");
    for ((file, _), bytes) in XHTML_FILES.iter().zip(xhtml_bytes) {
        out.push_str(&format!("// Source: {file}, {bytes} bytes.\n"));
    }
    out.push_str(
        "// The character entity sets of XHTML 1.0 (https://www.w3.org/TR/xhtml1/DTD/).\n",
    );
    out.push_str("// Portions (C) International Organization for Standardization 1986:\n");
    out.push_str("// Permission to copy in any form is granted for use with conforming SGML\n");
    out.push_str("// systems and applications as defined in ISO 8879, provided this notice is\n");
    out.push_str("// included in all copies.\n");
    out.push_str("//\n");
    out.push_str(&format!(
        "// HTML: {} references, {legacy} of them without a semicolon, {pairs} with two code points.\n",
        references.len()
    ));
    out.push_str(&format!("// XHTML: {} entities.\n\n", entities.len()));
    out.push_str(
        "/// HTML named character references without the leading `&`, sorted by\n\
         /// bytes, with their values. Legacy names without a semicolon are listed\n\
         /// in addition to their terminated spellings.\n",
    );
    out.push_str("pub(crate) const HTML_REFERENCES: &[(&str, &str)] = &[\n");
    for (name, value) in references {
        out.push_str(&format!("    (\"{name}\", \"{}\"),\n", escape(value)));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// XHTML 1.0 entity names, which JSX compilers decode, sorted by bytes,\n\
         /// with their code points.\n",
    );
    out.push_str("pub(crate) const XHTML_ENTITIES: &[(&str, char)] = &[\n");
    for (name, value) in entities {
        out.push_str(&format!("    (\"{name}\", '{}'),\n", escape(&[*value])));
    }
    out.push_str("];\n");
    out
}

// Printable ASCII other than quotes and backslashes stays literal; every
// other character becomes a braced escape, so the file is ASCII.
fn escape(value: &[char]) -> String {
    value
        .iter()
        .map(|&ch| {
            if (ch.is_ascii_graphic() && !matches!(ch, '"' | '\'' | '\\')) || ch == ' ' {
                ch.to_string()
            } else {
                format!("\\u{{{:x}}}", u32::from(ch))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(entries: &[&str]) -> String {
        format!("{{\n{}\n}}\n", entries.join(",\n"))
    }

    #[test]
    fn html_entries_are_parsed_strictly() {
        let escape = "\\u";
        let entry =
            format!("  \"&amp;\": {{ \"codepoints\": [38], \"characters\": \"{escape}0026\" }}");
        assert_eq!(
            html_reference(&entry, true),
            Ok(("amp;".to_owned(), vec!['&']))
        );
        let pair = format!(
            "  \"&x;\": {{ \"codepoints\": [128512], \"characters\": \"{escape}D83D{escape}DE00\" }}"
        );
        assert_eq!(
            html_reference(&pair, true).map(|(_, value)| value),
            Ok(vec!['\u{1f600}'])
        );
        for bad in [
            entry.replace("38", "39"),
            entry.replace("&amp;", "&a-p;"),
            entry.replace("&amp;", "&;"),
            entry.replace(" }", " } x"),
            entry.replace(&format!("{escape}0026"), "&"),
            entry.replace("[38]", "[38, 39, 40]"),
        ] {
            assert!(html_reference(&bad, true).is_err(), "{bad}");
        }
        assert!(html_reference(&entry, false).is_err());
    }

    #[test]
    fn html_lists_reject_duplicates_counts_and_other_layouts() {
        let escape = "\\u";
        let amp =
            format!("  \"&amp;\": {{ \"codepoints\": [38], \"characters\": \"{escape}0026\" }}");
        let error = html_references(&html(&[&amp, &amp])).unwrap_err();
        assert!(error.contains("duplicate"), "{error}");
        let error = html_references(&html(&[&amp])).unwrap_err();
        assert!(error.contains("expected 2231"), "{error}");
        assert!(html_references("{\n").is_err());
    }

    #[test]
    fn xhtml_declarations_are_parsed_and_counted() {
        let notice = "<!-- Portions (C) International Organization for Standardization 1986\n\
                      provided this notice is included in all copies. -->\n";
        let text = format!(
            "{notice}<!ENTITY amp     \"&#38;#38;\"> <!-- ampersand -->\n<!ENTITY apos\t \"&#39;\">\n   <!ENTITY % x PUBLIC\n"
        );
        assert_eq!(
            xhtml_entities(&text, "f", 2),
            Ok(vec![("amp".to_owned(), '&'), ("apos".to_owned(), '\'')])
        );
        assert!(xhtml_entities(&text, "f", 3).is_err());
        assert!(xhtml_entities("<!ENTITY a \"&#1;\">\n", "f", 1).is_err());
        let bad = format!("{notice}<!ENTITY a \"x\">\n");
        assert!(xhtml_entities(&bad, "f", 1).is_err());
        let surrogate = format!("{notice}<!ENTITY a \"&#55296;\">\n");
        assert!(xhtml_entities(&surrogate, "f", 1).is_err());
    }

    #[test]
    fn values_render_as_ascii() {
        assert_eq!(escape(&['a', '"', '\u{e9}', ' ']), "a\\u{22}\\u{e9} ");
    }
}
