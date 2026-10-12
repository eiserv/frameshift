//! Development-only measurement harness for experiment E1 (`D-040`, `D-043`).
//!
//! It generates deterministic corpora of fictional JavaScript, JSX, and
//! TypeScript. It parses them through the engine facade and reports throughput and peak heap
//! memory against the E1 budgets in `docs/ENGINEERING.md` (section 4.1).
//! With `--dir`, it reads the JavaScript and TypeScript files of a directory
//! instead and also reports which of them parse. Timing depends on the
//! machine, and budgets are judged on the reference hardware. The tool writes
//! no files, starts no processes, and performs no network access. No product
//! crate depends on it.
//! `--check` validates generated parser and empty-edit contracts without timing.

mod alloc;
mod check;
mod corpus;
mod directory;
mod measure;

use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use frameshift_engine::{ParseLimits, SourceGoal};

use crate::{
    corpus::{Corpus, CorpusFile, DEFAULT_SEED, GENERATOR_VERSION, generate_file},
    directory::{Extension, Failure, Outcome, Sample},
    measure::{FileMemory, Timing},
};

#[global_allocator]
static ALLOCATOR: alloc::CountingAllocator = alloc::CountingAllocator::new();

/// E1 throughput budget, in decimal megabytes per second on one core.
const THROUGHPUT_BUDGET: f64 = 30.0;
/// E1 peak memory budget for a whole corpus with all trees retained (`D-045`).
const MEMORY_BUDGET: f64 = 20.0;
/// Files in the default corpus, like the 1,000 modules of the `analyze`
/// budget in `docs/ENGINEERING.md`.
const DEFAULT_FILES: usize = 1_000;
const DEFAULT_ITERATIONS: usize = 5;
/// File sizes of the scaling series, in KiB.
const SCALING_KIB: [usize; 7] = [1, 4, 16, 64, 256, 1_024, 4_096];

const USAGE: &str = "usage: frameshift-bench [--seed <n>] [--files <n>] [--iterations <n>]\n       \
                     frameshift-bench --check [--seed <n>] [--files <n>]\n       \
                     frameshift-bench --dir <path> [--iterations <n>]\n       \
                     frameshift-bench --print <index> [--seed <n>] [--files <n>]\n       \
                     frameshift-bench --print-component <index> [--seed <n>] [--files <n>]\n       \
                     frameshift-bench --print-typescript <index> [--seed <n>] [--files <n>]";

struct Options {
    check: bool,
    seed: u64,
    files: usize,
    iterations: usize,
    print: Option<usize>,
    print_component: Option<usize>,
    print_typescript: Option<usize>,
    directory: Option<PathBuf>,
}

fn main() -> ExitCode {
    let options = match parse_options(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("frameshift-bench: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Some(root) = &options.directory {
        if cfg!(debug_assertions) {
            eprintln!("frameshift-bench: measurements require an optimized build (--release)");
            return ExitCode::from(2);
        }
        return match run_directory(root, options.iterations) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("frameshift-bench: {message}");
                ExitCode::FAILURE
            }
        };
    }
    let corpus = Corpus::mixed(options.seed, options.files);
    let components = Corpus::components(options.seed, options.files);
    let typescript = Corpus::typescript(options.seed, options.files);
    if options.check {
        return match run_checks(&options, &corpus, &components, &typescript) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("frameshift-bench: {message}");
                ExitCode::FAILURE
            }
        };
    }
    let printed = match (
        options.print,
        options.print_component,
        options.print_typescript,
    ) {
        (Some(index), _, _) => Some((&corpus, index)),
        (None, Some(index), _) => Some((&components, index)),
        (None, None, Some(index)) => Some((&typescript, index)),
        (None, None, None) => None,
    };
    if let Some((corpus, index)) = printed {
        let Some(file) = corpus.files.get(index) else {
            eprintln!(
                "frameshift-bench: file {index} does not exist; the corpus has {} files",
                corpus.files.len()
            );
            return ExitCode::from(2);
        };
        print!("{}", file.text);
        return ExitCode::SUCCESS;
    }
    // Unoptimized timings are an order of magnitude slower and would be
    // misleading if recorded.
    if cfg!(debug_assertions) {
        eprintln!("frameshift-bench: measurements require an optimized build (--release)");
        return ExitCode::from(2);
    }
    match run(&options, &corpus, &components, &typescript) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("frameshift-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        check: false,
        seed: DEFAULT_SEED,
        files: DEFAULT_FILES,
        iterations: DEFAULT_ITERATIONS,
        print: None,
        print_component: None,
        print_typescript: None,
        directory: None,
    };
    while let Some(flag) = args.next() {
        if flag == "--check" {
            options.check = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        if flag == "--dir" {
            options.directory = Some(PathBuf::from(value));
            continue;
        }
        let number = parse_number(&value).ok_or_else(|| format!("invalid number {value:?}"))?;
        let count = || usize::try_from(number).map_err(|_| format!("{value} is too large"));
        match flag.as_str() {
            "--seed" => options.seed = number,
            "--files" => options.files = count()?,
            "--iterations" => options.iterations = count()?,
            "--print" => options.print = Some(count()?),
            "--print-component" => options.print_component = Some(count()?),
            "--print-typescript" => options.print_typescript = Some(count()?),
            _ => return Err(format!("unknown option {flag:?}")),
        }
    }
    if options.files == 0 || options.iterations == 0 {
        return Err("--files and --iterations must be positive".to_owned());
    }
    let modes = [
        options.check,
        options.directory.is_some(),
        options.print.is_some(),
        options.print_component.is_some(),
        options.print_typescript.is_some(),
    ];
    if modes.into_iter().filter(|&selected| selected).count() > 1 {
        return Err(
            "choose only one of --check, --dir, --print, --print-component, and --print-typescript"
                .to_owned(),
        );
    }
    Ok(options)
}

