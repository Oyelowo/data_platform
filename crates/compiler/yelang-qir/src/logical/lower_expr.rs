//! Lower HIR expressions into QExpr nodes.

use yelang_hir::hir::expr::Expr;
use yelang_hir::ids::{DefId, ExprId};
use yelang_lexer::Literal;

use crate::errors::LoweringError;
use crate::expr::{QLit, QExpr};
use crate::ids::{BinderId, QExprId};
use crate::logical::lower::LoweringCtxt;
use crate::logical::plan::LogicalPlan;

/// Lower a HIR expression into a QExprId.
pub fn lower_hir_expr(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    expr_id: ExprId,
) -> Result<QExprId, LoweringError> {
    let expr = ctx.krate().expr(expr_id).ok_or(LoweringError::UnsupportedExpr)?;
    let ty = ctx.results.expr_ty(expr_id).unwrap_or_else(|| yelang_ty::ty::TyId::new(1));

    match expr {
        Expr::Lit { lit } => {
            let qlit = lower_lit(lit, ctx)?;
            Ok(plan.alloc_expr(QExpr::Lit(qlit, ty)))
        }
        Expr::Path { res } => {
            // TODO: resolve local binder -> Column, global path -> Call/Const.
            let _ = res;
            Ok(plan.alloc_expr(QExpr::Error(ty)))
        }
        Expr::Binary { op, left, right } => {
            let l = lower_hir_expr(plan, ctx, *left)?;
            let r = lower_hir_expr(plan, ctx, *right)?;
            let qop = lower_bin_op(*op);
            Ok(plan.alloc_expr(QExpr::Binary(qop, l, r, ty)))
        }
        Expr::Unary { op, expr } => {
            let e = lower_hir_expr(plan, ctx, *expr)?;
            let qop = lower_unary_op(*op);
            Ok(plan.alloc_expr(QExpr::Unary(qop, e, ty)))
        }
        Expr::Call { func, args } => {
            let f = lower_hir_expr(plan, ctx, *func)?;
            let a: Result<Vec<_>, _> = args
                .iter()
                .map(|arg| lower_hir_expr(plan, ctx, *arg))
                .collect();
            // TODO: resolve callee DefId from HIR path.
            let def = DefId::new(1);
            let _ = f;
            Ok(plan.alloc_expr(QExpr::Call(def, a?, ty)))
        }
        Expr::MethodCall { receiver, method: _, args, trait_def_id } => {
            let recv = lower_hir_expr(plan, ctx, *receiver)?;
            let a: Result<Vec<_>, _> = args
                .iter()
                .map(|arg| lower_hir_expr(plan, ctx, *arg))
                .collect();
            let method_def = trait_def_id.unwrap_or_else(|| DefId::new(1));
            Ok(plan.alloc_expr(QExpr::MethodCall {
                receiver: recv,
                method: method_def,
                args: a?,
                ty,
            }))
        }
        Expr::Field { expr, field } => {
            let base = lower_hir_expr(plan, ctx, *expr)?;
            Ok(plan.alloc_expr(QExpr::Field(base, field.symbol, ty)))
        }
        Expr::Index { expr, index } => {
            let base = lower_hir_expr(plan, ctx, *expr)?;
            let idx = lower_hir_expr(plan, ctx, *index)?;
            Ok(plan.alloc_expr(QExpr::Index(base, idx, ty)))
        }
        Expr::Tuple { exprs } => {
            let elems: Result<Vec<_>, _> = exprs
                .iter()
                .map(|e| lower_hir_expr(plan, ctx, *e))
                .collect();
            Ok(plan.alloc_expr(QExpr::Tuple(elems?, ty)))
        }
        Expr::Array { exprs } => {
            let elems: Result<Vec<_>, _> = exprs
                .iter()
                .map(|e| lower_hir_expr(plan, ctx, *e))
                .collect();
            Ok(plan.alloc_expr(QExpr::Array(elems?, ty)))
        }
        Expr::If { cond, then_branch, else_branch } => {
            let c = lower_hir_expr(plan, ctx, *cond)?;
            let t = lower_hir_expr(plan, ctx, *then_branch)?;
            let e = else_branch
                .map(|b| lower_hir_expr(plan, ctx, b))
                .transpose()?
                .unwrap_or_else(|| plan.alloc_expr(QExpr::Lit(QLit::Unit, ty)));
            Ok(plan.alloc_expr(QExpr::If(c, t, e, ty)))
        }
        Expr::Closure { params, body, .. } => {
            let ps: Vec<BinderId> = params.iter().map(|_| BinderId(0)).collect();
            let body_node = ctx.krate().body(*body).ok_or(LoweringError::UnsupportedExpr)?;
            let body_expr = body_node.value;
            let b = lower_hir_expr(plan, ctx, body_expr)?;
            Ok(plan.alloc_expr(QExpr::Closure {
                params: ps,
                body: b,
                captures: vec![],
                ty,
            }))
        }
        Expr::Query(query_id) => {
            let _ = super::lower_query::lower_query(
                plan,
                ctx,
                ctx.krate().query(*query_id).ok_or(LoweringError::UnsupportedClause)?,
            )?;
            // A subquery expression evaluates to the result of the plan.
            Ok(plan.alloc_expr(QExpr::Error(ty)))
        }
        _ => Ok(plan.alloc_expr(QExpr::Error(ty))),
    }
}

