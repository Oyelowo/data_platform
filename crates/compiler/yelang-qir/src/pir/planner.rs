//! Top-down physical planner.
//!
//! Converts an optimized `LogicalPlan` into a `PhysicalPlan` by choosing an
//! implementation for each logical operator and inserting property enforcers
//! (sort, exchange) when a child does not satisfy the requirements of its
//! parent.  This is a pragmatic first planner: it generates one primary
//! candidate per logical operator and falls back to conservative choices when
//! advanced statistics or distribution are not available.  It can be upgraded
//! to a full Cascades memo planner later without changing the PIR shape.

use yelang_arena::FxHashMap;

use crate::backend::capability::{BackendCapability, Support};
use crate::errors::PlanError;
use crate::expr::{OrderKey, QBinaryOp, QExpr, QExprId, QLit};
use crate::ids::{BinderId, LirId, PirId};
use crate::logical::operator::{AggregateOp, ConstructKind, JoinKind, LirOp, ScanSource, SetOpKind};
use crate::logical::plan::LogicalPlan;
use crate::pir::cost::{compute_cost, estimate_cardinality};
use crate::pir::operator::{AggMode, ExchangeKind, PhysicalAggregateOp, PirOp};
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Cost, Partitioning, PhysicalOrdering, PhysicalProps};

/// A candidate physical implementation of a logical operator group.
#[derive(Clone, Debug)]
struct Candidate {
    id: PirId,
    props: PhysicalProps,
    #[allow(dead_code)]
    cost: Cost,
    total: f64,
}

/// Top-down physical planner.
pub struct Planner<'a> {
    logical: &'a LogicalPlan,
    backend: &'a dyn BackendCapability,
    physical: PhysicalPlan,
    /// Cache of (LirId, required PhysicalProps) -> best PirId.
    memo: FxHashMap<(LirId, PhysicalProps), PirId>,
}

impl<'a> Planner<'a> {
    pub fn new(logical: &'a LogicalPlan, backend: &'a dyn BackendCapability) -> Self {
        Self {
            logical,
            backend,
            physical: PhysicalPlan::empty(),
            memo: FxHashMap::default(),
        }
    }

    pub fn plan(mut self) -> Result<PhysicalPlan, PlanError> {
        let root = self.logical.root.ok_or(PlanError::NoValidPlan)?;
        let phys_root = self.optimize(root, PhysicalProps::any())?;
        self.physical.set_root(phys_root);
        self.physical.exprs = self.logical.exprs.clone();
        Ok(self.physical)
    }

    /// Return the cheapest physical operator that implements `lir` and satisfies
    /// `required` physical properties.
    fn optimize(&mut self, lir: LirId, required: PhysicalProps) -> Result<PirId, PlanError> {
        if let Some(&id) = self.memo.get(&(lir, required.clone())) {
            return Ok(id);
        }

        let candidates = self.implement(lir, &required)?;
        let best = pick_best(candidates, &required)?;
        let id = enforce(&mut self.physical, best, &required);
        self.memo.insert((lir, required), id);
        Ok(id)
    }

    /// Generate physical candidates for a logical operator.
    fn implement(&mut self, lir: LirId, required: &PhysicalProps) -> Result<Vec<Candidate>, PlanError> {
        let op = self.logical.operator(lir).clone();
        let _lprops = &self.logical.props[lir];

        match op {
            LirOp::Scan { source, .. } => Ok(vec![self.implement_scan(lir, source, required)?]),
            LirOp::Values { rows, .. } => Ok(vec![self.implement_values(rows, required)?]),
            LirOp::Filter { input, predicate } => Ok(vec![self.implement_filter(input, predicate, required)?]),
            LirOp::Map { input, projection } => Ok(vec![self.implement_map(input, projection, required)?]),
            LirOp::FlatMap { input, projection } => Ok(vec![self.implement_flat_map(input, projection, required)?]),
            LirOp::OrderBy { input, keys } => Ok(vec![self.implement_order_by(input, keys, required)?]),
            LirOp::Slice { input, offset, limit } => Ok(vec![self.implement_slice(input, offset, limit, required)?]),
            LirOp::Distinct { input, by } => Ok(vec![self.implement_distinct(input, by, required)?]),
            LirOp::GroupBy { input, key, key_ty, .. } => Ok(vec![self.implement_group_by(input, key, key_ty, required)?]),
            LirOp::Aggregate { input, agg } => Ok(vec![self.implement_aggregate(input, vec![agg], vec![], required)?]),
            LirOp::AggregateGroupBy { input, group_keys, aggregates } => {
                Ok(vec![self.implement_aggregate(input, aggregates, group_keys, required)?])
            }
            LirOp::Join { kind, left, right, predicate } => {
                Ok(vec![self.implement_join(kind, left, right, predicate, required)?])
            }
            LirOp::DependentJoin { outer, inner, predicate } => {
                Ok(vec![self.implement_dependent_join(outer, inner, predicate, required)?])
            }
            LirOp::EdgeExpand { input, edge, direction, predicate } => {
                Ok(vec![self.implement_edge_expand(input, edge, direction, predicate, required)?])
            }
            LirOp::AttachField { input, field, value_plan } => {
                Ok(vec![self.implement_attach_field(input, field, value_plan, required)?])
            }
            LirOp::Construct { kind, fields } => Ok(vec![self.implement_construct(kind, fields, required)?]),
            LirOp::SetOp { op, left, right } => Ok(vec![self.implement_set_op(op, left, right, required)?]),
            LirOp::Window { input, func, partition, order, frame } => {
                Ok(vec![self.implement_window(input, func, partition, order, frame, required)?])
            }
            LirOp::Expr(expr) => Ok(vec![self.implement_expr(expr, required)?]),
        }
    }

