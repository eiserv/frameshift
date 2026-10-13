//! The E2 fixtures match their golden files on every platform.

use std::path::Path;

use frameshift_e2::{compare_with_golden, fixtures, run_fixture};

#[test]
fn every_fixture_matches_its_golden_files() {
    // Cargo runs integration tests in the package directory.
    let fixtures = fixtures(Path::new("fixtures")).unwrap();
    assert_eq!(fixtures.len(), 3);
    for fixture in fixtures {
        let run = run_fixture(&fixture).unwrap();
        let differences = compare_with_golden(&fixture, &run).unwrap();
        assert!(
            differences.is_empty(),
            "{}: {differences:?}",
            fixture.display()
        );
    }
}
