//! Coverage features from 8-bit counters (`D-044`).
//!
//! As in libFuzzer, the hit count of a counter in one execution falls into
//! one of eight buckets (1, 2, 3, 4 to 7, 8 to 15, 16 to 31, 32 to 127, and
//! 128 or more), and each pair of counter and bucket is a feature. An input
//! is interesting when it reaches a feature that no earlier input reached.
//! Counters wrap at 256, which can hide a bucket; that weakens the guidance
//! but never the checks.

/// Returns the bucket bit of a hit count, or zero for no hits.
pub const fn bucket(count: u8) -> u8 {
    match count {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        4..=7 => 8,
        8..=15 => 16,
        16..=31 => 32,
        32..=127 => 64,
        128..=u8::MAX => 128,
    }
}

/// The bucket bit of every hit count, so merging needs no branches per
/// counter.
const BUCKETS: [u8; 256] = {
    let mut buckets = [0; 256];
    let mut count = 0;
    while count < 256 {
        buckets[count] = bucket(count as u8);
        count += 1;
    }
    buckets
};

/// The features reached by all executions so far.
pub struct Features {
    seen: Vec<u8>,
    count: usize,
}

impl Features {
    /// Creates an empty feature set for `counters` counters.
    pub fn new(counters: usize) -> Self {
        Self {
            seen: vec![0; counters],
            count: 0,
        }
    }

    /// Adds the features of one execution's counters and returns how many
    /// of them are new. Counters beyond the size of the set are ignored.
    pub fn merge(&mut self, counters: &[u8]) -> usize {
        let mut new = 0;
        // Words of eight counters are compared at once: most counters stay
        // zero, and each nonzero one sets exactly one bucket bit, so the new
        // bits of a word count its new features.
        for (counters, seen) in counters.chunks(8).zip(self.seen.chunks_mut(8)) {
            let length = counters.len().min(seen.len());
            let mut word = [0; 8];
            word[..length].copy_from_slice(&counters[..length]);
            if u64::from_ne_bytes(word) == 0 {
                continue;
            }
            let buckets = u64::from_ne_bytes(word.map(|count| BUCKETS[usize::from(count)]));
            let mut known = [0; 8];
            known[..length].copy_from_slice(&seen[..length]);
            let known = u64::from_ne_bytes(known);
            let fresh = buckets & !known;
            if fresh != 0 {
                new += fresh.count_ones() as usize;
                seen[..length].copy_from_slice(&(known | buckets).to_ne_bytes()[..length]);
            }
        }
        self.count += new;
        new
    }

    /// Returns the number of features reached so far.
    pub fn count(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_counts_fall_into_eight_buckets() {
        let buckets: Vec<u8> = [0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 127, 128, 255]
            .into_iter()
            .map(bucket)
            .collect();
        assert_eq!(
            buckets,
            [0, 1, 2, 4, 8, 8, 16, 16, 32, 32, 64, 64, 128, 128]
        );
    }

    #[test]
    fn merging_counts_each_counter_and_bucket_once() {
        let mut features = Features::new(11);
        let mut counters = [0; 11];
        assert_eq!(features.merge(&counters), 0);
        counters[0] = 1;
        counters[10] = 5;
        assert_eq!(features.merge(&counters), 2);
        assert_eq!(features.merge(&counters), 0);
        counters[0] = 6;
        counters[10] = 7;
        assert_eq!(features.merge(&counters), 1);
        counters[3] = 200;
        assert_eq!(features.merge(&counters), 1);
        assert_eq!(features.count(), 4);
        // A longer snapshot does not reach beyond the set.
        assert_eq!(features.merge(&[0; 20]), 0);
        let mut longer = [0; 20];
        longer[15] = 1;
        assert_eq!(features.merge(&longer), 0);
    }

    // The word-wise merge agrees with a counter-by-counter reference.
    #[test]
    fn merging_agrees_with_a_reference() {
        let mut rng = frameshift_corpus::Rng::new(5);
        let mut features = Features::new(1_003);
        let mut reference = vec![0_u8; 1_003];
        for _ in 0..200 {
            let counters: Vec<u8> = (0..1_003)
                .map(|_| match rng.below(4) {
                    0 => rng.below(256) as u8,
                    _ => 0,
                })
                .collect();
            let mut expected = 0;
            for (seen, &count) in reference.iter_mut().zip(&counters) {
                let bit = bucket(count);
                if *seen & bit != bit {
                    *seen |= bit;
                    expected += 1;
                }
            }
            assert_eq!(features.merge(&counters), expected);
        }
        assert_eq!(features.seen, reference);
    }
}
