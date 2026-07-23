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
//! THIR (typed) ──► Plan extraction ──► Logical optimization ──► Physical planning ──► Execution
//!                  (this crate)        (this crate)            (future)              (future)
//! ```
//!
//! # Key types
//!
//! - [`Plan`] — the logical operator enum (Scan, Filter, Join, Aggregate, Traverse, …)
//! - [`PlanArena`] — dense arena storing the plan tree, keyed by [`PlanId`]
//! - [`PlanMeta`] — per-node algebraic metadata (correlation, partitioning, ordering)
//! - [`AggKind`] — three-tier aggregate recognition (known / trait-based / opaque)

pub mod extract;
pub mod optimize;
pub mod plan;
pub mod tree;

pub use extract::extract_query;
pub use optimize::{OptRule, Optimizer};
pub use plan::{
    AggCall, AggKind, DepJoinKind, Direction, EdgeRef, ExprRef, JoinKind, NodeRef, OrderSpec,
    Partitioning, Plan, PlanArena, PlanId, PlanMeta, PlanOrigin, PlanRange, SourceRef,
    TraversePath, TraverseSegment, UserDefinedPlanNode,
};
