//! Thin adapter for `frameshift migrate` v0 (`D-101`): argument parsing,
//! one engine call for the plan, one for the checked write, and text output.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use frameshift_engine::foundation::{PathLimits, SourcePath};
use frameshift_engine::migrate::{self, MigrateErrorKind, MigrateOptions, OperationKind, TaskKind};

use crate::args::{Format, UsageError};
use crate::render::escape;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Migrate(Options),
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Options {
    source: OsString,
    output: PathBuf,
    package: Option<SourcePath>,
    dry_run: bool,
}

pub(crate) const HELP: &str = "Usage: frameshift migrate [<source>] --to astro-preact --out <dir> [--package <dir>] [--dry-run]\n\
Migrate the Next.js App Router project at <source>, by default the current directory, into a\n\
new Astro project with Preact at <dir> (migrate v0, proposed in D-101). Static pages, layouts,\n\
and components that the analysis rates auto or review are converted; everything else is\n\
copied unchanged below .frameshift/unmigrated/ and listed in .frameshift/SUMMARY.md.\n\
<dir> must not exist, and must not be inside <source> or contain it. Files are written to a\n\
staging directory next to <dir> and renamed into place when complete. The source is never\n\
modified, and no project code or external tool runs. --dry-run lists the planned files and\n\
writes nothing. Exit status: 0 on success, 2 for invalid arguments, 3 when the source cannot\n\
be analyzed, 4 when the output location is refused or cannot be written.\n";

// Options that CLI.md specifies for later milestones.
const PLANNED: &[&str] = &[
    "--plan",
    "--strict",
    "--on-existing",
    "--clean-staging",
    "--report",
    "--show-files",
    "--env-names",
    "--config",
    "--no-config",
];

const VALUE_OPTIONS: &[&str] = &["--to", "--out", "--package", "--format"];

fn command(args: &[OsString]) -> Option<(usize, &str)> {
    let mut index = 0;
    while index < args.len() {
        let text = args[index].to_str()?;
        if VALUE_OPTIONS.contains(&text) {
            index += 2;
            continue;
        }
        if !text.starts_with('-') {
            return Some((index, text));
        }
        index += 1;
    }
    None
}

/// Parses a `migrate` invocation, or returns `None` for other commands.
pub(crate) fn parse(args: &[OsString]) -> Option<Result<Request, UsageError>> {
    let (index, command) = command(args)?;
    if command == "help" {
        if args.get(index + 1).and_then(|s| s.to_str()) != Some("migrate") {
            return None;
        }
        return Some(if args.len() == 2 {
            Ok(Request::Help)
        } else {
            Err(error(args, "help takes one command name"))
        });
    }
    if command != "migrate" {
        return None;
    }
    Some(parse_migrate(args, index).map_err(|message| error(args, message)))
}

fn error(args: &[OsString], message: &'static str) -> UsageError {
    let json = args.iter().any(|s| s == "--format=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--format" && pair[1] == "json");
    UsageError {
        format: if json { Format::Json } else { Format::Text },
        message,
    }
}

fn parse_migrate(args: &[OsString], command: usize) -> Result<Request, &'static str> {
    let mut seen = BTreeSet::new();
    let mut operands = Vec::new();
    let (mut target, mut output, mut package, mut dry_run, mut help) =
        (None, None, None, false, false);
    let mut i = 0;
    while i < args.len() {
        if i == command {
            i += 1;
            continue;
        }
        let raw = &args[i];
        i += 1;
        let Some(text) = raw.to_str() else {
            operands.push(raw.clone());
            continue;
        };
        if !text.starts_with('-') {
            operands.push(raw.clone());
            continue;
        }
        let (key, inline) = text
            .split_once('=')
            .map_or((text, None), |(key, value)| (key, Some(value)));
        let key = if key == "-h" { "--help" } else { key };
        if !seen.insert(key.to_owned()) {
            return Err("option may be supplied only once");
        }
        if PLANNED.contains(&key) {
            return Err("option is not available in migrate v0");
        }
        if matches!(key, "--help" | "--dry-run") {
            if inline.is_some() {
                return Err("flag does not accept a value");
            }
            if key == "--help" {
                help = true;
            } else {
                dry_run = true;
            }
            continue;
        }
        if !VALUE_OPTIONS.contains(&key) {
            return Err("unknown option; run 'frameshift help migrate' for usage");
        }
        let value = match inline {
            Some(value) => OsString::from(value),
            None => {
                let value = args.get(i).ok_or("option requires a value")?.clone();
                i += 1;
                value
            }
        };
        if key == "--out" {
            output = Some(PathBuf::from(value));
            continue;
        }
        let value = value.to_str().ok_or("option values must be valid UTF-8")?;
        match key {
            "--to" => target = Some(value.to_owned()),
            "--format" if value == "text" => {}
            "--format" => return Err("migrate v0 supports only text output"),
            _ => {
                package =
                    if value == "." {
                        None
                    } else {
                        Some(SourcePath::new(value, PathLimits::default()).map_err(
                            |_| "--package requires a canonical root-relative directory",
                        )?)
                    };
            }
        }
    }
    if help {
        if !operands.is_empty() || seen.len() != 1 {
            return Err("command help takes no operands or other options");
        }
        return Ok(Request::Help);
    }
    match target.as_deref() {
        Some("astro-preact") => {}
        Some(_) => return Err("the only available target is astro-preact"),
        None => return Err("migrate requires --to astro-preact"),
    }
    let output = output.ok_or("migrate requires --out <dir>")?;
    if operands.len() > 1 {
        return Err("migrate takes at most one source");
    }
    Ok(Request::Migrate(Options {
        source: operands.pop().unwrap_or_else(|| ".".into()),
        output,
        package,
        dry_run,
    }))
}

