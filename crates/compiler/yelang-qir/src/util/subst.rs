//! Capture-aware substitution of pipeline binders inside QIR scalar expressions.
//!
//! Rewrites such as filter push-down and map fusion need to replace a column
//! reference (`QExpr::Column`) by another expression.  This module performs that
//! substitution while respecting local binders introduced by closures, `let`,
//! and pattern matches, so an inner parameter never accidentally captures a
//! replacement expression from an outer scope.

use yelang_arena::FxHashMap;
use yelang_arena::FxHashSet;

use crate::expr::{AggregateCall, MatchArm, Pattern, QExpr, QExprId};
use crate::ids::BinderId;
use crate::lir::plan::LogicalPlan;

/// Replace every occurrence of the binders in `subst` by the corresponding
/// expression.  Local binders shadow the substitution in the obvious way.
///
/// Returns the original expression id when no replacement occurs.
pub fn subst_columns(
    plan: &mut LogicalPlan,
    expr: QExprId,
    subst: &FxHashMap<BinderId, QExprId>,
) -> QExprId {
    if subst.is_empty() {
        return expr;
    }
    subst_rec(plan, expr, subst)
}

fn subst_rec(
    plan: &mut LogicalPlan,
    expr: QExprId,
    subst: &FxHashMap<BinderId, QExprId>,
) -> QExprId {
    let node = plan.expr(expr).clone();
    match node {
        QExpr::Column(b, _) => subst.get(&b).copied().unwrap_or(expr),

        QExpr::Lit(_, _) | QExpr::Error(_) => expr,

        QExpr::Field(base, field, ty) => {
            let b = subst_rec(plan, base, subst);
            if b == base {
                expr
            } else {
                plan.alloc_expr(QExpr::Field(b, field, ty))
            }
        }

        QExpr::Index(base, idx, ty) => {
            let b = subst_rec(plan, base, subst);
            let i = subst_rec(plan, idx, subst);
            if b == base && i == idx {
                expr
            } else {
                plan.alloc_expr(QExpr::Index(b, i, ty))
            }
        }

        QExpr::Binary(op, l, r, ty) => {
            let l2 = subst_rec(plan, l, subst);
            let r2 = subst_rec(plan, r, subst);
            if l2 == l && r2 == r {
                expr
            } else {
                plan.alloc_expr(QExpr::Binary(op, l2, r2, ty))
            }
        }

        QExpr::Unary(op, e, ty) => {
            let e2 = subst_rec(plan, e, subst);
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Unary(op, e2, ty))
            }
        }

        QExpr::Call(def, args, ty) => {
            let args2 = subst_many(plan, &args, subst);
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
            let r2 = subst_rec(plan, receiver, subst);
            let args2 = subst_many(plan, &args, subst);
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
                .map(|(name, e)| (*name, subst_rec(plan, *e, subst)))
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
            let elems2 = subst_many(plan, &elems, subst);
            if elems2.iter().zip(elems.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::Tuple(elems2, ty))
            }
        }

        QExpr::Array(elems, ty) => {
            let elems2 = subst_many(plan, &elems, subst);
            if elems2.iter().zip(elems.iter()).all(|(a, b)| a == b) {
                expr
            } else {
                plan.alloc_expr(QExpr::Array(elems2, ty))
            }
        }

        QExpr::If(c, t, e, ty) => {
            let c2 = subst_rec(plan, c, subst);
            let t2 = subst_rec(plan, t, subst);
            let e2 = subst_rec(plan, e, subst);
            if c2 == c && t2 == t && e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::If(c2, t2, e2, ty))
            }
        }

        QExpr::Match { scrutinee, arms, ty } => {
            let s2 = subst_rec(plan, scrutinee, subst);
            let mut changed = s2 != scrutinee;
            let arms2: Vec<_> = arms
                .iter()
                .map(|arm| {
                    let mut bound_here: Vec<BinderId> = Vec::new();
                    collect_pattern_binders(&arm.pat, &mut bound_here);
                    let subst2 = without_binders(subst, &bound_here);
                    let guard2 = arm.guard.map(|g| subst_rec(plan, g, &subst2));
                    let body2 = subst_rec(plan, arm.body, &subst2);
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
            captures: _,
            ty,
        } => {
            let subst2 = without_binders(subst, &params);
            let body2 = subst_rec(plan, body, &subst2);
            if body2 == body {
                expr
            } else {
                let mut free = FxHashSet::default();
                let mut bound = FxHashSet::default();
                for p in &params {
                    bound.insert(*p);
                }
                collect_free_binders(plan, body2, &bound, &mut free);
                plan.alloc_expr(QExpr::Closure {
                    params,
                    body: body2,
                    captures: free.into_iter().collect(),
                    ty,
                })
            }
        }

        QExpr::Let { name, value, body, ty } => {
            let value2 = subst_rec(plan, value, subst);
            let subst2 = without_binder(subst, name);
            let body2 = subst_rec(plan, body, &subst2);
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
            let input2 = subst_rec(plan, call.input, subst);
            let per_row2 = subst_rec(plan, call.per_row, subst);
            if input2 == call.input && per_row2 == call.per_row {
                expr
            } else {
                plan.alloc_expr(QExpr::AggregateCall(
                    AggregateCall {
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
            let input2 = subst_rec(plan, input, subst);
            let partition2 = subst_many(plan, &partition, subst);
            let order2: Vec<_> = order
                .iter()
                .map(|k| {
                    let e2 = subst_rec(plan, k.expr, subst);
                    crate::expr::OrderKey { expr: e2, ..k.clone() }
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

        QExpr::Cast(e, kind, ty) => {
            let e2 = subst_rec(plan, e, subst);
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Cast(e2, kind, ty))
            }
        }

        QExpr::Subplan(_, _) => {
            // Subplans contain whole operator trees; they are rewritten by the
            // operator-level passes, not by expression substitution.
            expr
        }
    }
}

fn subst_many(
    plan: &mut LogicalPlan,
    exprs: &[QExprId],
    subst: &FxHashMap<BinderId, QExprId>,
) -> Vec<QExprId> {
    exprs.iter().map(|e| subst_rec(plan, *e, subst)).collect()
}

fn without_binders(
    subst: &FxHashMap<BinderId, QExprId>,
    binders: &[BinderId],
) -> FxHashMap<BinderId, QExprId> {
    let mut out = subst.clone();
    for b in binders {
        out.remove(b);
    }
    out
}

fn without_binder(
    subst: &FxHashMap<BinderId, QExprId>,
    binder: BinderId,
) -> FxHashMap<BinderId, QExprId> {
    let mut out = subst.clone();
    out.remove(&binder);
    out
}

fn collect_pattern_binders(pat: &Pattern, out: &mut Vec<BinderId>) {
    match pat {
        Pattern::Wild | Pattern::Literal(_) => {}
        Pattern::Bind(b, _) => out.push(*b),
        Pattern::Record(fields) => {
            for (_, p) in fields {
                collect_pattern_binders(p, out);
            }
        }
        Pattern::Tuple(elems) | Pattern::Array(elems) => {
            for p in elems {
                collect_pattern_binders(p, out);
            }
        }
    }
}

/// Collect the free `BinderId`s of `expr`, ignoring binders in `bound`.
pub fn free_binders(plan: &LogicalPlan, expr: QExprId) -> FxHashSet<BinderId> {
    let mut free = FxHashSet::default();
    let bound = FxHashSet::default();
    collect_free_binders(plan, expr, &bound, &mut free);
    free
}

fn collect_free_binders(
    plan: &LogicalPlan,
    expr: QExprId,
    bound: &FxHashSet<BinderId>,
    free: &mut FxHashSet<BinderId>,
) {
    match plan.expr(expr) {
        QExpr::Column(b, _) => {
            if !bound.contains(b) {
                free.insert(*b);
            }
        }

        QExpr::Lit(_, _) | QExpr::Error(_) => {}

        QExpr::Field(base, _, _) | QExpr::Cast(base, _, _) | QExpr::Unary(_, base, _) => {
            collect_free_binders(plan, *base, bound, free);
        }

        QExpr::Index(base, idx, _) | QExpr::Binary(_, base, idx, _) => {
            collect_free_binders(plan, *base, bound, free);
            collect_free_binders(plan, *idx, bound, free);
        }

        QExpr::Call(_, args, _) => {
            for a in args {
                collect_free_binders(plan, *a, bound, free);
            }
        }

        QExpr::MethodCall { receiver, args, .. } => {
            collect_free_binders(plan, *receiver, bound, free);
            for a in args {
                collect_free_binders(plan, *a, bound, free);
            }
        }

        QExpr::Record(fields, _) => {
            for (_, e) in fields {
                collect_free_binders(plan, *e, bound, free);
            }
        }

        QExpr::Tuple(elems, _) | QExpr::Array(elems, _) => {
            for e in elems {
                collect_free_binders(plan, *e, bound, free);
            }
        }

        QExpr::If(c, t, e, _) => {
            collect_free_binders(plan, *c, bound, free);
            collect_free_binders(plan, *t, bound, free);
            collect_free_binders(plan, *e, bound, free);
        }

        QExpr::Match { scrutinee, arms, .. } => {
            collect_free_binders(plan, *scrutinee, bound, free);
            for arm in arms {
                let mut bound_here = bound.clone();
                collect_pattern_binders_into_set(&arm.pat, &mut bound_here);
                if let Some(g) = arm.guard {
                    collect_free_binders(plan, g, &bound_here, free);
                }
                collect_free_binders(plan, arm.body, &bound_here, free);
            }
        }

        QExpr::Closure { params, body, .. } => {
            let mut bound_here = bound.clone();
            for p in params {
                bound_here.insert(*p);
            }
            collect_free_binders(plan, *body, &bound_here, free);
        }

        QExpr::Let { name, value, body, .. } => {
            collect_free_binders(plan, *value, bound, free);
            let mut bound_here = bound.clone();
            bound_here.insert(*name);
            collect_free_binders(plan, *body, &bound_here, free);
        }

        QExpr::AggregateCall(call, _) => {
            collect_free_binders(plan, call.input, bound, free);
            collect_free_binders(plan, call.per_row, bound, free);
        }

        QExpr::WindowCall {
            input,
            partition,
            order,
            ..
        } => {
            collect_free_binders(plan, *input, bound, free);
            for e in partition {
                collect_free_binders(plan, *e, bound, free);
            }
            for k in order {
                collect_free_binders(plan, k.expr, bound, free);
            }
        }

        QExpr::Subplan(_, _) => {
            // Treat subplans as closed terms for expression-level analysis.
        }
    }
}

fn collect_pattern_binders_into_set(pat: &Pattern, bound: &mut FxHashSet<BinderId>) {
    match pat {
        Pattern::Wild | Pattern::Literal(_) => {}
        Pattern::Bind(b, _) => {
            bound.insert(*b);
        }
        Pattern::Record(fields) => {
            for (_, p) in fields {
                collect_pattern_binders_into_set(p, bound);
            }
        }
        Pattern::Tuple(elems) | Pattern::Array(elems) => {
            for p in elems {
                collect_pattern_binders_into_set(p, bound);
            }
        }
    }
}
