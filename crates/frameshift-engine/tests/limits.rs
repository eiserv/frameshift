//! Shared resource limit contracts through the public engine facade.

use std::collections::BTreeSet;

use frameshift_engine::foundation::{
    Budget, FeatureId, HashLimits, IdentifierLimits, PathLimits, RunLimits, SourcePath, UnitId,
    UnitKind,
};
use frameshift_engine::limits::{Limits, LimitsError};

const ALGORITHM_MAX: u64 = (1 << 61) - 1;

#[test]
fn documented_defaults_are_consistent() {
    assert_eq!(
        RunLimits::default(),
        RunLimits {
            max_files: 100_000,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            max_directory_depth: 255,
            max_entries: 1_000_000,
        }
    );
    let limits = Limits::default();
    assert_eq!(limits.validate(), Ok(()));
    assert_eq!(limits.run, RunLimits::default());
    assert_eq!(limits.path, PathLimits::default());
    let entries = limits.entries();
    let value = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.value)
    };
    assert_eq!(value("run.max-files"), Some(100_000));
    assert_eq!(value("run.max-entries"), Some(1_000_000));
    assert_eq!(value("ignore.max-total-bytes"), Some(4 * 1024 * 1024));
    assert_eq!(value("ignore.max-match-work"), Some(1 << 34));
    assert_eq!(value("text.max-source-bytes"), Some(16 * 1024 * 1024));
    assert_eq!(value("path.max-components"), Some(256));
    assert_eq!(value("hash.max-bytes"), Some(ALGORITHM_MAX));
    assert_eq!(value("parse.lexical.max-comments"), Some(100_000));
    assert_eq!(value("parse.recovery.max-diagnostics"), Some(256));
    assert_eq!(value("parse.recovery.max-work"), Some(1_048_576));
    assert_eq!(value("emit.max-depth"), Some(256));
    assert_eq!(value("sass.max-nodes"), Some(1_000_000));
    assert_eq!(value("sass.max-depth"), Some(128));
    assert_eq!(value("sass.max-work"), Some(16_777_216));
    assert_eq!(value("css.max-tokens"), Some(4_194_304));
    assert_eq!(value("css.parser.max-depth"), Some(128));
    assert_eq!(value("css.parser.max-diagnostics"), Some(256));
    assert_eq!(value("css.parser.max-nodes"), Some(4_194_304));
    assert_eq!(value("css.parser.max-work"), Some(16_777_216));
    assert_eq!(value("json.max-depth"), Some(256));
    assert_eq!(value("json.max-nodes"), Some(4_194_304));
    assert_eq!(value("yaml.max-depth"), Some(128));
    assert_eq!(value("yaml.max-nodes"), Some(4_194_304));
    assert_eq!(value("yarn.max-depth"), Some(64));
    assert_eq!(value("yarn.max-nodes"), Some(4_194_304));
    assert_eq!(value("package.max-lockfile-bytes"), Some(64 * 1024 * 1024));
    assert_eq!(value("workspace.max-patterns"), Some(1024));
    assert_eq!(value("workspace.max-pattern-bytes"), Some(4096));
    assert_eq!(value("workspace.max-alternatives"), Some(256));
    assert_eq!(value("workspace.max-match-work"), Some(100_000_000));
    let package = limits.package();
    assert_eq!(package.text, limits.text);
    assert_eq!(package.json, limits.json);
    assert_eq!(package.yaml, limits.yaml);
    assert_eq!(package.yarn, limits.yarn);
    assert_eq!(package.path, limits.path);
    assert_eq!(package.package, limits.package);
    assert_eq!(package.workspace, limits.workspace);
}

