//! Subquery unnesting: convert scalar subplans embedded in expressions into
//! joins.
//!
//! A `QExpr::Subplan(lir, ty)` inside a filter predicate, projection, or other
//! unary operator expression is rewritten so that the subplan becomes a sibling
//! input of the operator.  The operator's row binder is re-bound to the joined
//! pair `{ left: original_row, right: subquery_result }` and every occurrence
//! of the original row is replaced by a field access on the left component.
//!
//! Correlated subplans become `DependentJoin`s and are then flattened by the
//! dedicated `DecorrelatePass`.  Uncorrelated subplans become cross joins.

use yelang_arena::FxHashSet;
use yelang_interner::Symbol;

use crate::errors::LoweringError;
use crate::expr::{MatchArm, QExpr, QExprId};
use crate::ids::{BinderId, LirId};
use crate::logical::operator::{JoinKind, LirOp};
use crate::logical::plan::LogicalPlan;
use crate::logical::props::CardinalityClass;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::reachable_ids;
use crate::util::subst::free_binders;

fn left_field() -> Symbol {
    Symbol::from(1)
}
fn right_field() -> Symbol {
    Symbol::from(2)
}

pub struct UnnestSubqueriesPass;

impl RewritePass for UnnestSubqueriesPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids = reachable_ids(plan);
        let mut changed = false;
        for id in ids {
            changed |= unnest_operator(plan, id)?;
        }
        Ok(changed)
    }
}

/// Try to unnest every scalar subplan embedded in `id`'s expressions.
fn unnest_operator(plan: &mut LogicalPlan, id: LirId) -> Result<bool, LoweringError> {
    let mut changed = false;
    loop {
        let op = plan.operator(id).clone();
        let Some(input) = single_input(&op) else {
            break;
        };
        let Some((sub_lir, sub_ty)) = find_first_subplan(plan, &op) else {
            break;
        };

        // Only unnest subplans that are guaranteed to produce at most one row.
        // Collection-valued subplans (EXISTS / IN) need semi/anti-join handling
        // and are left for a later pass.
        let cardinality = plan.props.get(sub_lir).map(|p| p.cardinality).unwrap_or(CardinalityClass::Many);
        if !cardinality.is_scalar() {
            break;
        }

        unnest_once(plan, id, input, sub_lir, sub_ty)?;
        changed = true;
    }
    Ok(changed)
}

/// Single out the one logical child that supplies the row binder for this
/// operator's expressions.  Binary operators are intentionally excluded because
/// their expressions may reference either side.
fn single_input(op: &LirOp) -> Option<LirId> {
    match op {
        LirOp::Filter { input, .. }
        | LirOp::Map { input, .. }
        | LirOp::FlatMap { input, .. }
        | LirOp::OrderBy { input, .. }
        | LirOp::Slice { input, .. }
        | LirOp::Distinct { input, .. }
        | LirOp::GroupBy { input, .. }
        | LirOp::Aggregate { input, .. }
        | LirOp::AggregateGroupBy { input, .. }
        | LirOp::EdgeExpand { input, .. }
        | LirOp::Window { input, .. } => Some(*input),
        _ => None,
    }
}

/// Find the first scalar subplan referenced by any expression in `op`.
fn find_first_subplan(plan: &LogicalPlan, op: &LirOp) -> Option<(LirId, yelang_ty::ty::TyId)> {
    for expr in op.expressions() {
        if let Some(found) = find_subplan_in_expr(plan, expr) {
            return Some(found);
        }
    }
    None
}

