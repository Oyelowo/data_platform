//! Logical planning.
//!
//! This module groups everything that produces and rewrites the *logical*
//! plan tree:
//! - [`plan`] — the algebraic operator types ([`plan::Plan`], [`plan::PlanArena`], …)
//! - [`lower`] — HIR/THIR → [`plan::Plan`] lowering
//! - [`optimize`] — Plan → Plan optimization passes (decorrelation, pushdown,
//!   simplify, join reordering, projection pruning)
//!
//! Physical planning lives in [`crate::physical`].

pub mod lower;
pub mod optimize;
pub mod plan;

pub use lower::{lower_expr_as_plan, lower_query};
pub use optimize::{
    decorrelate, EliminateTrivialFilter, EliminateTrivialLimit, JoinReorder, MergeAdjacentFilters,
    PruneUnusedFields, PushDownFilter,
};
pub use plan::{
    AggCall, AggKind, DepJoinKind, Direction, EdgeRef, ExprRef, FrameBound, FrameUnit, GroupKey,
    JoinKind, NodeRef, OrderSpec, Partitioning, Plan, PlanArena, PlanId, PlanMeta, PlanOrigin,
    PlanRange, SortKey, SortSpec, SourceRef, TraversePath, TraverseSegment, UserDefinedPlanNode,
    WindowFrame, WindowFunc, WindowKind,
};
