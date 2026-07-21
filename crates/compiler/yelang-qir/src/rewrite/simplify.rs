//! Simplification rewrite: constant folding and boolean / arithmetic identities.
//!
//! All `QExpr` nodes are side-effect-free by construction, so short-circuit
//! rewrites such as `x && false -> false` are valid without needing to prove
//! purity of `x`.

use crate::errors::LoweringError;
use crate::expr::{QBinaryOp, QExpr, QExprId, QLit, QUnaryOp};
use crate::lir::operator::LirOp;
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct SimplifyPass;

impl RewritePass for SimplifyPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let ids: Vec<_> = crate::rewrite::reachable_ids(plan);

        let mut changed = false;
        for id in ids {
            let op = plan.operator(id).clone();
            let (new_op, op_changed) = simplify_op(plan, op);
            if op_changed {
                *plan.operator_mut(id) = new_op;
                changed = true;
            }
        }
        Ok(changed)
    }
}

fn simplify_op(plan: &mut LogicalPlan, op: LirOp) -> (LirOp, bool) {
    let mut changed = false;
    let mut simplify = |e: QExprId| -> QExprId {
        let e2 = simplify_expr(plan, e);
        changed |= e2 != e;
        e2
    };

    let new_op = match op {
        LirOp::Filter { input, predicate } => LirOp::Filter {
            input,
            predicate: simplify(predicate),
        },
        LirOp::Map {
            input,
            projection,
        } => LirOp::Map {
            input,
            projection: simplify(projection),
        },
        LirOp::FlatMap {
            input,
            projection,
        } => LirOp::FlatMap {
            input,
            projection: simplify(projection),
        },
        LirOp::Join {
            kind,
            left,
            right,
            predicate,
        } => LirOp::Join {
            kind,
            left,
            right,
            predicate: predicate.map(&mut simplify),
        },
        LirOp::DependentJoin {
            outer,
            inner,
            predicate,
        } => LirOp::DependentJoin {
            outer,
            inner,
            predicate: predicate.map(&mut simplify),
        },
        LirOp::Distinct { input, by } => LirOp::Distinct {
            input,
            by: by.map(|keys| keys.into_iter().map(&mut simplify).collect()),
        },
        LirOp::GroupBy {
            input,
            key,
            key_ty,
            vals_label,
        } => LirOp::GroupBy {
            input,
            key: simplify(key),
            key_ty,
            vals_label,
        },
        LirOp::Aggregate { input, agg } => {
            let mut agg = agg;
            agg.per_row = simplify(agg.per_row);
            LirOp::Aggregate { input, agg }
        }
        LirOp::AggregateGroupBy {
            input,
            group_keys,
            mut aggregates,
        } => {
            for agg in aggregates.iter_mut() {
                agg.per_row = simplify(agg.per_row);
            }
            LirOp::AggregateGroupBy {
                input,
                group_keys,
                aggregates,
            }
        }
        LirOp::OrderBy { input, keys } => LirOp::OrderBy {
            input,
            keys: keys
                .into_iter()
                .map(|mut k| {
                    k.expr = simplify(k.expr);
                    k
                })
                .collect(),
        },
        LirOp::Slice {
            input,
            offset,
            limit,
        } => LirOp::Slice {
            input,
            offset: simplify(offset),
            limit: limit.map(&mut simplify),
        },
        LirOp::Window {
            input,
            func,
            partition,
            order,
            frame,
        } => LirOp::Window {
            input,
            func,
            partition: partition.into_iter().map(&mut simplify).collect(),
            order: order
                .into_iter()
                .map(|mut k| {
                    k.expr = simplify(k.expr);
                    k
                })
                .collect(),
            frame,
        },
        other => other,
    };

    (new_op, changed)
}