#[test]
fn entries_are_sorted_named_and_complete() {
    let entries = Limits::default().entries();
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].name.as_bytes() < pair[1].name.as_bytes())
    );
    for entry in entries {
        FeatureId::new(entry.name, IdentifierLimits::default())
            .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
    }

    // Distinct values per field prove that every field has exactly one entry.
    let mut limits = Limits::default();
    let mut next = 0;
    let mut value = || {
        next += 1;
        next
    };
    limits.run.max_files = value();
    limits.run.max_file_bytes = value() as u64;
    limits.run.max_total_bytes = value() as u64;
    limits.run.max_directory_depth = value();
    limits.run.max_entries = value();
    limits.text.max_source_bytes = value();
    limits.path.max_path_bytes = value();
    limits.path.max_components = value();
    limits.identifier.max_bytes = value();
    limits.finding.max_text_bytes = value();
    limits.finding.max_findings = value();
    limits.hash.max_bytes = value() as u64;
    limits.parse.max_nodes = value();
    limits.parse.max_tokens = value();
    limits.parse.max_nesting_depth = value();
    limits.parse.lexical.max_comments = value();
    limits.parse.lexical.max_string_bytes = value();
    limits.parse.lexical.max_number_bytes = value();
    limits.parse.lexical.max_template_bytes = value();
    limits.parse.lexical.max_identifier_bytes = value();
    limits.parse.lexical.max_regexp_bytes = value();
    limits.parse.lexical.max_jsx_text_bytes = value();
    limits.edit.max_edits = value();
    limits.edit.max_output_bytes = value();
    limits.emit.max_output_bytes = value();
    limits.emit.max_depth = value();
    limits.ignore.max_total_bytes = value() as u64;
    limits.ignore.max_match_work = value() as u64;
    limits.json.max_depth = value();
    limits.json.max_nodes = value();
    limits.package.max_lockfile_bytes = value() as u64;
    limits.yaml.max_depth = value();
    limits.yaml.max_nodes = value();
    limits.yarn.max_depth = value();
    limits.yarn.max_nodes = value();
    limits.workspace.max_patterns = value();
    limits.workspace.max_pattern_bytes = value();
    limits.workspace.max_alternatives = value();
    limits.workspace.max_match_work = value() as u64;
    limits.css.max_tokens = value();
    limits.css_parser.max_nodes = value();
    limits.css_parser.max_depth = value();
    limits.css_parser.max_diagnostics = value();
    limits.css_parser.max_work = value();
    limits.sass.max_nodes = value();
    limits.sass.max_depth = value();
    limits.sass.max_work = value();
    limits.recovery.max_diagnostics = value();
    limits.recovery.max_work = value();
    limits.modules.max_files = value();
    limits.modules.max_edges = value();
    limits.modules.max_config_depth = value();
    limits.modules.max_work = value() as u64;
    let values: BTreeSet<u64> = limits.entries().iter().map(|entry| entry.value).collect();
    assert_eq!(values, (1..=53).collect());
    let entries = limits.entries();
    let named = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
    assert_eq!(named("run.max-files").value, 1);
    assert_eq!(named("run.max-entries").value, 5);
    assert_eq!(named("parse.lexical.max-jsx-text-bytes").value, 22);
    assert_eq!(named("emit.max-depth").value, 26);
    assert_eq!(named("ignore.max-match-work").value, 28);
    assert_eq!(named("json.max-nodes").value, 30);
    assert_eq!(named("package.max-lockfile-bytes").value, 31);
    assert_eq!(named("yarn.max-nodes").value, 35);
    assert_eq!(named("workspace.max-match-work").value, 39);
    assert_eq!(named("css.max-tokens").value, 40);
    assert_eq!(limits.entries(), entries);
}

#[test]
fn each_relation_has_an_inclusive_boundary() {
    let base = Limits::default();

    let mut limits = base;
    limits.run.max_total_bytes = limits.run.max_file_bytes;
    assert_eq!(limits.validate(), Ok(()));
    limits.run.max_total_bytes -= 1;
    assert_eq!(limits.validate(), Err(LimitsError::FileExceedsTotal));

    let mut limits = base;
    limits.run.max_directory_depth = limits.path.max_components - 1;
    assert_eq!(limits.validate(), Ok(()));
    limits.run.max_directory_depth += 1;
    assert_eq!(
        limits.validate(),
        Err(LimitsError::DepthExceedsPathComponents)
    );

    let mut limits = base;
    limits.hash.max_bytes = limits.run.max_file_bytes;
    assert_eq!(limits.validate(), Ok(()));
    limits.hash.max_bytes -= 1;
    assert_eq!(limits.validate(), Err(LimitsError::FileExceedsHash));

    let mut limits = base;
    limits.identifier.max_bytes = UnitId::max_file_id_bytes(limits.path.max_path_bytes);
    assert_eq!(limits.validate(), Ok(()));
    limits.identifier.max_bytes -= 1;
    assert_eq!(limits.validate(), Err(LimitsError::PathExceedsUnitId));

    let mut limits = base;
    limits.edit.max_output_bytes = limits.text.max_source_bytes;
    assert_eq!(limits.validate(), Ok(()));
    limits.edit.max_output_bytes -= 1;
    assert_eq!(limits.validate(), Err(LimitsError::TextExceedsEditOutput));

    let mut limits = base;
    limits.run.max_entries = limits.run.max_files;
    assert_eq!(limits.validate(), Ok(()));
    limits.run.max_entries -= 1;
    assert_eq!(limits.validate(), Err(LimitsError::FilesExceedEntries));
}

