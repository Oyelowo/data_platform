/*! yelang-trait-solver: Next-generation recursive goal-driven trait solver.
 *
 * This crate implements the *recursive goal-driven* solver (matching
 * `rustc_next_trait_solver`), not the legacy `select`/`fulfill` approach.
 *
 * ## Architecture
 *
 * - `eval_ctxt::EvalCtxt` — The solver engine. Evaluates goals recursively.
 * - `search_graph::SearchGraph` — Detects cycles and caches results.
 * - `candidate` — Assembles candidates from impls, param-env, built-ins.
 * - `response` — `CanonicalResponse`, `Certainty`.
 * - `builtin` — Built-in impls for `Sized`, `Copy`, `Clone`.
 * - `normalize` — Associated type normalization.
 *
 * ## Solver Loop
 *
 * ```text
 * Goal → Canonicalize → Search graph check → Assemble candidates
 *   → Evaluate each in isolated probe → Merge responses → Cache result
 * ```
 */

pub mod builtin;
pub mod candidate;
pub mod canonicalize;
pub mod eval_ctxt;
pub mod goal;
pub mod instantiate;
pub mod normalize;
pub mod response;
pub mod search_graph;

pub use builtin::*;
pub use candidate::*;
pub use canonicalize::*;
pub use eval_ctxt::*;
pub use goal::*;
pub use instantiate::*;
pub use normalize::*;
pub use response::*;
pub use search_graph::*;

#[cfg(test)]
mod tests;
