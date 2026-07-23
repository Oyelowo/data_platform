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

use std::sync::Arc;

use yelang_arena::{DefId, FxHashMap, Id, IndexVec, SecondaryMap};
use yelang_hir::ids::{ExprId, QueryId};
use yelang_interner::Symbol;
use yelang_thir::ids::ThirExprId;

/// Expression reference used throughout the plan tree.
///
/// Uses THIR [`ThirExprId`] — the typed, desugared IR. The extraction
/// converts HIR `ExprId` → `ThirExprId` via [`PlanArena::to_thir`].
/// The analysis converts back via [`PlanArena::to_hir`].
pub type ExprRef = ThirExprId;

// ---------------------------------------------------------------------------
// PlanId
// ---------------------------------------------------------------------------

/// Tag type for [`PlanId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagPlan;

/// Dense, typed key into [`PlanArena::nodes`].
pub type PlanId = Id<TagPlan>;

// ---------------------------------------------------------------------------
// PlanArena
// ---------------------------------------------------------------------------

/// Arena-allocated logical plan tree.
///
/// Children are referenced by [`PlanId`], never boxed. This gives stable
/// identity (the optimizer can track nodes across rewrites), cache-friendly
/// layout, and trivial side-table construction via [`SecondaryMap`].
#[derive(Debug, Clone)]
pub struct PlanArena {
    /// The operator nodes, densely packed.
    pub nodes: IndexVec<PlanId, Plan>,
    /// Per-node metadata (output fields, correlation, partitioning, …).
    pub meta: SecondaryMap<PlanId, PlanMeta>,
    /// Provenance: which THIR expression or HIR query produced each node.
    pub origin: SecondaryMap<PlanId, PlanOrigin>,
    /// HIR ExprId → THIR ThirExprId. Populated from ThirBodies before extraction.
    pub expr_mapping: FxHashMap<ExprId, ExprRef>,
    /// THIR ThirExprId → HIR ExprId. Used by the analysis to walk HIR exprs.
    pub reverse_expr_mapping: FxHashMap<ExprRef, ExprId>,
}

impl PlanArena {
    pub fn new() -> Self {
        Self {
            nodes: IndexVec::new(),
            meta: SecondaryMap::new(),
            origin: SecondaryMap::new(),
            expr_mapping: FxHashMap::default(),
            reverse_expr_mapping: FxHashMap::default(),
        }
    }

    /// Convert an HIR ExprId to a THIR ExprRef.
    /// Returns a default (invalid) ThirExprId if no mapping exists.
    pub fn to_thir(&self, hir_id: ExprId) -> ExprRef {
        self.expr_mapping.get(&hir_id).copied().unwrap_or_default()
    }

    /// Convert a THIR ExprRef back to an HIR ExprId.
    /// Returns a default (invalid) ExprId if no mapping exists.
    pub fn to_hir(&self, thir_id: ExprRef) -> ExprId {
        self.reverse_expr_mapping.get(&thir_id).copied().unwrap_or_default()
    }

    /// Populate the expression mappings from THIR bodies.
    pub fn load_expr_mappings(&mut self, bodies: &yelang_thir::ThirBodies) {
        self.expr_mapping = bodies.expr_mapping.clone();
        self.reverse_expr_mapping = bodies.reverse_expr_mapping.clone();
    }

    /// Allocate a plan node and return its [`PlanId`].
    pub fn alloc(&mut self, plan: Plan) -> PlanId {
        self.nodes.push(plan)
    }

    /// Allocate a plan node with origin tracking.
    pub fn alloc_with_origin(&mut self, plan: Plan, origin: PlanOrigin) -> PlanId {
        let id = self.nodes.push(plan);
        self.origin.insert(id, origin);
        id
    }

    /// Look up a node by id.
    pub fn get(&self, id: PlanId) -> Option<&Plan> {
        self.nodes.get(id)
    }

    /// Look up a node mutably.
    pub fn get_mut(&mut self, id: PlanId) -> Option<&mut Plan> {
        self.nodes.get_mut(id)
    }

    /// Index into the arena. Panics on invalid id.
    pub fn plan(&self, id: PlanId) -> &Plan {
        &self.nodes[id]
    }

