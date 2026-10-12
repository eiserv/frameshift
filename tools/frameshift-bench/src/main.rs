//! Development-only measurement harness for experiment E1 (`D-040`).
//!
//! It generates deterministic corpora of fictional JavaScript and JSX,
//! parses them through the engine facade, and reports parser throughput and peak heap
//! memory against the E1 budgets in `docs/ENGINEERING.md` (section 4.1).
//! Timing depends on the machine, and budgets are judged on the reference
//! hardware. The tool reads no files, starts no processes, and performs no
//! network access. No product crate depends on it.

mod alloc;
mod corpus;
mod measure;

use std::{env, process::ExitCode, time::Duration};

use frameshift_engine::{ParseLimits, SourceGoal};

use crate::{
    corpus::{Corpus, DEFAULT_SEED, GENERATOR_VERSION, generate_file},
    measure::{FileMemory, Timing},
};

#[global_allocator]
static ALLOCATOR: alloc::CountingAllocator = alloc::CountingAllocator::new();

/// E1 throughput budget, in decimal megabytes per second on one core.
const THROUGHPUT_BUDGET: f64 = 30.0;
/// E1 peak memory budget, as a multiple of the source size.
const MEMORY_BUDGET: f64 = 20.0;
/// Files in the default corpus, like the 1,000 modules of the `analyze`
/// budget in `docs/ENGINEERING.md`.
const DEFAULT_FILES: usize = 1_000;
const DEFAULT_ITERATIONS: usize = 5;
/// File sizes of the scaling series, in KiB.
const SCALING_KIB: [usize; 7] = [1, 4, 16, 64, 256, 1_024, 4_096];

const USAGE: &str = "usage: frameshift-bench [--seed <n>] [--files <n>] [--iterations <n>]\n       \
                     frameshift-bench --print <index> [--seed <n>] [--files <n>]\n       \
                     frameshift-bench --print-component <index> [--seed <n>] [--files <n>]";

struct Options {
    seed: u64,
    files: usize,
    iterations: usize,
    print: Option<usize>,
    print_component: Option<usize>,
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
    let corpus = Corpus::mixed(options.seed, options.files);
    let components = Corpus::components(options.seed, options.files);
    let printed = match (options.print, options.print_component) {
        (Some(index), _) => Some((&corpus, index)),
        (None, Some(index)) => Some((&components, index)),
        (None, None) => None,
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
    match run(&options, &corpus, &components) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("frameshift-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        seed: DEFAULT_SEED,
        files: DEFAULT_FILES,
        iterations: DEFAULT_ITERATIONS,
        print: None,
        print_component: None,
    };
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        let number = parse_number(&value).ok_or_else(|| format!("invalid number {value:?}"))?;
        let count = || usize::try_from(number).map_err(|_| format!("{value} is too large"));
        match flag.as_str() {
            "--seed" => options.seed = number,
            "--files" => options.files = count()?,
            "--iterations" => options.iterations = count()?,
            "--print" => options.print = Some(count()?),
            "--print-component" => options.print_component = Some(count()?),
            _ => return Err(format!("unknown option {flag:?}")),
        }
    }
    if options.files == 0 || options.iterations == 0 {
        return Err("--files and --iterations must be positive".to_owned());
    }
    Ok(options)
}

fn parse_number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn run(options: &Options, corpus: &Corpus, components: &Corpus) -> Result<(), String> {
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
    let bytes = corpus.bytes();
    let throughput = measure::throughput(&corpus.files, ParseLimits::default(), iterations)
        .map_err(|error| format!("{label}, {error}"))?;
    print_timing("text and parse", bytes, &throughput.text_and_parse, false);
    print_timing("parse", bytes, &throughput.parse, true);
    let files = measure::file_memory(&corpus.files, ParseLimits::default())
        .map_err(|error| format!("{label}, {error}"))?;
    print_file_memory(&files);
    let all = measure::corpus_memory(&corpus.files, ParseLimits::default())
        .map_err(|error| format!("{label}, {error}"))?;
    let retained = all.source + all.indexes + all.trees;
    println!(
        "  all trees retained   {} source + {} indexes + {} trees = {} ({:.1}x), peak {:.1}x",
        megabytes(all.source),
        megabytes(all.indexes),
        megabytes(all.trees),
        megabytes(retained),
        ratio(retained, all.source),
        ratio(all.peak, all.source)
    );
    Ok(())
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
    let bytes: usize = files.iter().map(|file| file.bytes).sum();
    let nodes: usize = files.iter().map(|file| file.nodes).sum();
    let comments: usize = files.iter().map(|file| file.comments).sum();
    println!(
        "  syntax               {} nodes ({:.3} per byte), {} comments",
        grouped(nodes),
        nodes as f64 / bytes as f64,
        grouped(comments)
    );
    let default_nodes = ParseLimits::default().max_nodes;
    println!(
        "  node limit           the default {} nodes are reached at about {} KB of this mix",
        grouped(default_nodes),
        grouped((default_nodes as f64 * bytes as f64 / nodes as f64 / 1_000.0) as usize)
    );
    let mut peaks: Vec<(f64, usize)> = files
        .iter()
        .enumerate()
        .map(|(index, file)| (ratio(file.peak, file.bytes), index))
        .collect();
    peaks.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let (median, _) = peaks[peaks.len() / 2];
    let (worst, index) = peaks[peaks.len() - 1];
    let verdict = if worst <= MEMORY_BUDGET {
        "meets"
    } else {
        "misses"
    };
    println!(
        "  peak per file        median {median:.1}x, max {worst:.1}x (file {index}, {} bytes)  {verdict} {MEMORY_BUDGET}x",
        grouped(files[index].bytes)
    );
    let mut retained: Vec<f64> = files
        .iter()
        .map(|file| ratio(file.retained, file.bytes))
        .collect();
    retained.sort_by(f64::total_cmp);
    println!(
        "  retained per file    median {:.1}x, max {:.1}x",
        retained[retained.len() / 2],
        retained[retained.len() - 1]
    );
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
        let defaults = options(&[]).expect("no options");
        assert_eq!(
            (defaults.seed, defaults.files, defaults.iterations),
            (DEFAULT_SEED, DEFAULT_FILES, DEFAULT_ITERATIONS)
        );
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
