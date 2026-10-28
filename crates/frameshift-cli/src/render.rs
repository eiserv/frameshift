//! Presentation of command results. No migration policy lives here.

use std::io::{self, Write};

use frameshift_engine::foundation::{FindingList, FindingLocation, Severity, SourcePath};
use frameshift_engine::inspect::{FileSummary, Inspection, PackageInspection};
use frameshift_engine::nextjs::{
    ConfigLoad, Detection, Router, RouterDirectory, Verdict, VersionEvidence,
};
use frameshift_engine::package::{DetectionSource, IgnoredReason, LockfileState, Workspace};
use frameshift_engine::version::VersionInfo;

use crate::args::{Format, HelpTopic};

const ROOT_HELP: &str = concat!(
    "Frameshift: deterministic framework migration engine\n",
    "\nUsage: frameshift <command> [options]\n",
    "\nCommands available in this build:\n",
    "  help [command]      Print usage for available commands\n",
    "  inspect [<source>]  Identify a project without changing it\n",
    "  analyze [<source>]  Assess migration to astro-preact\n",
    "  report <file>       Render a stored analysis report\n",
    "  explain <id>        Explain a finding, feature, or assessment rule\n",
    "  rules               List assessment rule metadata\n",
    "  migrate [<source>]  Migrate static pages into a new Astro project (v0)\n",
    "  version             Print engine and build information\n",
    "\nOptions:\n",
    "  -h, --help          Print usage\n",
    "  --version           Print the short version string\n",
    "  --format text|json  Select command output (default: text)\n",
    "  --package <dir>     Select one workspace package\n",
    "\nPlanning and verification commands are not implemented.\n",
);

pub(crate) fn help(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::Root => ROOT_HELP,
        HelpTopic::Help => {
            "Usage: frameshift help [help|inspect|analyze|report|explain|rules|migrate|version]\n\
Print usage for the available commands. Without a topic, print the command overview.\n"
        }
        HelpTopic::Inspect => {
            "Usage: frameshift inspect [<source>] [--package <dir>] [--format text|json]\n\
Identify the project at <source>, by default the current directory: its files, package\n\
manager, lockfile, workspace, and the framework, version, configuration file, and routers\n\
of each package. Inspection reads directory listings, .gitignore files, package.json files,\n\
the lockfile, and pnpm-workspace.yaml. It never writes, runs project code, or uses the network.\n\
<source> is the workspace root; nothing above it is read. Without --package, the root and\n\
every member are inspected. --package <dir> inspects one member, given relative to <source>\n\
with / separators, or the root with '.'.\n\
A project without a supported framework is a valid result. Exit status: 0 on success,\n\
2 for invalid arguments, 3 when the source cannot be read or changes during the run.\n"
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

/// Writes a `frameshift.error` document. `exit_code` is one of the codes
/// that the error schema lists.
pub(crate) fn error_document(
    output: &mut impl Write,
    exit_code: u8,
    message: &str,
) -> io::Result<()> {
    write!(
        output,
        "{{\n  \"kind\": \"frameshift.error\",\n  \"schemaVersion\": 1,\n  \"exitCode\": {exit_code},\n  \"message\": "
    )?;
    json_string(output, message)?;
    output.write_all(b"\n}\n")
}

// The shell's own documents hold strings, not arbitrary JSON values; the
// engine renders its versioned documents itself.
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
            // As the engine's document writer does (`document.rs`).
            '\u{00}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => write!(output, "\\u{:04x}", scalar as u32)?,
            _ => output.write_all(scalar.encode_utf8(&mut [0; 4]).as_bytes())?,
        }
    }
    output.write_all(b"\"")
}

/// Escapes text from the source project for a terminal: backslashes,
/// control characters, line and paragraph separators, and bidirectional
/// formatting characters, so that a name can neither inject escape
/// sequences nor disguise the text around it.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for scalar in text.chars() {
        match scalar {
            '\\' => out.push_str("\\\\"),
            '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => out.push_str(&format!("\\u{{{:x}}}", scalar as u32)),
            scalar if scalar.is_control() => {
                out.push_str(&format!("\\u{{{:x}}}", scalar as u32));
            }
            scalar => out.push(scalar),
        }
    }
    out
}

