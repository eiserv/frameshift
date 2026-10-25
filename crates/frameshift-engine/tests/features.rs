//! Feature catalog contracts (`D-099`): the catalog agrees with the
//! compatibility matrix.

use frameshift_engine::catalog::{self, FAMILIES, FEATURES, Status, V1};
use frameshift_engine::foundation::{FeatureId, IdentifierLimits};

fn statuses(cell: &str) -> V1 {
    match cell {
        "Out of scope" => V1::OutOfScope,
        _ => {
            let mut statuses: Vec<Status> = cell
                .split([',', ' '])
                .filter(|word| !word.is_empty() && *word != "or")
                .map(|word| match word {
                    "A" => Status::Auto,
                    "R" => Status::Review,
                    "U" => Status::Unsupported,
                    "Unknown" => Status::Unknown,
                    other => panic!("unexpected V1 cell word {other:?}"),
                })
                .collect();
            statuses.sort();
            V1::Assessed(Box::leak(statuses.into_boxed_slice()))
        }
    }
}

/// Every row of every table with the matrix header, as (ID, V1) pairs.
fn matrix() -> Vec<(String, V1)> {
    let mut rows = Vec::new();
    let mut table = false;
    for line in include_str!("../../../docs/MIGRATION_SPEC.md").lines() {
        if line == "| Feature | Feature ID | V1 | Notes |" {
            table = true;
            continue;
        }
        if !line.starts_with('|') {
            table = false;
        }
        if table && !line.starts_with("| ---") {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for id in cells[2].split('`').skip(1).step_by(2) {
                rows.push((id.to_owned(), statuses(cells[3])));
            }
        }
    }
    rows
}

#[test]
fn catalog_and_matrix_agree() {
    let rows = matrix();
    let mut concrete = Vec::new();
    let mut families = Vec::new();
    for (id, v1) in &rows {
        if let Some(prefix) = id.strip_suffix(".*") {
            let family = FAMILIES
                .iter()
                .find(|family| family.prefix == prefix)
                .unwrap_or_else(|| panic!("family {id} is not in the catalog"));
            assert_eq!(family.v1, *v1, "{id}");
            families.push(prefix.to_owned());
        } else {
            let feature =
                catalog::feature(id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(feature.v1, *v1, "{id}");
            concrete.push(id.clone());
        }
    }
    concrete.sort();
    let before = concrete.len();
    concrete.dedup();
    assert_eq!(before, concrete.len(), "a feature ID is listed twice");
    let catalog: Vec<&str> = FEATURES.iter().map(|feature| feature.id).collect();
    assert_eq!(
        concrete, catalog,
        "the matrix and the catalog list other IDs"
    );
    families.sort();
    let catalog: Vec<&str> = FAMILIES.iter().map(|family| family.prefix).collect();
    assert_eq!(families, catalog);
    for feature in FEATURES {
        assert_eq!(
            FeatureId::new(feature.id, IdentifierLimits::default())
                .unwrap()
                .as_str(),
            feature.id
        );
    }
    assert_eq!(catalog::PATH, "nextjs-to-astro-preact");
}
