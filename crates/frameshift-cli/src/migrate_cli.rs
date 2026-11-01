//! Thin adapter for `frameshift migrate` v0 (`D-101`) and `frameshift plan`
//! (`D-102`): argument parsing, one engine call for the plan, one for the
//! checked write, and rendering.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use frameshift_engine::config::PackageChoice;
use frameshift_engine::limits::Limits;
use frameshift_engine::migrate::{
    self, MigrateErrorKind, MigrateOptions, Operation, OperationKind, Plan, TaskKind,
};

use crate::args::{Format, UsageError};
use crate::config_cli::{ConfigChoice, Selection};
use crate::render::{self, escape};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Migrate(Options),
    Plan(PlanOptions),
    Help,
    PlanHelp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Options {
    source: OsString,
    output: PathBuf,
    selection: Selection,
    dry_run: bool,
    plan: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlanOptions {
    source: OsString,
    output: Option<PathBuf>,
    selection: Selection,
    show: Show,
    format: Format,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Show {
    Overview,
    Operations,
    Files,
    Tasks,
}

pub(crate) const HELP: &str = "Usage: frameshift migrate [<source>] [--to astro-preact] --out <dir> [--package <dir>]\n       [--config <file>|--no-config] [--plan <plan.json>] [--dry-run]\n\
Migrate the Next.js App Router project at <source>, by default the current directory, into a\n\
new Astro project with Preact at <dir> (migrate v0, D-101). Static pages, layouts,\n\
and components that the analysis rates auto or review are converted; everything else is\n\
copied unchanged below .frameshift/unmigrated/ and listed in .frameshift/SUMMARY.md.\n\
<dir> must not exist, and must not be inside <source> or contain it. Files are written to a\n\
staging directory next to <dir> and renamed into place when complete. The source is never\n\
modified, and no project code or external tool runs. --plan refuses to continue unless the\n\
recomputed plan document equals the given one byte for byte; the given file is compared, never\n\
executed. --dry-run lists the planned files and writes nothing. frameshift.jsonc in <source>\n\
or the --config file can set the target and the package; flags override it. Exit status: 0 on\n\
success, 2 for invalid arguments, 3 when the source or configuration cannot be read or the plan\n\
document differs, 4 when the output location is refused or cannot be written.\n";

pub(crate) const PLAN_HELP: &str = "Usage: frameshift plan [<source>] [--to astro-preact] [--package <dir>] [--out <plan.json>]\n       [--config <file>|--no-config] [--show operations|files|tasks] [--format text|json]\n\
Compute what `frameshift migrate` would do for the project at <source>, by default the current\n\
directory, without writing project files. The frameshift.plan document records the source\n\
snapshot, versions, options, the digest of every output file, and the manual tasks, under one\n\
plan hash. --out writes it to a new file outside <source> for review or approval, and\n\
--format json prints it. --show lists every operation, output file, or manual task.\n\
frameshift.jsonc in <source> or the --config file can set the target and the package; flags\n\
override it. Exit status: 0 on success, 2 for invalid arguments, 3 when the source or\n\
configuration cannot be read, 4 when the plan document cannot be written.\n";

// How many operations the plan overview lists.
const OVERVIEW_OPERATIONS: usize = 8;

// Options that CLI.md specifies for later milestones.
const PLANNED: &[&str] = &[
    "--strict",
    "--on-existing",
    "--clean-staging",
    "--report",
    "--show-files",
    "--env-names",
];

const VALUE_OPTIONS: &[&str] = &[
    "--to",
    "--out",
    "--package",
    "--format",
    "--plan",
    "--show",
    "--config",
];

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
        let request = match args.get(index + 1).and_then(|s| s.to_str()) {
            Some("migrate") => Request::Help,
            Some("plan") => Request::PlanHelp,
            _ => return None,
        };
        return Some(if args.len() == 2 {
            Ok(request)
        } else {
            Err(error(args, "help takes one command name"))
        });
    }
    if command != "migrate" && command != "plan" {
        return None;
    }
    Some(parse_command(args, index, command == "plan").map_err(|message| error(args, message)))
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

fn parse_command(
    args: &[OsString],
    command: usize,
    planning: bool,
) -> Result<Request, &'static str> {
    let mut seen = BTreeSet::new();
    let mut operands = Vec::new();
    let (mut target, mut output, mut dry_run, mut help) = (None, None, false, false);
    let (mut package, mut config, mut no_config) = (None, None, false);
    let (mut approved, mut show, mut format) = (None, Show::Overview, Format::Text);
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
        if (planning && matches!(key, "--dry-run" | "--plan")) || (!planning && key == "--show") {
            return Err("unknown option; run 'frameshift help' for usage");
        }
        if matches!(key, "--help" | "--dry-run" | "--no-config") {
            if inline.is_some() {
                return Err("flag does not accept a value");
            }
            match key {
                "--help" => help = true,
                "--dry-run" => dry_run = true,
                _ => no_config = true,
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
        if key == "--plan" {
            approved = Some(PathBuf::from(value));
            continue;
        }
        if key == "--config" {
            config = Some(PathBuf::from(value));
            continue;
        }
        let value = value.to_str().ok_or("option values must be valid UTF-8")?;
        match key {
            "--to" => target = Some(value.to_owned()),
            "--format" if value == "text" => {}
            "--format" if planning && value == "json" => format = Format::Json,
            "--format" if planning => return Err("--format requires text or json"),
            "--format" => return Err("migrate v0 supports only text output"),
            "--show" => {
                show = match value {
                    "operations" => Show::Operations,
                    "files" => Show::Files,
                    "tasks" => Show::Tasks,
                    _ => return Err("--show requires operations, files, or tasks"),
                };
            }
            _ => {
                package = Some(
                    PackageChoice::parse(value, &Limits::default())
                        .ok_or("--package requires a canonical root-relative directory")?,
                );
            }
        }
    }
    if help {
        if !operands.is_empty() || seen.len() != 1 {
            return Err("command help takes no operands or other options");
        }
        return Ok(if planning {
            Request::PlanHelp
        } else {
            Request::Help
        });
    }
    if target.as_deref().is_some_and(|t| t != "astro-preact") {
        return Err("the only available target is astro-preact");
    }
    let selection = Selection {
        target: target.is_some(),
        package,
        config: match (config, no_config) {
            (Some(_), true) => return Err("--config and --no-config exclude each other"),
            (Some(path), false) => ConfigChoice::File(path),
            (None, true) => ConfigChoice::Ignored,
            (None, false) => ConfigChoice::Source,
        },
    };
    if operands.len() > 1 {
        return Err(if planning {
            "plan takes at most one source"
        } else {
            "migrate takes at most one source"
        });
    }
    let source = operands.pop().unwrap_or_else(|| ".".into());
    if planning {
        if format == Format::Json && show != Show::Overview {
            return Err("--show applies only to text output");
        }
        return Ok(Request::Plan(PlanOptions {
            source,
            output,
            selection,
            show,
            format,
        }));
    }
    let output = output.ok_or("migrate requires --out <dir>")?;
    Ok(Request::Migrate(Options {
        source,
        output,
        selection,
        dry_run,
        plan: approved,
    }))
}

fn write_operation(stdout: &mut impl Write, operation: &Operation) -> io::Result<()> {
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
    writeln!(stdout, "{}", if operation.review { "  review" } else { "" })
}

// The eight hexadecimal digits that identify a digest in text output.
fn short(digest: &str) -> &str {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    hex.get(..8).unwrap_or(hex)
}

fn manual_tasks(plan: &Plan) -> usize {
    plan.tasks
        .iter()
        .filter(|t| t.kind != TaskKind::NotCopied)
        .count()
}

fn run_plan(
    options: &PlanOptions,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    let source = Path::new(&options.source);
    let failed = |stdout: &mut _, stderr: &mut _, code, message: &str| {
        if options.format == Format::Json {
            render::error_document(stdout, code, message)?;
        }
        fail(stderr, code, message)
    };
    let package = match options
        .selection
        .resolve(source, "plan requires --to astro-preact")
    {
        Ok(package) => package,
        Err(failure) => return failed(stdout, stderr, failure.code, &failure.message),
    };
    let plan = match migrate::plan(
        source,
        &MigrateOptions {
            package,
            ..MigrateOptions::default()
        },
    ) {
        Ok(plan) => plan,
        Err(error) => return failed(stdout, stderr, code(error.kind()), &error.to_string()),
    };
    if let Some(path) = &options.output {
        let written = crate::analysis_cli::report_destination(source, path)
            .map_err(|_| "the plan document must be a new file outside the source")
            .and_then(|destination| {
                crate::analysis_cli::write_new(&destination, plan.document().as_bytes())
                    .map_err(|_| "the plan document already exists or cannot be written")
            });
        if let Err(message) = written {
            return failed(stdout, stderr, 4, message);
        }
    }
    if options.format == Format::Json {
        stdout.write_all(plan.document().as_bytes())?;
        return Ok(0);
    }
    let hash = plan.hash().to_string();
    let snapshot = plan.binding.snapshot.to_string();
    writeln!(
        stdout,
        "Frameshift {}  plan  nextjs {} (App Router) -> astro-preact (profile {})",
        plan.binding.engine_version,
        plan.binding
            .source_version
            .as_deref()
            .map_or_else(|| "unknown version".to_owned(), escape),
        plan.binding.target_profile
    )?;
    writeln!(
        stdout,
        "Plan {}   {} operations   input snapshot {}\n",
        short(&hash),
        plan.operations.len(),
        short(&snapshot)
    )?;
    match options.show {
        Show::Overview | Show::Operations => {
            let shown = if options.show == Show::Overview {
                OVERVIEW_OPERATIONS.min(plan.operations.len())
            } else {
                plan.operations.len()
            };
            for operation in &plan.operations[..shown] {
                write_operation(stdout, operation)?;
            }
            if shown < plan.operations.len() {
                writeln!(
                    stdout,
                    "... {} more (use --show operations)",
                    plan.operations.len() - shown
                )?;
            }
        }
        Show::Files => {
            for operation in &plan.operations {
                writeln!(stdout, "{}", escape(operation.output.as_str()))?;
            }
        }
        Show::Tasks => {
            for task in &plan.tasks {
                writeln!(
                    stdout,
                    "{:<10} {}: {}",
                    task.kind.as_str(),
                    task.source
                        .as_ref()
                        .map_or_else(|| "project".to_owned(), |path| escape(path.as_str())),
                    task.text
                )?;
            }
        }
    }
    writeln!(
        stdout,
        "\nManual tasks: {} (use --show tasks)",
        manual_tasks(&plan)
    )?;
    if options.output.is_some() {
        writeln!(stdout, "Plan document written.")?;
    }
    Ok(0)
}

fn read_plan_document(path: &Path) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open the plan document")?
        .take(Limits::default().analysis.max_report_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read the plan document")?;
    Ok(bytes)
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
        Request::PlanHelp => {
            stdout.write_all(PLAN_HELP.as_bytes())?;
            return Ok(0);
        }
        Request::Plan(options) => return run_plan(&options, stdout, stderr),
        Request::Migrate(options) => options,
    };
    let source = Path::new(&options.source);
    let package = match options
        .selection
        .resolve(source, "migrate requires --to astro-preact")
    {
        Ok(package) => package,
        Err(failure) => return fail(stderr, failure.code, &failure.message),
    };
    let plan = match migrate::plan(
        source,
        &MigrateOptions {
            package,
            ..MigrateOptions::default()
        },
    ) {
        Ok(plan) => plan,
        Err(error) => return fail(stderr, code(error.kind()), &error.to_string()),
    };
    if let Some(path) = &options.plan {
        match read_plan_document(path) {
            Ok(stored) if plan.matches_document(&stored) => {
                writeln!(stdout, "Plan {} verified.", short(&plan.hash().to_string()))?;
            }
            Ok(_) => {
                return fail(
                    stderr,
                    3,
                    "the plan document is invalid or differs from the recomputed plan",
                );
            }
            Err(message) => return fail(stderr, 3, message),
        }
    }
    let destination = match migrate::check_output(source, &options.output) {
        Ok(destination) => destination,
        Err(error) => return fail(stderr, code(error.kind()), &error.to_string()),
    };
    let count = |kind| plan.operations.iter().filter(|o| o.kind == kind).count();
    let tasks = manual_tasks(&plan);
    if options.dry_run {
        writeln!(stdout, "Dry run: nothing will be written.")?;
        for operation in &plan.operations {
            write_operation(stdout, operation)?;
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
                vec![
                    "migrate",
                    "--to",
                    "astro-preact",
                    "--out",
                    "o",
                    "--config",
                    "c",
                    "--no-config",
                ],
                "--config and --no-config exclude each other",
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
