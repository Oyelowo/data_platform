//! Predicate pushdown through joins and set operations.
//!
//! This pass rewrites filter predicates that sit above structural operators so
//! that work is pushed closer to the data.  It is intentionally conservative:
//! rewrites are only performed when they are unconditionally correct without
//! field-provenance tracking.
//!
//! Supported rewrites:
//!
//!   Filter(Filter(input, p1), p2)           => Filter(input, p1 && p2)
//!   Filter(Join(Cross, l, r, None), p)      => Join(Inner, l, r, Some(p))
//!   Filter(Join(Inner, l, r, Some(jp)), p)  => Join(Inner, l, r, Some(jp && p))
//!
//! Set-operation pushdown (UnionAll, Union) and side-only join predicate pushdown
//! require tracking which output fields come from which child.  Those rewrites
//! are left to a future pass once the plan carries binder provenance.

use yelang_arena::FxHashMap;
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::{QBinaryOp, QExpr, QExprId};
use crate::ids::{BinderId, LirId};
use crate::lir::operator::{JoinKind, LirOp};
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::{apply_id_rewrites, as_closure, reachable_ids};

pub struct PredicatePushdownPass;

impl RewritePass for PredicatePushdownPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<LirId> = reachable_ids(plan);
        let mut rewrites: FxHashMap<LirId, LirId> = FxHashMap::default();

        for id in ids {
            let (input, predicate) = match plan.operator(id) {
                LirOp::Filter { input, predicate } => (*input, *predicate),
                _ => continue,
            };

            // Clone the child operator so we can mutate the plan while pattern-matching.
            let child_op = plan.operator(input).clone();
            let out_ty = plan.props[id].output_ty;

            let new_id = match child_op {
                LirOp::Filter {
                    input: inner,
                    predicate: inner_pred,
                } => {
                    // Filter(Filter(input, p1), p2) => Filter(input, p1 && p2)
                    let merged = merge_predicates(plan, inner_pred, predicate);
                    plan.filter(inner, merged, out_ty)
                }

                LirOp::Join {
                    kind: JoinKind::Cross,
                    left,
                    right,
                    predicate: None,
                } => {
                    // Filter(CrossJoin(l, r), p) => InnerJoin(l, r, p)
                    plan.join(JoinKind::Inner, left, right, Some(predicate), out_ty)
                }

                LirOp::Join {
                    kind: JoinKind::Inner,
                    left,
                    right,
                    predicate: join_pred,
                } => {
                    // Filter(InnerJoin(l, r, jp), p) => InnerJoin(l, r, jp && p)
                    let new_pred = join_pred.map(|jp| merge_predicates(plan, jp, predicate));
                    plan.join(JoinKind::Inner, left, right, new_pred.or(Some(predicate)), out_ty)
                }

                _ => continue,
            };

            rewrites.insert(id, new_id);
        }

        apply_id_rewrites(plan, &rewrites);
        Ok(!rewrites.is_empty())
    }
}

/// Combine two predicates over the same row into a single predicate.
///
/// If either predicate is a single-parameter closure, the result is a closure
/// over the same parameter whose body is the conjunction of the bodies.  This
/// preserves the invariant that filter predicates are closures when the
/// surrounding pipeline expects them to be.
fn merge_predicates(plan: &mut LogicalPlan, left: QExprId, right: QExprId) -> QExprId {
    let left_closure = as_closure(plan, left);
    let right_closure = as_closure(plan, right);

    match (left_closure, right_closure) {
        (Some((lp, lb)), Some((rp, rb))) if lp == rp => {
            // Same closure parameter: collapse into one closure.
            let conj = build_and(plan, lb, rb);
            make_closure(plan, lp, conj)
        }
        (Some((lp, lb)), Some((rp, rb))) => {
            // Different parameters: keep both binders in scope.  Rename would be
            // required for correctness; as a safe fallback, build the conjunction
            // of the two closure bodies by substituting one parameter for the
            // other.  Because the two predicates are over the same input row,
            // the parameters are semantically equivalent.
            let mut subst = FxHashMap::default();
            subst.insert(rp, plan.alloc_expr(QExpr::Column(lp, bool_ty(plan, left))));
            let substituted = crate::util::subst::subst_columns(plan, rb, &subst);
            let conj = build_and(plan, lb, substituted);
            make_closure(plan, lp, conj)
        }
        (Some((lp, lb)), None) => {
            let conj = build_and(plan, lb, right);
            make_closure(plan, lp, conj)
        }
        (None, Some((rp, rb))) => {
            let conj = build_and(plan, left, rb);
            make_closure(plan, rp, conj)
        }
        (None, None) => build_and(plan, left, right),
    }
}

fn build_and(plan: &mut LogicalPlan, l: QExprId, r: QExprId) -> QExprId {
    let ty = bool_ty(plan, l);
    plan.alloc_expr(QExpr::Binary(QBinaryOp::And, l, r, ty))
}

fn make_closure(plan: &mut LogicalPlan, param: BinderId, body: QExprId) -> QExprId {
    let free = crate::util::subst::free_binders(plan, body);
    let captures: Vec<_> = free.into_iter().filter(|b| *b != param).collect();
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures,
        ty: bool_ty(plan, body),
    })
}

fn bool_ty(plan: &LogicalPlan, expr: QExprId) -> TyId {
    plan.expr(expr).ty()
}
