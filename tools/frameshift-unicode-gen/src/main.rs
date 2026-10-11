//! Development-only generator for Frameshift's pinned Unicode identifier tables.
//!
//! It reads a local copy of the Unicode Character Database file
//! `DerivedCoreProperties.txt` and writes, or with `--check` compares, the range
//! tables compiled into `frameshift-syntax`. It performs no network access, and
//! no product crate depends on it. Changing the pinned version is a deliberate
//! decision, recorded in `docs/DECISIONS.md`.

use std::{env, fs, process::ExitCode};

/// The only accepted input version.
const UNICODE_VERSION: &str = "18.0.0";

/// Extracted properties and the Rust constants that hold them.
const PROPERTIES: [(&str, &str); 2] = [("ID_Start", "ID_START"), ("ID_Continue", "ID_CONTINUE")];

const USAGE: &str =
    "usage: frameshift-unicode-gen [--check] <DerivedCoreProperties.txt> <unicode_tables.rs>";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (check, input, output) = match args.as_slice() {
        [input, output] => (false, input, output),
        [flag, input, output] if flag == "--check" => (true, input, output),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(check, input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("frameshift-unicode-gen: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(check: bool, input: &str, output: &str) -> Result<(), String> {
    let bytes = fs::read(input).map_err(|error| format!("cannot read {input}: {error}"))?;
    let input_bytes = bytes.len();
    let text = String::from_utf8(bytes).map_err(|_| format!("{input} is not UTF-8"))?;
    let generated = generate(&text, input_bytes)?;
    if check {
        let current =
            fs::read_to_string(output).map_err(|error| format!("cannot read {output}: {error}"))?;
        if current != generated {
            return Err(format!(
                "{output} differs from the tables generated from {input}"
            ));
        }
        return Ok(());
    }
    fs::write(output, generated).map_err(|error| format!("cannot write {output}: {error}"))
}

struct Property {
    name: &'static str,
    constant: &'static str,
    ranges: Vec<(u32, u32)>,
    declared_total: Option<u64>,
}

/// Parses, validates, and renders the tables. Pure, so output is a function of
/// the input text and its byte length only.
fn generate(text: &str, input_bytes: usize) -> Result<String, String> {
    let expected = format!("# DerivedCoreProperties-{UNICODE_VERSION}.txt");
    if text.lines().next() != Some(expected.as_str()) {
        return Err(format!("the first line must be {expected:?}"));
    }
    // Leading comment lines hold the date and Unicode's copyright and terms notice.
    let header: Vec<&str> = text
        .lines()
        .skip(1)
        .map_while(|line| line.strip_prefix("# "))
        .collect();
    let mut properties = PROPERTIES.map(|(name, constant)| Property {
        name,
        constant,
        ranges: Vec::new(),
        declared_total: None,
    });
    let mut section = None;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if let Some(name) = line.strip_prefix("# Derived Property: ") {
            section = properties
                .iter()
                .position(|property| property.name == name.trim());
            continue;
        }
        if let Some(total) = line.strip_prefix("# Total code points: ") {
            if let Some(position) = section {
                let property = &mut properties[position];
                if property.declared_total.is_some() {
                    return Err(format!(
                        "line {number}: repeated total for {}",
                        property.name
                    ));
                }
                let total = total
                    .trim()
                    .parse()
                    .map_err(|_| format!("line {number}: invalid code point total"))?;
                property.declared_total = Some(total);
            }
            continue;
        }
        let data = line.split('#').next().unwrap_or_default().trim();
        let fields: Vec<&str> = data.split(';').map(str::trim).collect();
        let [range, name] = fields.as_slice() else {
            continue;
        };
        let Some(position) = properties
            .iter()
            .position(|property| property.name == *name)
        else {
            continue;
        };
        // Totals are attributed per section, so data must stay inside its section.
        if section != Some(position) {
            return Err(format!("line {number}: {name} data outside its section"));
        }
        let range = parse_range(range).map_err(|message| format!("line {number}: {message}"))?;
        properties[position].ranges.push(range);
    }
    let mut totals = Vec::new();
    for property in &mut properties {
        totals.push(normalize(property)?);
    }
    let [start, continuation] = &properties;
    for &(first, last) in &start.ranges {
        if !contains(&continuation.ranges, first, last) {
            return Err(format!(
                "ID_Start range {first:04X}..{last:04X} is not in ID_Continue"
            ));
        }
    }
    Ok(render(&header, input_bytes, &properties, &totals))
}

fn parse_range(field: &str) -> Result<(u32, u32), String> {
    let (start, end) = field.split_once("..").unwrap_or((field, field));
    let (start, end) = (parse_code_point(start)?, parse_code_point(end)?);
    if start > end {
        return Err(format!("reversed range {field}"));
    }
    Ok((start, end))
}

fn parse_code_point(digits: &str) -> Result<u32, String> {
    if !(4..=6).contains(&digits.len()) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("invalid code point {digits:?}"));
    }
    let value = u32::from_str_radix(digits, 16).map_err(|error| error.to_string())?;
    if value > 0x10ffff {
        return Err(format!("code point {digits} exceeds U+10FFFF"));
    }
    Ok(value)
}

