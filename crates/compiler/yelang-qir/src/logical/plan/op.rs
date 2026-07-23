//! The [`Plan`] enum — the 20-variant logical operator tree.

use std::sync::Arc;

use yelang_interner::Symbol;

use super::agg::AggCall;
use super::arena::PlanId;
use super::join::{DepJoinKind, JoinKind};
use super::keys::{GroupKey, PlanRange, SortSpec, WindowFunc};
use super::source::{SourceRef, TraversePath};
use super::user::UserDefinedPlanNode;
use super::ExprRef;

/// A logical operator in the query plan tree.
///
/// Every variant that takes an `input: PlanId` is a unary operator.
/// Binary operators (`Join`, `CoGroup`, `DependentJoin`, `GroupJoin`) take
/// two child plan ids. Leaves (`Scan`, `Constant`, `Empty`) take none.
#[derive(Debug, Clone)]
pub enum Plan {
    // ── Sources ────────────────────────────────────────────────────────
    /// Read from a storage-backed table, a local collection, or a
    /// function call that returns a collection.
    Scan {
        source: SourceRef,
        /// Predicate pushed into the scan (optimizer may add/remove).
        filter: Option<ExprRef>,
        /// Field list pushed into the scan (optimizer may trim).
        projection: Option<Vec<Symbol>>,
        /// Row range pushed into the scan.
        range: Option<PlanRange>,
    },

    // ── Relational transforms ──────────────────────────────────────────
    /// Keep only rows matching `pred`.
    ///
    /// Lowered from: `[where …]`, `.filter(…)`, pipeline `where`.
    Filter {
        input: PlanId,
        pred: ExprRef,
    },

    /// Compute new output columns from expressions.
    ///
    /// Lowered from: `select … { id, name }`, `.map(…)` when the
    /// projection changes the schema shape.
    Project {
        input: PlanId,
        /// `(output_name, expression)` pairs.
        exprs: Vec<(Symbol, ExprRef)>,
    },

    /// Apply a closure to each element, optionally flattening.
    ///
    /// Lowered from: `[*]` (flatten_depth 0), `[**]` (depth 1),
    /// `.map(…)`, `.flat_map(…)`.
    ///
    /// When `flatten_depth > 0`, the closure's result is flattened by
    /// that many levels (`[**]` = depth 1, `[***]` = depth 2, …).
    Map {
        input: PlanId,
        /// The closure body — a THIR expression referencing the element binder.
        func: ExprRef,
        /// How many nesting levels to flatten. 0 = no flatten (plain map).
        flatten_depth: usize,
    },

    // ── Joins ──────────────────────────────────────────────────────────
    /// Set-algebraic join between two collections.
    ///
    /// Lowered from: `.join(…)`, `.semi_join(…)`, `.anti_join(…)`,
    /// and from decorrelated `links` / subqueries.
    Join {
        left: PlanId,
        right: PlanId,
        kind: JoinKind,
        /// Equi-join key pairs: `(left_key_expr, right_key_expr)`.
        on: Vec<(ExprRef, ExprRef)>,
        /// Residual (non-equi) predicate applied after the join.
        filter: Option<ExprRef>,
    },

    // ── Aggregation ────────────────────────────────────────────────────
    /// Group rows by keys and compute aggregates per group.
    ///
    /// Lowered from: `group by { … } into groups`, `.group_by(…)`,
    /// `.aggregate(…)`.
    Aggregate {
        input: PlanId,
        /// Grouping keys: `(output_name, key)`.
        keys: Vec<(Symbol, GroupKey)>,
        /// Aggregate computations per group.
        aggs: Vec<AggCall>,
        /// The `into <label>` name for the resulting group collection.
        into: Symbol,
    },

    // ── Window functions ───────────────────────────────────────────────
    /// Compute window functions over partitions.
    ///
    /// Lowered from: `ROW_NUMBER() OVER (...)`, `RANK() OVER (...)`,
    /// `SUM(x) OVER (...)`, `LAG(x) OVER (...)`, etc.
    ///
    /// Each window function has its own PARTITION BY, ORDER BY, and frame.
    /// Functions sharing the same window spec are grouped into one node.
    Window {
        input: PlanId,
        funcs: Vec<WindowFunc>,
    },

    // ── Ordering / slicing / dedup ─────────────────────────────────────
    /// Sort rows by one or more keys.
    ///
    /// Lowered from: `order by …`, `.order_by(…)`, `[order by …]`.
    Sort {
        input: PlanId,
        specs: Vec<SortSpec>,
    },

