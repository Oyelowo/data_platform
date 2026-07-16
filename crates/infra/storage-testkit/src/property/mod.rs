//! Property-based tests for storage engines.

use storage_traits::Engine;

pub mod operation_sequence;

/// Run all property-based tests against `factory`.
pub fn run<E, F>(factory: F)
where
    E: Engine,
    F: Fn() -> E,
{
    operation_sequence::direct_ops_match_model(&factory);
    operation_sequence::scans_are_sorted(&factory);
}