fn find_subplan_in_expr(
    plan: &LogicalPlan,
    expr: QExprId,
) -> Option<(LirId, yelang_ty::ty::TyId)> {
    match plan.expr(expr) {
        QExpr::Subplan(lir, ty) => Some((*lir, *ty)),
        QExpr::Field(base, _, _) | QExpr::Cast(base, _) | QExpr::Unary(_, base, _) => {
            find_subplan_in_expr(plan, *base)
        }
        QExpr::Index(base, idx, _) | QExpr::Binary(_, base, idx, _) => {
            find_subplan_in_expr(plan, *base).or_else(|| find_subplan_in_expr(plan, *idx))
        }
        QExpr::Call(_, args, _) | QExpr::Tuple(args, _) | QExpr::Array(args, _) => {
            args.iter().find_map(|a| find_subplan_in_expr(plan, *a))
        }
        QExpr::MethodCall { receiver, args, .. } => {
            find_subplan_in_expr(plan, *receiver).or_else(|| {
                args.iter().find_map(|a| find_subplan_in_expr(plan, *a))
            })
        }
        QExpr::Record(fields, _) => fields
            .iter()
            .find_map(|(_, e)| find_subplan_in_expr(plan, *e)),
        QExpr::If(c, t, e, _) => find_subplan_in_expr(plan, *c)
            .or_else(|| find_subplan_in_expr(plan, *t))
            .or_else(|| find_subplan_in_expr(plan, *e)),
        QExpr::Match { scrutinee, arms, .. } => find_subplan_in_expr(plan, *scrutinee).or_else(|| {
            arms.iter().find_map(|arm| {
                arm.guard
                    .and_then(|g| find_subplan_in_expr(plan, g))
                    .or_else(|| find_subplan_in_expr(plan, arm.body))
            })
        }),
        QExpr::Closure { body, .. } | QExpr::Let { body, .. } => find_subplan_in_expr(plan, *body),
        QExpr::AggregateCall(call, _) => {
            find_subplan_in_expr(plan, call.input).or_else(|| find_subplan_in_expr(plan, call.per_row))
        }
        QExpr::WindowCall {
            input,
            partition,
            order,
            ..
        } => find_subplan_in_expr(plan, *input)
            .or_else(|| partition.iter().find_map(|e| find_subplan_in_expr(plan, *e)))
            .or_else(|| order.iter().find_map(|k| find_subplan_in_expr(plan, k.expr))),
        QExpr::Lit(_, _) | QExpr::Column(_, _) | QExpr::Error(_) => None,
    }
}

/// Rewrite a single scalar subplan occurrence inside operator `id`.
fn unnest_once(
    plan: &mut LogicalPlan,
    id: LirId,
    input: LirId,
    sub_lir: LirId,
    sub_ty: yelang_ty::ty::TyId,
) -> Result<(), LoweringError> {
    let input_binder = plan.props[input]
        .output_binder
        .unwrap_or_else(|| plan.fresh_binder());
    let correlated = subplan_correlated(plan, sub_lir, input_binder);

    let input_ty = plan.props[input].output_ty;
    // The joined row type is a synthetic pair.  We reuse the input type as the
    // row type for the join because QIR has already been type-checked; the only
    // consumer of this type is the `Column` expression that names the pair.
    let join_ty = input_ty;

    let join_id = if correlated {
        plan.dependent_join(input, sub_lir, None, join_ty)
    } else {
        plan.join(JoinKind::Cross, input, sub_lir, None, join_ty)
    };
    plan.props[join_id].output_binder = Some(input_binder);

    let row_col = plan.alloc_expr(QExpr::Column(input_binder, join_ty));
    let left_expr = plan.alloc_expr(QExpr::Field(row_col, left_field(), input_ty));
    let right_expr = plan.alloc_expr(QExpr::Field(row_col, right_field(), sub_ty));

    let mut op = plan.operator(id).clone();
    op.map_children(|child| if child == input { join_id } else { child });
    op.map_expressions(|expr| {
        rewrite_expr(plan, expr, input_binder, left_expr, sub_lir, right_expr)
    });

    plan.operators[id] = op;
    // The operator's output binder now names the synthetic joined row.
    plan.props[id].output_binder = Some(input_binder);
    Ok(())
}

/// True when `sub_lir` references `outer_binder` from the surrounding operator.
fn subplan_correlated(plan: &LogicalPlan, sub_lir: LirId, outer_binder: BinderId) -> bool {
    let mut free = FxHashSet::default();
    collect_subplan_free_binders(plan, sub_lir, &mut free);
    free.contains(&outer_binder)
}

fn collect_subplan_free_binders(plan: &LogicalPlan, id: LirId, free: &mut FxHashSet<BinderId>) {
    let op = plan.operator(id);
    for expr in op.expressions() {
        for b in free_binders(plan, expr) {
            free.insert(b);
        }
    }
    for child in op.children() {
        collect_subplan_free_binders(plan, child, free);
    }
    for b in introduced_binders(plan, id) {
        free.remove(&b);
    }
}