    /// Skip and/or take a number of rows.
    ///
    /// Lowered from: `range a..b`, `.take(n)`, `.skip(n)`, `[n]`, `[a..b]`.
    Limit {
        input: PlanId,
        /// Number of rows to skip (offset). `None` = 0.
        skip: Option<ExprRef>,
        /// Maximum number of rows to return. `None` = unbounded.
        fetch: Option<ExprRef>,
    },

    /// Remove duplicate rows, optionally by key.
    ///
    /// Lowered from: `.distinct()`, `.unique_by(…)`.
    Distinct {
        input: PlanId,
        /// If `Some`, distinct by these key expressions. If `None`, by all columns.
        on: Option<Vec<ExprRef>>,
    },

    // ── Set operations ─────────────────────────────────────────────────
    /// Concatenate multiple inputs.
    ///
    /// Lowered from: multi-root `from` (cross product is Join::Cross,
    /// union is this), `.union(…)`.
    Union {
        inputs: Vec<PlanId>,
    },

    // ── Graph traversal (links) ────────────────────────────────────────
    /// Correlated graph traversal with nested materialization.
    ///
    /// Lowered from: `links (users)->[writes@w:Edge]->(books@b:Book)`.
    ///
    /// Each path is anchored at a bound collection label and consists of
    /// an arbitrary number of segments with mixed directions. The result
    /// materializes nested arrays on the parent elements.
    ///
    /// The optimizer may decorrelate this into a `Join` chain when
    /// cardinality estimates favour hash/merge joins over nested loops.
    Traverse {
        input: PlanId,
        paths: Vec<TraversePath>,
    },

    // ── Correlation (introduced by extraction, eliminated by optimizer) ─
    /// Dependent join — the algebraic form of a correlated subquery.
    ///
    /// The inner plan references symbols produced by the outer plan.
    /// **Must not survive decorrelation.** The physical planner asserts
    /// that no `DependentJoin` nodes remain.
    ///
    /// See: Neumann & Kemper, "Unnesting Arbitrary Queries" (BTW 2015)
    /// and Neumann, "Improving Unnesting of Complex Queries" (BTW 2025).
    DependentJoin {
        outer: PlanId,
        inner: PlanId,
        /// Join/filter predicate (may reference outer symbols).
        pred: Option<ExprRef>,
        kind: DepJoinKind,
    },

    /// Fused join + aggregation. Introduced during decorrelation when a
    /// `DependentJoin` wraps an `Aggregate`. Solves the COUNT bug:
    /// empty groups get correct static values (COUNT→0, SUM→0, AVG→NULL).
    ///
    /// See: Fent, Neumann et al., "GroupJoin" (VLDB 2021/2022).
    GroupJoin {
        left: PlanId,
        right: PlanId,
        /// Equi-join key pairs.
        on: Vec<(ExprRef, ExprRef)>,
        /// Aggregates computed per join group.
        aggs: Vec<AggCall>,
    },

    // ── Subqueries (pre-decorrelation) ─────────────────────────────────
    /// A scalar subquery that produces a single value per outer row.
    ///
    /// **Must not survive decorrelation.**
    ScalarSubquery {
        plan: PlanId,
        /// Outer symbols this subquery references (correlation set).
        correlation: Vec<Symbol>,
    },

    /// An EXISTS / NOT EXISTS subquery.
    ///
    /// **Must not survive decorrelation.**
    Exists {
        plan: PlanId,
        /// Outer symbols this subquery references.
        correlation: Vec<Symbol>,
        /// `true` for NOT EXISTS.
        negated: bool,
    },

    // ── Iteration (fixpoint) ───────────────────────────────────────────
    /// Fixpoint iteration for recursive/iterative algorithms.
    ///
    /// Lowered from: recursive queries, PageRank-style iteration,
    /// `iterate(base, recursion)` in SaneQL terms.
    Repeat {
        input: PlanId,
        /// `λ(iteration_state, collection) -> (new_collection, continue: bool)`
        func: ExprRef,
        /// Safety bound. `None` = iterate until fixpoint.
        max_iters: Option<usize>,
    },

    // ── User-defined / opaque ──────────────────────────────────────────
    /// A user-defined logical operator that the compiler cannot optimize
    /// through. Participates in the plan tree but acts as an optimization
    /// barrier.
    ///
    /// Lowered from: custom `Queryable` methods the compiler doesn't
    /// recognize, or user-registered plan extensions.
    Extension {
        node: Arc<dyn UserDefinedPlanNode>,
    },

    // ── Leaves ─────────────────────────────────────────────────────────
    /// A constant/literal value (e.g. `select 1 from …`).
    Constant {
        value: ExprRef,
    },

    /// An empty relation. `produce_one_row: true` emits a single empty
    /// tuple (useful as the identity for cross joins and as the seed
    /// for scalar aggregation without GROUP BY).
    Empty {
        produce_one_row: bool,
    },
}
