//! Logical query plan — the algebraic operator tree.
//!
//! This is NOT a new IR. It is a *view* of typed expressions that makes
//! the relational/collection algebra explicit so the optimizer can rewrite it.
//! Every predicate, projection, and closure body is an [`ExprRef`] reference
//! back into the HIR — the plan tree never duplicates expression structure.
//!
//! Two entry points produce the same plan tree:
//! - `ThirExpr::Query(QueryId)` — from `select … from …` syntax
//! - `Queryable` method chains — from explicit `.filter().map()` calls
//!
//! After optimization, no [`Plan::DependentJoin`], [`Plan::ScalarSubquery`],
//! or [`Plan::Exists`] nodes may remain. The physical planner asserts this.

mod agg;
mod arena;
mod join;
mod keys;
mod op;
mod source;
mod user;

pub use agg::{AggCall, AggKind};
pub use arena::{Partitioning, PlanArena, PlanId, PlanMeta, PlanOrigin, TagPlan};
pub use join::{DepJoinKind, JoinKind};
pub use keys::{
    FrameBound, FrameUnit, GroupKey, OrderSpec, PlanRange, SortKey, SortSpec, WindowFrame,
    WindowFunc, WindowKind,
};
pub use op::Plan;
pub use source::{Direction, EdgeRef, NodeRef, SourceRef, TraversePath, TraverseSegment};
pub use user::UserDefinedPlanNode;

/// Expression reference used throughout the plan tree.
///
/// Uses THIR [`ThirExprId`](yelang_thir::ids::ThirExprId) — the typed, desugared IR. The extraction
/// converts HIR `ExprId` → `ThirExprId` via [`PlanArena::to_thir`].
/// The analysis walks THIR expressions directly via [`PlanArena::thir_expr`].
pub type ExprRef = yelang_thir::ids::ThirExprId;