fn lower_lit(lit: &Literal, ctx: &LoweringCtxt<'_>) -> Result<QLit, LoweringError> {
    use yelang_lexer::{FloatLit, IntegerLit, StringLit};
    Ok(match lit {
        Literal::Int(IntegerLit { value, .. }) => {
            let s = ctx.tcx.resolve_symbol(*value).unwrap_or("0");
            QLit::Int(s.parse::<i128>().unwrap_or(0))
        }
        Literal::Float(FloatLit { value, .. }) => {
            let s = ctx.tcx.resolve_symbol(*value).unwrap_or("0");
            QLit::Float(s.parse::<f64>().unwrap_or(0.0))
        }
        Literal::Bool(v) => QLit::Bool(*v),
        Literal::Str(StringLit { value, .. }) => QLit::Str(*value),
        Literal::Char(c) => QLit::Str(symbol_from_char(*c)),
        Literal::Unit => QLit::Unit,
        _ => QLit::Unit,
    })
}

fn symbol_from_char(c: char) -> yelang_interner::Symbol {
    yelang_interner::Symbol::from(c as u32)
}

fn lower_bin_op(op: yelang_hir::hir::core::BinOp) -> crate::expr::QBinaryOp {
    use yelang_hir::hir::core::BinOp as BinaryOp;
    match op {
        BinaryOp::Eq => crate::expr::QBinaryOp::Eq,
        BinaryOp::Ne => crate::expr::QBinaryOp::Ne,
        BinaryOp::Lt => crate::expr::QBinaryOp::Lt,
        BinaryOp::Lte => crate::expr::QBinaryOp::Lte,
        BinaryOp::Gt => crate::expr::QBinaryOp::Gt,
        BinaryOp::Gte => crate::expr::QBinaryOp::Gte,
        BinaryOp::Add => crate::expr::QBinaryOp::Add,
        BinaryOp::Subtract => crate::expr::QBinaryOp::Sub,
        BinaryOp::Multiply => crate::expr::QBinaryOp::Mul,
        BinaryOp::Divide => crate::expr::QBinaryOp::Div,
        BinaryOp::Modulo => crate::expr::QBinaryOp::Mod,
        BinaryOp::And => crate::expr::QBinaryOp::And,
        BinaryOp::Or => crate::expr::QBinaryOp::Or,
        _ => crate::expr::QBinaryOp::Eq, // placeholder
    }
}

fn lower_unary_op(op: yelang_hir::hir::core::UnOp) -> crate::expr::QUnaryOp {
    use yelang_hir::hir::core::UnOp as UnaryOp;
    match op {
        UnaryOp::Not => crate::expr::QUnaryOp::Not,
        UnaryOp::Neg => crate::expr::QUnaryOp::Neg,
        _ => crate::expr::QUnaryOp::Not,
    }
}