fn run_checks(
    options: &Options,
    mixed: &Corpus,
    components: &Corpus,
    typescript: &Corpus,
) -> Result<(), String> {
    println!(
        "frameshift-bench: E1 corpus contract check, generator v{GENERATOR_VERSION}, seed {:#x}",
        options.seed
    );
    for (label, corpus) in [
        ("mixed", mixed),
        ("components", components),
        ("typescript", typescript),
    ] {
        let checked = check::corpus(&corpus.files).map_err(|error| format!("{label}, {error}"))?;
        println!(
            "  {label}: {} files, {} bytes, {} nodes, {} comments, fingerprint {:016x}",
            checked.files,
            checked.bytes,
            checked.nodes,
            checked.comments,
            corpus.fingerprint()
        );
    }
    println!(
        "checked: complete source spans, reachable source-ordered trees, template and comment spans, repeated parses, byte-exact empty edits and provenance"
    );
    Ok(())
}

fn parse_number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn run(
    options: &Options,
    corpus: &Corpus,
    components: &Corpus,
    typescript: &Corpus,
) -> Result<(), String> {
    println!("frameshift-bench: E1 parser measurement (D-040)");
    println!(
        "corpus: generator v{GENERATOR_VERSION}, seed {:#x}, {} files ({} scripts), {} bytes, fingerprint {:016x}",
        options.seed,
        grouped(corpus.files.len()),
        grouped(corpus.scripts()),
        grouped(corpus.bytes()),
        corpus.fingerprint()
    );
    println!(
        "components: {} JSX modules, {} bytes, fingerprint {:016x}",
        grouped(components.files.len()),
        grouped(components.bytes()),
        components.fingerprint()
    );
    println!(
        "typescript: {} TypeScript, TSX, and declaration modules, {} bytes, fingerprint {:016x}",
        grouped(typescript.files.len()),
        grouped(typescript.bytes()),
        typescript.fingerprint()
    );
    println!(
        "timing: one warm-up and {} measured passes on one thread; MB = 10^6 bytes",
        options.iterations
    );

    println!();
    println!("Mixed corpus, default parse limits");
    measure_corpus("mixed corpus", corpus, options.iterations)?;
    println!();
    println!("Component corpus (JSX dialect), default parse limits");
    measure_corpus("component corpus", components, options.iterations)?;
    println!();
    println!("TypeScript corpus (TypeScript, TSX, and declaration dialects), default parse limits");
    measure_corpus("TypeScript corpus", typescript, options.iterations)?;

    println!();
    println!("Scaling series, one module per size, unbounded node and token counts");
    println!("  size       nodes      parse min   MB/s    peak");
    let unbounded = ParseLimits {
        max_nodes: usize::MAX,
        max_tokens: usize::MAX,
        ..ParseLimits::default()
    };
    for kib in SCALING_KIB {
        let file = generate_file(options.seed ^ kib as u64, SourceGoal::Module, kib * 1_024);
        let files = [file];
        let timing = measure::throughput(&files, unbounded, options.iterations)
            .map_err(|error| format!("{kib} KiB module, {error}"))?;
        let memory = measure::file_memory(&files, unbounded)
            .map_err(|error| format!("{kib} KiB module, {error}"))?;
        let size = files[0].text.len();
        println!(
            "  {:>5} KiB  {:>9}  {:>9.3} ms  {:>5.1}  {:>5.1}x",
            kib,
            grouped(memory[0].nodes),
            timing.parse.min.as_secs_f64() * 1_000.0,
            rate(size, timing.parse.min),
            ratio(memory[0].peak, size)
        );
    }
    Ok(())
}

