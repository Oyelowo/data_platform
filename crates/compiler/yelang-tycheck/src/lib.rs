/*! yelang-tycheck: Body type checker.
 *
 * This crate type-checks function bodies, expressions, and patterns.
 * It uses `yelang-infer` for unification and `yelang-trait-solver`
 * for trait bound resolution.
 *
 * ## Architecture
 *
 * - `collector` — Collect item signatures from HIR into `Ty`.
 * - `check` — Type-check expressions and statements.
 * - `method` — Method lookup and resolution.
 * - `coerce` — Coercion logic.
 * - `pat` — Pattern type checking.
 * - `writeback` — Write inferred types back to HIR.
 */

pub mod check;
pub mod coerce;
pub mod collector;
pub mod method;
pub mod pat;
pub mod writeback;

pub use check::*;
pub use coerce::*;
pub use collector::*;
pub use method::*;
pub use pat::*;
pub use writeback::*;

#[cfg(test)]
mod tests;