    /// Index mutably into the arena. Panics on invalid id.
    pub fn plan_mut(&mut self, id: PlanId) -> &mut Plan {
        &mut self.nodes[id]
    }

    /// Attach metadata to a node.
    pub fn set_meta(&mut self, id: PlanId, meta: PlanMeta) {
        self.meta.insert(id, meta);
    }

    /// Read metadata for a node.
    pub fn meta(&self, id: PlanId) -> Option<&PlanMeta> {
        self.meta.get(id)
    }

    /// Iterate over all `(PlanId, &Plan)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (PlanId, &Plan)> {
        self.nodes.iter_enumerated()
    }

    /// Returns `true` if any node in the arena is a `DependentJoin`,
    /// `ScalarSubquery`, or `Exists`. Used as a post-decorrelation assertion.
    pub fn has_correlated_nodes(&self) -> bool {
        self.nodes.iter().any(|p| {
            matches!(
                p,
                Plan::DependentJoin { .. } | Plan::ScalarSubquery { .. } | Plan::Exists { .. }
            )
        })
    }
}

impl Default for PlanArena {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PlanOrigin
// ---------------------------------------------------------------------------

/// Where a plan node came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanOrigin {
    /// From `select … from …` / `create` / `update` / etc. syntax.
    QuerySyntax(QueryId),
    /// From a `Queryable` method call in THIR (`.filter()`, `.map()`, …).
    MethodCall(ExprRef),
    /// From an `@intrinsic(query_*)` call.
    Intrinsic(ExprRef),
    /// Created by an optimization pass (e.g. decorrelation introduced a join).
    Synthetic,
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

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
        /// Grouping keys: `(output_name, key_expression)`.
        keys: Vec<(Symbol, ExprRef)>,
        /// Aggregate computations per group.
        aggs: Vec<AggCall>,
        /// The `into <label>` name for the resulting group collection.
        into: Symbol,
    },

    // ── Ordering / slicing / dedup ─────────────────────────────────────
    /// Sort rows by one or more keys.
    ///
    /// Lowered from: `order by …`, `.order_by(…)`, `[order by …]`.
    Sort {
        input: PlanId,
        specs: Vec<OrderSpec>,
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

// ---------------------------------------------------------------------------
// SourceRef
// ---------------------------------------------------------------------------

/// Where a `Scan` reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// A `@table`-annotated struct — storage-backed.
    Table { def: DefId, name: Symbol },
    /// A local variable holding a collection value.
    Local { name: Symbol },
    /// A function/method call that returns a collection.
    Call { func: ExprRef },
}

// ---------------------------------------------------------------------------
// JoinKind
// ---------------------------------------------------------------------------

/// The type of a set-algebraic join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    /// EXISTS / IN — keeps left rows that have at least one match.
    Semi,
    /// NOT EXISTS / NOT IN — keeps left rows that have no match.
    Anti,
    /// Cartesian product (no predicate).
    Cross,
}

// ---------------------------------------------------------------------------
// DepJoinKind
// ---------------------------------------------------------------------------

/// The type of a dependent (correlated) join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepJoinKind {
    /// Regular dependent join (scalar subquery).
    Join,
    /// Dependent semi join (EXISTS).
    Semi,
    /// Dependent anti join (NOT EXISTS).
    Anti,
    /// Dependent left outer join.
    LeftOuter,
    /// Dependent single join — guarantees at most one match per outer row.
    /// Used for scalar subqueries. See [NLK17].
    Single,
}

// ---------------------------------------------------------------------------
// TraversePath / TraverseSegment
// ---------------------------------------------------------------------------

/// A single traversal path in a `links` clause.
///
/// Multiple paths can share the same anchor. Each path is an arbitrary-length
/// chain of segments with potentially mixed directions.
#[derive(Debug, Clone)]
pub struct TraversePath {
    /// The collection label this path starts from (must be bound in `from`).
    pub anchor: Symbol,
    /// The chain of edge→node hops.
    pub segments: Vec<TraverseSegment>,
}

/// One hop in a traversal path: traverse an edge to reach a target node.
#[derive(Debug, Clone)]
pub struct TraverseSegment {
    /// The edge type to traverse.
    pub edge: EdgeRef,
    /// Direction of traversal.
    pub direction: Direction,
    /// The target node type reached via this edge.
    pub target: NodeRef,
    /// Optional predicate on the edge element.
    pub edge_pred: Option<ExprRef>,
    /// Optional predicate on the target node element.
    pub target_pred: Option<ExprRef>,
    /// For variable-length hops: `{1..3}`.
    pub hop_range: Option<PlanRange>,
}

