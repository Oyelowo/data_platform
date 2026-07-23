//! [`AggCall`], [`AggKind`].

use yelang_arena::DefId;
use yelang_interner::Symbol;

use super::ExprRef;

/// A single aggregate computation inside an `Aggregate` or `GroupJoin`.
#[derive(Debug, Clone)]
pub struct AggCall {
    /// The output column name for this aggregate's result.
    pub output: Symbol,
    /// What kind of aggregate this is.
    pub kind: AggKind,
}

/// The three tiers of aggregate recognition.
#[derive(Debug, Clone)]
pub enum AggKind {
    // ── Tier 1: compiler-known (full optimization) ─────────────────────
    Count,
    Sum { expr: ExprRef },
    Avg { expr: ExprRef },
    Min { expr: ExprRef },
    Max { expr: ExprRef },

    // ── Tier 2: user-defined via `Aggregate` trait (partial opt) ───────
    /// The compiler knows this is an aggregate (init/accumulate/merge/finish)
    /// and can parallelize partial aggregation across shards because `merge`
    /// is associative. It cannot rewrite the internals.
    UserAggregate {
        /// The `DefId` of the `Aggregate` trait impl.
        impl_def: DefId,
        /// Constructor arguments (e.g. `Percentile { p: 0.99 }`).
        args: Vec<ExprRef>,
        /// The expression being aggregated (input to `accumulate`).
        input_expr: Option<ExprRef>,
    },

    // ── Tier 3: fully opaque (no optimization through it) ──────────────
    /// A method the compiler doesn't recognize as an aggregate.
    /// Treated as a black box.
    Opaque { call: ExprRef },
}
