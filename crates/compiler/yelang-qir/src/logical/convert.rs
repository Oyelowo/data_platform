//! Helpers for converting THIR expressions and subplans to QIR expressions.

use yelang_arena::DefId;
use yelang_thir::{ThirExpr, ThirExprId};
use yelang_ty::ty::{Ty, TyId};

use crate::errors::LoweringError;
use crate::expr::{QLit, QExpr};
use super::extract::{extract_expr, queryable_method_callee};
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::plan::LogicalPlan;

/// Lower an arbitrary THIR expression to a scalar QExpr.
pub fn lower_scalar_expr(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    expr: ThirExprId,
) -> Result<QExprId, LoweringError> {
    let Some(expr_data) = ctx.thir.exprs.get(expr) else {
        return Err(LoweringError::UnsupportedExpr);
    };
    let ty = ctx
        .thir_expr_ty(expr)
        .or_else(|| ctx.tcx.interner().mk_ty(Ty::Tuple(yelang_ty::list::List::empty())).into())
        .unwrap();

    match expr_data {
        ThirExpr::Literal(lit) => lower_literal(plan, lit, ty, ctx),
        ThirExpr::Var(def_id) => Ok(plan.alloc_expr(QExpr::Call(*def_id, vec![], ty))),
        ThirExpr::Local(pat_id) => {
            let binder = ctx
                .lookup_binder(*pat_id)
                .ok_or(LoweringError::UnsupportedExpr)?;
            Ok(plan.alloc_expr(QExpr::Column(binder, ty)))
        }
        ThirExpr::Field { base, field } => {
            let base_expr = lower_scalar_expr(plan, ctx, *base)?;
            Ok(plan.alloc_expr(QExpr::Field(base_expr, *field, ty)))
        }
        ThirExpr::Index { base, index } => {
            let base_expr = lower_scalar_expr(plan, ctx, *base)?;
            let index_expr = lower_scalar_expr(plan, ctx, *index)?;
            Ok(plan.alloc_expr(QExpr::Index(base_expr, index_expr, ty)))
        }
        ThirExpr::Binary { op, left, right } => {
            let left_expr = lower_scalar_expr(plan, ctx, *left)?;
            let right_expr = lower_scalar_expr(plan, ctx, *right)?;
            let qop = lower_binary_op(*op);
            Ok(plan.alloc_expr(QExpr::Binary(qop, left_expr, right_expr, ty)))
        }
        ThirExpr::Unary { op, expr } => {
            let inner = lower_scalar_expr(plan, ctx, *expr)?;
            let qop = lower_unary_op(*op);
            Ok(plan.alloc_expr(QExpr::Unary(qop, inner, ty)))
        }
        ThirExpr::Call { func, args } => {
            if let Some(_method_def_id) = queryable_method_callee(ctx, *func) {
                return extract_expr(plan, ctx, expr);
            }
            let _func_expr = lower_scalar_expr(plan, ctx, *func)?;
            let mut lowered_args = Vec::with_capacity(args.len());
            for &arg in args {
                lowered_args.push(lower_scalar_expr(plan, ctx, arg)?);
            }
            let def_id = function_def_id(ctx, *func).unwrap_or_else(|| DefId::new(0));
            Ok(plan.alloc_expr(QExpr::Call(def_id, lowered_args, ty)))
        }
        ThirExpr::Closure { params, body } => lower_closure(plan, ctx, params, *body, ty),
        ThirExpr::Struct { path: _, fields, rest } => {
            if rest.is_some() {
                return Err(LoweringError::UnsupportedExpr);
            }
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for (name, field_expr) in fields.iter() {
                lowered_fields.push((*name, lower_scalar_expr(plan, ctx, *field_expr)?));
            }
            Ok(plan.alloc_expr(QExpr::Record(lowered_fields, ty)))
        }
        ThirExpr::Tuple { fields } => {
            let mut lowered = Vec::with_capacity(fields.len());
            for &field in fields {
                lowered.push(lower_scalar_expr(plan, ctx, field)?);
            }
            Ok(plan.alloc_expr(QExpr::Tuple(lowered, ty)))
        }
        ThirExpr::Array { exprs } => {
            let mut lowered = Vec::with_capacity(exprs.len());
            for &e in exprs {
                lowered.push(lower_scalar_expr(plan, ctx, e)?);
            }
            Ok(plan.alloc_expr(QExpr::Array(lowered, ty)))
        }
        ThirExpr::Block { stmts, tail } => lower_block(plan, ctx, stmts, *tail, ty),
        ThirExpr::Cast { expr, ty: _ } => {
            let inner = lower_scalar_expr(plan, ctx, *expr)?;
            // TODO(phase3): determine CastKind from source/target types.
            Ok(plan.alloc_expr(QExpr::Cast(inner, crate::expr::CastKind::Numeric, ty)))
        }
        ThirExpr::If { cond, then_branch, else_branch } => {
            let cond_expr = lower_scalar_expr(plan, ctx, *cond)?;
            let then_body = ctx.thir.bodies.bodies.get(*then_branch).ok_or(LoweringError::UnsupportedExpr)?;
            let then_expr = lower_scalar_expr(plan, ctx, then_body.value)?;
            let else_expr = if let Some(else_body) = else_branch {
                let body = ctx.thir.bodies.bodies.get(*else_body).ok_or(LoweringError::UnsupportedExpr)?;
                lower_scalar_expr(plan, ctx, body.value)?
            } else {
                // Unit expression for if-without-else.
                plan.alloc_expr(QExpr::Tuple(vec![], ty))
            };
            Ok(plan.alloc_expr(QExpr::If(cond_expr, then_expr, else_expr, ty)))
        }
        ThirExpr::Match { scrutinee, arms } => {
            let scrut_expr = lower_scalar_expr(plan, ctx, *scrutinee)?;
            let mut lowered_arms = Vec::with_capacity(arms.len());
            for arm in arms {
                // TODO(phase3): support pattern bindings in match arms.
                let body = ctx.thir.bodies.bodies.get(arm.body).ok_or(LoweringError::UnsupportedExpr)?;
                let body_expr = lower_scalar_expr(plan, ctx, body.value)?;
                lowered_arms.push(crate::expr::MatchArm {
                    pat: crate::expr::Pattern::Wild,
                    guard: None,
                    body: body_expr,
                });
            }
            Ok(plan.alloc_expr(QExpr::Match { scrutinee: scrut_expr, arms: lowered_arms, ty }))
        }
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

fn lower_literal(
    plan: &mut LogicalPlan,
    lit: &yelang_hir::hir::core::Lit,
    ty: TyId,
    ctx: &super::ExtractCtxt<'_>,
) -> Result<QExprId, LoweringError> {
    let qlit = match lit {
        yelang_hir::hir::core::Lit::Int(n) => {
            let s = ctx.tcx.resolve_symbol(n.value).unwrap_or("0");
            QLit::Int(s.parse::<i128>().unwrap_or(0))
        }
        yelang_hir::hir::core::Lit::Float(n) => {
            let s = ctx.tcx.resolve_symbol(n.value).unwrap_or("0.0");
            QLit::Float(s.parse::<f64>().unwrap_or(0.0))
        }
        yelang_hir::hir::core::Lit::Bool(b) => QLit::Bool(*b),
        yelang_hir::hir::core::Lit::Str(s) => QLit::Str(s.value),
        yelang_hir::hir::core::Lit::Char(c) => QLit::Int(*c as i128),
        yelang_hir::hir::core::Lit::Unit => QLit::Unit,
        _ => QLit::Unit,
    };
    Ok(plan.alloc_expr(QExpr::Lit(qlit, ty)))
}

fn lower_binary_op(op: yelang_ast::BinaryOp) -> crate::expr::QBinaryOp {
    use crate::expr::QBinaryOp;
    use yelang_ast::BinaryOp;
    match op {
        BinaryOp::Eq => QBinaryOp::Eq,
        BinaryOp::Ne => QBinaryOp::Ne,
        BinaryOp::Lt => QBinaryOp::Lt,
        BinaryOp::Lte => QBinaryOp::Lte,
        BinaryOp::Gt => QBinaryOp::Gt,
        BinaryOp::Gte => QBinaryOp::Gte,
        BinaryOp::Add => QBinaryOp::Add,
        BinaryOp::Subtract => QBinaryOp::Sub,
        BinaryOp::Multiply => QBinaryOp::Mul,
        BinaryOp::Divide => QBinaryOp::Div,
        BinaryOp::Modulo => QBinaryOp::Mod,
        BinaryOp::And => QBinaryOp::And,
        BinaryOp::Or => QBinaryOp::Or,
        _ => QBinaryOp::Add,
    }
}

fn lower_unary_op(op: yelang_ast::UnaryOp) -> crate::expr::QUnaryOp {
    use crate::expr::QUnaryOp;
    use yelang_ast::UnaryOp;
    match op {
        UnaryOp::Not => QUnaryOp::Not,
        UnaryOp::Neg => QUnaryOp::Neg,
        _ => QUnaryOp::Not,
    }
}

fn function_def_id(ctx: &super::ExtractCtxt<'_>, expr: ThirExprId) -> Option<DefId> {
    ctx.thir.exprs.get(expr).and_then(|e| match e {
        ThirExpr::Var(def_id) => Some(*def_id),
        _ => None,
    })
}

fn lower_closure(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    params: &[yelang_thir::ThirPatId],
    body_id: yelang_thir::ThirBodyId,
    ty: TyId,
) -> Result<QExprId, LoweringError> {
    ctx.push_binder_scope();
    let mut param_binders = Vec::with_capacity(params.len());
    for &pat_id in params {
        let binder = plan.fresh_binder();
        param_binders.push(binder);
        ctx.insert_binder(pat_id, binder);
    }
    let body = ctx
        .thir
        .bodies
        .bodies
        .get(body_id)
        .ok_or(LoweringError::UnsupportedExpr)?;
    let body_expr = lower_scalar_expr(plan, ctx, body.value)?;
    ctx.pop_binder_scope();
    Ok(plan.alloc_expr(QExpr::Closure {
        params: param_binders,
        body: body_expr,
        captures: vec![],
        ty,
    }))
}

fn lower_block(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    stmts: &[yelang_thir::ThirStmtId],
    tail: Option<yelang_thir::ThirExprId>,
    ty: TyId,
) -> Result<QExprId, LoweringError> {
    ctx.push_binder_scope();
    let mut result_expr = None;
    for &stmt_id in stmts {
        let Some(stmt) = ctx.thir.stmts.get(stmt_id) else { continue };
        match stmt {
            yelang_thir::ThirStmt::Let { pat, init, .. } => {
                let binder = plan.fresh_binder();
                ctx.insert_binder(*pat, binder);
                if let Some(init_expr) = init {
                    let value = lower_scalar_expr(plan, ctx, *init_expr)?;
                    let body = if let Some(tail_expr) = tail {
                        lower_scalar_expr(plan, ctx, tail_expr)?
                    } else {
                        plan.alloc_expr(QExpr::Tuple(vec![], ty))
                    };
                    result_expr = Some(plan.alloc_expr(QExpr::Let {
                        name: binder,
                        value,
                        body,
                        ty,
                    }));
                }
            }
            yelang_thir::ThirStmt::Expr { expr } => {
                result_expr = Some(lower_scalar_expr(plan, ctx, *expr)?);
            }
            _ => {}
        }
    }
    let expr = if let Some(tail_expr) = tail {
        lower_scalar_expr(plan, ctx, tail_expr)?
    } else {
        result_expr.unwrap_or_else(|| plan.alloc_expr(QExpr::Tuple(vec![], ty)))
    };
    ctx.pop_binder_scope();
    Ok(expr)
}

/// Convert a QExpr that wraps a subplan into the underlying LirId.
///
/// If the expression is a scalar value whose type is a known `Queryable` ADT,
/// it is first wrapped in a `Scan` operator so that pipeline operators can be
/// appended to it.
pub fn expr_to_lir(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    expr: QExprId,
) -> Result<LirId, LoweringError> {
    match plan.expr(expr) {
        QExpr::Subplan(lir, _) => Ok(*lir),
        other => {
            let ty = other.ty();
            if let Some(elem_ty) = ctx.queryable_element_ty(ty) {
                let source = crate::lir::operator::ScanSource::Expr(expr);
                Ok(plan.scan(source, elem_ty))
            } else {
                Err(LoweringError::UnsupportedExpr)
            }
        }
    }
}

/// Allocate a fresh binder for the output of an operator.
pub fn fresh_output_binder(plan: &mut LogicalPlan) -> BinderId {
    plan.fresh_binder()
}
