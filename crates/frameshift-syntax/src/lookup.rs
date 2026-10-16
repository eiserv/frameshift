//! Hash tables restricted to lookups, so no result can depend on their order.
//!
//! Standard hash collections iterate in an unspecified order, so the workspace
//! disallows them (D-060). Early-error checks need only membership and keyed
//! access; these wrappers provide that and nothing that iterates or drains.
#![expect(
    clippy::disallowed_types,
    reason = "the parser's only hash collections; the wrappers expose no iteration (D-060)"
)]

use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet, TryReserveError, hash_map::Entry},
    hash::Hash,
};

pub(crate) struct LookupMap<K, V>(HashMap<K, V>);

impl<K, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K: Eq + Hash, V> LookupMap<K, V> {
    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.get(key)
    }

    pub(crate) fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.get_mut(key)
    }

    pub(crate) fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.0.contains_key(key)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }

    pub(crate) fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.0.entry(key)
    }

    pub(crate) fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.0.try_reserve(additional)
    }
}

pub(crate) struct LookupSet<T>(HashSet<T>);

impl<T> Default for LookupSet<T> {
    fn default() -> Self {
        Self(HashSet::new())
    }
}

impl<T: Eq + Hash> LookupSet<T> {
    // Reports whether the value was absent.
    pub(crate) fn insert(&mut self, value: T) -> bool {
        self.0.insert(value)
    }
}