    // -------------------------------------------------------------------------
    // Operator implementations
    // -------------------------------------------------------------------------

    fn implement_scan(
        &mut self,
        lir: LirId,
        source: ScanSource,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let projection = self.logical.props[lir].demand.clone();
        let op = PirOp::TableScan {
            source,
            predicate: None,
            projection,
        };
        Ok(self.candidate(op, PhysicalProps::any()))
    }

    fn implement_values(&mut self, rows: Vec<QExprId>, _required: &PhysicalProps) -> Result<Candidate, PlanError> {
        Ok(self.candidate(PirOp::Values { rows }, PhysicalProps::any()))
    }

    fn implement_filter(
        &mut self,
        input: LirId,
        predicate: QExprId,
        required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, required.clone())?;
        Ok(self.candidate(PirOp::Filter { input: child, predicate }, PhysicalProps::any()))
    }

    fn implement_map(
        &mut self,
        input: LirId,
        projection: QExprId,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::Project { input: child, projection }, PhysicalProps::any()))
    }

    fn implement_flat_map(
        &mut self,
        input: LirId,
        projection: QExprId,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::FlatMap { input: child, projection }, PhysicalProps::any()))
    }

    fn implement_order_by(
        &mut self,
        input: LirId,
        keys: Vec<OrderKey>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        let mut props = PhysicalProps::any();
        props.ordering = PhysicalOrdering { keys: keys.clone() };
        Ok(self.candidate(PirOp::Sort { input: child, keys }, props))
    }

    fn implement_slice(
        &mut self,
        input: LirId,
        offset: QExprId,
        limit: Option<QExprId>,
        required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let offset_val = eval_const_usize(self.logical, offset).unwrap_or(0);
        let limit_val = limit.and_then(|l| eval_const_usize(self.logical, l));
        let child = self.optimize(input, required.clone())?;
        Ok(self.candidate(PirOp::Slice { input: child, offset: offset_val, limit: limit_val }, PhysicalProps::any()))
    }

    fn implement_distinct(
        &mut self,
        input: LirId,
        by: Option<Vec<QExprId>>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::Distinct { input: child, by }, PhysicalProps::any()))
    }

    fn implement_group_by(
        &mut self,
        input: LirId,
        key: QExprId,
        _key_ty: yelang_ty::ty::TyId,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::GroupBy { input: child, key }, PhysicalProps::any()))
    }

    fn implement_aggregate(
        &mut self,
        input: LirId,
        aggs: Vec<AggregateOp>,
        group_keys: Vec<QExprId>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        let phys_aggs: Vec<_> = aggs
            .into_iter()
            .map(|a| PhysicalAggregateOp {
                agg_def: a.agg_def,
                impl_def: a.impl_def,
                class: a.class,
                input_expr: a.per_row,
                acc_ty: a.acc_ty,
                out_ty: a.out_ty,
            })
            .collect();

        // For distributed backends, insert partial/final aggregation.  For local
        // memory backends the executor can treat Full mode as a single pass.
        let mode = if self.backend.supports_exchange(&ExchangeKind::Gather) && group_keys.is_empty() {
            // Scalar aggregate: partial per node, final merge at coordinator.
            AggMode::Full
        } else {
            AggMode::Full
        };

        Ok(self.candidate(
            PirOp::HashAggregate {
                input: child,
                group_keys,
                aggregates: phys_aggs,
                mode,
            },
            PhysicalProps::any(),
        ))
    }

    fn implement_join(
        &mut self,
        kind: JoinKind,
        left: LirId,
        right: LirId,
        predicate: Option<QExprId>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let left_id = self.optimize(left, PhysicalProps::any())?;
        let right_id = self.optimize(right, PhysicalProps::any())?;

        if let Some(pred) = predicate {
            if let Some((left_key, right_key)) = extract_equi_join_keys(self.logical, pred, &self.logical.props[left].output_binder, &self.logical.props[right].output_binder) {
                if self.backend.supports_hash_join() == Support::Yes {
                    return Ok(self.candidate(
                        PirOp::HashJoin {
                            build: left_id,
                            probe: right_id,
                            build_key: left_key,
                            probe_key: right_key,
                            kind,
                        },
                        PhysicalProps::any(),
                    ));
                }
            }
        }

        // Conservative fallback: nested-loop join.
        Ok(self.candidate(
            PirOp::NestedLoopJoin {
                outer: left_id,
                inner: right_id,
                predicate,
                kind,
            },
            PhysicalProps::any(),
        ))
    }

    fn implement_dependent_join(
        &mut self,
        outer: LirId,
        inner: LirId,
        predicate: Option<QExprId>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        // Until full lateral execution is in place, plan as a nested-loop join.
        // The inner side will be re-executed (or parameterized) per outer row.
        self.implement_join(JoinKind::Inner, outer, inner, predicate, _required)
    }

    fn implement_edge_expand(
        &mut self,
        input: LirId,
        edge: yelang_hir::ids::DefId,
        direction: crate::logical::operator::EdgeDirection,
        predicate: Option<QExprId>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::EdgeExpand { input: child, edge, direction, predicate }, PhysicalProps::any()))
    }

    fn implement_attach_field(
        &mut self,
        input: LirId,
        field: yelang_interner::Symbol,
        value_plan: LirId,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        let value = self.optimize(value_plan, PhysicalProps::any())?;
        Ok(self.candidate(PirOp::AttachField { input: child, field, value_plan: value }, PhysicalProps::any()))
    }

    fn implement_construct(
        &mut self,
        kind: ConstructKind,
        fields: Vec<(yelang_interner::Symbol, LirId)>,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let phys_fields: Result<Vec<_>, _> = fields
            .into_iter()
            .map(|(name, id)| Ok((name, self.optimize(id, PhysicalProps::any())?)))
            .collect();
        Ok(self.candidate(PirOp::Construct { kind, fields: phys_fields? }, PhysicalProps::any()))
    }

    fn implement_set_op(
        &mut self,
        op: SetOpKind,
        left: LirId,
        right: LirId,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let left_id = self.optimize(left, PhysicalProps::any())?;
        let right_id = self.optimize(right, PhysicalProps::any())?;
        match op {
            SetOpKind::UnionAll => Ok(self.candidate(PirOp::UnionAll { inputs: vec![left_id, right_id] }, PhysicalProps::any())),
            SetOpKind::Union => Ok(self.candidate(PirOp::Union { inputs: vec![left_id, right_id] }, PhysicalProps::any())),
            SetOpKind::Intersect | SetOpKind::IntersectAll => Ok(self.candidate(PirOp::Intersect { left: left_id, right: right_id }, PhysicalProps::any())),
            SetOpKind::Except | SetOpKind::ExceptAll => Ok(self.candidate(PirOp::Except { left: left_id, right: right_id }, PhysicalProps::any())),
        }
    }

    fn implement_window(
        &mut self,
        input: LirId,
        func: crate::expr::WindowFunc,
        partition: Vec<QExprId>,
        order: Vec<OrderKey>,
        frame: crate::expr::WindowFrame,
        _required: &PhysicalProps,
    ) -> Result<Candidate, PlanError> {
        let child = self.optimize(input, PhysicalProps::any())?;
        Ok(self.candidate(
            PirOp::Window {
                input: child,
                func,
                partition,
                order,
                frame,
            },
            PhysicalProps::any(),
        ))
    }

    fn implement_expr(&mut self, expr: QExprId, __required: &PhysicalProps) -> Result<Candidate, PlanError> {
        Ok(self.candidate(PirOp::Expr(expr), PhysicalProps::any()))
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    fn candidate(&mut self, op: PirOp, props: PhysicalProps) -> Candidate {
        let id = self.physical.alloc(op.clone(), props.clone(), Cost::zero());
        let cost = compute_cost(&self.physical, id);
        self.physical.costs[id] = cost;
        let rows = estimate_cardinality(&self.physical, id).expected;
        Candidate { id, props, cost, total: cost.total(rows) }
    }
}

