//! Run-wide input limits and atomic cumulative budgets.

use std::fmt;

/// Inclusive, deterministic bounds on the source input of one run.
///
/// Limits count sizes and entries, never time, so results do not depend on
/// machine speed. Future discovery and snapshot code applies them in canonical
/// path order; an input beyond a limit must become an explicit finding, never
/// silent truncation. These values perform no I/O themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunLimits {
    /// Maximum admitted source files. Defaults to 100,000.
    pub max_files: usize,
    /// Maximum bytes of one admitted file. Defaults to 64 MiB.
    pub max_file_bytes: u64,
    /// Maximum cumulative bytes of all admitted files. Defaults to 1 GiB.
    pub max_total_bytes: u64,
    /// Maximum depth of a traversed directory, where the root has depth 0 and
    /// `a/b` has depth 2. Defaults to 255, so every file in a traversed
    /// directory fits the default 256-component path limit.
    pub max_directory_depth: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_files: 100_000,
            max_file_bytes: 64 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            max_directory_depth: 255,
        }
    }
}

/// A rejected budget charge, carrying only the inclusive bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetExceeded {
    limit: u64,
}

impl BudgetExceeded {
    /// Returns the inclusive bound that the charge would have exceeded.
    pub const fn limit(self) -> u64 {
        self.limit
    }
}

impl fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cumulative amount exceeds limit {}", self.limit)
    }
}

impl std::error::Error for BudgetExceeded {}

/// A cumulative counter with an inclusive bound and all-or-nothing charges.
///
/// A charge that would exceed the bound, including by arithmetic overflow,
/// changes nothing. The owner must charge in a canonical order, such as sorted
/// path order: which charge fails would otherwise depend on scheduling. The
/// budget has one owner and is never shared between concurrent workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    used: u64,
    limit: u64,
}

impl Budget {
    /// Starts an unused budget with an inclusive bound.
    pub const fn new(limit: u64) -> Self {
        Self { used: 0, limit }
    }

    /// Adds `amount` if the total stays within the bound. Zero always succeeds.
    pub fn charge(&mut self, amount: u64) -> Result<(), BudgetExceeded> {
        let exceeded = BudgetExceeded { limit: self.limit };
        let total = self.used.checked_add(amount).ok_or(exceeded)?;
        if total > self.limit {
            return Err(exceeded);
        }
        self.used = total;
        Ok(())
    }

    /// Returns the accepted cumulative amount.
    pub const fn used(&self) -> u64 {
        self.used
    }

    /// Returns the inclusive bound.
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// Returns the amount that one further charge may still add.
    pub const fn remaining(&self) -> u64 {
        self.limit - self.used
    }
}