pub(crate) fn run(
    request: Request,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    let options = match request {
        Request::Help => {
            stdout.write_all(HELP.as_bytes())?;
            return Ok(0);
        }
        Request::Migrate(options) => options,
    };
    let source = Path::new(&options.source);
    let plan = match migrate::plan(
        source,
        &MigrateOptions {
            package: options.package.clone(),
            ..MigrateOptions::default()
        },
    ) {
        Ok(plan) => plan,
        Err(error) => return fail(stderr, code(error.kind()), &error.to_string()),
    };
    let destination = match migrate::check_output(source, &options.output) {
        Ok(destination) => destination,
        Err(error) => return fail(stderr, code(error.kind()), &error.to_string()),
    };
    let count = |kind| plan.operations.iter().filter(|o| o.kind == kind).count();
    let tasks = plan
        .tasks
        .iter()
        .filter(|t| t.kind != TaskKind::NotCopied)
        .count();
    if options.dry_run {
        writeln!(stdout, "Dry run: nothing will be written.")?;
        for operation in &plan.operations {
            write!(
                stdout,
                "{:<10} {}  {}",
                operation.kind.as_str(),
                escape(operation.output.as_str()),
                operation.rule
            )?;
            if let Some(source) = &operation.source {
                write!(stdout, "  from {}", escape(source.as_str()))?;
            }
            writeln!(stdout, "{}", if operation.review { "  review" } else { "" })?;
        }
        writeln!(
            stdout,
            "{} operations planned ({} converted to Astro, {} preserved); 0 files written.",
            plan.operations.len(),
            count(OperationKind::Create),
            count(OperationKind::Preserve)
        )?;
    } else {
        if let Err(error) = migrate::execute(&plan, &destination) {
            return fail(stderr, code(error.kind()), &error.to_string());
        }
        writeln!(
            stdout,
            "Migrated: {} files written ({} converted to Astro, {} kept as scripts, {} copied, {} generated, {} preserved).",
            plan.operations.len(),
            count(OperationKind::Create),
            count(OperationKind::Transform),
            count(OperationKind::Copy),
            count(OperationKind::Config),
            count(OperationKind::Preserve)
        )?;
    }
    writeln!(
        stdout,
        "Manual tasks: {tasks}. See {} in the output directory.",
        migrate::SUMMARY
    )?;
    Ok(0)
}

fn code(kind: MigrateErrorKind) -> u8 {
    match kind {
        MigrateErrorKind::Input | MigrateErrorKind::Limit => 3,
        MigrateErrorKind::Output => 4,
        MigrateErrorKind::Internal => 70,
    }
}

// Engine errors describe categories, never paths or source text.
fn fail(stderr: &mut impl Write, code: u8, message: &str) -> io::Result<u8> {
    writeln!(stderr, "error: {message}")?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strings(args: &[&str]) -> Option<Result<Request, UsageError>> {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse(&args)
    }

    #[test]
    fn parses_the_v0_contract() {
        let Some(Ok(Request::Migrate(options))) = parse_strings(&[
            "migrate",
            "site",
            "--to",
            "astro-preact",
            "--out=out",
            "--dry-run",
        ]) else {
            panic!("expected a migrate request");
        };
        assert_eq!(options.source, OsString::from("site"));
        assert_eq!(options.output, PathBuf::from("out"));
        assert!(options.dry_run);
        assert!(parse_strings(&["inspect"]).is_none());
        assert_eq!(parse_strings(&["help", "migrate"]), Some(Ok(Request::Help)));
        for (args, message) in [
            (
                vec!["migrate", "--out", "o"],
                "migrate requires --to astro-preact",
            ),
            (
                vec!["migrate", "--to", "astro-preact"],
                "migrate requires --out <dir>",
            ),
            (
                vec!["migrate", "--to", "vue", "--out", "o"],
                "the only available target is astro-preact",
            ),
            (
                vec!["migrate", "--to", "astro-preact", "--out", "o", "--strict"],
                "option is not available in migrate v0",
            ),
            (
                vec![
                    "migrate",
                    "--to",
                    "astro-preact",
                    "--out",
                    "o",
                    "--format",
                    "json",
                ],
                "migrate v0 supports only text output",
            ),
            (
                vec!["migrate", "a", "b", "--to", "astro-preact", "--out", "o"],
                "migrate takes at most one source",
            ),
        ] {
            let Some(Err(error)) = parse_strings(&args) else {
                panic!("expected a usage error for {args:?}");
            };
            assert_eq!(error.message, message);
        }
    }
}