fn directory(path: Option<&SourcePath>) -> String {
    path.map_or_else(|| ".".to_owned(), |path| escape(path.as_str()))
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub(crate) fn inspection(output: &mut impl Write, inspection: &Inspection) -> io::Result<()> {
    let info = frameshift_engine::version::info();
    writeln!(output, "Frameshift {}  inspect\n", info.engine_version)?;
    writeln!(output, "Files          {}", files(&inspection.files))?;
    package_manager(output, &inspection.workspace)?;
    writeln!(
        output,
        "Workspace      {}",
        workspace(&inspection.workspace)
    )?;
    for package in &inspection.packages {
        output.write_all(b"\n")?;
        self::package(output, package)?;
    }
    output.write_all(b"\nMigration      no migration path ships in this build\n")?;
    findings(output, &inspection.findings)
}

fn files(summary: &FileSummary) -> String {
    let bytes = summary.admitted_bytes;
    let mut text = format!(
        "{} admitted ({bytes} {}), {}, {} ignored, {} other",
        summary.admitted,
        if bytes == 1 { "byte" } else { "bytes" },
        plural(summary.directories, "directory", "directories"),
        summary.ignored_total(),
        summary.other_total(),
    );
    if summary.unrepresentable != 0 {
        text.push_str(&format!(", {} unrepresentable", summary.unrepresentable));
    }
    if !summary.complete {
        text.push_str("; incomplete");
        if let Some(stop) = summary.stop {
            text.push_str(&format!(" (stopped at {})", stop.limit_name()));
        }
    }
    text
}

fn package_manager(output: &mut impl Write, workspace: &Workspace) -> io::Result<()> {
    let root = &workspace.root;
    let detection = &root.detection;
    let manager = match detection.manager {
        None => "not determined".to_owned(),
        Some(kind) => {
            let mut text = kind.name().to_owned();
            if let Some(spec) = root
                .package_manager
                .as_ref()
                .filter(|spec| spec.kind == kind)
            {
                text.push_str(&format!(" {}", spec.version));
            }
            match detection.source {
                Some(DetectionSource::Field) => text.push_str(" (packageManager field)"),
                Some(DetectionSource::Lockfile) => text.push_str(" (from the lockfile)"),
                None => {}
            }
            text
        }
    };
    writeln!(output, "Package mgr    {manager}")?;
    let mut lockfile = match detection.lockfile {
        None => "none".to_owned(),
        Some(kind) => match &root.lockfile {
            LockfileState::Parsed(lockfile) => {
                format!("{} (format {})", kind.file_name(), lockfile.version())
            }
            LockfileState::NotParsed(_) | LockfileState::None => {
                format!("{} (not parsed)", kind.file_name())
            }
        },
    };
    if !detection.ignored.is_empty() {
        let ignored: Vec<String> = detection
            .ignored
            .iter()
            .map(|ignored| {
                let reason = match ignored.reason {
                    IgnoredReason::OtherManager => "other package manager",
                    IgnoredReason::Superseded => "superseded",
                    IgnoredReason::Ambiguous => "ambiguous",
                };
                format!("{} ({reason})", ignored.kind.file_name())
            })
            .collect();
        lockfile.push_str(&format!("; ignored: {}", ignored.join(", ")));
    }
    writeln!(output, "Lockfile       {lockfile}")
}

fn workspace(workspace: &Workspace) -> String {
    if !workspace.is_workspace() {
        return "single package".to_owned();
    }
    let releases: Vec<&str> = workspace
        .semantics
        .iter()
        .map(|release| release.as_str())
        .collect();
    let mut text = format!(
        "{}, evaluated as {}",
        plural(workspace.members.len(), "member", "members"),
        releases.join(", ")
    );
    if !workspace.undetermined.is_empty() {
        text.push_str(&format!(
            "; {} undetermined",
            plural(workspace.undetermined.len(), "directory", "directories")
        ));
    }
    if !workspace.complete {
        text.push_str("; incomplete");
    }
    text
}

fn package(output: &mut impl Write, package: &PackageInspection) -> io::Result<()> {
    write!(
        output,
        "Package        {}",
        directory(package.directory.as_ref())
    )?;
    match (&package.name, package.manifest) {
        (Some(name), _) => write!(output, " ({})", escape(name))?,
        (None, frameshift_engine::inspect::ManifestStatus::Absent) => {
            output.write_all(b" (no package.json)")?;
        }
        (None, frameshift_engine::inspect::ManifestStatus::NotAnalyzed) => {
            output.write_all(b" (package.json not analyzed)")?;
        }
        (None, frameshift_engine::inspect::ManifestStatus::Analyzed) => {}
    }
    output.write_all(b"\n")?;
    let detection = &package.nextjs;
    writeln!(output, "  Framework    {}", framework(detection))?;
    if detection.verdict == Verdict::Absent {
        return Ok(());
    }
    let config = match &detection.config {
        None => "none".to_owned(),
        Some(config) => {
            let note = match config.load {
                ConfigLoad::Loaded => "",
                ConfigLoad::DependsOnNode => " (loaded only when Node.js strips types)",
                ConfigLoad::Unsupported => " (ignored or rejected by this Next.js version)",
            };
            format!("{}{note}", escape(config.path.as_str()))
        }
    };
    writeln!(output, "  Config       {config}")?;
    let router = |name: &str, directory: &Option<RouterDirectory>| {
        directory.as_ref().map_or_else(String::new, |directory| {
            format!(
                "{name} in {} ({})",
                escape(directory.path.as_str()),
                plural(directory.routes(), "route file", "route files")
            )
        })
    };
    let routers = match detection.router() {
        Router::App => router("App Router", &detection.app),
        Router::Pages => router("Pages Router", &detection.pages),
        Router::Hybrid => format!(
            "{}; {}",
            router("App Router", &detection.app),
            router("Pages Router", &detection.pages)
        ),
        Router::None => "no route files".to_owned(),
    };
    writeln!(output, "  Router       {routers}")
}

fn framework(detection: &Detection) -> String {
    match detection.verdict {
        Verdict::Absent => "no supported framework detected".to_owned(),
        Verdict::Inconclusive => "Next.js evidence incomplete".to_owned(),
        Verdict::NextJs => match &detection.version {
            VersionEvidence::Locked(version) => format!("Next.js {version} (locked)"),
            VersionEvidence::LowerBound(version) => {
                format!("Next.js {version} or later (lowest version of the declared range)")
            }
            VersionEvidence::Unknown => "Next.js, version unknown".to_owned(),
        },
    }
}

const SEVERITIES: [Severity; 5] = [
    Severity::Error,
    Severity::Blocker,
    Severity::Unknown,
    Severity::Review,
    Severity::Info,
];

fn findings(output: &mut impl Write, findings: &FindingList) -> io::Result<()> {
    let list = findings.as_slice();
    if list.is_empty() {
        return output.write_all(b"Findings       none\n");
    }
    let counts: Vec<String> = SEVERITIES
        .iter()
        .filter_map(|severity| {
            let count = list
                .iter()
                .filter(|finding| finding.severity() == *severity)
                .count();
            (count != 0).then(|| format!("{count} {}", severity.as_str()))
        })
        .collect();
    writeln!(
        output,
        "Findings       {} ({})",
        list.len(),
        counts.join(", ")
    )?;
    for finding in list {
        let location = match finding.location() {
            FindingLocation::Project => "project".to_owned(),
            FindingLocation::File(path) => escape(path.as_str()),
            FindingLocation::Text(text) => {
                let start = text.start();
                format!(
                    "{}:{}:{}",
                    escape(text.file().as_str()),
                    start.line,
                    start.column
                )
            }
        };
        writeln!(
            output,
            "  {:<8}{}  {location}  {}",
            finding.severity().as_str(),
            finding.code().as_str(),
            escape(finding.message())
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_escape_controls_and_bidi_formatting_and_preserve_unicode() {
        let mut output = Vec::new();
        let controls: String = ('\u{00}'..='\u{1f}').collect();
        json_string(&mut output, &format!("{controls}\"\\é😀\u{9b}\u{2069}")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
                "\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f",
                "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
                "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f",
                "\\\"\\\\é😀\\u009b\\u2069\""
            )
        );
    }

    #[test]
    fn terminal_text_escapes_controls_separators_and_bidi_formatting() {
        assert_eq!(
            escape("a\u{1b}[31m\\b\u{7f}\u{85}\u{2028}\u{202e}\u{2066}\u{61c}é😀"),
            "a\\u{1b}[31m\\\\b\\u{7f}\\u{85}\\u{2028}\\u{202e}\\u{2066}\\u{61c}é😀"
        );
    }
}
