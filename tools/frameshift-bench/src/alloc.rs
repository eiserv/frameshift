//! Counting global allocator; the only module of this tool with unsafe code (`D-040`).
//!
//! It forwards every request unchanged to the system allocator and counts the
//! requested bytes of live allocations. Allocator overhead, fragmentation, and
//! the transient old block of a moving reallocation are not observed, so the
//! figures are heap demand, not resident memory.

#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Live and peak requested heap bytes.
///
/// Relaxed atomics suffice: measurements run on one thread, and the counters
/// never order other memory accesses.
pub struct Counters {
    current: AtomicUsize,
    peak: AtomicUsize,
}

impl Counters {
    const fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }
    }

    fn grow(&self, bytes: usize) {
        let current = self.current.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.peak.fetch_max(current, Ordering::Relaxed);
    }

    fn shrink(&self, bytes: usize) {
        self.current.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Returns the requested bytes of all live allocations.
    pub fn current(&self) -> usize {
        self.current.load(Ordering::Relaxed)
    }

    /// Returns the largest value of [`Counters::current`] since the last reset.
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    /// Lowers the peak to the current value and returns that value, which is
    /// the baseline for the next measurement.
    pub fn reset_peak(&self) -> usize {
        let current = self.current();
        self.peak.store(current, Ordering::Relaxed);
        current
    }
}

/// A global allocator that counts requested bytes and delegates to [`System`].
pub struct CountingAllocator {
    counters: Counters,
}

impl CountingAllocator {
    /// Creates an allocator with zeroed counters.
    pub const fn new() -> Self {
        Self {
            counters: Counters::new(),
        }
    }

    /// Returns the counters of this allocator.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }
}

// SAFETY: Every method passes its arguments unchanged to `System`, which
// implements the `GlobalAlloc` contract, and returns its result unchanged.
// Counting only reads sizes from layouts and never touches the memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller upholds `alloc`'s contract for `layout`.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            self.counters.grow(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller upholds `alloc_zeroed`'s contract for `layout`.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            self.counters.grow(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The caller guarantees that `pointer` was allocated by this
        // allocator, and so by `System`, with `layout`.
        unsafe { System.dealloc(pointer, layout) };
        self.counters.shrink(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The caller upholds `realloc`'s contract for `pointer`,
        // `layout`, and `new_size`, and the block came from `System`.
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        // A failed reallocation leaves the original block in place.
        if !moved.is_null() {
            match new_size.checked_sub(layout.size()) {
                Some(growth) => self.counters.grow(growth),
                None => self.counters.shrink(layout.size() - new_size),
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_track_live_bytes_and_the_peak_since_reset() {
        let counters = Counters::new();
        counters.grow(100);
        counters.grow(50);
        counters.shrink(120);
        assert_eq!((counters.current(), counters.peak()), (30, 150));
        assert_eq!(counters.reset_peak(), 30);
        assert_eq!(counters.peak(), 30);
        counters.grow(10);
        counters.shrink(40);
        assert_eq!((counters.current(), counters.peak()), (0, 40));
    }

    // A local instance keeps the result independent of tests running
    // concurrently on the process-wide allocator.
    #[test]
    fn allocator_counts_requested_sizes_through_reallocation() {
        let allocator = CountingAllocator::new();
        let layout = Layout::from_size_align(64, 8).expect("valid layout");
        // SAFETY: `layout` has a non-zero size, every pointer is checked for
        // null before use, and each block is released with its current layout.
        unsafe {
            let pointer = allocator.alloc(layout);
            assert!(!pointer.is_null());
            pointer.write_bytes(0xA5, 64);
            let grown = allocator.realloc(pointer, layout, 200);
            assert!(!grown.is_null());
            assert_eq!(grown.read(), 0xA5);
            assert_eq!(allocator.counters().current(), 200);
            let grown_layout = Layout::from_size_align(200, 8).expect("valid layout");
            let shrunk = allocator.realloc(grown, grown_layout, 16);
            assert!(!shrunk.is_null());
            assert_eq!(allocator.counters().current(), 16);
            let zeroed = allocator.alloc_zeroed(layout);
            assert!(!zeroed.is_null());
            assert_eq!(zeroed.read(), 0);
            assert_eq!(allocator.counters().current(), 80);
            allocator.dealloc(zeroed, layout);
            let shrunk_layout = Layout::from_size_align(16, 8).expect("valid layout");
            allocator.dealloc(shrunk, shrunk_layout);
        }
        assert_eq!(allocator.counters().current(), 0);
        assert_eq!(allocator.counters().peak(), 200);
    }
}
