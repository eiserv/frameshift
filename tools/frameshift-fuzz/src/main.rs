//! Development-only coverage-guided fuzzer for the parsers, scanners,
//! character reference decoders, and span edit core (`D-044`).
//!
//! An instrumented build (`instrument.toml`) records which basic blocks each
//! input reaches. `run` supervises a worker process that mutates inputs,
//! keeps those that reach new coverage, and stops at the first contract
//! violation, crash, or hang, which it saves with a replay command. `replay`
//! runs saved inputs again in any build. The tool writes only below the run
//! directory, starts no process other than its own worker, and uses no
//! network. Generated inputs derive from fictional seeds. No product crate
//! depends on it.

mod astro;
mod collections;
mod coverage;
mod edits;
mod gitignore;
mod hashes;
mod identifiers;
mod json;
mod lexical;
mod mutate;
mod package;
mod paths;
mod record;
mod runtime;
mod seeds;
mod semver;
mod supervisor;
mod targets;
mod worker;
mod workspace;
mod yaml;
mod yarn;

use std::{
    env, fs, io,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::Duration,
};

// The Windows section markers resolve the counter bounds that instrumented
// code refers to; nothing else uses the crate.
use frameshift_fuzz_sections as _;

use crate::{
    supervisor::{ENVIRONMENT, VIOLATION},
    targets::Target,
    worker::Stop,
};

const USAGE: &str = "usage: frameshift-fuzz targets\n       \
                     frameshift-fuzz run --target <name> --dir <path> [--seconds <n>] [--executions <n>] [--seed <n>] [--max-len <n>] [--timeout <seconds>]\n       \
                     frameshift-fuzz replay --target <name> <path>...";

/// Seed of the mutation sequence unless `--seed` selects another.
const DEFAULT_SEED: u64 = 0x00E1_F022_5EED_0001;
/// Default wall-clock budget of a run, in seconds.
const DEFAULT_SECONDS: u64 = 60;
/// Default heartbeat timeout, in seconds. Executions of inputs within the
/// maximum length take milliseconds, so this leaves a wide margin.
const DEFAULT_TIMEOUT: u64 = 10;
/// Stack of the thread that executes inputs. The parser's nesting bound keeps
/// bounded recursion far below it, so an overflow points to unbounded
/// recursion rather than to the build configuration.
const STACK_SIZE: usize = 64 * 1024 * 1024;
/// Exit code of a worker whose execution thread panicked, as for a panic on
/// the main thread.
const PANIC: u8 = 101;

enum Command {
    Targets,
    Run(supervisor::Settings),
    Worker(worker::Settings),
    Replay { target: Target, paths: Vec<PathBuf> },
}

fn main() -> ExitCode {
    let command = match parse(env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("frameshift-fuzz: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(ENVIRONMENT);
        }
    };
    match command {
        Command::Targets => {
            for target in Target::ALL {
                println!("{:<11} {}", target.name(), target.summary());
            }
            ExitCode::SUCCESS
        }
        Command::Run(settings) => supervisor::run(&settings),
        Command::Worker(settings) => match on_large_stack(move || worker::run(&settings)) {
            Ok(Some(Ok(summary))) => {
                println!(
                    "budget complete without findings: {} executions, {} features, {} inputs",
                    summary.executions, summary.features, summary.corpus
                );
                ExitCode::SUCCESS
            }
            Ok(Some(Err(Stop::Violation(message)))) => {
                eprintln!("frameshift-fuzz: contract violation: {message}");
                ExitCode::from(VIOLATION)
            }
            Ok(Some(Err(Stop::Environment(message)))) => {
                eprintln!("frameshift-fuzz: {message}");
                ExitCode::from(ENVIRONMENT)
            }
            Ok(None) => ExitCode::from(PANIC),
            Err(error) => thread_error(&error),
        },
        Command::Replay { target, paths } => match on_large_stack(move || replay(target, &paths)) {
            Ok(Some(Ok(true))) => ExitCode::SUCCESS,
            Ok(Some(Ok(false))) => ExitCode::from(VIOLATION),
            Ok(Some(Err(message))) => {
                eprintln!("frameshift-fuzz: {message}");
                ExitCode::from(ENVIRONMENT)
            }
            Ok(None) => ExitCode::from(PANIC),
            Err(error) => thread_error(&error),
        },
    }
}

// Runs `work` on a thread with a large stack and returns its result, or
// `None` if it panicked. The panic message has already been printed.
fn on_large_stack<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<Option<T>> {
    let thread = thread::Builder::new()
        .name("executions".to_owned())
        .stack_size(STACK_SIZE)
        .spawn(work)?;
    Ok(thread.join().ok())
}

fn thread_error(error: &io::Error) -> ExitCode {
    eprintln!("frameshift-fuzz: cannot start the execution thread: {error}");
    ExitCode::from(ENVIRONMENT)
}

