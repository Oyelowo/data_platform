/*! yelang-infer: Type inference engine with custom unification.
 *
 * This crate provides:
 * - A custom union-find unification table with rollback (our `ena` equivalent)
 * - `InferCtxt` — the inference context for creating and unifying type variables
 * - Structural type equality with occurs check
 * - Snapshots for speculative trait solving
 */

pub mod context;
pub mod error;
pub mod snapshot;
pub mod type_variable;
pub mod unify;

pub use context::*;
pub use error::*;
pub use type_variable::*;
pub use unify::*;

#[cfg(test)]
mod tests;
