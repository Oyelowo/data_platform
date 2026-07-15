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
pub mod fn_ctxt;
pub mod hir_ty_lower;
pub mod method;
pub mod pat;
pub mod typeck_results;
pub mod writeback;

pub use check::*;
pub use coerce::*;
pub use collector::*;
pub use fn_ctxt::*;
pub use hir_ty_lower::*;
pub use method::*;
pub use pat::*;
pub use typeck_results::*;
pub use writeback::*;

#[cfg(test)]
mod tests;
