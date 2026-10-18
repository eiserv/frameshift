//! Dense identities assigned from sorted keys, and results addressed by them.
//!
//! Parallel stages store results by a precomputed identity rather than by
//! completion order. Ordered standard collections remain suitable for other
//! output; hash-based collections are reserved for lookups (ENGINEERING.md,
//! section 6).

use std::{cmp::Ordering, fmt, hash, marker::PhantomData};

/// A zero-based position in one [`KeyTable`], typed by that table's key.
///
/// Identities follow the sorted key order, never insertion, enumeration, or
/// completion order. They are local to one table: using an identity with a
/// different table or its slots is a caller error that range checks detect
/// only partially. They are not persistent document keys.
pub struct DenseId<K> {
    index: usize,
    key: PhantomData<fn() -> K>,
}

impl<K> DenseId<K> {
    const fn new(index: usize) -> Self {
        Self {
            index,
            key: PhantomData,
        }
    }

    /// Returns the zero-based position in sorted key order.
    pub const fn index(self) -> usize {
        self.index
    }
}

// Manual implementations avoid requiring these traits of the key type.

impl<K> Clone for DenseId<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for DenseId<K> {}

impl<K> PartialEq for DenseId<K> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<K> Eq for DenseId<K> {}

impl<K> PartialOrd for DenseId<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for DenseId<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.index.cmp(&other.index)
    }
}

impl<K> hash::Hash for DenseId<K> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<K> fmt::Debug for DenseId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DenseId({})", self.index)
    }
}

/// A rejected collection operation, carrying positions and counts only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionError {
    /// More keys were supplied than the caller's inclusive bound.
    TooManyEntries {
        /// Supplied key count.
        entries: usize,
        /// Maximum permitted key count.
        limit: usize,
    },
    /// Two keys compare equal under the key's total order.
    DuplicateKey {
        /// Sorted position of the second of the first equal pair.
        index: usize,
    },
    /// An identity lies outside the slots' range.
    UnknownId {
        /// The rejected position.
        index: usize,
        /// The number of slots.
        len: usize,
    },
    /// A slot already holds a result.
    OccupiedSlot {
        /// The occupied position.
        index: usize,
    },
    /// A slot holds no result at completion.
    MissingResult {
        /// The first empty position in identity order.
        index: usize,
    },
    /// Collection storage could not be reserved.
    AllocationFailed,
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyEntries { entries, limit } => {
                write!(f, "collection of {entries} entries exceeds limit {limit}")
            }
            Self::DuplicateKey { index } => write!(f, "duplicate key at sorted position {index}"),
            Self::UnknownId { index, len } => {
                write!(f, "identity {index} is outside {len} slots")
            }
            Self::OccupiedSlot { index } => write!(f, "slot {index} already holds a result"),
            Self::MissingResult { index } => write!(f, "slot {index} holds no result"),
            Self::AllocationFailed => f.write_str("collection storage allocation failed"),
        }
    }
}

impl std::error::Error for CollectionError {}

/// Unique keys in their total order, with a dense identity for each.
///
/// Identities are assigned before parallel work starts. Construction is
/// independent of the order in which keys were collected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyTable<K> {
    keys: Vec<K>,
}

impl<K: Ord> KeyTable<K> {
    /// Takes ownership of keys in any order, sorts them in place, and rejects
    /// keys that compare equal.
    ///
    /// The caller passes its subsystem bound, which is checked before sorting.
    /// Sorting performs O(n log n) comparisons without allocating. Equal keys
    /// fail closed rather than being merged, because callers derive keys from
    /// facts that must already be unique.
    pub fn new(mut keys: Vec<K>, max_entries: usize) -> Result<Self, CollectionError> {
        if keys.len() > max_entries {
            return Err(CollectionError::TooManyEntries {
                entries: keys.len(),
                limit: max_entries,
            });
        }
        keys.sort_unstable();
        if let Some(index) = keys
            .windows(2)
            .position(|pair| pair[0].cmp(&pair[1]) == Ordering::Equal)
        {
            return Err(CollectionError::DuplicateKey { index: index + 1 });
        }
        Ok(Self { keys })
    }

