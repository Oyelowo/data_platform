//! Volatility contract for functions and expressions.
//!
//! Mirrors Postgres's proven three levels:
//! - Pure: deterministic, no effects, foldable/reorderable/shippable.
//! - Stable: deterministic within one query execution (reads snapshot).
//! - Volatile: non-deterministic or has side effects.

use std::cmp::max;

/// Volatility level of a function or expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Volatility {
    /// Deterministic, no effects. Can be folded, reordered, shipped anywhere.
    Pure,
    /// Deterministic within one query execution against a snapshot.
    Stable,
    /// Non-deterministic or effectful. Cannot be reordered or elided.
    Volatile,
}

impl Volatility {
    /// The least restrictive volatility that covers both inputs.
    pub fn combine(a: Volatility, b: Volatility) -> Volatility {
        max(a, b)
    }

    /// Combine many volatilities.
    pub fn combine_many<I: IntoIterator<Item = Volatility>>(iter: I) -> Volatility {
        iter.into_iter().fold(Volatility::Pure, Volatility::combine)
    }

    /// True if this expression can be constant-folded.
    pub fn is_pure(self) -> bool {
        self == Volatility::Pure
    }

    /// True if this expression can be shipped to a remote shard.
    pub fn is_shippable(self) -> bool {
        self <= Volatility::Stable
    }
}

impl Default for Volatility {
    fn default() -> Self {
        Volatility::Stable
    }
}
