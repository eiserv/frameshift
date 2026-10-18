//! Deterministic collection and budget contracts against independent models.
//!
//! The first input byte selects bounds; the remaining bytes become keys,
//! slot operations, and budget charges.

use std::fmt::Debug;

use frameshift_engine::foundation::{Budget, CollectionError, KeyTable};

/// Checks key tables, dense slots, and budgets against plain models.
pub fn run(input: &[u8]) -> Result<(), String> {
    let (control, data) = input
        .split_first()
        .map_or((0, &[][..]), |(&c, rest)| (c, rest));
    let limit = usize::from(control);
    table(data.to_vec(), limit).map_err(|error| format!("byte keys: {error}"))?;
    let words = data
        .split(|&byte| byte == b'/')
        .map(<[u8]>::to_vec)
        .collect();
    table(words, limit).map_err(|error| format!("byte-string keys: {error}"))?;
    slots(data, usize::from(control % 32))?;
    budget(data, control)
}

fn table<K: Ord + Clone + Debug>(keys: Vec<K>, limit: usize) -> Result<(), String> {
    let mut sorted = keys.clone();
    sorted.sort();
    let expected = if keys.len() > limit {
        Err(CollectionError::TooManyEntries {
            entries: keys.len(),
            limit,
        })
    } else if let Some(index) = (1..sorted.len()).find(|&i| sorted[i - 1] == sorted[i]) {
        Err(CollectionError::DuplicateKey { index })
    } else {
        Ok(sorted)
    };
    let mut reversed = keys.clone();
    reversed.reverse();
    let forward = KeyTable::new(keys, limit);
    if KeyTable::new(reversed, limit) != forward {
        return Err("input order changed the table or its error".to_owned());
    }
    match (forward, expected) {
        (Ok(table), Ok(sorted)) => {
            if table.as_slice() != sorted.as_slice() || table.len() != sorted.len() {
                return Err("keys are not in their total order".to_owned());
            }
            for (position, (id, key)) in table.iter().enumerate() {
                if id.index() != position
                    || table.find(key) != Some(id)
                    || table.get(id) != Some(key)
                {
                    return Err(format!("identity {position} does not round-trip"));
                }
            }
            Ok(())
        }
        (Err(actual), Err(model)) if actual == model => Ok(()),
        (actual, model) => Err(format!(
            "table returned {:?}, model expected {:?}",
            actual.map(|table| table.len()),
            model.map(|sorted| sorted.len())
        )),
    }
}

// Each byte inserts at `byte % (len + 1)`; `len` stands for an identity of a
// larger table. The model is a vector of optional values.
fn slots(data: &[u8], len: usize) -> Result<(), String> {
    let small = KeyTable::new((0..len).collect(), len).map_err(|e| e.to_string())?;
    let large = KeyTable::new((0..=len).collect(), len + 1).map_err(|e| e.to_string())?;
    let mut slots = small.slots::<usize>().map_err(|e| e.to_string())?;
    let mut model: Vec<Option<usize>> = vec![None; len];
    let mut accepted = Vec::new();
    for (value, &byte) in data.iter().enumerate() {
        let index = usize::from(byte) % (len + 1);
        let id = large
            .iter()
            .nth(index)
            .map(|(id, _)| id)
            .ok_or("missing id")?;
        let expected = if index == len {
            Err(CollectionError::UnknownId { index, len })
        } else if model[index].is_some() {
            Err(CollectionError::OccupiedSlot { index })
        } else {
            model[index] = Some(value);
            accepted.push((index, value));
            Ok(())
        };
        if slots.insert(id, value) != expected {
            return Err(format!(
                "insertion {value} at {index} disagrees with the model"
            ));
        }
        if slots.filled() != model.iter().flatten().count() || slots.len() != len {
            return Err("a slot count disagrees with the model".to_owned());
        }
    }
    let finished = slots.finish();
    match model.iter().position(Option::is_none) {
        Some(index) => {
            if finished != Err(CollectionError::MissingResult { index }) {
                return Err(format!("completion did not report missing slot {index}"));
            }
        }
        None => {
            let map = finished.map_err(|e| e.to_string())?;
            let values: Vec<usize> = model.into_iter().flatten().collect();
            if map.as_slice() != values.as_slice() {
                return Err("completed results are not in identity order".to_owned());
            }
            let mut again = small.slots().map_err(|e| e.to_string())?;
            for &(index, value) in accepted.iter().rev() {
                let id = small
                    .iter()
                    .nth(index)
                    .map(|(id, _)| id)
                    .ok_or("missing id")?;
                again.insert(id, value).map_err(|e| e.to_string())?;
            }
            if again.finish().map_err(|e| e.to_string())? != map {
                return Err("reversed insertion changed the results".to_owned());
            }
        }
    }
    Ok(())
}

// Charges are big-endian byte pairs; a pair starting with 0xff charges
// u64::MAX. The model accumulates in u128, where no charge can overflow.
fn budget(data: &[u8], control: u8) -> Result<(), String> {
    let limit = if control & 0x80 == 0 {
        u64::from(control) * 512
    } else {
        u64::MAX - u64::from(control & 0x7f)
    };
    let mut budget = Budget::new(limit);
    let mut used: u128 = 0;
    for pair in data.chunks(2) {
        let amount = match pair {
            [0xff, ..] => u64::MAX,
            [high, low] => u64::from(u16::from_be_bytes([*high, *low])),
            [single] => u64::from(*single),
            _ => 0,
        };
        let fits = used + u128::from(amount) <= u128::from(limit);
        let result = budget.charge(amount);
        if result.is_ok() != fits || result.is_err_and(|error| error.limit() != limit) {
            return Err(format!("charge of {amount} disagrees with the model"));
        }
        if fits {
            used += u128::from(amount);
        }
        if u128::from(budget.used()) != used
            || u128::from(budget.remaining()) != u128::from(limit) - used
            || budget.limit() != limit
        {
            return Err("budget accounting disagrees with the model".to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn generated_inputs_preserve_contracts() {
        for control in [0, 1, 3, 31, 32, 64, 127, 128, 200, 255] {
            for length in 0..=48 {
                let mut input = vec![control];
                input.extend((0..length).map(|i| (i * 37 % 11) as u8));
                run(&input).unwrap();
                input.truncate(1);
                input.extend((0..length).map(|i| i as u8));
                run(&input).unwrap();
            }
        }
        for target in crate::targets::Target::Collections.seeds() {
            run(&target).unwrap();
        }
        run(&[255; 4096]).unwrap();
        run(&[]).unwrap();
    }
}
