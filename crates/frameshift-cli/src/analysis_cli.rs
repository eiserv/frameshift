//! Thin command adapter and escaped report projections for M5.

use crate::args::{Format, UsageError};
use frameshift_engine::analysis::{
    self, AnalyzeOptions,
    metrics::Metric,
    stored::{StoredReport, Value},
};
use frameshift_engine::foundation::{PathLimits, SourcePath};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Analyze(Options),
    Report(Options),
    Explain(String),
    Rules(Option<String>),
    Help(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Options {
    source: OsString,
    package: Option<SourcePath>,
    format: String,
    output: Option<PathBuf>,
    fail_on: BTreeSet<String>,
    unweighted: bool,
    all: bool,
    section: String,
    section_given: bool,
    route: Option<String>,
    file: Option<String>,
    severity: Option<String>,
    code: Option<String>,
}
const VALUE_OPTIONS: &[&str] = &[
    "--to",
    "--from",
    "--format",
    "--package",
    "--fail-on",
    "--report",
    "--out",
    "--section",
    "--route",
    "--file",
    "--severity",
    "--code",
];
fn command(args: &[OsString]) -> Option<(usize, &str)> {
    let mut index = 0;
    while index < args.len() {
        let s = args[index].to_str()?;
        if VALUE_OPTIONS.contains(&s) {
            index += 2;
            continue;
        }
        if !s.starts_with('-') {
            return Some((index, s));
        }
        index += 1;
    }
    None
}
pub(crate) fn parse(args: &[OsString]) -> Option<Result<Request, UsageError>> {
    let (command_index, command) = command(args)?;
    if command == "help" {
        let topic = args.get(command_index + 1).and_then(|s| s.to_str())?;
        if ["analyze", "report", "explain", "rules"].contains(&topic) {
            return Some(if args.len() == 2 {
                Ok(Request::Help(topic.into()))
            } else {
                Err(error(args, "help takes one command name"))
            });
        }
        return None;
    }
    if !["analyze", "report", "explain", "rules"].contains(&command) {
        return None;
    }
    Some(parse_command(args, command_index, command).map_err(|message| error(args, message)))
}
fn error(args: &[OsString], message: &'static str) -> UsageError {
    let json = args.iter().any(|s| s == "--format=json")
        || args
            .windows(2)
            .any(|p| p[0] == "--format" && p[1] == "json");
    UsageError {
        format: if json { Format::Json } else { Format::Text },
        message,
    }
}
fn parse_command(
    args: &[OsString],
    command_index: usize,
    command: &str,
) -> Result<Request, &'static str> {
    let mut options = Options {
        source: ".".into(),
        package: None,
        format: "text".into(),
        output: None,
        fail_on: BTreeSet::new(),
        unweighted: false,
        all: false,
        section: "summary".into(),
        section_given: false,
        route: None,
        file: None,
        severity: None,
        code: None,
    };
    let mut seen = BTreeSet::new();
    let mut operands = Vec::new();
    let mut target = None;
    let mut adapter = None;
    let mut help = false;
    let mut i = 0;
    while i < args.len() {
        if i == command_index {
            i += 1;
            continue;
        }
        let raw = &args[i];
        i += 1;
        let Some(s) = raw.to_str() else {
            operands.push(raw.clone());
            continue;
        };
        if !s.starts_with('-') {
            operands.push(raw.clone());
            continue;
        }
        let (key, inline) = s.split_once('=').map_or((s, None), |(k, v)| (k, Some(v)));
        let key = match key {
            "-h" => "--help",
            other => other,
        };
        if !seen.insert(key.to_owned()) {
            return Err("option may be supplied only once");
        }
        if ["--help", "--all", "--unweighted", "--unsupported"].contains(&key) {
            if inline.is_some() {
                return Err("flag does not accept a value");
            }
            match key {
                "--help" => help = true,
                "--all" => options.all = true,
                "--unweighted" => options.unweighted = true,
                "--unsupported" if command == "rules" => options.section = "unsupported".into(),
                _ => return Err("option is unavailable for this command"),
            };
            continue;
        }
        if !VALUE_OPTIONS.contains(&key) {
            return Err("unknown option; run 'frameshift help' for usage");
        }
        let value = if let Some(v) = inline {
            OsString::from(v)
        } else {
            let v = args.get(i).ok_or("option requires a value")?.clone();
            i += 1;
            v
        };
        if key == "--report" || key == "--out" {
            if (key == "--report" && command != "analyze")
                || (key == "--out" && command != "report")
                || options.output.is_some()
            {
                return Err("output option is unavailable or duplicated");
            };
            options.output = Some(PathBuf::from(value));
            continue;
        }
        let value = value.to_str().ok_or("option values must be valid UTF-8")?;
        match key {
            "--to" if matches!(command, "analyze" | "rules") => target = Some(value.to_owned()),
            "--from" if command == "analyze" => adapter = Some(value.to_owned()),
            "--format" if matches!(command, "analyze" | "report") => options.format = value.into(),
            "--package" if command == "analyze" => {
                options.package =
                    if value == "." {
                        None
                    } else {
                        Some(SourcePath::new(value, PathLimits::default()).map_err(
                            |_| "--package requires a canonical root-relative directory",
                        )?)
                    }
            }
            "--fail-on" if command == "analyze" => {
                for condition in value.split(',') {
                    if !["unknown", "unsupported", "review", "blocked-routes"].contains(&condition)
                        || !options.fail_on.insert(condition.into())
                    {
                        return Err(
                            "--fail-on requires distinct unknown, unsupported, review, or blocked-routes conditions",
                        );
                    }
                }
            }
            "--section" if command == "report" => {
                if ![
                    "summary",
                    "routes",
                    "units",
                    "files",
                    "dependencies",
                    "config",
                    "rules",
                    "findings",
                    "verification",
                    "tasks",
                ]
                .contains(&value)
                {
                    return Err("unknown report section");
                };
                options.section = value.into();
                options.section_given = true
            }
            "--route" if command == "report" => options.route = Some(value.into()),
            "--file" if command == "report" => {
                SourcePath::new(value, PathLimits::default())
                    .map_err(|_| "--file requires a canonical root-relative path")?;
                options.file = Some(value.into())
            }
            "--severity" if command == "report" => {
                if !["error", "blocker", "unknown", "review", "info"].contains(&value) {
                    return Err("invalid finding severity");
                };
                options.severity = Some(value.into())
            }
            "--code" if command == "report" => {
                frameshift_engine::foundation::FindingCode::new(value)
                    .map_err(|_| "invalid finding code")?;
                options.code = Some(value.into())
            }
            _ => return Err("option is unavailable for this command"),
        }
    }
    if help {
        if !operands.is_empty() || seen.len() != 1 {
            return Err("command help takes no operands or other options");
        };
        return Ok(Request::Help(command.into()));
    }
    if target.as_deref().is_some_and(|t| t != "astro-preact") {
        return Err("the only available target is astro-preact");
    }
    if adapter.as_deref().is_some_and(|s| s != "nextjs") {
        return Err("the only available source adapter is nextjs");
    }
    if !["text", "json"].contains(&options.format.as_str())
        && !(command == "report" && options.format == "markdown")
    {
        return Err("invalid output format");
    }
    if (options.unweighted || options.all) && !matches!(command, "analyze" | "report") {
        return Err("presentation options are unavailable for this command");
    }
    match command {
        "analyze" => {
            if target.is_none() {
                return Err("analyze requires --to astro-preact");
            };
            if operands.len() > 1 {
                return Err("analyze takes at most one source");
            };
            if let Some(source) = operands.pop() {
                options.source = source
            };
            Ok(Request::Analyze(options))
        }
        "report" => {
            if operands.len() != 1 {
                return Err("report requires a report file or output directory");
            };
            options.source = operands.pop().unwrap();
            Ok(Request::Report(options))
        }
        "explain" => {
            if operands.len() != 1 || !seen.is_empty() {
                return Err("explain requires one finding code, feature ID, or assessment rule ID");
            };
            Ok(Request::Explain(
                operands[0]
                    .to_str()
                    .ok_or("explanation identifier must be UTF-8")?
                    .into(),
            ))
        }
        "rules" => {
            if operands.is_empty() {
                Ok(Request::Rules(if options.section == "unsupported" {
                    Some("unsupported".into())
                } else {
                    None
                }))
            } else if operands.len() == 2 && operands[0] == "show" && seen.is_empty() {
                Ok(Request::Rules(Some(
                    operands[1]
                        .to_str()
                        .ok_or("rule identifier must be UTF-8")?
                        .into(),
                )))
            } else {
                Err("rules accepts no operand, or show <rule-id>")
            }
        }
        _ => Err("unknown command"),
    }
}
fn read_report(path: &Path) -> Result<StoredReport, String> {
    use std::io::Read;
    let limits = frameshift_engine::limits::Limits::default();
    let path = if path.is_dir() {
        path.join(".frameshift/report.json")
    } else {
        path.to_path_buf()
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open analysis report")?
        .take(limits.analysis.max_report_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read analysis report")?;
    StoredReport::parse(bytes, &limits).map_err(|e| e.to_string())
}
pub(crate) fn run(
    request: Request,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> io::Result<u8> {
    match request {
        Request::Help(topic) => {
            stdout.write_all(help(&topic).as_bytes())?;
            Ok(0)
        }
        Request::Explain(id) => explain(&id, stdout, stderr),
        Request::Rules(id) => {
            let unsupported = id.as_deref() == Some("unsupported");
            if let Some(id) = id.filter(|s| s != "unsupported") {
                return explain(&id, stdout, stderr);
            }
            for feature in frameshift_engine::catalog::FEATURES {
                let frameshift_engine::catalog::V1::Assessed(statuses) = feature.v1 else {
                    continue;
                };
                if unsupported
                    && !statuses.contains(&frameshift_engine::catalog::Status::Unsupported)
                {
                    continue;
                }
                writeln!(
                    stdout,
                    "{}/assessment.{}  v1  {}  {:?}",
                    frameshift_engine::catalog::PATH,
                    feature.id.replace('.', "-"),
                    feature.name,
                    feature.v1
                )?;
            }
            Ok(0)
        }
        Request::Analyze(options) => {
            match analysis::analyze(
                Path::new(&options.source),
                &AnalyzeOptions {
                    package: options.package.clone(),
                    ..Default::default()
                },
            ) {
                Ok(result) => {
                    let document = match result.document() {
                        Ok(d) => d,
                        Err(e) => return fail(&options, stdout, stderr, 3, &e.to_string()),
                    };
                    // The generated report and the stored-report reader share
                    // the same contract, including all cross-reference checks.
                    let stored = match StoredReport::parse(
                        document.as_bytes().to_vec(),
                        &Default::default(),
                    ) {
                        Ok(s) => s,
                        Err(error) => {
                            if error.kind() == analysis::AnalyzeErrorKind::Limit {
                                return fail(&options, stdout, stderr, 3, &error.to_string());
                            }
                            return fail(
                                &options,
                                stdout,
                                stderr,
                                70,
                                "internal analysis report invariant failed",
                            );
                        }
                    };
                    let document = stored.document();
                    if let Some(path) = &options.output {
                        let destination = match report_destination(Path::new(&options.source), path)
                        {
                            Ok(path) => path,
                            Err(message) => return fail(&options, stdout, stderr, 4, message),
                        };
                        if let Err(e) = write_new(&destination, document.as_bytes()) {
                            return fail(&options, stdout, stderr, 4, e);
                        }
                    }
                    let failed = options
                        .fail_on
                        .iter()
                        .any(|condition| match condition.as_str() {
                            "blocked-routes" => result.assessment.routes.iter().any(|r| {
                                r.status == frameshift_engine::catalog::Status::Unsupported
                            }),
                            s => {
                                result
                                    .assessment
                                    .units
                                    .iter()
                                    .any(|u| u.status.as_str() == s)
                                    || result.findings.iter().any(|(_, f)| match f.severity() {
                                        frameshift_engine::foundation::Severity::Blocker => {
                                            s == "unsupported"
                                        }
                                        frameshift_engine::foundation::Severity::Unknown => {
                                            s == "unknown"
                                        }
                                        frameshift_engine::foundation::Severity::Review => {
                                            s == "review"
                                        }
                                        frameshift_engine::foundation::Severity::Error
                                        | frameshift_engine::foundation::Severity::Info => false,
                                    })
                            }
                        });
                    render(&stored, &options, stdout)?;
                    Ok(if failed { 1 } else { 0 })
                }
                Err(error) => fail(
                    &options,
                    stdout,
                    stderr,
                    if error.kind() == analysis::AnalyzeErrorKind::Internal {
                        70
                    } else {
                        3
                    },
                    &error.to_string(),
                ),
            }
        }
        Request::Report(options) => match read_report(Path::new(&options.source)) {
            Ok(report) => {
                let mut rendered = Vec::new();
                render(&report, &options, &mut rendered)?;
                if let Some(path) = &options.output {
                    if let Err(e) = write_new(path, &rendered) {
                        return fail(&options, stdout, stderr, 4, e);
                    }
                } else {
                    stdout.write_all(&rendered)?
                }
                Ok(0)
            }
            Err(error) => fail(&options, stdout, stderr, 3, &error),
        },
    }
}
fn report_destination(source: &Path, destination: &Path) -> Result<PathBuf, &'static str> {
    let root =
        std::fs::canonicalize(source).map_err(|_| "cannot establish report output containment")?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent =
        std::fs::canonicalize(parent).map_err(|_| "report output directory does not exist")?;
    if parent.starts_with(root) {
        return Err("analysis report output must be outside the source root");
    }
    let name = destination
        .file_name()
        .ok_or("report output must name a new file")?;
    Ok(parent.join(name))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    // create_new rejects existing files and links. Report destinations are
    // explicit output paths; never overwrite an analyzed or stored source.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "report output already exists or cannot be created")?;
    file.write_all(bytes)
        .and_then(|_| file.flush())
        .map_err(|_| "cannot write report output")
}
fn fail(
    options: &Options,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    code: u8,
    message: &str,
) -> io::Result<u8> {
    if options.format == "json" {
        crate::render::error_document(stdout, code, message)?
    }
    writeln!(stderr, "error: {}", escape(message))?;
    Ok(code)
}
fn explain(id: &str, stdout: &mut impl Write, stderr: &mut impl Write) -> io::Result<u8> {
    if let Some((message, explanation, remediation)) = analysis::knowledge::explain(id) {
        writeln!(stdout, "{id}  {message}\n\n{explanation}\n\n{remediation}")?;
        return Ok(0);
    }
    let feature = if let Some(name) = id.strip_prefix("nextjs-to-astro-preact/assessment.") {
        frameshift_engine::catalog::FEATURES.iter().find(|f| {
            matches!(f.v1, frameshift_engine::catalog::V1::Assessed(_))
                && f.id.replace('.', "-") == name
        })
    } else {
        frameshift_engine::catalog::feature(id)
    };
    if let Some(feature) = feature {
        if feature.v1 == frameshift_engine::catalog::V1::OutOfScope {
            writeln!(
                stdout,
                "{}  {}\n\nOutside V1 application assessment. No assessment rule is shipped for this feature.\nSpecification: MIGRATION_SPEC.md section 17",
                feature.id, feature.name
            )?;
            return Ok(0);
        }
        writeln!(
            stdout,
            "{}  {}\n\nV1 statuses: {:?}\nAssessment rule version: 1\nSpecification: MIGRATION_SPEC.md section 17\n\nPredicates use strict binding, route, configuration, and render-context evidence.\nMissing preconditions are unknown. This is an assessment rule; no output is emitted.\nReview and unsupported results are explained in the report's occurrence reasons.",
            feature.id, feature.name, feature.v1
        )?;
        Ok(0)
    } else {
        writeln!(
            stderr,
            "error: explanation identifier is not in the shipped catalogs"
        )?;
        Ok(2)
    }
}
pub(crate) fn help(topic: &str) -> &'static str {
    match topic {
        "analyze" => {
            "Usage: frameshift analyze [<source>] --to astro-preact [--from nextjs]\n       [--package <dir>] [--format text|json] [--report <new-file>]\n       [--fail-on unknown,unsupported,review,blocked-routes] [--unweighted] [--all]\n\nAnalyze one App Router package without writing source, executing code, or using the network.\nThe source is the workspace root. --package selects one member; '.' selects the root.\nExit: 0 assessed, 1 requested condition failed, 2 usage, 3 input, 4 report output refused.\nA high feasibility value never establishes behavioral equivalence.\n"
        }
        "report" => {
            "Usage: frameshift report <report.json|output-dir> [--format text|markdown|json]\n       [--out <new-file>] [--section summary|routes|units|files|dependencies|config|rules|findings|verification|tasks]\n       [--route <pattern>] [--file <path>] [--severity <minimum>] [--code <code>]\n       [--unweighted] [--all]\n\nRender stored analysis evidence without re-analyzing or opening source files.\nAn output directory selects .frameshift/report.json. Output files must be new.\n"
        }
        "explain" => {
            "Usage: frameshift explain <finding-code|assessment-rule-id|feature-id>\nPrint the shipped explanation without reading or executing a project.\n"
        }
        "rules" => {
            "Usage: frameshift rules [--to astro-preact] [--unsupported]\n       frameshift rules show <assessment-rule-id>\nList assessment metadata; these rules emit no migration operations.\n"
        }
        _ => "",
    }
}
fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    v.get(key).unwrap_or(&NULL)
}
fn text(v: &Value, key: &str) -> String {
    get(v, key).text().map(escape).unwrap_or_else(|| "-".into())
}
fn integer(v: &Value, key: &str) -> u64 {
    get(v, key).number().unwrap_or(0)
}
fn metric(v: &Value) -> String {
    Metric {
        numerator: integer(v, "numerator"),
        denominator: integer(v, "denominator"),
    }
    .display()
}
fn escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_control()
            || matches!(ch,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{2028}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        {
            out.push_str(&format!("\\u{{{:04x}}}", ch as u32))
        } else {
            out.push(ch)
        }
    }
    out
}
fn render(report: &StoredReport, options: &Options, out: &mut impl Write) -> io::Result<()> {
    if options.format == "markdown" {
        let mut text_options = options.clone();
        text_options.format = "text".into();
        let mut rendered = Vec::new();
        render(report, &text_options, &mut rendered)?;
        writeln!(
            out,
            "# Frameshift analysis\n\n```text\n{}\n```",
            String::from_utf8_lossy(&rendered).replace('`', "\\u0060")
        )?;
        return Ok(());
    }
    let root = report.value();
    let filtered = options.route.is_some()
        || options.file.is_some()
        || options.code.is_some()
        || options.severity.is_some();
    if options.format == "json" && !filtered && options.section == "summary" {
        return out.write_all(
            if options.section_given {
                get(root, "summary").document()
            } else {
                report.document()
            }
            .as_bytes(),
        );
    }
    if options.section == "summary" && !filtered {
        let summary = get(root, "summary");
        let title = format!(
            "Frameshift {}  analyze  nextjs -> astro-preact",
            text(root, "engineVersion")
        );
        if options.format == "markdown" {
            writeln!(out, "# {title}\n")?
        } else {
            writeln!(out, "{title}\n")?
        }
        writeln!(out, "{}\n", text(summary, "verdict"))?;
        writeln!(
            out,
            "Assessment ({} units, total weight {}, weights-v1)",
            integer(summary, "unitCount"),
            integer(summary, "totalWeight")
        )?;
        let distribution = get(
            summary,
            if options.unweighted {
                "unweightedDistribution"
            } else {
                "distribution"
            },
        );
        let counts: Vec<_> = distribution
            .array()
            .iter()
            .map(|d| integer(get(d, "metric"), "numerator"))
            .collect();
        if let Ok(counts) = <[u64; 4]>::try_from(counts) {
            let bar = analysis::metrics::bar(counts, 60);
            let chars = ['#', '=', '?', 'x'];
            let bar: String = bar
                .into_iter()
                .zip(chars)
                .flat_map(|(n, ch)| std::iter::repeat_n(ch, n))
                .collect();
            writeln!(out, "[{bar}]")?
        }
        for d in distribution.array() {
            let m = get(d, "metric");
            writeln!(
                out,
                "  {:12} {:>7}  {}/{}  {} units",
                text(d, "status"),
                metric(m),
                integer(m, "numerator"),
                integer(m, "denominator"),
                integer(d, "count")
            )?;
        }
        writeln!(
            out,
            "Analysis coverage {}  Feasibility with review {}",
            metric(get(summary, "analysisCoverage")),
            metric(get(summary, "feasibilityWithReview"))
        )?;
        writeln!(
            out,
            "Transformation rule coverage {} (assessment rules emit no output)",
            metric(get(summary, "ruleCoverage"))
        )?;
        writeln!(out, "\nRoutes (including dependencies)")?;
        for route in get(root, "routes").array() {
            writeln!(
                out,
                "  {:12} {}{}",
                readiness(get(route, "status").text().unwrap_or("unknown")),
                text(route, "pattern"),
                get(route, "blocker")
                    .text()
                    .map_or(String::new(), |b| format!(" via {}", escape(b)))
            )?;
        }
        let mut classes = std::collections::BTreeMap::new();
        for d in get(root, "dependencies").array() {
            *classes.entry(text(d, "class")).or_insert(0usize) += 1;
        }
        writeln!(
            out,
            "\nDependencies ({}): {:?}",
            get(root, "dependencies").array().len(),
            classes
        )?;
        let mut files = std::collections::BTreeMap::new();
        for f in get(root, "files").array() {
            *files.entry(text(f, "class")).or_insert(0usize) += 1;
        }
        writeln!(
            out,
            "Files: {:?}; unrepresentable entries: {}",
            files,
            integer(root, "unrepresentableEntries")
        )?;
        writeln!(out, "\nFindings")?;
        render_findings(root, options, out)?;
        writeln!(
            out,
            "\nA high feasibility value does not mean the migrated site will behave identically."
        )?;
        return Ok(());
    }
    let section = if filtered && options.section == "summary" {
        "findings"
    } else {
        &options.section
    };
    let values = get(root, section);
    let context = FilterContext::new(root);
    let values: Vec<_> = values
        .array()
        .iter()
        .filter(|v| matches_filter(&context, v, section, options))
        .cloned()
        .collect();
    let projection = Value::Array(values);
    if options.format == "json" {
        out.write_all(projection.document().as_bytes())
    } else {
        if options.format == "markdown" {
            writeln!(out, "## {}\n\n```json", escape(section))?
        }
        // JSON string escaping also protects text and Markdown code fences
        // from control characters, multiline data, and injected backticks.
        let printed = projection.document().replace('`', "\\u0060");
        out.write_all(printed.as_bytes())?;
        if options.format == "markdown" {
            writeln!(out, "```")?
        }
        Ok(())
    }
}
fn readiness(status: &str) -> &str {
    match status {
        "auto" => "automatic",
        "review" => "with review",
        "unsupported" => "blocked",
        _ => "uncertain",
    }
}
fn render_findings(root: &Value, options: &Options, out: &mut impl Write) -> io::Result<()> {
    let findings = get(root, "findings").array();
    for finding in findings
        .iter()
        .take(if options.all { usize::MAX } else { 10 })
    {
        let location = get(finding, "location");
        let path = text(location, "path");
        writeln!(
            out,
            "  {} {}  {}  {}",
            text(finding, "severity"),
            text(finding, "code"),
            text(finding, "message"),
            if path == "-" { "project".into() } else { path }
        )?;
    }
    if !options.all && findings.len() > 10 {
        writeln!(
            out,
            "  {} more findings; use --all or report --section findings",
            findings.len() - 10
        )?
    }
    Ok(())
}
struct FilterContext<'a> {
    units_by_path: std::collections::BTreeMap<&'a str, BTreeSet<String>>,
    findings_by_unit: std::collections::BTreeMap<&'a str, Vec<&'a Value>>,
    findings_by_id: std::collections::BTreeMap<&'a str, &'a Value>,
    rule_units: std::collections::BTreeMap<&'a str, BTreeSet<String>>,
    route_units: std::collections::BTreeMap<&'a str, BTreeSet<String>>,
}
impl<'a> FilterContext<'a> {
    fn new(root: &'a Value) -> Self {
        let mut c = Self {
            units_by_path: Default::default(),
            findings_by_unit: Default::default(),
            findings_by_id: Default::default(),
            rule_units: Default::default(),
            route_units: Default::default(),
        };
        for file in get(root, "files").array() {
            if let Some(path) = get(file, "path").text() {
                c.units_by_path.entry(path).or_default().extend(
                    get(file, "units")
                        .array()
                        .iter()
                        .filter_map(Value::text)
                        .map(str::to_owned),
                );
            }
        }
        for finding in get(root, "findings").array() {
            if let Some(id) = get(finding, "id").text() {
                c.findings_by_id.insert(id, finding);
            }
            if let Some(unit) = get(finding, "unit").text() {
                c.findings_by_unit.entry(unit).or_default().push(finding);
            }
        }
        for occurrence in get(root, "occurrences").array() {
            if let (Some(rule), Some(unit)) = (
                get(occurrence, "rule").text(),
                get(occurrence, "unit").text(),
            ) {
                c.rule_units.entry(rule).or_default().insert(unit.into());
            }
        }
        for route in get(root, "routes").array() {
            if let Some(pattern) = get(route, "pattern").text() {
                c.route_units.entry(pattern).or_default().extend(
                    get(route, "closure")
                        .array()
                        .iter()
                        .filter_map(Value::text)
                        .map(str::to_owned),
                );
            }
        }
        c
    }
}
fn related_units(c: &FilterContext<'_>, v: &Value, section: &str) -> BTreeSet<String> {
    let mut units = BTreeSet::new();
    if section == "units"
        && let Some(id) = get(v, "id").text()
    {
        units.insert(id.into());
    }
    if let Some(id) = get(v, "unit").text() {
        units.insert(id.into());
    }
    for key in ["units", "closure"] {
        units.extend(
            get(v, key)
                .array()
                .iter()
                .filter_map(Value::text)
                .map(str::to_owned),
        );
    }
    if section == "dependencies" {
        for path in get(v, "usedIn").array().iter().filter_map(Value::text) {
            if let Some(ids) = c.units_by_path.get(path) {
                units.extend(ids.iter().cloned());
            }
        }
    }
    if section == "rules"
        && let Some(ids) = get(v, "id").text().and_then(|id| c.rule_units.get(id))
    {
        units.extend(ids.iter().cloned());
    }
    if section == "tasks"
        && let Some(finding) = get(v, "finding")
            .text()
            .and_then(|id| c.findings_by_id.get(id))
        && let Some(id) = get(finding, "unit").text()
    {
        units.insert(id.into());
    }
    units
}
fn matches_filter(c: &FilterContext<'_>, v: &Value, section: &str, o: &Options) -> bool {
    let units = related_units(c, v, section);
    if o.code.is_some() || o.severity.is_some() {
        let related: Vec<_> = if section == "findings" {
            vec![v]
        } else if section == "tasks" {
            get(v, "finding")
                .text()
                .and_then(|id| c.findings_by_id.get(id))
                .copied()
                .into_iter()
                .collect()
        } else {
            units
                .iter()
                .filter_map(|id| c.findings_by_unit.get(id.as_str()))
                .flat_map(|f| f.iter().copied())
                .collect()
        };
        let rank = |s: &str| {
            ["error", "blocker", "unknown", "review", "info"]
                .iter()
                .position(|x| *x == s)
                .unwrap_or(usize::MAX)
        };
        if !related.iter().any(|f| {
            o.code
                .as_ref()
                .is_none_or(|code| get(f, "code").text() == Some(code))
                && o.severity.as_ref().is_none_or(|minimum| {
                    rank(get(f, "severity").text().unwrap_or("")) <= rank(minimum)
                })
        }) {
            return false;
        }
    }
    if let Some(file) = &o.file {
        let direct = get(v, "path")
            .text()
            .or_else(|| get(get(v, "location"), "path").text())
            == Some(file);
        let via_unit = c
            .units_by_path
            .get(file.as_str())
            .is_some_and(|ids| !ids.is_disjoint(&units));
        let dependency = get(v, "usedIn")
            .array()
            .iter()
            .any(|p| p.text() == Some(file));
        if !direct && !via_unit && !dependency {
            return false;
        }
    }
    if let Some(pattern) = &o.route {
        let Some(closure) = c.route_units.get(pattern.as_str()) else {
            return false;
        };
        if section == "routes" {
            return get(v, "pattern").text() == Some(pattern);
        }
        if closure.is_disjoint(&units) {
            return false;
        }
    }
    true
}
