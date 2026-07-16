//! Conformance test suite for storage engines.

use storage_traits::Engine;

pub mod boundaries;
pub mod crud;
pub mod isolation;
pub mod ordering;
pub mod transactions;

/// Run the full conformance suite against any engine.
///
/// `factory` must produce a fresh, empty engine on each call.
pub fn run<E, F>(factory: F)
where
    E: Engine,
    F: Fn() -> E,
{
    crud::run(&factory);
    ordering::run(&factory);
    transactions::run(&factory);
    boundaries::run(&factory);
    isolation::run(&factory);
}