fn measure_corpus(label: &str, corpus: &Corpus, iterations: usize) -> Result<(), String> {
    measure_files(label, &corpus.files, iterations)
}

fn measure_files(label: &str, files: &[CorpusFile], iterations: usize) -> Result<(), String> {
    let bytes: usize = files.iter().map(|file| file.text.len()).sum();
    let throughput = measure::throughput(files, ParseLimits::default(), iterations)
        .map_err(|error| format!("{label}, {error}"))?;
    print_timing("text and parse", bytes, &throughput.text_and_parse, false);
    print_timing("parse", bytes, &throughput.parse, true);
    let memory = measure::file_memory(files, ParseLimits::default())
        .map_err(|error| format!("{label}, {error}"))?;
    print_file_memory(&memory);
    let all = measure::corpus_memory(files, ParseLimits::default())
        .map_err(|error| format!("{label}, {error}"))?;
    let retained = all.source + all.indexes + all.trees;
    if all.source == 0 {
        println!(
            "  all trees retained   {} heap, peak {}; not comparable: no source bytes",
            grouped(retained),
            grouped(all.peak)
        );
        return Ok(());
    }
    let verdict = if ratio(all.peak, all.source) <= MEMORY_BUDGET {
        format!("meets {MEMORY_BUDGET}x")
    } else {
        format!("misses {MEMORY_BUDGET}x")
    };
    println!(
        "  all trees retained   {} source + {} indexes + {} trees = {} ({:.1}x), peak {:.1}x  {verdict}",
        megabytes(all.source),
        megabytes(all.indexes),
        megabytes(all.trees),
        megabytes(retained),
        ratio(retained, all.source),
        ratio(all.peak, all.source)
    );
    Ok(())
}

// Counts of files by outcome.
#[derive(Default)]
struct Tally {
    files: usize,
    bytes: usize,
    accepted: usize,
    beyond_limits: usize,
    script_only: usize,
    rejected: usize,
    text: usize,
}

impl Tally {
    fn add(&mut self, bytes: usize, outcome: &Outcome) {
        self.files += 1;
        self.bytes += bytes;
        match outcome {
            Outcome::Accepted(_) => self.accepted += 1,
            Outcome::BeyondLimits(_) => self.beyond_limits += 1,
            Outcome::ScriptOnly(_) => self.script_only += 1,
            Outcome::Rejected(_) => self.rejected += 1,
            Outcome::Text(_) | Outcome::Unreadable => self.text += 1,
        }
    }

    // Files that parse in a goal and dialect that fit them, with limits
    // raised where the defaults stopped them.
    fn parsed(&self) -> usize {
        self.accepted + self.beyond_limits + self.script_only
    }

    fn row(&self, label: &str) {
        println!(
            "  {label:<36} {:>6} {:>12} {:>8} {:>6} {:>6} {:>8} {:>5}  {:>7}",
            grouped(self.files),
            grouped(self.bytes),
            grouped(self.accepted),
            grouped(self.beyond_limits),
            grouped(self.script_only),
            grouped(self.rejected),
            grouped(self.text),
            percent(self.parsed(), self.files)
        );
    }
}