/// Insert enforcers (sort, gather) until `candidate` satisfies `required`.
fn enforce(plan: &mut PhysicalPlan, candidate: Candidate, required: &PhysicalProps) -> PirId {
    let mut id = candidate.id;

    if !candidate.props.ordering_satisfies(&required.ordering) {
        id = plan.alloc(
            PirOp::Sort {
                input: id,
                keys: required.ordering.keys.clone(),
            },
            PhysicalProps {
                ordering: required.ordering.clone(),
                ..candidate.props.clone()
            },
            Cost::zero(),
        );
    }

    if !candidate.props.partitioning_satisfies(&required.partitioning) {
        let kind = match &required.partitioning {
            Partitioning::Singleton => ExchangeKind::Gather,
            Partitioning::Hash(keys) => ExchangeKind::RepartitionBy(keys.clone()),
            Partitioning::Range(keys) => ExchangeKind::RangePartition(keys.clone()),
            Partitioning::Replicated => ExchangeKind::Broadcast,
            Partitioning::Any => return id,
        };
        id = plan.alloc(
            PirOp::Exchange { input: id, kind },
            PhysicalProps {
                partitioning: required.partitioning.clone(),
                ..candidate.props.clone()
            },
            Cost::zero(),
        );
    }

    id
}

fn pick_best(candidates: Vec<Candidate>, required: &PhysicalProps) -> Result<Candidate, PlanError> {
    candidates
        .into_iter()
        .filter(|c| c.props.satisfies(required))
        .min_by(|a, b| a.total.partial_cmp(&b.total).unwrap_or(std::cmp::Ordering::Equal))
        .ok_or(PlanError::NoValidPlan)
}

