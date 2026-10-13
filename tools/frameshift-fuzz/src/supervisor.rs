//! The supervising process of a fuzzing run (`D-044`).
//!
//! `frameshift-fuzz run` starts this executable again as a worker process
//! and watches it. The worker's status lines are its heartbeats. When the
//! worker exits with a violation, terminates abnormally (a panic, a stack
//! overflow, an abort, or exhausted memory), or sends no heartbeat within the
//! timeout, the supervisor stops it if needed and saves the recorded input
//! as a finding with a description. It starts no other process, and it
//! writes only below the run directory.

use std::{
    env, fs,
    io::{self, BufRead, BufReader},
    path::Path,
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use crate::{
    record,
    targets::Target,
    worker::{self, Paths},
};

/// Worker exit code for a contract violation.
pub const VIOLATION: u8 = 1;
/// Worker and supervisor exit code for usage and environment errors.
pub const ENVIRONMENT: u8 = 2;

/// Interval between checks for the exit of a worker that closed its output.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What one supervised run does.
pub struct Settings {
    /// The worker's settings.
    pub worker: worker::Settings,
    /// The longest interval without a heartbeat before the worker counts as
    /// hanging.
    pub timeout: Duration,
}

/// Supervises one worker and returns the exit code of the run.
pub fn run(settings: &Settings) -> ExitCode {
    match supervise(settings) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("frameshift-fuzz: {error}");
            ExitCode::from(ENVIRONMENT)
        }
    }
}

enum Ending {
    Exited(ExitStatus),
    Hung,
}

fn supervise(settings: &Settings) -> io::Result<u8> {
    let worker = &settings.worker;
    let target = worker.target;
    let paths = Paths::new(&worker.directory, target);
    fs::create_dir_all(&paths.corpus)?;
    fs::create_dir_all(&paths.findings)?;
    match fs::remove_file(&paths.current) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    println!("{}: {}", target.name(), target.summary());
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("worker")
        .args(["--target", target.name()])
        .arg("--dir")
        .arg(&worker.directory)
        .args(["--seed", &worker.seed.to_string()])
        .args(["--seconds", &worker.seconds.to_string()])
        .args(["--max-len", &worker.max_len.to_string()]);
    if let Some(executions) = worker.executions {
        command.args(["--executions", &executions.to_string()]);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let ending = watch(&mut child, target, settings.timeout)?;
    let kind = match ending {
        Ending::Exited(status) if status.success() => return Ok(0),
        Ending::Exited(status) if status.code() == Some(i32::from(ENVIRONMENT)) => {
            eprintln!("frameshift-fuzz: the worker could not run ({status})");
            return Ok(ENVIRONMENT);
        }
        Ending::Exited(status) if status.code() == Some(i32::from(VIOLATION)) => {
            ("violation", status.to_string())
        }
        Ending::Exited(status) => ("crash", status.to_string()),
        Ending::Hung => (
            "hang",
            format!("no heartbeat for {} s", settings.timeout.as_secs()),
        ),
    };
    save_finding(settings, &paths, kind)?;
    Ok(VIOLATION)
}

// Forwards the worker's status lines until it exits or stops sending them.
fn watch(child: &mut Child, target: Target, timeout: Duration) -> io::Result<Ending> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("the worker has no standard output"))?;
    let (sender, receiver) = mpsc::channel();
    // The reader stops at the end of the output or at the first read error.
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let failed = line.is_err();
            if sender.send(line).is_err() || failed {
                break;
            }
        }
    });
    loop {
        match receiver.recv_timeout(timeout) {
            Ok(Ok(line)) => println!("{}: {line}", target.name()),
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => return stop(child),
        }
    }
    // Without heartbeats, the worker must exit within the timeout.
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            return Ok(Ending::Exited(status));
        }
        thread::sleep(POLL_INTERVAL);
    }
    stop(child)
}

fn stop(child: &mut Child) -> io::Result<Ending> {
    child.kill()?;
    child.wait()?;
    Ok(Ending::Hung)
}

fn save_finding(
    settings: &Settings,
    paths: &Paths,
    (kind, cause): (&str, String),
) -> io::Result<()> {
    let target = settings.worker.target;
    let Some(input) = record::read(&paths.current).unwrap_or(None) else {
        println!(
            "{}: {kind} ({cause}) before any input was recorded",
            target.name()
        );
        return Ok(());
    };
    let name = format!("{kind}-{:016x}", worker::fingerprint(&input));
    let path = paths.findings.join(&name);
    fs::write(&path, &input)?;
    let description = describe(settings, kind, &cause, &path);
    fs::write(paths.findings.join(format!("{name}.txt")), &description)?;
    print!("{description}");
    Ok(())
}

fn describe(settings: &Settings, kind: &str, cause: &str, path: &Path) -> String {
    let worker = &settings.worker;
    format!(
        "finding: {kind} in target {} ({cause})\ninput: {} ({} bytes)\nworker seed: {:#x}\nreplay: cargo run -p frameshift-fuzz --release --offline --locked -- replay --target {} {}\n",
        worker.target.name(),
        path.display(),
        fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        worker.seed,
        worker.target.name(),
        path.display()
    )
}
