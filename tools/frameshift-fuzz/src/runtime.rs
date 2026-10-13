//! SanitizerCoverage runtime; with `frameshift-fuzz-sections`, the only
//! unsafe code of the fuzzing tool (`D-044`).
//!
//! In an instrumented build (`instrument.toml`), every basic block of the
//! crates in this binary increments its own 8-bit counter, and module
//! constructors report the bounds of the counter section through
//! [`__sanitizer_cov_8bit_counters_init`] before `main` runs. This module
//! records the bounds and lets the worker clear the counters before an
//! execution and copy them afterwards. In an uninstrumented build, nothing
//! reports bounds, and [`Counters::get`] returns `None`.

#![allow(unsafe_code)]

use std::{
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

static START: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static END: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static CONFLICT: AtomicBool = AtomicBool::new(false);

/// Receives the bounds of the counter section from an instrumented module
/// constructor.
///
/// Every module of a binary reports the same merged section, so the first
/// nonempty bounds are kept, and different bounds disable the counters
/// rather than leaving the runtime to guess which ones are complete.
// SAFETY: The SanitizerCoverage interface names this callback, no other code
// in the program defines the symbol, and the function never dereferences its
// arguments.
#[unsafe(no_mangle)]
pub extern "C" fn __sanitizer_cov_8bit_counters_init(start: *mut u8, end: *mut u8) {
    if start.is_null() || end <= start {
        return;
    }
    match START.compare_exchange(ptr::null_mut(), start, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => END.store(end, Ordering::SeqCst),
        Err(previous) => {
            if previous != start || END.load(Ordering::SeqCst) != end {
                CONFLICT.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// The coverage counters of an instrumented binary.
///
/// The raw pointer makes this type neither `Send` nor `Sync`, so the
/// counters stay on the thread that obtained them.
pub struct Counters {
    start: *mut u8,
    len: usize,
}

impl Counters {
    /// Returns the registered counters, or `None` in an uninstrumented build
    /// or after conflicting registrations.
    ///
    /// Only the worker obtains the counters. It runs every execution on the
    /// thread that holds them and starts no other thread.
    pub fn get() -> Option<Self> {
        let start = START.load(Ordering::SeqCst);
        let end = END.load(Ordering::SeqCst);
        if start.is_null() || CONFLICT.load(Ordering::SeqCst) {
            return None;
        }
        // Registration accepted only bounds with `end > start`.
        Some(Self {
            start,
            len: end.addr() - start.addr(),
        })
    }

    /// Returns the number of counters, one per instrumented basic block.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Sets every counter to zero.
    pub fn clear(&self) {
        // SAFETY: `start` and `len` are the bounds that the module
        // constructors reported for the counter section, which the linker
        // allocates as writable static memory for the whole run, so the range
        // is valid for writes of `len` bytes. No Rust reference to it exists.
        // Instrumented code increments the counters with plain loads and
        // stores on the thread that runs it. This type is neither `Send` nor
        // `Sync`, and the worker starts no other thread, so the increments
        // are sequenced with this write and never overlap it.
        unsafe { ptr::write_bytes(self.start, 0, self.len) };
    }

    /// Copies every counter into `into`, replacing its contents.
    pub fn copy_to(&self, into: &mut Vec<u8>) {
        if into.len() != self.len {
            into.clear();
            into.resize(self.len, 0);
        }
        // SAFETY: The counter range is valid for reads of `len` bytes for the
        // reasons given in `clear`, and the increments are sequenced with this
        // read in the same way. `into` holds exactly `len` initialized bytes
        // in its own allocation, which cannot overlap the static counters.
        unsafe { ptr::copy_nonoverlapping(self.start, into.as_mut_ptr(), self.len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests are not instrumented, so no module constructor registers
    // counters, and the worker must refuse to run.
    #[test]
    fn uninstrumented_builds_have_no_counters() {
        assert!(Counters::get().is_none());
    }
}
