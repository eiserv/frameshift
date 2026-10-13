//! Command-line driver of experiment E2.
//!
//! `check` compares every fixture with its golden files, `update` rewrites
//! the golden files, and `write --out <dir>` writes each generated project
//! below a new directory, for example to build it with Astro.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use frameshift_e2::{compare_with_golden, fixtures, run_fixture, tree, update_golden};

const USAGE: &str = "usage: frameshift-e2 check|update [--fixtures <dir>]\n       frameshift-e2 write --out <new dir> [--fixtures <dir>]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let mut command = None;
    let mut fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let mut out: Option<PathBuf> = None;
    let mut iter = arguments.into_iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "check" | "update" | "write" if command.is_none() => command = Some(argument),
            "--fixtures" => {
                fixtures_root = PathBuf::from(iter.next().ok_or(USAGE)?);
            }
            "--out" => out = Some(PathBuf::from(iter.next().ok_or(USAGE)?)),
            _ => return Err(USAGE.to_owned()),
        }
    }
    let command = command.ok_or(USAGE)?;
    if (command == "write") != out.is_some() {
        return Err(USAGE.to_owned());
    }
    let fixtures = fixtures(&fixtures_root)
        .map_err(|error| format!("{}: {error}", fixtures_root.display()))?;
    if let Some(out) = &out
        && out.exists()
    {
        return Err(format!("{} already exists", out.display()));
    }
    let mut failed = false;
    for fixture in fixtures {
        let name = fixture
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let run = run_fixture(&fixture)?;
        match command.as_str() {
            "check" => {
                let differences = compare_with_golden(&fixture, &run)?;
                if differences.is_empty() {
                    println!("{name}: ok ({} files)", run.migration.outputs.len());
                } else {
                    failed = true;
                    for difference in differences {
                        println!("{name}: {difference}");
                    }
                }
            }
            "update" => {
                update_golden(&fixture, &run).map_err(|error| format!("{name}: {error}"))?;
                println!("{name}: updated");
            }
            _ => {
                let target = out.as_deref().unwrap_or(Path::new(".")).join(&name);
                tree::write_tree(&target, run.files())
                    .map_err(|error| format!("{name}: {error}"))?;
                println!("{name}: written to {}", target.display());
            }
        }
    }
    if failed {
        Err("golden comparison failed".to_owned())
    } else {
        Ok(())
    }
}