/// Try to evaluate an expression to a non-negative usize constant.
fn eval_const_usize(plan: &LogicalPlan, expr: QExprId) -> Option<usize> {
    match plan.expr(expr) {
        QExpr::Lit(QLit::Int(n), _) if *n >= 0 => Some(*n as usize),
        _ => None,
    }
}

/// Try to extract equality keys `(left, right)` from a join predicate closure.
fn extract_equi_join_keys(
    plan: &LogicalPlan,
    predicate: QExprId,
    left_binder: &Option<BinderId>,
    right_binder: &Option<BinderId>,
) -> Option<(QExprId, QExprId)> {
    let body = crate::rewrite::as_closure(plan, predicate).map(|(_, b)| b).unwrap_or(predicate);
    let (op, l, r) = match plan.expr(body) {
        QExpr::Binary(QBinaryOp::Eq, l, r, _) => (QBinaryOp::Eq, *l, *r),
        _ => return None,
    };
    let _ = op;

    let left_refs = free_binders(plan, l);
    let right_refs = free_binders(plan, r);

    match (left_binder, right_binder) {
        (Some(lb), Some(rb)) => {
            if left_refs.contains(lb) && right_refs.contains(rb) {
                Some((l, r))
            } else if left_refs.contains(rb) && right_refs.contains(lb) {
                Some((r, l))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn free_binders(plan: &LogicalPlan, expr: QExprId) -> yelang_arena::FxHashSet<BinderId> {
    crate::util::subst::free_binders(plan, expr)
}

impl PhysicalProps {
    fn ordering_satisfies(&self, required: &PhysicalOrdering) -> bool {
        if required.keys.is_empty() {
            return true;
        }
        self.ordering.keys.starts_with(&required.keys)
    }

    fn partitioning_satisfies(&self, required: &Partitioning) -> bool {
        use crate::pir::props::partitioning_satisfies;
        partitioning_satisfies(&self.partitioning, required)
    }
}

/// Top-level entry point.
pub fn plan_logical(
    logical: &LogicalPlan,
    backend: &dyn BackendCapability,
) -> Result<PhysicalPlan, PlanError> {
    Planner::new(logical, backend).plan()
}
