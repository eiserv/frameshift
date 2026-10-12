//! Timing and heap measurements of `parse_js_program` (`D-040`).
//!
//! Heap figures include each file's source bytes, because the engine keeps
//! source text alongside its trees. Measurements run on the calling thread
//! only, so the global counters see no other allocations.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use frameshift_engine::{ParseLimits, SourceText, TextLimits, parse_js_program};

use crate::{ALLOCATOR, corpus::CorpusFile};

/// Minimum and median of repeated measurements.
pub struct Timing {
    /// The fastest run, the least disturbed by other activity.
    pub min: Duration,
    /// The upper median run.
    pub median: Duration,
}

impl Timing {
    fn from_samples(mut samples: Vec<Duration>) -> Self {
        samples.sort_unstable();
        Self {
            min: samples[0],
            median: samples[samples.len() / 2],
        }
    }
}

/// Times of all files: source text construction plus parsing, and parsing alone.
pub struct Throughput {
    /// `SourceText::new` and `parse_js_program`.
    pub text_and_parse: Timing,
    /// `parse_js_program` only.
    pub parse: Timing,
}

/// Parses every file once untimed, then `iterations` times timed. Each file's
/// tree is dropped after its timing ends, so drops are not measured.
pub fn throughput(
    files: &[CorpusFile],
    limits: ParseLimits,
    iterations: usize,
) -> Result<Throughput, String> {
    let mut totals = Vec::with_capacity(iterations);
    let mut parses = Vec::with_capacity(iterations);
    for iteration in 0..=iterations {
        let mut total = Duration::ZERO;
        let mut parse = Duration::ZERO;
        for (index, file) in files.iter().enumerate() {
            let text = file.text.clone();
            let started = Instant::now();
            let source = SourceText::new(text, TextLimits::default())
                .map_err(|error| format!("file {index}: {error:?}"))?;
            let indexed = Instant::now();
            let tree = parse_js_program(&source, file.goal, file.dialect, limits)
                .map_err(|error| format!("file {index}: {error:?}"))?;
            let parsed = Instant::now();
            black_box(&tree);
            total += parsed - started;
            parse += parsed - indexed;
        }
        if iteration > 0 {
            totals.push(total);
            parses.push(parse);
        }
    }
    Ok(Throughput {
        text_and_parse: Timing::from_samples(totals),
        parse: Timing::from_samples(parses),
    })
}

/// Heap use of one file parsed on its own.
pub struct FileMemory {
    /// Source bytes.
    pub bytes: usize,
    /// Syntax nodes in the tree.
    pub nodes: usize,
    /// Comments retained by the tree.
    pub comments: usize,
    /// Source bytes plus the largest heap growth while building the source
    /// text and parsing, with the tree still alive.
    pub peak: usize,
    /// Source bytes plus the heap held by the source text index and tree.
    pub retained: usize,
}

/// Measures each file separately, in order.
pub fn file_memory(files: &[CorpusFile], limits: ParseLimits) -> Result<Vec<FileMemory>, String> {
    let counters = ALLOCATOR.counters();
    let mut results = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        let text = file.text.clone();
        let base = counters.reset_peak();
        let source = SourceText::new(text, TextLimits::default())
            .map_err(|error| format!("file {index}: {error:?}"))?;
        let tree = parse_js_program(&source, file.goal, file.dialect, limits)
            .map_err(|error| format!("file {index}: {error:?}"))?;
        let growth = |value: usize| {
            value
                .checked_sub(base)
                .ok_or_else(|| format!("file {index}: heap fell below its baseline"))
        };
        let bytes = file.text.len();
        results.push(FileMemory {
            bytes,
            nodes: tree.nodes().len(),
            comments: tree.comments().len(),
            peak: bytes + growth(counters.peak())?,
            retained: bytes + growth(counters.current())?,
        });
    }
    Ok(results)
}

/// Heap use with every file's source text and tree alive at once, as when an
/// analysis keeps all parsed files.
pub struct CorpusMemory {
    /// Total source bytes.
    pub source: usize,
    /// Heap held by the source text indexes and their container.
    pub indexes: usize,
    /// Heap held by the trees and their container.
    pub trees: usize,
    /// Source bytes plus the largest heap growth during the whole pass.
    pub peak: usize,
}

/// Builds all source texts, then parses all files while keeping every tree.
pub fn corpus_memory(files: &[CorpusFile], limits: ParseLimits) -> Result<CorpusMemory, String> {
    let counters = ALLOCATOR.counters();
    let texts: Vec<String> = files.iter().map(|file| file.text.clone()).collect();
    let source: usize = texts.iter().map(String::len).sum();
    let base = counters.reset_peak();
    let mut sources = Vec::with_capacity(texts.len());
    for (index, text) in texts.into_iter().enumerate() {
        sources.push(
            SourceText::new(text, TextLimits::default())
                .map_err(|error| format!("file {index}: {error:?}"))?,
        );
    }
    let after_indexes = counters.current();
    let mut trees = Vec::with_capacity(sources.len());
    for (index, (text, file)) in sources.iter().zip(files).enumerate() {
        trees.push(
            parse_js_program(text, file.goal, file.dialect, limits)
                .map_err(|error| format!("file {index}: {error:?}"))?,
        );
    }
    let after_trees = counters.current();
    black_box(&trees);
    let below = || "heap fell below its baseline".to_owned();
    Ok(CorpusMemory {
        source,
        indexes: after_indexes.checked_sub(base).ok_or_else(below)?,
        trees: after_trees.checked_sub(after_indexes).ok_or_else(below)?,
        peak: source + counters.peak().checked_sub(base).ok_or_else(below)?,
    })
}
