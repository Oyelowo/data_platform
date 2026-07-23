//! [`PlanArena`], [`PlanId`], [`PlanMeta`], [`PlanOrigin`], [`Partitioning`].

use slotmap::SlotMap;
use yelang_arena::{FxHashMap, Id, IndexVec, SecondaryMap};
use yelang_hir::ids::{ExprId, QueryId};
use yelang_interner::Symbol;
use yelang_thir::ids::ThirExprId;

use super::keys::OrderSpec;
use super::op::Plan;
use super::ExprRef;

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
    /// THIR expression arena — a copy of `ThirBodies::exprs` so the
    /// analysis can walk typed expressions without depending on the HIR.
    pub thir_exprs: SlotMap<ThirExprId, yelang_thir::ThirExpr>,
}

impl PlanArena {
    pub fn new() -> Self {
        Self {
            nodes: IndexVec::new(),
            meta: SecondaryMap::new(),
            origin: SecondaryMap::new(),
            expr_mapping: FxHashMap::default(),
            thir_exprs: SlotMap::with_key(),
        }
    }

    /// Convert an HIR ExprId to a THIR ExprRef.
    /// Returns a default (invalid) ThirExprId if no mapping exists.
    pub fn to_thir(&self, hir_id: ExprId) -> ExprRef {
        self.expr_mapping.get(&hir_id).copied().unwrap_or_default()
    }

    /// Look up a THIR expression by its [`ExprRef`].
    pub fn thir_expr(&self, id: ExprRef) -> Option<&yelang_thir::ThirExpr> {
        self.thir_exprs.get(id)
    }

    /// If the expression referenced by `id` is a field access `_.field`, return
    /// the field's name.
    ///
    /// Used to extract equi-join key columns from a join's `on` expressions:
    /// a predicate like `a.id == b.id` lowers each side to a `Field` access
    /// whose `field` symbol is the column being joined on.
    pub fn field_name(&self, id: ExprRef) -> Option<Symbol> {
        match self.thir_expr(id)? {
            yelang_thir::ThirExpr::Field { field, .. } => Some(*field),
            _ => None,
        }
    }

    /// Copy the THIR expression arena from [`yelang_thir::ThirBodies`].
    pub fn load_thir_exprs(&mut self, bodies: &yelang_thir::ThirBodies) {
        self.thir_exprs = bodies.exprs.clone();
    }

    /// Populate the expression mappings and THIR expressions from THIR bodies.
    pub fn load_expr_mappings(&mut self, bodies: &yelang_thir::ThirBodies) {
        self.expr_mapping = bodies.expr_mapping.clone();
        self.load_thir_exprs(bodies);
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
