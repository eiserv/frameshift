//! The fuzzing loop of the worker process (`D-044`).
//!
//! The worker executes the target's seeds and saved corpus inputs, then
//! mutates corpus inputs until its budget ends. Each execution clears the
//! coverage counters, runs the target, and copies the counters, so only the
//! code that the input reaches contributes features. An input that reaches
//! a new feature joins the corpus and is saved. Before each execution, the
//! input is recorded (`record.rs`) so the supervisor can recover it after a
//! crash or hang. One thread runs everything, so a seed and an execution
//! budget reproduce a run exactly.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    coverage::Features, mutate::Mutator, record::Record, runtime::Counters, targets::Target,
};

/// Printed when the binary has no coverage counters.
const NOT_INSTRUMENTED: &str = "no coverage counters are registered; build with --config tools/frameshift-fuzz/instrument.toml and without RUSTFLAGS";

/// Interval between status lines, which also serve the supervisor as
/// heartbeats.
const STATUS_INTERVAL: Duration = Duration::from_secs(1);

/// What one worker run does.
pub struct Settings {
    /// The target to fuzz.
    pub target: Target,
    /// The run directory, which holds one subdirectory per target.
    pub directory: PathBuf,
    /// Seed of the mutation sequence.
    pub seed: u64,
    /// Wall-clock budget of the single-threaded worker, in seconds.
    pub seconds: u64,
    /// Optional execution budget, which makes a run reproducible.
    pub executions: Option<u64>,
    /// Maximum length of mutated inputs.
    pub max_len: usize,
}

/// Why a worker stopped before its budget ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// The target reported a violated contract.
    Violation(String),
    /// The worker could not run: no instrumentation or an I/O failure.
    Environment(String),
}

/// Counts at the end of a completed budget.
#[derive(Debug, PartialEq, Eq)]
pub struct Summary {
    /// Executions, including those of seeds and saved inputs.
    pub executions: u64,
    /// Coverage features reached.
    pub features: usize,
    /// Inputs in the corpus.
    pub corpus: usize,
}

/// The files of one target in a run directory.
pub struct Paths {
    /// Saved interesting inputs, named by their fingerprint.
    pub corpus: PathBuf,
    /// Inputs that caused crashes, hangs, or violations.
    pub findings: PathBuf,
    /// The record of the input being executed.
    pub current: PathBuf,
    /// The slowest input of the last completed run.
    pub slowest: PathBuf,
}

impl Paths {
    /// Returns the paths of `target` below `directory`.
    pub fn new(directory: &Path, target: Target) -> Self {
        let root = directory.join(target.name());
        Self {
            corpus: root.join("corpus"),
            findings: root.join("findings"),
            current: root.join("current"),
            slowest: root.join("slowest"),
        }
    }
}

/// The coverage counters that one execution reaches.
pub trait Coverage {
    /// Returns the number of counters.
    fn len(&self) -> usize;
    /// Sets every counter to zero.
    fn clear(&self);
    /// Copies the counters into `into`.
    fn copy_to(&self, into: &mut Vec<u8>);
}

impl Coverage for Counters {
    fn len(&self) -> usize {
        Counters::len(self)
    }

    fn clear(&self) {
        Counters::clear(self);
    }

    fn copy_to(&self, into: &mut Vec<u8>) {
        Counters::copy_to(self, into);
    }
}

/// Runs the worker with the instrumented binary's counters, writing status
/// lines to standard output.
pub fn run(settings: &Settings) -> Result<Summary, Stop> {
    let counters = Counters::get().ok_or_else(|| Stop::Environment(NOT_INSTRUMENTED.to_owned()))?;
    let target = settings.target;
    fuzz(
        settings,
        &counters,
        |input| target.run(input),
        &mut io::stdout(),
    )
}