/// Reference to an edge type in a traversal.
#[derive(Debug, Clone)]
pub struct EdgeRef {
    /// The `DefId` of the edge struct (must have `_from`/`_to` fields).
    pub def: DefId,
    /// The collection label (e.g. `writes`).
    pub label: Symbol,
    /// The `@binder` for the edge element (e.g. `w`).
    pub binder: Symbol,
}

/// Reference to a node type in a traversal.
#[derive(Debug, Clone)]
pub struct NodeRef {
    /// The `DefId` of the node struct.
    pub def: DefId,
    /// The collection label (e.g. `books`).
    pub label: Symbol,
    /// The `@binder` for the node element (e.g. `b`).
    pub binder: Symbol,
}

/// Traversal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `->` — follow `_from` → `_to`.
    Forward,
    /// `<-` — follow `_to` → `_from`.
    Backward,
    /// `<->` — both directions.
    Both,
}

// ---------------------------------------------------------------------------
// AggCall / AggKind
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// OrderSpec / PlanRange
// ---------------------------------------------------------------------------

/// One key in an `ORDER BY` clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSpec {
    /// The expression to sort by.
    pub expr: ExprRef,
    /// `true` for descending.
    pub desc: bool,
}

/// A row range (offset/limit or slice).
#[derive(Debug, Clone)]
pub struct PlanRange {
    /// Start offset (inclusive). `None` = 0.
    pub start: Option<ExprRef>,
    /// End bound. `None` = unbounded.
    pub end: Option<ExprRef>,
    /// Whether the end bound is inclusive (`..=`) or exclusive (`..`).
    pub inclusive: bool,
}

// ---------------------------------------------------------------------------
// PlanMeta
// ---------------------------------------------------------------------------

/// Per-node metadata the optimizer and physical planner need.
///
/// This is NOT a duplicate of THIR types — it captures *algebraic*
/// properties (correlation, partitioning, ordering guarantees) that
/// the THIR expression tree doesn't make explicit.
#[derive(Debug, Clone, Default)]
pub struct PlanMeta {
    /// Fields/columns this node's output exposes.
    pub output_fields: Vec<Symbol>,
    /// Fields referenced by predicates/projections in this node.
    pub referenced_fields: Vec<Symbol>,
    /// Outer symbols this subtree references (for decorrelation).
    /// Empty after decorrelation completes.
    pub correlation: Vec<Symbol>,
    /// Guaranteed output ordering, if any.
    pub ordering: Option<Vec<OrderSpec>>,
    /// How data is partitioned across nodes (for distributed planning).
    pub partitioning: Partitioning,
    /// Estimated row count (for cost-based decisions).
    pub est_cardinality: Option<usize>,
}

/// Data distribution across execution nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Partitioning {
    /// No guarantee.
    #[default]
    Any,
    /// Hash-partitioned by these keys.
    HashBy(Vec<Symbol>),
    /// Replicated on all nodes.
    Broadcast,
    /// Single node only.
    Single,
    /// Range-partitioned by these keys.
    RangeBy(Vec<Symbol>),
}

// ---------------------------------------------------------------------------
// UserDefinedPlanNode
// ---------------------------------------------------------------------------

/// Trait for user-defined logical operators.
///
/// The compiler cannot optimize through these, but they participate in
/// the plan tree and can declare which optimizations are safe.
pub trait UserDefinedPlanNode: std::fmt::Debug + Send + Sync {
    /// Display name for diagnostics and EXPLAIN output.
    fn name(&self) -> &str;

    /// Child plan inputs.
    fn inputs(&self) -> Vec<PlanId>;

    /// Output field names this operator produces.
    fn output_fields(&self) -> Vec<Symbol>;

    /// Can the optimizer push a filter below this node?
    fn supports_filter_pushdown(&self, _pred: &ExprRef) -> bool {
        false
    }

    /// Can the optimizer push a projection into this node?
    fn supports_projection_pushdown(&self, _fields: &[Symbol]) -> bool {
        false
    }
}