fn run_directory(root: &Path, iterations: usize) -> Result<(), String> {
    let sample = Sample::read(root)?;
    println!("frameshift-bench: E1 directory measurement (D-043)");
    println!("root: {}", root.display());
    let skipped = &sample.skipped;
    println!(
        "walk: {} files selected; not followed: {} symbolic links; skipped: {} ignored and {} unvisited directories, {} other files",
        grouped(sample.files.len()),
        grouped(skipped.links),
        grouped(skipped.ignored_directories),
        grouped(skipped.unvisited_directories),
        grouped(skipped.other_files)
    );
    println!(
        "mapping: .js and .jsx as JSX modules (as scripts if only that parses), .mjs as modules, .cjs as scripts, .ts, .mts, and .cts as TypeScript modules, .tsx as TSX modules"
    );

    let mut total = Tally::default();
    let mut by_extension: BTreeMap<Extension, Tally> = BTreeMap::new();
    let mut by_project: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for file in &sample.files {
        total.add(file.bytes, &file.outcome);
        by_extension
            .entry(file.extension)
            .or_default()
            .add(file.bytes, &file.outcome);
        let project = file.path.split('/').next().unwrap_or_default();
        by_project
            .entry(project)
            .or_default()
            .add(file.bytes, &file.outcome);
        if let Outcome::Rejected(failure) = &file.outcome {
            *by_kind
                .entry(format!("{:?}", failure.error.kind))
                .or_default() += 1;
        }
    }

    println!();
    println!(
        "  {:<36} {:>6} {:>12} {:>8} {:>6} {:>6} {:>8} {:>5}  {:>7}",
        "", "files", "bytes", "accepted", "limits", "script", "rejected", "text", "parsed"
    );
    for extension in Extension::ALL {
        if let Some(tally) = by_extension.get(&extension) {
            tally.row(extension.spelling());
        }
    }
    total.row("all files");
    println!();
    for (project, tally) in &by_project {
        tally.row(project);
    }
    println!();
    println!(
        "  accepted: default limits and mapped goal; limits: parsed only with raised node, token, and lexical limits; script: .js or .jsx parsed only as a script; text: not UTF-8, too large, or unreadable"
    );

    if !by_kind.is_empty() {
        println!();
        println!("Rejections by error kind");
        let mut kinds: Vec<(&String, &usize)> = by_kind.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (kind, count) in kinds {
            println!("  {count:>6}  {kind}");
        }
    }

    let listed: Vec<_> = sample
        .files
        .iter()
        .filter(|file| !matches!(file.outcome, Outcome::Accepted(_)))
        .collect();
    if !listed.is_empty() {
        println!();
        println!("Files not accepted with the default limits");
        for file in listed {
            let (label, detail) = match &file.outcome {
                Outcome::BeyondLimits(failure) => ("limits", describe(failure)),
                Outcome::ScriptOnly(failure) => ("script", describe(failure)),
                Outcome::Rejected(failure) => ("rejected", describe(failure)),
                Outcome::Text(error) => ("text", format!("  {error:?}")),
                Outcome::Unreadable => ("text", "  unreadable".to_owned()),
                Outcome::Accepted(_) => continue,
            };
            println!("  {label:<8}  {}{detail}", file.path);
        }
    }

    let accepted: Vec<CorpusFile> = sample
        .files
        .into_iter()
        .filter_map(|file| match file.outcome {
            Outcome::Accepted(file) => Some(file),
            Outcome::BeyondLimits(_)
            | Outcome::ScriptOnly(_)
            | Outcome::Rejected(_)
            | Outcome::Text(_)
            | Outcome::Unreadable => None,
        })
        .collect();
    if accepted.is_empty() {
        return Ok(());
    }
    println!();
    println!(
        "Accepted files, default parse limits; one warm-up and {iterations} measured passes on one thread; MB = 10^6 bytes"
    );
    measure_files("directory", &accepted, iterations)
}

fn describe(failure: &Failure) -> String {
    format!(
        ":{}:{}  {:?}",
        failure.position.line, failure.position.column, failure.error.kind
    )
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "-".to_owned();
    }
    // Truncated, so a rate never appears higher than it is.
    let hundredths = part * 10_000 / whole;
    format!("{}.{:02}%", hundredths / 100, hundredths % 100)
}

fn print_timing(label: &str, bytes: usize, timing: &Timing, budget: bool) {
    let best = rate(bytes, timing.min);
    let verdict = if !budget {
        String::new()
    } else if best >= THROUGHPUT_BUDGET {
        format!("  meets {THROUGHPUT_BUDGET} MB/s")
    } else {
        format!("  misses {THROUGHPUT_BUDGET} MB/s")
    };
    println!(
        "  {label:<20} min {:>8.3} s ({best:>5.1} MB/s), median {:>8.3} s ({:>5.1} MB/s){verdict}",
        timing.min.as_secs_f64(),
        timing.median.as_secs_f64(),
        rate(bytes, timing.median)
    );
}