/// Runs the fuzzing loop with any coverage source and target function.
pub fn fuzz(
    settings: &Settings,
    coverage: &impl Coverage,
    run: impl Fn(&[u8]) -> Result<(), String>,
    status: &mut impl Write,
) -> Result<Summary, Stop> {
    let paths = Paths::new(&settings.directory, settings.target);
    let environment = |error: io::Error| Stop::Environment(error.to_string());
    fs::create_dir_all(&paths.corpus).map_err(environment)?;
    let mut loop_state = Loop {
        coverage,
        run,
        record: Record::create(&paths.current).map_err(environment)?,
        features: Features::new(coverage.len()),
        counters: Vec::new(),
        executions: 0,
        slowest: (Duration::ZERO, Vec::new()),
        started: Instant::now(),
        next_status: STATUS_INTERVAL,
    };

    let mut corpus = settings.target.seeds();
    corpus.extend(saved_inputs(&paths.corpus).map_err(environment)?);
    if corpus.is_empty() {
        corpus.push(Vec::new());
    }
    let _ = writeln!(
        status,
        "coverage: {} counters; {} seed and saved inputs",
        coverage.len(),
        corpus.len()
    );
    for input in &corpus {
        loop_state.execute(input)?;
        loop_state.status(corpus.len(), status, false);
    }

    let mut mutator = Mutator::new(settings.seed, settings.max_len);
    let dictionary = settings.target.dictionary();
    let mut input = Vec::new();
    let budget = Duration::from_secs(settings.seconds);
    while loop_state.started.elapsed() < budget
        && settings
            .executions
            .is_none_or(|limit| loop_state.executions < limit)
    {
        let base = &corpus[mutator.below(corpus.len())];
        let other = &corpus[mutator.below(corpus.len())];
        mutator.mutate(base, other, dictionary, &mut input);
        if loop_state.execute(&input)? > 0 {
            let path = paths.corpus.join(format!("{:016x}", fingerprint(&input)));
            if !path.exists() {
                fs::write(&path, &input).map_err(environment)?;
            }
            corpus.push(input.clone());
        }
        loop_state.status(corpus.len(), status, false);
    }
    fs::write(&paths.slowest, &loop_state.slowest.1).map_err(environment)?;
    loop_state.status(corpus.len(), status, true);
    Ok(Summary {
        executions: loop_state.executions,
        features: loop_state.features.count(),
        corpus: corpus.len(),
    })
}

struct Loop<'a, C, R> {
    coverage: &'a C,
    run: R,
    record: Record,
    features: Features,
    counters: Vec<u8>,
    executions: u64,
    slowest: (Duration, Vec<u8>),
    started: Instant,
    next_status: Duration,
}

impl<C: Coverage, R: Fn(&[u8]) -> Result<(), String>> Loop<'_, C, R> {
    // Executes one input and returns the number of new features.
    fn execute(&mut self, input: &[u8]) -> Result<usize, Stop> {
        self.record
            .write(input)
            .map_err(|error| Stop::Environment(error.to_string()))?;
        // Only the code between clearing and copying, which the input
        // determines, contributes features.
        self.coverage.clear();
        let started = Instant::now();
        let outcome = (self.run)(input);
        let elapsed = started.elapsed();
        self.coverage.copy_to(&mut self.counters);
        outcome.map_err(Stop::Violation)?;
        self.executions += 1;
        if elapsed > self.slowest.0 {
            self.slowest = (elapsed, input.to_vec());
        }
        Ok(self.features.merge(&self.counters))
    }

    fn status(&mut self, corpus: usize, out: &mut impl Write, last: bool) {
        let elapsed = self.started.elapsed();
        if !last && elapsed < self.next_status {
            return;
        }
        self.next_status = elapsed + STATUS_INTERVAL;
        let rate = self.executions as f64 / elapsed.as_secs_f64().max(1e-9);
        // A failed status write is not a finding; the supervisor notices a
        // missing heartbeat.
        let _ = writeln!(
            out,
            "{}{} executions, {} features, {} inputs, {rate:.0}/s, slowest {:.1} ms, {:.0} s",
            if last { "done: " } else { "" },
            self.executions,
            self.features.count(),
            corpus,
            self.slowest.0.as_secs_f64() * 1e3,
            elapsed.as_secs_f64()
        );
        let _ = out.flush();
    }
}