    /// Returns the identity of a key with a binary search.
    pub fn find(&self, key: &K) -> Option<DenseId<K>> {
        self.keys.binary_search(key).ok().map(DenseId::new)
    }
}

impl<K> KeyTable<K> {
    /// Returns the number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Reports whether the table has no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Borrows the keys in identity order.
    pub fn as_slice(&self) -> &[K] {
        &self.keys
    }

    /// Borrows the key of an identity, or `None` outside this table's range.
    pub fn get(&self, id: DenseId<K>) -> Option<&K> {
        self.keys.get(id.index)
    }

    /// Iterates identities and keys in sorted order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (DenseId<K>, &K)> {
        self.keys
            .iter()
            .enumerate()
            .map(|(index, key)| (DenseId::new(index), key))
    }

    /// Reserves one empty result slot per key, fallibly.
    pub fn slots<T>(&self) -> Result<DenseSlots<K, T>, CollectionError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(self.keys.len())
            .map_err(|_| CollectionError::AllocationFailed)?;
        slots.resize_with(self.keys.len(), || None);
        Ok(DenseSlots {
            slots,
            filled: 0,
            key: PhantomData,
        })
    }
}

/// Results addressed by dense identity, each filled exactly once.
///
/// Insertion order, such as worker completion order, cannot affect the result.
/// A second result for an identity or an identity outside the range fails
/// closed, as does completion with any slot still empty.
#[derive(Debug)]
pub struct DenseSlots<K, T> {
    slots: Vec<Option<T>>,
    filled: usize,
    key: PhantomData<fn() -> K>,
}

impl<K, T> DenseSlots<K, T> {
    /// Stores the result of an identity. On failure, every slot keeps its
    /// previous state and the rejected value is dropped.
    pub fn insert(&mut self, id: DenseId<K>, value: T) -> Result<(), CollectionError> {
        let len = self.slots.len();
        let slot = self
            .slots
            .get_mut(id.index)
            .ok_or(CollectionError::UnknownId {
                index: id.index,
                len,
            })?;
        if slot.is_some() {
            return Err(CollectionError::OccupiedSlot { index: id.index });
        }
        *slot = Some(value);
        self.filled += 1;
        Ok(())
    }

    /// Returns the number of slots.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Reports whether there are no slots.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns the number of filled slots.
    pub const fn filled(&self) -> usize {
        self.filled
    }

    /// Requires every slot to be filled and returns the results in identity
    /// order. The first empty slot is reported before any storage is reserved.
    pub fn finish(self) -> Result<DenseMap<K, T>, CollectionError> {
        if let Some(index) = self.slots.iter().position(Option::is_none) {
            return Err(CollectionError::MissingResult { index });
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.slots.len())
            .map_err(|_| CollectionError::AllocationFailed)?;
        values.extend(self.slots.into_iter().flatten());
        Ok(DenseMap {
            values,
            key: PhantomData,
        })
    }
}

/// One result for every identity of a table, in identity order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseMap<K, T> {
    values: Vec<T>,
    key: PhantomData<fn() -> K>,
}

impl<K, T> DenseMap<K, T> {
    /// Returns the number of results.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether there are no results.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Borrows the result of an identity, or `None` outside the range.
    pub fn get(&self, id: DenseId<K>) -> Option<&T> {
        self.values.get(id.index)
    }

    /// Borrows the results in identity order.
    pub fn as_slice(&self) -> &[T] {
        &self.values
    }

    /// Iterates identities and results in identity order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (DenseId<K>, &T)> {
        self.values
            .iter()
            .enumerate()
            .map(|(index, value)| (DenseId::new(index), value))
    }

    /// Returns the results in identity order.
    pub fn into_values(self) -> Vec<T> {
        self.values
    }
}