fn print_file_memory(files: &[FileMemory]) {
    print!("{}", syntax_summary(files));
    // An empty file has no meaningful ratio to its size; real directories
    // contain some.
    let empty = files.iter().filter(|file| file.bytes == 0).count();
    if empty > 0 {
        println!(
            "  empty files          {} excluded from the ratios below",
            grouped(empty)
        );
    }
    let mut peaks: Vec<(f64, usize)> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| file.bytes > 0)
        .map(|(index, file)| (ratio(file.peak, file.bytes), index))
        .collect();
    if peaks.is_empty() {
        return;
    }
    peaks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let (median, _) = peaks[peaks.len() / 2];
    let (worst, index) = peaks[peaks.len() - 1];
    println!(
        "  peak per file        median {median:.1}x, max {worst:.1}x (file {index}, {} bytes, {} heap), diagnostic only",
        grouped(files[index].bytes),
        grouped(files[index].peak)
    );
    let mut retained: Vec<f64> = files
        .iter()
        .filter(|file| file.bytes > 0)
        .map(|file| ratio(file.retained, file.bytes))
        .collect();
    retained.sort_by(f64::total_cmp);
    println!(
        "  retained per file    median {:.1}x, max {:.1}x",
        retained[retained.len() / 2],
        retained[retained.len() - 1]
    );
}

fn syntax_summary(files: &[FileMemory]) -> String {
    let bytes: usize = files.iter().map(|file| file.bytes).sum();
    let nodes: usize = files.iter().map(|file| file.nodes).sum();
    let comments: usize = files.iter().map(|file| file.comments).sum();
    if bytes == 0 {
        return format!(
            "  syntax               {} nodes, {} comments; no source bytes\n",
            grouped(nodes),
            grouped(comments)
        );
    }
    let summary = format!(
        "  syntax               {} nodes ({:.3} per byte), {} comments",
        grouped(nodes),
        nodes as f64 / bytes as f64,
        grouped(comments)
    );
    let default_nodes = ParseLimits::default().max_nodes;
    format!(
        "{summary}\n  node limit           the default {} nodes are reached at about {} KB of this mix\n",
        grouped(default_nodes),
        grouped((default_nodes as f64 * bytes as f64 / nodes as f64 / 1_000.0) as usize)
    )
}

fn rate(bytes: usize, duration: Duration) -> f64 {
    bytes as f64 / duration.as_secs_f64() / 1e6
}

fn ratio(value: usize, bytes: usize) -> f64 {
    value as f64 / bytes as f64
}

fn megabytes(bytes: usize) -> String {
    format!("{:.2} MB", bytes as f64 / 1e6)
}

fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_sources_have_no_node_density_or_size_estimate() {
        let files = [FileMemory {
            bytes: 0,
            nodes: 1,
            comments: 0,
            peak: 672,
            retained: 352,
        }];
        assert_eq!(
            syntax_summary(&files),
            "  syntax               1 nodes, 0 comments; no source bytes\n"
        );
    }

    fn options(args: &[&str]) -> Result<Options, String> {
        parse_options(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn options_accept_decimal_and_hexadecimal_values() {
        let parsed = options(&["--seed", "0x1f", "--files", "12", "--iterations", "3"])
            .expect("valid options");
        assert_eq!(
            (parsed.seed, parsed.files, parsed.iterations, parsed.print),
            (31, 12, 3, None)
        );
        let component = options(&["--print-component", "4"]).expect("valid options");
        assert_eq!(
            (component.print, component.print_component),
            (None, Some(4))
        );
        let typescript = options(&["--print-typescript", "5"]).expect("valid options");
        assert_eq!(typescript.print_typescript, Some(5));
        let directory = options(&["--dir", "corpus", "--iterations", "2"]).expect("valid");
        assert_eq!(
            (directory.directory, directory.iterations),
            (Some(PathBuf::from("corpus")), 2)
        );
        let defaults = options(&[]).expect("no options");
        assert_eq!(
            (defaults.seed, defaults.files, defaults.iterations),
            (DEFAULT_SEED, DEFAULT_FILES, DEFAULT_ITERATIONS)
        );
        let checked =
            options(&["--check", "--seed", "0x1f", "--files", "12"]).expect("check options");
        assert!(checked.check);
        assert_eq!((checked.seed, checked.files), (31, 12));
    }

    #[test]
    fn conflicting_modes_are_rejected() {
        for args in [
            &["--check", "--dir", "sample"][..],
            &["--check", "--print", "0"],
            &["--check", "--print-component", "0"],
            &["--check", "--print-typescript", "0"],
            &["--dir", "sample", "--print", "0"],
            &["--print", "0", "--print-component", "0"],
            &["--print-component", "0", "--print-typescript", "0"],
        ] {
            assert!(options(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn invalid_options_are_rejected() {
        for args in [
            &["--files"][..],
            &["--files", "0"],
            &["--iterations", "0"],
            &["--seed", "-1"],
            &["--seed", "0xg"],
            &["--verbose", "1"],
        ] {
            assert!(options(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn grouping_inserts_thousands_separators() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
