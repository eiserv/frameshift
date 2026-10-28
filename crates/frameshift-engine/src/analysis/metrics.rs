//! Exact, overflow-safe integer metrics for migration reporting.

use frameshift_catalog::Status;
use frameshift_core::UnitKind;

/// Published fixed weights. They measure significance, not effort.
pub const VERSION: &str = "weights-v1";
/// Assessment distribution order, including zero unknown shares.
pub const STATUSES: [Status; 4] = [
    Status::Auto,
    Status::Review,
    Status::Unknown,
    Status::Unsupported,
];

/// Weight of a counting unit, independent of project size or fan-in.
pub const fn weight(kind: UnitKind, client: bool) -> u64 {
    match kind {
        UnitKind::Route | UnitKind::Provider => 8,
        UnitKind::Layout => 5,
        UnitKind::Component => {
            if client {
                5
            } else {
                3
            }
        }
        UnitKind::Endpoint => 4,
        UnitKind::RouteFile | UnitKind::Module | UnitKind::Stylesheet | UnitKind::Config => 2,
    }
}

/// Exact numerator and denominator plus a truncated percentage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metric {
    /// Qualifying weight or count.
    pub numerator: u64,
    /// Whole population; zero means not applicable.
    pub denominator: u64,
}
impl Metric {
    /// Truncated basis points, or `None` for an empty population.
    pub fn basis_points(self) -> Option<u64> {
        (self.denominator != 0)
            .then(|| ((u128::from(self.numerator) * 10_000) / u128::from(self.denominator)) as u64)
    }
    /// Conservative display. Positive shares below 0.1% stay visible.
    pub fn display(self) -> String {
        match self.basis_points() {
            None => "n/a".into(),
            Some(0..=9) if self.numerator > 0 => "<0.1%".into(),
            Some(bp) => format!("{}.{:01}%", bp / 100, bp / 10 % 10),
        }
    }
}

/// Allocates a fixed-width bar by largest remainder. Ties keep class order.
/// Returns all zeroes for an empty population. Products use 128-bit integers.
pub fn bar(counts: [u64; 4], width: usize) -> [usize; 4] {
    let total: u128 = counts.iter().map(|&n| u128::from(n)).sum();
    if total == 0 {
        return [0; 4];
    }
    let mut out = counts.map(|n| (u128::from(n) * width as u128 / total) as usize);
    let mut order = [0, 1, 2, 3];
    order.sort_by_key(|&i| std::cmp::Reverse(u128::from(counts[i]) * width as u128 % total));
    for &i in order.iter().take(width - out.iter().sum::<usize>()) {
        out[i] += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_boundaries_and_no_overflow() {
        for (n, d, expected) in [
            (0, 0, "n/a"),
            (9996, 10000, "99.9%"),
            (1, 10001, "<0.1%"),
            (10000, 10000, "100.0%"),
            (0, 3, "0.0%"),
            (u64::MAX - 1, u64::MAX, "99.9%"),
        ] {
            assert_eq!(
                Metric {
                    numerator: n,
                    denominator: d
                }
                .display(),
                expected
            );
        }
        assert_eq!(bar([1, 1, 1, 0], 5), [2, 2, 1, 0]);
        assert_eq!(bar([u64::MAX; 4], 60), [15; 4]);
        assert_eq!(bar([0; 4], 60), [0; 4]);
    }
}