#[test]
fn relations_use_the_effective_hash_bound_and_report_in_fixed_order() {
    let mut limits = Limits::default();
    limits.run.max_total_bytes = u64::MAX;
    limits.hash.max_bytes = u64::MAX;
    limits.run.max_file_bytes = ALGORITHM_MAX;
    assert_eq!(limits.validate(), Ok(()));
    limits.run.max_file_bytes = ALGORITHM_MAX + 1;
    assert_eq!(limits.validate(), Err(LimitsError::FileExceedsHash));

    let mut limits = Limits::default();
    limits.edit.max_output_bytes = 0;
    limits.identifier.max_bytes = 0;
    limits.hash.max_bytes = 0;
    limits.run.max_directory_depth = usize::MAX;
    limits.run.max_total_bytes = 0;
    limits.run.max_entries = 0;
    let mut reported = Vec::new();
    while let Err(error) = limits.validate() {
        reported.push(error);
        match error {
            LimitsError::FileExceedsTotal => limits.run.max_total_bytes = u64::MAX,
            LimitsError::DepthExceedsPathComponents => limits.run.max_directory_depth = 0,
            LimitsError::FileExceedsHash => limits.hash.max_bytes = u64::MAX,
            LimitsError::PathExceedsUnitId => limits.identifier.max_bytes = usize::MAX,
            LimitsError::TextExceedsEditOutput => limits.edit.max_output_bytes = usize::MAX,
            LimitsError::FilesExceedEntries => limits.run.max_entries = usize::MAX,
        }
    }
    assert_eq!(
        reported,
        [
            LimitsError::FileExceedsTotal,
            LimitsError::DepthExceedsPathComponents,
            LimitsError::FileExceedsHash,
            LimitsError::PathExceedsUnitId,
            LimitsError::TextExceedsEditOutput,
            LimitsError::FilesExceedEntries,
        ]
    );
    for error in reported {
        assert!(error.to_string().is_ascii());
    }

    assert_eq!(HashLimits::default().effective_max_bytes(), ALGORITHM_MAX);
    assert_eq!(HashLimits { max_bytes: 7 }.effective_max_bytes(), 7);
    assert_eq!(
        HashLimits {
            max_bytes: u64::MAX
        }
        .effective_max_bytes(),
        ALGORITHM_MAX
    );
}

#[test]
fn file_unit_identity_bound_is_attained_by_a_non_ascii_path() {
    let limits = PathLimits::default();
    let longest = "é".repeat(limits.max_path_bytes / 2);
    let path = SourcePath::new(&longest, limits).unwrap();
    assert_eq!(path.as_str().len(), limits.max_path_bytes);
    let bound = UnitId::max_file_id_bytes(limits.max_path_bytes);
    for kind in [
        UnitKind::Layout,
        UnitKind::Provider,
        UnitKind::Component,
        UnitKind::RouteFile,
        UnitKind::Module,
        UnitKind::Stylesheet,
    ] {
        let id = UnitId::for_file(kind, &path, None, IdentifierLimits::default()).unwrap();
        assert!(id.as_str().len() <= bound, "{kind:?}");
        if kind == UnitKind::RouteFile || kind == UnitKind::Stylesheet {
            assert_eq!(id.as_str().len(), bound);
        }
    }
    assert_eq!(UnitId::max_file_id_bytes(0), "route-file:".len());
    assert_eq!(UnitId::max_file_id_bytes(usize::MAX), usize::MAX);
    let tight = IdentifierLimits { max_bytes: bound };
    assert!(UnitId::for_file(UnitKind::RouteFile, &path, None, tight).is_ok());
    let below = IdentifierLimits {
        max_bytes: bound - 1,
    };
    assert!(UnitId::for_file(UnitKind::RouteFile, &path, None, below).is_err());
}

#[test]
fn budgets_are_inclusive_and_atomic() {
    let mut budget = Budget::new(10);
    assert_eq!(
        (budget.used(), budget.limit(), budget.remaining()),
        (0, 10, 10)
    );
    budget.charge(4).unwrap();
    budget.charge(6).unwrap();
    assert_eq!((budget.used(), budget.remaining()), (10, 0));
    budget.charge(0).unwrap();
    let error = budget.charge(1).unwrap_err();
    assert_eq!(error.limit(), 10);
    assert_eq!(error.to_string(), "cumulative amount exceeds limit 10");
    assert_eq!(budget.used(), 10);

    let mut budget = Budget::new(u64::MAX);
    budget.charge(u64::MAX - 1).unwrap();
    assert!(budget.charge(2).is_err());
    assert_eq!(budget.used(), u64::MAX - 1);
    budget.charge(1).unwrap();
    assert_eq!(budget.remaining(), 0);

    let mut budget = Budget::new(5);
    budget.charge(3).unwrap();
    assert!(budget.charge(u64::MAX).is_err());
    assert_eq!(budget.used(), 3);
    let mut empty = Budget::new(0);
    empty.charge(0).unwrap();
    assert!(empty.charge(1).is_err());
}

#[test]
fn charge_order_decides_which_input_exceeds_a_budget() {
    // Owners charge in canonical order; the same sizes in another order would
    // reject a different input.
    let sizes = [("a", 4), ("b", 5), ("c", 3)];
    let admitted = |order: &[usize]| {
        let mut budget = Budget::new(8);
        order
            .iter()
            .filter(|&&index| budget.charge(sizes[index].1).is_ok())
            .map(|&index| sizes[index].0)
            .collect::<Vec<_>>()
    };
    assert_eq!(admitted(&[0, 1, 2]), ["a", "c"]);
    assert_eq!(admitted(&[1, 2, 0]), ["b", "c"]);
}
