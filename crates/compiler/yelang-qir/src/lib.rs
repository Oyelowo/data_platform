//! Query planning for Yelang.
//!
//! This crate defines the **logical plan tree** — an algebraic operator
//! representation that makes the relational/collection structure of queries
//! explicit for optimization. It is NOT a new IR: every expression is a
//! reference back into THIR. The plan tree is a *view* that the optimizer
//! rewrites using algebraic rules (predicate pushdown, decorrelation,
//! cMap fusion, join reordering, …).
//!
//! # Pipeline position
//!
//! ```text
//! THIR (typed) ──► Plan lowering ──► Logical optimization ──► Physical planning ──► Execution
//!                  (this crate)        (this crate)            (future)              (future)
//! ```
//!
//! # Key types
//!
//! - [`Plan`] — the logical operator enum (Scan, Filter, Join, Aggregate, Traverse, …)
//! - [`PlanArena`] — dense arena storing the plan tree, keyed by [`PlanId`]
//! - [`PlanMeta`] — per-node algebraic metadata (correlation, partitioning, ordering)
//! - [`AggKind`] — three-tier aggregate recognition (known / trait-based / opaque)

pub mod analysis;
pub mod logical;
pub mod optimize;
pub mod physical;
pub mod tree;

pub use logical::{lower_expr_as_plan, lower_query};
pub use optimize::{OptRule, Optimizer};
pub use logical::{
    AggCall, AggKind, DepJoinKind, Direction, EdgeRef, ExprRef, FrameBound, FrameUnit, GroupKey,
    JoinKind, NodeRef, OrderSpec, Partitioning, Plan, PlanArena, PlanId, PlanMeta, PlanOrigin,
    PlanRange, SortKey, SortSpec, SourceRef, TraversePath, TraverseSegment, UserDefinedPlanNode,
    WindowFrame, WindowFunc, WindowKind,
};

// Backward-compatible module re-exports: the logical plan types now live under
// [`logical`], but downstream code (and this crate's own tests) historically
// referenced them as `yelang_qir::plan` / `yelang_qir::lower`.
pub use logical::lower;
pub use logical::plan;