// Runs every input file again and reports each outcome. Panics are caught
// per input, so one failing input does not hide the others.
fn replay(target: Target, paths: &[PathBuf]) -> Result<bool, String> {
    let mut files = Vec::new();
    for path in paths {
        collect(path, &mut files).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let mut passed = true;
    for file in files {
        let input = fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let outcome = match panic::catch_unwind(AssertUnwindSafe(|| target.run(&input))) {
            Ok(Ok(())) => "ok".to_owned(),
            Ok(Err(violation)) => format!("contract violation: {violation}"),
            Err(_) => "panic".to_owned(),
        };
        passed &= outcome == "ok";
        println!("{}: {outcome}", file.display());
    }
    Ok(passed)
}

// Adds a file, or the files of a directory in name order, skipping the
// descriptions that the supervisor writes next to findings.
fn collect(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if !path.is_dir() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<_>>()?;
    entries.sort();
    files.extend(
        entries
            .into_iter()
            .filter(|entry| entry.is_file() && entry.extension().is_none_or(|ext| ext != "txt")),
    );
    Ok(())
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let command = args.next().ok_or("a command is required")?;
    let mut target = None;
    let mut directory = None;
    let mut seed = DEFAULT_SEED;
    let mut seconds = DEFAULT_SECONDS;
    let mut executions = None;
    let mut max_len = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let mut paths = Vec::new();
    while let Some(argument) = args.next() {
        if !argument.starts_with("--") {
            paths.push(PathBuf::from(argument));
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{argument} requires a value"))?;
        let number = || parse_number(&value).ok_or_else(|| format!("invalid number {value:?}"));
        match argument.as_str() {
            "--target" => {
                target =
                    Some(Target::parse(&value).ok_or_else(|| format!("unknown target {value:?}"))?);
            }
            "--dir" => directory = Some(PathBuf::from(value)),
            "--seed" => seed = number()?,
            "--seconds" => seconds = number()?,
            "--executions" => executions = Some(number()?),
            "--max-len" => {
                max_len = Some(usize::try_from(number()?).map_err(|_| "--max-len is too large")?);
            }
            "--timeout" => timeout = number()?,
            _ => return Err(format!("unknown option {argument:?}")),
        }
    }
    let required = |target: Option<Target>| target.ok_or("--target is required");
    let worker = |target: Target, directory: Option<PathBuf>| -> Result<worker::Settings, String> {
        let max_len = max_len.unwrap_or(target.max_len());
        if max_len == 0 || timeout == 0 {
            return Err("--max-len and --timeout must be positive".to_owned());
        }
        Ok(worker::Settings {
            target,
            directory: directory.ok_or("--dir is required")?,
            seed,
            seconds,
            executions,
            max_len,
        })
    };
    let positional = |allowed: bool| {
        if !allowed && !paths.is_empty() {
            return Err(format!("unexpected argument {:?}", paths[0]));
        }
        Ok(())
    };
    match command.as_str() {
        "targets" => {
            positional(false)?;
            Ok(Command::Targets)
        }
        "run" => {
            positional(false)?;
            Ok(Command::Run(supervisor::Settings {
                worker: worker(required(target)?, directory)?,
                timeout: Duration::from_secs(timeout),
            }))
        }
        "worker" => {
            positional(false)?;
            Ok(Command::Worker(worker(required(target)?, directory)?))
        }
        "replay" => {
            if paths.is_empty() {
                return Err("replay requires at least one input path".to_owned());
            }
            Ok(Command::Replay {
                target: required(target)?,
                paths,
            })
        }
        _ => Err(format!("unknown command {command:?}")),
    }
}

fn parse_number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn run_options_have_defaults_and_accept_hexadecimal_seeds() {
        let Ok(Command::Run(settings)) = command(&["run", "--target", "lexical", "--dir", "out"])
        else {
            panic!("run expected");
        };
        let worker = &settings.worker;
        assert_eq!(worker.target, Target::Lexical);
        assert_eq!(worker.directory, PathBuf::from("out"));
        assert_eq!(
            (
                worker.seed,
                worker.seconds,
                worker.executions,
                worker.max_len
            ),
            (DEFAULT_SEED, DEFAULT_SECONDS, None, 512)
        );
        assert_eq!(settings.timeout, Duration::from_secs(DEFAULT_TIMEOUT));
        let Ok(Command::Worker(worker)) = command(&[
            "worker",
            "--target",
            "edits",
            "--dir",
            "out",
            "--seed",
            "0x1f",
            "--seconds",
            "5",
            "--executions",
            "9",
            "--max-len",
            "16",
        ]) else {
            panic!("worker expected");
        };
        assert_eq!(
            (
                worker.seed,
                worker.seconds,
                worker.executions,
                worker.max_len
            ),
            (31, 5, Some(9), 16)
        );
        let Ok(Command::Replay { target, paths }) =
            command(&["replay", "--target", "program", "a", "b"])
        else {
            panic!("replay expected");
        };
        assert_eq!((target, paths.len()), (Target::Program, 2));
    }

    #[test]
    fn replay_runs_files_and_directories_in_name_order() {
        let directory =
            std::env::temp_dir().join(format!("frameshift-fuzz-replay-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("b"), "&amp;").unwrap();
        fs::write(directory.join("a"), "&lt;").unwrap();
        fs::write(directory.join("a.txt"), "finding description").unwrap();
        let mut files = Vec::new();
        collect(&directory, &mut files).unwrap();
        assert_eq!(files, [directory.join("a"), directory.join("b")]);
        assert_eq!(
            replay(Target::References, std::slice::from_ref(&directory)),
            Ok(true)
        );
        assert!(replay(Target::References, &[directory.join("missing")]).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn invalid_commands_are_rejected() {
        for args in [
            &[][..],
            &["fuzz"],
            &["run", "--dir", "out"],
            &["run", "--target", "lexical"],
            &["run", "--target", "parser", "--dir", "out"],
            &["run", "--target", "lexical", "--dir", "out", "extra"],
            &[
                "run",
                "--target",
                "lexical",
                "--dir",
                "out",
                "--max-len",
                "0",
            ],
            &[
                "run",
                "--target",
                "lexical",
                "--dir",
                "out",
                "--timeout",
                "0",
            ],
            &["run", "--target", "lexical", "--dir", "out", "--seed", "-1"],
            &[
                "run",
                "--target",
                "lexical",
                "--dir",
                "out",
                "--verbose",
                "1",
            ],
            &["replay", "--target", "lexical"],
            &["targets", "extra"],
        ] {
            assert!(command(args).is_err(), "{args:?}");
        }
    }
}
