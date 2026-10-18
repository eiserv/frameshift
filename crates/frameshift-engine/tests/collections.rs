//! Deterministic collection contracts through the public engine facade.

use std::{sync::mpsc, thread};

use frameshift_engine::foundation::{
    CollectionError, DenseId, DenseMap, KeyTable, PathLimits, SourcePath,
};

fn path(value: &str) -> SourcePath {
    SourcePath::new(value, PathLimits::default()).unwrap()
}

fn paths(values: &[&str]) -> Vec<SourcePath> {
    values.iter().map(|value| path(value)).collect()
}

fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for (index, &first) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(index);
        for mut tail in permutations(&rest) {
            tail.insert(0, first);
            result.push(tail);
        }
    }
    result
}

const NAMES: [&str; 5] = [
    "src/a.ts",
    "src/A.ts",
    "é.ts",
    "e\u{301}.ts",
    "app/page.tsx",
];

#[test]
fn identities_follow_byte_order_for_every_input_order() {
    let mut expected = NAMES.to_vec();
    expected.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    for order in permutations(&[0, 1, 2, 3, 4]) {
        let keys = order.iter().map(|&index| path(NAMES[index])).collect();
        let table = KeyTable::new(keys, NAMES.len()).unwrap();
        let sorted: Vec<_> = table.as_slice().iter().map(SourcePath::as_str).collect();
        assert_eq!(sorted, expected);
        for (position, (id, key)) in table.iter().enumerate() {
            assert_eq!(id.index(), position);
            assert_eq!(table.get(id), Some(key));
            assert_eq!(table.find(key), Some(id));
        }
    }
    let table = KeyTable::new(paths(&NAMES), NAMES.len()).unwrap();
    assert_eq!(table.len(), 5);
    assert!(!table.is_empty());
    assert_eq!(table.find(&path("src/b.ts")), None);
    let empty = KeyTable::<SourcePath>::new(Vec::new(), 0).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.iter().len(), 0);
}

#[test]
fn bounds_precede_duplicate_detection_and_duplicates_fail_closed() {
    assert!(KeyTable::new(paths(&NAMES), 5).is_ok());
    assert_eq!(
        KeyTable::new(paths(&NAMES), 4),
        Err(CollectionError::TooManyEntries {
            entries: 5,
            limit: 4
        })
    );
    assert_eq!(
        KeyTable::new(paths(&["b", "a", "b"]), 2),
        Err(CollectionError::TooManyEntries {
            entries: 3,
            limit: 2
        })
    );
    // The reported position refers to sorted order, never to input order.
    assert_eq!(
        KeyTable::new(paths(&["c", "b", "a", "c", "b"]), 5),
        Err(CollectionError::DuplicateKey { index: 2 })
    );
    assert_eq!(
        KeyTable::new(vec![7, 7], 2),
        Err(CollectionError::DuplicateKey { index: 1 })
    );
}