/// Binders introduced by `id` and made available to its downstream operators.
fn introduced_binders(plan: &LogicalPlan, id: LirId) -> Vec<BinderId> {
    let op = plan.operator(id);
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
        | LirOp::DependentJoin { .. } => plan.props[id].output_binder.into_iter().collect(),
        _ => vec![],
    }
}

/// Rewrite an expression for subquery unnesting.
///
/// Every reference to the original row binder is rewritten to access the left
/// field of the synthetic joined row, and the target subplan is replaced by a
/// right-field access.  Local binders are not considered shadowing here because
/// `input_binder` is the operator's pipeline binder, not a lambda parameter.
fn rewrite_expr(
    plan: &mut LogicalPlan,
    expr: QExprId,
    input_binder: BinderId,
    left_expr: QExprId,
    sub_lir: LirId,
    right_expr: QExprId,
) -> QExprId {
    let node = plan.expr(expr).clone();
    match node {
        QExpr::Column(b, _) if b == input_binder => left_expr,
        QExpr::Subplan(lir, _) if lir == sub_lir => right_expr,
        QExpr::Subplan(_, _) => expr,

        QExpr::Lit(_, _) | QExpr::Column(_, _) | QExpr::Error(_) => expr,

        QExpr::Field(base, field, ty) => {
            let b = rewrite_expr(plan, base, input_binder, left_expr, sub_lir, right_expr);
            if b == base {
                expr
            } else {
                plan.alloc_expr(QExpr::Field(b, field, ty))
            }
        }

        QExpr::Index(base, idx, ty) => {
            let b = rewrite_expr(plan, base, input_binder, left_expr, sub_lir, right_expr);
            let i = rewrite_expr(plan, idx, input_binder, left_expr, sub_lir, right_expr);
            if b == base && i == idx {
                expr
            } else {
                plan.alloc_expr(QExpr::Index(b, i, ty))
            }
        }

        QExpr::Binary(op, l, r, ty) => {
            let l2 = rewrite_expr(plan, l, input_binder, left_expr, sub_lir, right_expr);
            let r2 = rewrite_expr(plan, r, input_binder, left_expr, sub_lir, right_expr);
            if l2 == l && r2 == r {
                expr
            } else {
                plan.alloc_expr(QExpr::Binary(op, l2, r2, ty))
            }
        }

        QExpr::Unary(op, e, ty) => {
            let e2 = rewrite_expr(plan, e, input_binder, left_expr, sub_lir, right_expr);
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Unary(op, e2, ty))
            }
        }

        QExpr::Call(def, args, ty) => {
            let args2: Vec<_> = args
                .iter()
                .map(|a| rewrite_expr(plan, *a, input_binder, left_expr, sub_lir, right_expr))
                .collect();
            if args2.iter().zip(args.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::Call(def, args2, ty))
            }
        }

        QExpr::MethodCall {
            receiver,
            method,
            args,
            ty,
        } => {
            let r2 = rewrite_expr(plan, receiver, input_binder, left_expr, sub_lir, right_expr);
            let args2: Vec<_> = args
                .iter()
                .map(|a| rewrite_expr(plan, *a, input_binder, left_expr, sub_lir, right_expr))
                .collect();
            if r2 == receiver && args2.iter().zip(args.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::MethodCall {
                    receiver: r2,
                    method,
                    args: args2,
                    ty,
                })
            }
        }

        QExpr::Record(fields, ty) => {
            let fields2: Vec<_> = fields
                .iter()
                .map(|(name, e)| (*name, rewrite_expr(plan, *e, input_binder, left_expr, sub_lir, right_expr)))
                .collect();
            if fields2
                .iter()
                .zip(fields.iter())
                .all(|((_, a), (_, b))| a == b)
            {
                expr
            } else {
                plan.alloc_expr(QExpr::Record(fields2, ty))
            }
        }

        QExpr::Tuple(elems, ty) => {
            let elems2: Vec<_> = elems
                .iter()
                .map(|e| rewrite_expr(plan, *e, input_binder, left_expr, sub_lir, right_expr))
                .collect();
            if elems2.iter().zip(elems.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::Tuple(elems2, ty))
            }
        }

        QExpr::Array(elems, ty) => {
            let elems2: Vec<_> = elems
                .iter()
                .map(|e| rewrite_expr(plan, *e, input_binder, left_expr, sub_lir, right_expr))
                .collect();
            if elems2.iter().zip(elems.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::Array(elems2, ty))
            }
        }

        QExpr::If(c, t, e, ty) => {
            let c2 = rewrite_expr(plan, c, input_binder, left_expr, sub_lir, right_expr);
            let t2 = rewrite_expr(plan, t, input_binder, left_expr, sub_lir, right_expr);
            let e2 = rewrite_expr(plan, e, input_binder, left_expr, sub_lir, right_expr);
            if c2 == c && t2 == t && e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::If(c2, t2, e2, ty))
            }
        }

        QExpr::Match { scrutinee, arms, ty } => {
            let s2 = rewrite_expr(plan, scrutinee, input_binder, left_expr, sub_lir, right_expr);
            let mut changed = s2 != scrutinee;
            let arms2: Vec<_> = arms
                .iter()
                .map(|arm| {
                    let guard2 = arm
                        .guard
                        .map(|g| rewrite_expr(plan, g, input_binder, left_expr, sub_lir, right_expr));
                    let body2 = rewrite_expr(plan, arm.body, input_binder, left_expr, sub_lir, right_expr);
                    changed |= guard2 != arm.guard || body2 != arm.body;
                    MatchArm {
                        pat: arm.pat.clone(),
                        guard: guard2,
                        body: body2,
                    }
                })
                .collect();
            if !changed {
                expr
            } else {
                plan.alloc_expr(QExpr::Match {
                    scrutinee: s2,
                    arms: arms2,
                    ty,
                })
            }
        }

        QExpr::Closure {
            params,
            body,
            captures,
            ty,
        } => {
            let body2 = rewrite_expr(plan, body, input_binder, left_expr, sub_lir, right_expr);
            if body2 == body {
                expr
            } else {
                plan.alloc_expr(QExpr::Closure {
                    params,
                    body: body2,
                    captures,
                    ty,
                })
            }
        }

        QExpr::Let { name, value, body, ty } => {
            let value2 = rewrite_expr(plan, value, input_binder, left_expr, sub_lir, right_expr);
            let body2 = rewrite_expr(plan, body, input_binder, left_expr, sub_lir, right_expr);
            if value2 == value && body2 == body {
                expr
            } else {
                plan.alloc_expr(QExpr::Let {
                    name,
                    value: value2,
                    body: body2,
                    ty,
                })
            }
        }

        QExpr::AggregateCall(call, ty) => {
            let input2 = rewrite_expr(plan, call.input, input_binder, left_expr, sub_lir, right_expr);
            let per_row2 = rewrite_expr(plan, call.per_row, input_binder, left_expr, sub_lir, right_expr);
            if input2 == call.input && per_row2 == call.per_row {
                expr
            } else {
                plan.alloc_expr(QExpr::AggregateCall(
                    crate::expr::AggregateCall {
                        input: input2,
                        per_row: per_row2,
                        ..call
                    },
                    ty,
                ))
            }
        }

        QExpr::WindowCall {
            input,
            func,
            partition,
            order,
            frame,
            ty,
        } => {
            let input2 = rewrite_expr(plan, input, input_binder, left_expr, sub_lir, right_expr);
            let partition2: Vec<_> = partition
                .iter()
                .map(|e| rewrite_expr(plan, *e, input_binder, left_expr, sub_lir, right_expr))
                .collect();
            let order2: Vec<_> = order
                .iter()
                .map(|k| crate::expr::OrderKey {
                    expr: rewrite_expr(plan, k.expr, input_binder, left_expr, sub_lir, right_expr),
                    ..k.clone()
                })
                .collect();
            if input2 == input
                && partition2.iter().zip(partition.iter()).all(|(a, b)| a == b)
                && order2.iter().zip(order.iter()).all(|(a, b)| a.expr == b.expr)
            {
                expr
            } else {
                plan.alloc_expr(QExpr::WindowCall {
                    input: input2,
                    func,
                    partition: partition2,
                    order: order2,
                    frame,
                    ty,
                })
            }
        }

        QExpr::Cast(e, ty) => {
            let e2 = rewrite_expr(plan, e, input_binder, left_expr, sub_lir, right_expr);
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Cast(e2, ty))
            }
        }
    }
}

impl CardinalityClass {
    fn is_scalar(self) -> bool {
        matches!(self, CardinalityClass::One | CardinalityClass::ZeroOrOne)
    }
}