/// Reads the saved inputs of a corpus directory in file name order.
fn saved_inputs(directory: &Path) -> io::Result<Vec<Vec<u8>>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<_>>()?;
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.is_file())
        .map(fs::read)
        .collect()
}

/// Returns the 64-bit FNV-1a hash of `bytes`, which names saved inputs and
/// findings. It is not a security hash.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    // Simulates instrumented counters: each distinct input byte value hits
    // its own counter.
    struct ByteCoverage {
        counters: RefCell<Vec<u8>>,
        last: RefCell<Vec<u8>>,
    }

    impl Coverage for ByteCoverage {
        fn len(&self) -> usize {
            256
        }

        fn clear(&self) {
            self.counters.borrow_mut().fill(0);
        }

        fn copy_to(&self, into: &mut Vec<u8>) {
            let mut counters = self.counters.borrow_mut();
            for &byte in self.last.borrow().iter() {
                counters[usize::from(byte)] = counters[usize::from(byte)].saturating_add(1);
            }
            into.clone_from(&counters);
        }
    }

    fn settings(directory: &Path, executions: u64) -> Settings {
        Settings {
            target: Target::References,
            directory: directory.to_path_buf(),
            seed: 3,
            seconds: 600,
            executions: Some(executions),
            max_len: 64,
        }
    }

    fn fuzz_with(
        directory: &Path,
        executions: u64,
        check: impl Fn(&[u8]) -> Result<(), String>,
    ) -> Result<Summary, Stop> {
        let coverage = ByteCoverage {
            counters: RefCell::new(vec![0; 256]),
            last: RefCell::new(Vec::new()),
        };
        let run = |input: &[u8]| {
            coverage.last.replace(input.to_vec());
            check(input)
        };
        fuzz(
            &settings(directory, executions),
            &coverage,
            run,
            &mut Vec::new(),
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("frameshift-fuzz-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn runs_are_reproducible_and_save_new_coverage() {
        let first_directory = scratch("first");
        let second_directory = scratch("second");
        let first = fuzz_with(&first_directory, 2_000, |_| Ok(())).unwrap();
        let second = fuzz_with(&second_directory, 2_000, |_| Ok(())).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.executions, 2_000);
        assert!(first.corpus > Target::References.seeds().len());
        let saved = |directory: &Path| {
            let mut names: Vec<_> = fs::read_dir(Paths::new(directory, Target::References).corpus)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        assert_eq!(saved(&first_directory), saved(&second_directory));
        // A second run starts from the saved corpus and reaches its features
        // before mutating.
        let resumed = fuzz_with(&first_directory, 1, |_| Ok(())).unwrap();
        assert!(resumed.features >= first.features);
        fs::remove_dir_all(&first_directory).unwrap();
        fs::remove_dir_all(&second_directory).unwrap();
    }

    #[test]
    fn violations_stop_the_run_with_the_input_recorded() {
        let directory = scratch("violation");
        let stop = fuzz_with(&directory, 100_000, |input| {
            if input.contains(&b'!') {
                Err("found an exclamation mark".to_owned())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(
            stop,
            Stop::Violation("found an exclamation mark".to_owned())
        );
        let current = Paths::new(&directory, Target::References).current;
        let recorded = crate::record::read(&current).unwrap().unwrap();
        assert!(recorded.contains(&b'!'));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn fingerprints_follow_fnv_1a() {
        assert_eq!(fingerprint(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fingerprint(b"a"), 0xAF63_DC4C_8601_EC8C);
    }
}