#[test]
fn results_are_independent_of_insertion_order() {
    let table = KeyTable::new(paths(&NAMES), NAMES.len()).unwrap();
    let mut outcomes = Vec::new();
    for order in permutations(&[0, 1, 2, 3, 4]) {
        let mut slots = table.slots::<String>().unwrap();
        assert_eq!(slots.len(), 5);
        for (filled, &position) in order.iter().enumerate() {
            let (id, key) = table.iter().nth(position).unwrap();
            assert_eq!(slots.filled(), filled);
            slots.insert(id, key.as_str().to_uppercase()).unwrap();
        }
        outcomes.push(slots.finish().unwrap());
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
    let map = &outcomes[0];
    assert_eq!(map.len(), 5);
    for (id, key) in table.iter() {
        assert_eq!(map.get(id).unwrap(), &key.as_str().to_uppercase());
    }
    let values: Vec<_> = map.iter().map(|(id, value)| (id.index(), value)).collect();
    assert_eq!(values.len(), 5);
    assert!(
        values
            .iter()
            .enumerate()
            .all(|(index, (id, _))| index == *id)
    );
    assert_eq!(map.as_slice().len(), map.clone().into_values().len());
}

#[test]
fn worker_completion_order_cannot_change_results() {
    let names: Vec<String> = (0..64).rev().map(|n| format!("src/{n:02}.ts")).collect();
    let keys = names.iter().map(|name| path(name)).collect();
    let table = KeyTable::new(keys, names.len()).unwrap();
    let expected: Vec<usize> = table
        .as_slice()
        .iter()
        .map(|key| key.as_str().len())
        .collect();
    for _ in 0..4 {
        let (sender, receiver) = mpsc::channel();
        thread::scope(|scope| {
            for worker in 0..4 {
                let sender = sender.clone();
                let table = &table;
                scope.spawn(move || {
                    for (id, key) in table.iter().skip(worker).step_by(4) {
                        sender.send((id, key.as_str().len())).unwrap();
                    }
                });
            }
        });
        drop(sender);
        let mut slots = table.slots().unwrap();
        for (id, value) in receiver {
            slots.insert(id, value).unwrap();
        }
        assert_eq!(slots.finish().unwrap().into_values(), expected);
    }
}

#[test]
fn rejected_insertions_preserve_slots_and_missing_results_fail() {
    let small = KeyTable::new(vec![1, 2], 2).unwrap();
    let large = KeyTable::new(vec![1, 2, 3], 3).unwrap();
    let foreign: DenseId<i32> = large.find(&3).unwrap();
    let mut slots = small.slots::<&str>().unwrap();
    assert_eq!(
        slots.insert(foreign, "x"),
        Err(CollectionError::UnknownId { index: 2, len: 2 })
    );
    let first = small.find(&1).unwrap();
    slots.insert(first, "kept").unwrap();
    assert_eq!(
        slots.insert(first, "replaced"),
        Err(CollectionError::OccupiedSlot { index: 0 })
    );
    assert_eq!(slots.filled(), 1);
    assert_eq!(
        slots.finish(),
        Err(CollectionError::MissingResult { index: 1 })
    );

    let mut slots = small.slots::<&str>().unwrap();
    slots.insert(small.find(&2).unwrap(), "second").unwrap();
    assert_eq!(
        slots.finish(),
        Err(CollectionError::MissingResult { index: 0 })
    );

    let mut slots = small.slots().unwrap();
    slots.insert(small.find(&2).unwrap(), "second").unwrap();
    slots.insert(first, "kept").unwrap();
    let map: DenseMap<i32, &str> = slots.finish().unwrap();
    assert_eq!(map.as_slice(), ["kept", "second"]);
    assert_eq!(map.get(foreign), None);
    assert_eq!(small.get(foreign), None);

    let empty = KeyTable::<i32>::new(Vec::new(), 0).unwrap();
    let slots = empty.slots::<u8>().unwrap();
    assert!(slots.is_empty());
    assert!(slots.finish().unwrap().is_empty());
}

#[test]
fn identities_need_no_key_traits_and_errors_carry_no_keys() {
    fn assert_traits<T: Copy + Ord + std::hash::Hash + std::fmt::Debug + Send + Sync>() {}
    struct Opaque;
    assert_traits::<DenseId<Opaque>>();

    let secret = "private-name-123";
    let error = KeyTable::new(paths(&[secret, secret]), 2).unwrap_err();
    let table = KeyTable::new(paths(&[secret]), 1).unwrap();
    let id = table.find(&path(secret)).unwrap();
    assert_eq!(format!("{id:?}"), "DenseId(0)");
    for text in [
        error.to_string(),
        format!("{error:?}"),
        CollectionError::TooManyEntries {
            entries: 2,
            limit: 1,
        }
        .to_string(),
        CollectionError::UnknownId { index: 3, len: 2 }.to_string(),
        CollectionError::OccupiedSlot { index: 1 }.to_string(),
        CollectionError::MissingResult { index: 1 }.to_string(),
        CollectionError::AllocationFailed.to_string(),
    ] {
        assert!(!text.contains(secret), "{text}");
        assert!(text.is_ascii(), "{text}");
    }
}