/// Recursively constant-fold and apply algebraic identities to an expression.
///
/// Returns the original `QExprId` when no simplification applies.
pub fn simplify_expr(plan: &mut LogicalPlan, expr: QExprId) -> QExprId {
    let node = plan.expr(expr).clone();

    match node {
        QExpr::Binary(op, l, r, ty) => {
            let l2 = simplify_expr(plan, l);
            let r2 = simplify_expr(plan, r);
            if let Some(folded) = simplify_binary(plan, op, l2, r2, ty) {
                return folded;
            }
            if l2 == l && r2 == r {
                expr
            } else {
                plan.alloc_expr(QExpr::Binary(op, l2, r2, ty))
            }
        }
        QExpr::Unary(op, e, ty) => {
            let e2 = simplify_expr(plan, e);
            if let Some(folded) = simplify_unary(plan, op, e2, ty) {
                return folded;
            }
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Unary(op, e2, ty))
            }
        }
        QExpr::If(c, t, e, ty) => {
            let c2 = simplify_expr(plan, c);
            match plan.expr(c2) {
                QExpr::Lit(QLit::Bool(true), _) => return simplify_expr(plan, t),
                QExpr::Lit(QLit::Bool(false), _) => return simplify_expr(plan, e),
                _ => {
                    let t2 = simplify_expr(plan, t);
                    let e2 = simplify_expr(plan, e);
                    if c2 == c && t2 == t && e2 == e {
                        expr
                    } else {
                        plan.alloc_expr(QExpr::If(c2, t2, e2, ty))
                    }
                }
            }
        }
        QExpr::Field(base, field, ty) => {
            let b2 = simplify_expr(plan, base);
            if b2 == base {
                expr
            } else {
                plan.alloc_expr(QExpr::Field(b2, field, ty))
            }
        }
        QExpr::Index(base, idx, ty) => {
            let b2 = simplify_expr(plan, base);
            let i2 = simplify_expr(plan, idx);
            if b2 == base && i2 == idx {
                expr
            } else {
                plan.alloc_expr(QExpr::Index(b2, i2, ty))
            }
        }
        QExpr::Record(fields, ty) => {
            let mut changed = false;
            let fields2: Vec<_> = fields
                .iter()
                .map(|(name, e)| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    (*name, e2)
                })
                .collect();
            if !changed {
                expr
            } else {
                plan.alloc_expr(QExpr::Record(fields2, ty))
            }
        }
        QExpr::Tuple(elems, ty) => {
            let mut changed = false;
            let elems2: Vec<_> = elems
                .iter()
                .map(|e| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    e2
                })
                .collect();
            if !changed {
                expr
            } else {
                plan.alloc_expr(QExpr::Tuple(elems2, ty))
            }
        }
        QExpr::Array(elems, ty) => {
            let mut changed = false;
            let elems2: Vec<_> = elems
                .iter()
                .map(|e| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    e2
                })
                .collect();
            if !changed {
                expr
            } else {
                plan.alloc_expr(QExpr::Array(elems2, ty))
            }
        }
        QExpr::Call(def, args, ty) => {
            let mut changed = false;
            let args2: Vec<_> = args
                .iter()
                .map(|e| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    e2
                })
                .collect();
            if !changed {
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
            let r2 = simplify_expr(plan, receiver);
            let mut changed = r2 != receiver;
            let args2: Vec<_> = args
                .iter()
                .map(|e| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    e2
                })
                .collect();
            if !changed {
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
        QExpr::Cast(e, kind, ty) => {
            let e2 = simplify_expr(plan, e);
            if e2 == e {
                expr
            } else {
                plan.alloc_expr(QExpr::Cast(e2, kind, ty))
            }
        }
        QExpr::AggregateCall(call, ty) => {
            let input2 = simplify_expr(plan, call.input);
            let per_row2 = simplify_expr(plan, call.per_row);
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
            let input2 = simplify_expr(plan, input);
            let mut changed = input2 != input;
            let partition2: Vec<_> = partition
                .iter()
                .map(|e| {
                    let e2 = simplify_expr(plan, *e);
                    changed |= e2 != *e;
                    e2
                })
                .collect();
            let order2: Vec<_> = order
                .iter()
                .map(|k| {
                    let e2 = simplify_expr(plan, k.expr);
                    changed |= e2 != k.expr;
                    crate::expr::OrderKey { expr: e2, ..k.clone() }
                })
                .collect();
            if !changed {
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
        _ => expr,
    }
}

fn simplify_binary(
    plan: &mut LogicalPlan,
    op: QBinaryOp,
    l: QExprId,
    r: QExprId,
    ty: yelang_ty::ty::TyId,
) -> Option<QExprId> {
    use QBinaryOp::*;

    // Boolean short-circuit and constant identities.
    match op {
        And => {
            if is_bool_lit(plan, l, false) || is_bool_lit(plan, r, false) {
                return Some(bool_lit(plan, false, ty));
            }
            if is_bool_lit(plan, l, true) {
                return Some(r);
            }
            if is_bool_lit(plan, r, true) {
                return Some(l);
            }
        }
        Or => {
            if is_bool_lit(plan, l, true) || is_bool_lit(plan, r, true) {
                return Some(bool_lit(plan, true, ty));
            }
            if is_bool_lit(plan, l, false) {
                return Some(r);
            }
            if is_bool_lit(plan, r, false) {
                return Some(l);
            }
        }
        _ => {}
    }

    // Constant folding for two literal operands.
    let (lv, rv) = (plan.expr(l), plan.expr(r));
    match (lv, rv) {
        (QExpr::Lit(QLit::Int(a), _), QExpr::Lit(QLit::Int(b), _)) => match op {
            Add => Some(int_lit(plan, a.wrapping_add(*b), ty)),
            Sub => Some(int_lit(plan, a.wrapping_sub(*b), ty)),
            Mul => Some(int_lit(plan, a.wrapping_mul(*b), ty)),
            Div if *b != 0 => Some(int_lit(plan, a.wrapping_div(*b), ty)),
            Mod if *b != 0 => Some(int_lit(plan, a.wrapping_rem(*b), ty)),
            Eq => Some(bool_lit(plan, a == b, ty)),
            Ne => Some(bool_lit(plan, a != b, ty)),
            Lt => Some(bool_lit(plan, a < b, ty)),
            Lte => Some(bool_lit(plan, a <= b, ty)),
            Gt => Some(bool_lit(plan, a > b, ty)),
            Gte => Some(bool_lit(plan, a >= b, ty)),
            _ => None,
        },
        (QExpr::Lit(QLit::Float(a), _), QExpr::Lit(QLit::Float(b), _)) => match op {
            Add => Some(float_lit(plan, a + b, ty)),
            Sub => Some(float_lit(plan, a - b, ty)),
            Mul => Some(float_lit(plan, a * b, ty)),
            Div => Some(float_lit(plan, a / b, ty)),
            Eq => Some(bool_lit(plan, a == b, ty)),
            Ne => Some(bool_lit(plan, a != b, ty)),
            Lt => Some(bool_lit(plan, a < b, ty)),
            Lte => Some(bool_lit(plan, a <= b, ty)),
            Gt => Some(bool_lit(plan, a > b, ty)),
            Gte => Some(bool_lit(plan, a >= b, ty)),
            _ => None,
        },
        (QExpr::Lit(QLit::Bool(a), _), QExpr::Lit(QLit::Bool(b), _)) => match op {
            Eq => Some(bool_lit(plan, a == b, ty)),
            Ne => Some(bool_lit(plan, a != b, ty)),
            _ => None,
        },
        _ => None,
    }
}

fn simplify_unary(
    plan: &mut LogicalPlan,
    op: QUnaryOp,
    e: QExprId,
    ty: yelang_ty::ty::TyId,
) -> Option<QExprId> {
    match op {
        QUnaryOp::Not => match plan.expr(e) {
            QExpr::Lit(QLit::Bool(v), _) => Some(bool_lit(plan, !*v, ty)),
            _ => None,
        },
        QUnaryOp::Neg => match plan.expr(e) {
            QExpr::Lit(QLit::Int(v), _) => Some(int_lit(plan, v.wrapping_neg(), ty)),
            QExpr::Lit(QLit::Float(v), _) => Some(float_lit(plan, -v, ty)),
            _ => None,
        },
        QUnaryOp::BitNot => match plan.expr(e) {
            QExpr::Lit(QLit::Int(v), _) => Some(int_lit(plan, !*v, ty)),
            _ => None,
        },
    }
}

fn is_bool_lit(plan: &LogicalPlan, expr: QExprId, value: bool) -> bool {
    matches!(plan.expr(expr), QExpr::Lit(QLit::Bool(v), _) if *v == value)
}

fn bool_lit(plan: &mut LogicalPlan, value: bool, ty: yelang_ty::ty::TyId) -> QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Bool(value), ty))
}

fn int_lit(plan: &mut LogicalPlan, value: i128, ty: yelang_ty::ty::TyId) -> QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Int(value), ty))
}

fn float_lit(plan: &mut LogicalPlan, value: f64, ty: yelang_ty::ty::TyId) -> QExprId {
    plan.alloc_expr(QExpr::Lit(QLit::Float(value), ty))
}
