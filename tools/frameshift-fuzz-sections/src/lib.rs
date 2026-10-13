//! Section markers that bound the fuzzer's coverage counters on Windows;
//! with the fuzzer's runtime module, the only unsafe code of the fuzzing
//! tool (`D-044`).
//!
//! An instrumented build places the 8-bit counters of every function in the
//! COFF section `.SCOV$CM` and refers to `__start___sancov_cntrs` and
//! `__stop___sancov_cntrs`, which compiler-rt would otherwise define. The
//! linker orders grouped sections by the suffix after `$`, so markers in
//! `.SCOV$CA` and `.SCOV$CZ` enclose the counters, and LLVM starts reading
//! eight bytes after the start marker. LLVM renames its own references when
//! the module it instruments defines these names, so the markers live in
//! this crate, which has no functions and is therefore never instrumented.
//! ELF and Mach-O linkers define such markers themselves, so the crate is
//! empty on other platforms. Only `frameshift-fuzz` depends on it.

#![no_std]
#![cfg_attr(windows, allow(unsafe_code, non_upper_case_globals))]

#[cfg(windows)]
use core::sync::atomic::AtomicU64;

// The markers are atomics because the section they open or close holds the
// counters, which instrumented code writes. An immutable static would give
// the grouped section read-only attributes.

/// Precedes the coverage counters. LLVM starts reading eight bytes, the size
/// of this marker, after its address.
#[cfg(windows)]
// SAFETY: The fuzzer, the only crate that links this one, defines no other
// symbol with this name, and the section only orders the marker before the
// counters.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOV$CA")]
pub static __start___sancov_cntrs: AtomicU64 = AtomicU64::new(0);

/// Follows the coverage counters. LLVM stops reading at its address.
#[cfg(windows)]
// SAFETY: As for the start marker, this symbol has no other definition, and
// the section only orders the marker after the counters.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".SCOV$CZ")]
pub static __stop___sancov_cntrs: AtomicU64 = AtomicU64::new(0);