/// Sorts and merges ranges, then checks them against the declared total.
fn normalize(property: &mut Property) -> Result<u64, String> {
    property.ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(property.ranges.len());
    for &(start, end) in &property.ranges {
        if start <= 0xdfff && end >= 0xd800 {
            return Err(format!("{} contains surrogate code points", property.name));
        }
        match merged.last_mut() {
            Some(last) if start <= last.1 => {
                return Err(format!("{} has overlapping ranges", property.name));
            }
            Some(last) if start == last.1 + 1 => last.1 = end,
            Some(_) | None => merged.push((start, end)),
        }
    }
    let total = merged
        .iter()
        .map(|&(start, end)| u64::from(end - start + 1))
        .sum();
    if property.declared_total != Some(total) {
        return Err(format!(
            "{} has {total} code points, but the file declares {:?}",
            property.name, property.declared_total
        ));
    }
    property.ranges = merged;
    Ok(total)
}

fn contains(ranges: &[(u32, u32)], first: u32, last: u32) -> bool {
    let index = ranges.partition_point(|&(_, end)| end < first);
    ranges
        .get(index)
        .is_some_and(|&(start, end)| start <= first && last <= end)
}

fn render(header: &[&str], input_bytes: usize, properties: &[Property], totals: &[u64]) -> String {
    let mut out =
        String::from("// @generated by tools/frameshift-unicode-gen. Do not edit by hand.\n");
    out.push_str(&format!(
        "// Source: DerivedCoreProperties-{UNICODE_VERSION}.txt, {input_bytes} bytes.\n"
    ));
    for line in header {
        out.push_str(&format!("// {line}\n"));
    }
    out.push_str("//\n");
    for (property, total) in properties.iter().zip(totals) {
        out.push_str(&format!(
            "// {}: {total} code points in {} ranges.\n",
            property.name,
            property.ranges.len()
        ));
    }
    out.push_str("\n/// The Unicode Character Database version of the identifier tables.\n");
    out.push_str(&format!(
        "pub const UNICODE_VERSION: &str = \"{UNICODE_VERSION}\";\n"
    ));
    for property in properties {
        out.push_str(&format!(
            "\n/// Sorted, disjoint, nonadjacent inclusive code point ranges with `{}`.\n",
            property.name
        ));
        out.push_str(&format!(
            "pub(crate) const {}: &[(u32, u32)] = &[\n",
            property.constant
        ));
        for &(start, end) in &property.ranges {
            out.push_str(&format!("    (0x{start:04x}, 0x{end:04x}),\n"));
        }
        out.push_str("];\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::generate;

    const HEADER: &str = "# DerivedCoreProperties-18.0.0.txt\n# Date: fictional\n#\n";

    fn input(start: &str, start_total: u32, continuation: &str, continuation_total: u32) -> String {
        format!(
            "{HEADER}\n# Derived Property: ID_Start\n{start}\n# Total code points: {start_total}\n\
             \n# Derived Property: ID_Continue\n{continuation}\n# Total code points: {continuation_total}\n"
        )
    }

    #[test]
    fn ranges_are_sorted_merged_and_rendered_deterministically() {
        let text = input(
            "0061..007A ; ID_Start # comment\n0041..004F ; ID_Start\n0050..005A ; ID_Start",
            52,
            "0030..0039 ; ID_Continue\n0041..005A ; ID_Continue\n005F ; ID_Continue\n\
             0061..007A ; ID_Continue\n0041 ; XID_Start",
            63,
        );
        let output = generate(&text, text.len()).unwrap();
        assert_eq!(output, generate(&text, text.len()).unwrap());
        assert!(
            output.contains("// Date: fictional\n//\n// ID_Start: 52 code points in 2 ranges.")
        );
        assert!(output.contains("pub const UNICODE_VERSION: &str = \"18.0.0\";"));
        assert!(output.contains(
            "pub(crate) const ID_START: &[(u32, u32)] = &[\n    (0x0041, 0x005a),\n    (0x0061, 0x007a),\n];"
        ));
        assert!(output.contains("    (0x005f, 0x005f),\n"));
    }

    #[test]
    fn inconsistent_inputs_are_rejected() {
        for (text, message) in [
            (
                "# DerivedCoreProperties-17.0.0.txt\n".to_owned(),
                "first line",
            ),
            (
                input("0041 ; ID_Start", 2, "0041 ; ID_Continue", 1),
                "declares",
            ),
            (
                input(
                    "0041..0043 ; ID_Start\n0043 ; ID_Start",
                    4,
                    "0041..0043 ; ID_Continue",
                    3,
                ),
                "overlapping",
            ),
            (
                input("D7FF..D800 ; ID_Start", 2, "0041 ; ID_Continue", 1),
                "surrogate",
            ),
            (
                input("0041 ; ID_Start", 1, "0042 ; ID_Continue", 1),
                "not in ID_Continue",
            ),
            (
                input("110000 ; ID_Start", 1, "0041 ; ID_Continue", 1),
                "exceeds",
            ),
            (
                input("0041 ; ID_Continue", 1, "0041 ; ID_Continue", 1),
                "outside",
            ),
        ] {
            let error = generate(&text, text.len()).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
    }
}
