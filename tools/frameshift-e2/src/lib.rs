//! Development-only driver of experiment E2: edit-based rewriting and Astro
//! printing applied end to end to fictional Next.js fixtures.
//!
//! Each fixture directory holds a `source/` project, the golden `expected/`
//! Astro project, and the golden `provenance.txt` listing. Checking migrates
//! the source twice, verifies determinism, provenance, and comment
//! preservation, and compares every output byte with the golden files.

pub mod migrate;
pub mod tree;

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use frameshift_engine::{RegionOrigin, SourceText, TextLimits};

use migrate::{Migration, SourceFile, migrate};

/// Version of the `provenance.txt` format.
pub const PROVENANCE_FORMAT: u32 = 1;

/// The result of migrating one fixture.
pub struct FixtureRun {
    /// Source files, sorted by path.
    pub sources: Vec<SourceFile>,
    /// The migration.
    pub migration: Migration,
    /// The rendered provenance listing.
    pub provenance: String,
}

impl FixtureRun {
    /// Returns the output files as paths and bytes, sorted by path.
    pub fn files(&self) -> Vec<(&str, &[u8])> {
        self.migration
            .outputs
            .iter()
            .map(|output| (output.path.as_str(), output.emitted.as_str().as_bytes()))
            .collect()
    }
}

/// Migrates the `source/` project of the fixture at `fixture` twice and
/// fails unless both runs agree byte for byte.
pub fn run_fixture(fixture: &Path) -> Result<FixtureRun, String> {
    let first = run_once(fixture)?;
    let second = run_once(fixture)?;
    if first.files() != second.files() || first.provenance != second.provenance {
        return Err(format!("{}: repeated runs differ", fixture.display()));
    }
    Ok(first)
}

fn run_once(fixture: &Path) -> Result<FixtureRun, String> {
    let source_root = fixture.join("source");
    let files = tree::read_tree(&source_root)
        .map_err(|error| format!("{}: {error}", source_root.display()))?;
    let mut sources = Vec::new();
    for (path, bytes) in files {
        let text = SourceText::from_bytes(bytes, TextLimits::default())
            .map_err(|error| format!("{path}: {error}"))?;
        sources.push(SourceFile { path, text });
    }
    let migration = migrate(&sources).map_err(|error| error.to_string())?;
    let provenance = render_provenance(&sources, &migration);
    Ok(FixtureRun {
        sources,
        migration,
        provenance,
    })
}

/// Renders the provenance listing: the accounting of every source file, the
/// regions of every output file with their origins, and the assumptions.
/// Offsets are byte offsets (`D-029`).
pub fn render_provenance(sources: &[SourceFile], migration: &Migration) -> String {
    let path = |source: frameshift_engine::SourceId| {
        sources
            .get(source.0 as usize)
            .map_or("?", |file| file.path.as_str())
    };
    let mut out = String::new();
    let _ = writeln!(out, "frameshift-e2 provenance {PROVENANCE_FORMAT}");
    let _ = writeln!(out, "profile {}", migrate::PROFILE);
    for account in &migration.accounts {
        let _ = write!(
            out,
            "source {} {}",
            account.source,
            account.disposition.id()
        );
        for output in &account.outputs {
            let _ = write!(out, " {output}");
        }
        out.push('\n');
    }
    for output in &migration.outputs {
        let _ = writeln!(out, "output {}", output.path);
        for region in output.emitted.regions() {
            let (origin, anchor) = match region.origin {
                RegionOrigin::Copied(anchor) => ("copied", anchor),
                RegionOrigin::Generated(anchor) => ("generated", anchor),
            };
            let _ = writeln!(
                out,
                "  {}..{} {origin} {} {}..{}",
                region.output.start(),
                region.output.end(),
                path(anchor.source),
                anchor.span.start(),
                anchor.span.end()
            );
        }
    }
    for recorded in &migration.assumptions {
        let anchor = recorded.assumption.anchor;
        let _ = writeln!(
            out,
            "assumption {} {} {} {}..{}",
            recorded.output,
            recorded.assumption.kind.id(),
            path(anchor.source),
            anchor.span.start(),
            anchor.span.end()
        );
    }
    out
}

/// Compares a fixture run with the golden files and returns every difference.
pub fn compare_with_golden(fixture: &Path, run: &FixtureRun) -> Result<Vec<String>, String> {
    let expected_root = fixture.join("expected");
    let expected = tree::read_tree(&expected_root)
        .map_err(|error| format!("{}: {error}", expected_root.display()))?;
    let mut differences = Vec::new();
    let actual = run.files();
    for (path, bytes) in &actual {
        match expected
            .iter()
            .find(|(expected_path, _)| expected_path == path)
        {
            Some((_, expected_bytes)) if expected_bytes.as_slice() == *bytes => {}
            Some(_) => differences.push(format!("{path}: differs from the golden file")),
            None => differences.push(format!("{path}: no golden file")),
        }
    }
    for (path, _) in &expected {
        if !actual.iter().any(|(actual_path, _)| actual_path == path) {
            differences.push(format!("{path}: golden file not produced"));
        }
    }
    let provenance_path = fixture.join("provenance.txt");
    match fs::read(&provenance_path) {
        Ok(bytes) if bytes == run.provenance.as_bytes() => {}
        Ok(_) => differences.push("provenance.txt: differs from the golden file".to_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            differences.push("provenance.txt: no golden file".to_owned());
        }
        Err(error) => return Err(format!("{}: {error}", provenance_path.display())),
    }
    Ok(differences)
}

/// Replaces the golden files of a fixture with the run's outputs.
pub fn update_golden(fixture: &Path, run: &FixtureRun) -> io::Result<()> {
    let expected_root = fixture.join("expected");
    if expected_root.exists() {
        fs::remove_dir_all(&expected_root)?;
    }
    tree::write_tree(&expected_root, run.files())?;
    fs::write(fixture.join("provenance.txt"), &run.provenance)
}

/// Lists the fixture directories below `root`, sorted by name.
pub fn fixtures(root: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut fixtures = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if fs::symlink_metadata(entry.path())?.is_dir() {
            fixtures.push(entry.path());
        }
    }
    fixtures.sort();
    Ok(fixtures)
}
