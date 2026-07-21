//! Neumann-style top-down decorrelation.
//!
//! Transforms correlated subqueries (`DependentJoin`) into flat joins by pushing
//! the dependent-join operator down through the inner plan until correlation
//! disappears.  The pass uses explicit `output_binder` tracking in
//! `LogicalProps` to decide which binders are outer (free) vs locally bound.
//!
//! Supported rewrite rules:
//!
//!   DJ(outer, Filter(input, p), pred)    => Filter(DJ(outer, input, pred), p)
//!                                           when p does not reference outer binders
//!   DJ(outer, Map(input, f), pred)       => Map(DJ(outer, input, pred), f)
//!                                           when f does not reference outer binders
//!   DJ(outer, Aggregate(input, agg), p)  => Aggregate(DJ(outer, input, p), agg)
//!                                           when agg does not reference outer binders
//!   DJ(outer, inner, pred) with no outer refs => Join(Cross, outer, inner, pred)
//!
//! These rules are applied in a fixpoint loop, so a deeply nested inner plan is
//! flattened step by step.

use yelang_arena::{FxHashMap, FxHashSet};

use crate::errors::LoweringError;
use crate::ids::{BinderId, LirId};
use crate::lir::operator::{JoinKind, LirOp};
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::{apply_id_rewrites, reachable_ids};
use crate::util::subst::free_binders;

pub struct DecorrelatePass;

impl RewritePass for DecorrelatePass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<LirId> = reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            let (outer, inner, pred) = match plan.operator(id) {
                LirOp::DependentJoin { outer, inner, predicate } => (*outer, *inner, *predicate),
                _ => continue,
            };

            let outer_binders = outer_binders(plan, inner);
            if outer_binders.is_empty() {
                let out_ty = plan.props[id].output_ty;
                let new_id = plan.join(JoinKind::Cross, outer, inner, pred, out_ty);
                rewrites.insert(id, new_id);
                continue;
            }

            let child_op = plan.operator(inner).clone();
            let new_id = match child_op {
                LirOp::Filter { input, predicate } if !expr_refs_any(plan, predicate, &outer_binders) => {
                    push_through_unary(plan, outer, input, pred, |plan, new_input| {
                        let out_ty = plan.props[inner].output_ty;
                        let input_binder = plan.props[input].output_binder;
                        let id = plan.filter(new_input, predicate, out_ty);
                        plan.props[id].output_binder = input_binder;
                        id
                    })
                }

                LirOp::Map { input, projection } if !expr_refs_any(plan, projection, &outer_binders) => {
                    push_through_unary(plan, outer, input, pred, |plan, new_input| {
                        let out_ty = plan.props[inner].output_ty;
                        let output_binder = plan.props[inner].output_binder;
                        let id = plan.map(new_input, projection, out_ty);
                        plan.props[id].output_binder = output_binder;
                        id
                    })
                }

                LirOp::Aggregate { input, agg } if !expr_refs_any(plan, agg.per_row, &outer_binders) => {
                    push_through_unary(plan, outer, input, pred, |plan, new_input| {
                        let out_ty = plan.props[inner].output_ty;
                        plan.aggregate(new_input, agg, out_ty)
                    })
                }

                LirOp::OrderBy { input, keys } if keys.iter().all(|k| !expr_refs_any(plan, k.expr, &outer_binders)) => {
                    push_through_unary(plan, outer, input, pred, |plan, new_input| {
                        let out_ty = plan.props[inner].output_ty;
                        let id = plan.order_by(new_input, keys, out_ty);
                        plan.props[id].output_binder = plan.props[input].output_binder;
                        id
                    })
                }

                LirOp::Slice { input, offset, limit } => {
                    let slice_exprs_refs = expr_refs_any(plan, offset, &outer_binders)
                        || limit.map_or(false, |l| expr_refs_any(plan, l, &outer_binders));
                    if slice_exprs_refs {
                        continue;
                    }
                    push_through_unary(plan, outer, input, pred, |plan, new_input| {
                        let out_ty = plan.props[inner].output_ty;
                        let ordered = plan.props[inner].ordered;
                        let id = plan.slice_unchecked(new_input, offset, limit, out_ty, ordered);
                        plan.props[id].output_binder = plan.props[input].output_binder;
                        id
                    })
                }

                _ => continue,
            };

            rewrites.insert(id, new_id);
        }

        apply_id_rewrites(plan, &rewrites);
        Ok(!rewrites.is_empty())
    }
}

/// Build `DJ(outer, input, pred)` and then wrap it with `build_op`.
fn push_through_unary<F>(
    plan: &mut LogicalPlan,
    outer: LirId,
    input: LirId,
    pred: Option<crate::ids::QExprId>,
    build_op: F,
) -> LirId
where
    F: FnOnce(&mut LogicalPlan, LirId) -> LirId,
{
    let dj_out_ty = plan.props[input].output_ty;
    let dj_input_binder = plan.props[input].output_binder;
    let new_dj = plan.dependent_join(outer, input, pred, dj_out_ty);
    plan.props[new_dj].output_binder = dj_input_binder;
    build_op(plan, new_dj)
}

/// Return the set of binders referenced by `inner` that are not introduced by
/// any operator inside `inner`.  These are the outer (correlated) binders.
fn outer_binders(plan: &LogicalPlan, inner: LirId) -> FxHashSet<BinderId> {
    let mut free = FxHashSet::default();
    collect_plan_free_binders(plan, inner, &mut free);
    free
}

fn collect_plan_free_binders(plan: &LogicalPlan, op_id: LirId, free: &mut FxHashSet<BinderId>) {
    let op = plan.operator(op_id);
    for expr in op.expressions() {
        for b in free_binders(plan, expr) {
            free.insert(b);
        }
    }
    for child in op.children() {
        collect_plan_free_binders(plan, child, free);
    }
    for b in introduced_binders(plan, op_id) {
        free.remove(&b);
    }
}

/// Binders introduced by `op_id` and made available to downstream operators.
fn introduced_binders(plan: &LogicalPlan, op_id: LirId) -> Vec<BinderId> {
    let op = plan.operator(op_id);
    match op {
        LirOp::Scan { .. }
        | LirOp::Map { .. }
        | LirOp::FlatMap { .. }
        | LirOp::Filter { .. }
        | LirOp::OrderBy { .. }
        | LirOp::Slice { .. }
        | LirOp::Distinct { .. }
        | LirOp::Window { .. }
        | LirOp::AttachField { .. }
        | LirOp::DependentJoin { .. } => {
            plan.props[op_id].output_binder.into_iter().collect()
        }
        _ => vec![],
    }
}

fn expr_refs_any(plan: &LogicalPlan, expr: crate::ids::QExprId, binders: &FxHashSet<BinderId>) -> bool {
    free_binders(plan, expr).iter().any(|b| binders.contains(b))
}
