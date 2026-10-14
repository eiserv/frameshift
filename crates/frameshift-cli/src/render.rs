//! Presentation of shell results. No migration policy lives here.

use std::io::{self, Write};

use frameshift_engine::version::VersionInfo;

use crate::args::{Format, HelpTopic};

const ROOT_HELP: &str = concat!(
    "Frameshift: deterministic framework migration engine\n",
    "\nUsage: frameshift <command> [options]\n",
    "\nCommands available in this build:\n",
    "  help [command]      Print usage for help or version\n",
    "  version             Print engine and build information\n",
    "\nOptions:\n",
    "  -h, --help          Print usage\n",
    "  --version           Print the short version string\n",
    "  --format text|json  Select version output (default: text)\n",
    "\nProject inspection, analysis, migration, and verification are not implemented.\n",
);

pub(crate) fn help(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::Root => ROOT_HELP,
        HelpTopic::Help => {
            "Usage: frameshift help [help|version]\n\
Print usage for the available commands. Without a topic, print the command overview.\n"
        }
        HelpTopic::Version => {
            "Usage: frameshift version [--format text|json]\n\
Print engine version, shipped catalogs, build commit, and actual Rust compiler.\n\
Empty catalogs mean no migration data is shipped. An unknown build commit is null in JSON.\n\
Use frameshift --version for the short version string.\n"
        }
    }
}

pub(crate) fn version(
    output: &mut impl Write,
    format: Format,
    info: VersionInfo,
) -> io::Result<()> {
    match format {
        Format::Text => {
            writeln!(output, "Frameshift {}", info.engine_version)?;
            text_catalog(output, "Rulesets", info.rulesets)?;
            text_catalog(output, "Knowledge bases", info.knowledge_bases)?;
            text_catalog(output, "Target profiles", info.target_profiles)?;
            writeln!(
                output,
                "Build commit: {}",
                info.build_commit.unwrap_or("unknown")
            )?;
            writeln!(output, "Toolchain: {}", info.toolchain)
        }
        Format::Json => {
            output.write_all(b"{\n  \"kind\": \"frameshift.version\",\n  \"schemaVersion\": 1,\n  \"engineVersion\": ")?;
            json_string(output, info.engine_version)?;
            output.write_all(b",\n  \"rulesets\": ")?;
            json_catalog(output, info.rulesets)?;
            output.write_all(b",\n  \"knowledgeBases\": ")?;
            json_catalog(output, info.knowledge_bases)?;
            output.write_all(b",\n  \"targetProfiles\": ")?;
            json_catalog(output, info.target_profiles)?;
            output.write_all(b",\n  \"buildCommit\": ")?;
            match info.build_commit {
                Some(commit) => json_string(output, commit)?,
                None => output.write_all(b"null")?,
            }
            output.write_all(b",\n  \"toolchain\": ")?;
            json_string(output, info.toolchain)?;
            output.write_all(b"\n}\n")
        }
    }
}

fn text_catalog(output: &mut impl Write, label: &str, items: &[&str]) -> io::Result<()> {
    write!(output, "{label}: ")?;
    if items.is_empty() {
        output.write_all(b"none (not implemented)")?;
    } else {
        for (index, item) in items.iter().enumerate() {
            if index != 0 {
                output.write_all(b", ")?;
            }
            output.write_all(item.as_bytes())?;
        }
    }
    output.write_all(b"\n")
}

fn json_catalog(output: &mut impl Write, items: &[&str]) -> io::Result<()> {
    output.write_all(b"[")?;
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            output.write_all(b", ")?;
        }
        json_string(output, item)?;
    }
    output.write_all(b"]")
}

pub(crate) fn usage_error(output: &mut impl Write, message: &str) -> io::Result<()> {
    output.write_all(b"{\n  \"kind\": \"frameshift.error\",\n  \"schemaVersion\": 1,\n  \"exitCode\": 2,\n  \"message\": ")?;
    json_string(output, message)?;
    output.write_all(b"\n}\n")
}

// This shell writes strings, not arbitrary JSON values. A general document
// writer belongs to the later reporting layer and is not needed here.
fn json_string(output: &mut impl Write, value: &str) -> io::Result<()> {
    output.write_all(b"\"")?;
    for scalar in value.chars() {
        match scalar {
            '"' => output.write_all(b"\\\"")?,
            '\\' => output.write_all(b"\\\\")?,
            '\u{08}' => output.write_all(b"\\b")?,
            '\u{0c}' => output.write_all(b"\\f")?,
            '\n' => output.write_all(b"\\n")?,
            '\r' => output.write_all(b"\\r")?,
            '\t' => output.write_all(b"\\t")?,
            '\u{00}'..='\u{1f}' => write!(output, "\\u{:04x}", scalar as u32)?,
            _ => output.write_all(scalar.encode_utf8(&mut [0; 4]).as_bytes())?,
        }
    }
    output.write_all(b"\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_escape_every_control_character_and_preserve_unicode() {
        let mut output = Vec::new();
        let controls: String = ('\u{00}'..='\u{1f}').collect();
        json_string(&mut output, &format!("{controls}\"\\é😀")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
                "\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f",
                "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
                "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f",
                "\\\"\\\\é😀\""
            )
        );
    }
}
