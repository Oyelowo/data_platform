//! Lower HIR expressions that appear in query-syntax clauses to QExpr nodes.
//!
//! This is used by `query_syntax.rs` to translate `where`, `select`, `order by`,
//! `range`, and `group by` expressions without going through the legacy HIR->LIR
//! path. It operates on HIR `ExprId`s and a local HIR-pattern-to-binder map.

use yelang_arena::FxHashMap;
use yelang_hir::hir::expr::Expr;
use yelang_hir::hir::pat::Pat;
use yelang_hir::ids::{BodyId, ExprId, PatId};
use yelang_hir::res::Res;
use yelang_ty::ty::{Ty, TyId};

use crate::errors::LoweringError;
use crate::expr::{Pattern, QExpr, QLit};
use crate::ids::{BinderId, QExprId};
use crate::lir::plan::LogicalPlan;

/// Lower a HIR expression to a QExpr.
pub fn lower_hir_expr(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    expr_id: ExprId,
    binder_map: &mut FxHashMap<PatId, BinderId>,
) -> Result<QExprId, LoweringError> {
    let hir = ctx.tcx.crate_hir();
    let expr = hir.expr(expr_id).ok_or(LoweringError::UnsupportedExpr)?;
    let ty = ctx.results.expr_ty(expr_id).unwrap_or_else(unit_ty);

    match expr {
        Expr::Lit { lit } => Ok(plan.alloc_expr(QExpr::Lit(lower_lit(ctx, lit)?, ty))),
        Expr::Path { res } => lower_path(plan, ctx, res, ty, binder_map),
        Expr::Binary { op, left, right } => {
            let l = lower_hir_expr(plan, ctx, *left, binder_map)?;
            let r = lower_hir_expr(plan, ctx, *right, binder_map)?;
            let qop = lower_binary_op(*op);
            Ok(plan.alloc_expr(QExpr::Binary(qop, l, r, ty)))
        }
        Expr::Unary { op, expr } => {
            let e = lower_hir_expr(plan, ctx, *expr, binder_map)?;
            let qop = lower_unary_op(*op);
            Ok(plan.alloc_expr(QExpr::Unary(qop, e, ty)))
        }
        Expr::Field { expr, field } => {
            let base = lower_hir_expr(plan, ctx, *expr, binder_map)?;
            Ok(plan.alloc_expr(QExpr::Field(base, field.symbol, ty)))
        }
        Expr::Index { expr, index } => {
            let base = lower_hir_expr(plan, ctx, *expr, binder_map)?;
            let idx = lower_hir_expr(plan, ctx, *index, binder_map)?;
            Ok(plan.alloc_expr(QExpr::Index(base, idx, ty)))
        }
        Expr::Call { func, args } => {
            let mut lowered_args: Vec<QExprId> = Vec::with_capacity(args.len());
            for &arg in args {
                lowered_args.push(lower_hir_expr(plan, ctx, arg, binder_map)?);
            }
            let def_id = match hir.expr(*func) {
                Some(Expr::Path { res: Res::Def { def_id } }) => *def_id,
                _ => return Err(LoweringError::UnsupportedExpr),
            };
            Ok(plan.alloc_expr(QExpr::Call(def_id, lowered_args, ty)))
        }
        Expr::MethodCall { receiver, args, .. } => {
            let receiver_expr = lower_hir_expr(plan, ctx, *receiver, binder_map)?;
            let mut lowered_args: Vec<QExprId> = Vec::with_capacity(args.len());
            for &arg in args {
                lowered_args.push(lower_hir_expr(plan, ctx, arg, binder_map)?);
            }
            let method_def_id = ctx
                .results
                .method_resolution(expr_id)
                .and_then(|res| res.method_def_id)
                .ok_or(LoweringError::UnsupportedExpr)?;
            Ok(plan.alloc_expr(QExpr::MethodCall {
                receiver: receiver_expr,
                method: method_def_id,
                args: lowered_args,
                ty,
            }))
        }
        Expr::Tuple { exprs } => {
            let mut lowered = Vec::with_capacity(exprs.len());
            for &e in exprs {
                lowered.push(lower_hir_expr(plan, ctx, e, binder_map)?);
            }
            Ok(plan.alloc_expr(QExpr::Tuple(lowered, ty)))
        }
        Expr::Array { exprs } => {
            let mut lowered = Vec::with_capacity(exprs.len());
            for &e in exprs {
                lowered.push(lower_hir_expr(plan, ctx, e, binder_map)?);
            }
            Ok(plan.alloc_expr(QExpr::Array(lowered, ty)))
        }
        Expr::Struct { fields, rest, .. } => {
            if rest.is_some() {
                return Err(LoweringError::UnsupportedExpr);
            }
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let value = lower_hir_expr(plan, ctx, field.expr, binder_map)?;
                lowered_fields.push((field.ident.symbol, value));
            }
            Ok(plan.alloc_expr(QExpr::Record(lowered_fields, ty)))
        }
        Expr::Object { fields } => {
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let value = lower_hir_expr(plan, ctx, field.expr, binder_map)?;
                lowered_fields.push((field.ident.symbol, value));
            }
            Ok(plan.alloc_expr(QExpr::Record(lowered_fields, ty)))
        }
        Expr::Cast { expr, ty: hir_ty } => {
            let inner = lower_hir_expr(plan, ctx, *expr, binder_map)?;
            let inner_ty = plan.expr(inner).ty();
            let target_ty = hir_ty_to_ty(ctx, *hir_ty).unwrap_or(ty);
            let kind = classify_cast(ctx.tcx.interner(), inner_ty, target_ty);
            Ok(plan.alloc_expr(QExpr::Cast(inner, kind, target_ty)))
        }
        Expr::If { cond, then_branch, else_branch } => {
            let c = lower_hir_expr(plan, ctx, *cond, binder_map)?;
            let t = lower_hir_expr(plan, ctx, *then_branch, binder_map)?;
            let e = else_branch
                .map(|b| lower_hir_expr(plan, ctx, b, binder_map))
                .transpose()?
                .unwrap_or_else(|| plan.alloc_expr(QExpr::Lit(QLit::Unit, ty)));
            Ok(plan.alloc_expr(QExpr::If(c, t, e, ty)))
        }
        Expr::Closure { body, .. } => lower_hir_closure(plan, ctx, *body, ty, binder_map),
        Expr::Block { block } => {
            if let Some(tail) = block.expr {
                lower_hir_expr(plan, ctx, tail, binder_map)
            } else {
                Ok(plan.alloc_expr(QExpr::Lit(QLit::Unit, ty)))
            }
        }
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

fn lower_path(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    res: &Res,
    ty: TyId,
    binder_map: &mut FxHashMap<PatId, BinderId>,
) -> Result<QExprId, LoweringError> {
    match res {
        Res::Local { pat_id } => {
            if let Some(&binder) = binder_map.get(pat_id) {
                return Ok(plan.alloc_expr(QExpr::Column(binder, ty)));
            }
            if let Some(expr) = ctx.lookup_hir_local_value(*pat_id) {
                return Ok(expr);
            }
            Err(LoweringError::UnsupportedExpr)
        }
        Res::Def { def_id } => Ok(plan.alloc_expr(QExpr::Call(*def_id, vec![], ty))),
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

fn lower_hir_closure(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    body_id: BodyId,
    ty: TyId,
    binder_map: &mut FxHashMap<PatId, BinderId>,
) -> Result<QExprId, LoweringError> {
    let hir = ctx.tcx.crate_hir();
    let body = hir.body(body_id).ok_or(LoweringError::UnsupportedExpr)?;

    let mut param_binders = Vec::with_capacity(body.params.len());
    let mut pat_to_binder: FxHashMap<PatId, BinderId> = FxHashMap::default();

    for param in &body.params {
        let (pattern, binder) = lower_hir_pat(plan, ctx, param.pat, &mut pat_to_binder)?;
        param_binders.push(binder);
        let _ = pattern;
    }

    for (k, v) in pat_to_binder.iter() {
        binder_map.insert(*k, *v);
    }
    let body_expr = lower_hir_expr(plan, ctx, body.value, binder_map)?;
    // Remove the closure-local bindings so they don't leak into the outer scope.
    for param in &body.params {
        binder_map.remove(&param.pat);
    }

    Ok(plan.alloc_expr(QExpr::Closure {
        params: param_binders,
        body: body_expr,
        captures: vec![],
        ty,
    }))
}

fn lower_hir_pat(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    pat_id: PatId,
    out_map: &mut FxHashMap<PatId, BinderId>,
) -> Result<(Pattern, BinderId), LoweringError> {
    let hir = ctx.tcx.crate_hir();
    let pat = hir.pat(pat_id).ok_or(LoweringError::UnsupportedExpr)?;
    let pat_ty = ctx.results.pat_ty(pat_id).unwrap_or_else(unit_ty);

    match pat {
        Pat::Wild => Ok((Pattern::Wild, plan.fresh_binder())),
        Pat::Binding { subpat, .. } => {
            let binder = plan.fresh_binder();
            out_map.insert(pat_id, binder);
            if let Some(subpat_id) = subpat {
                let (_sub_pattern, _sub_binder) = lower_hir_pat(plan, ctx, *subpat_id, out_map)?;
            }
            Ok((Pattern::Bind(binder, pat_ty), binder))
        }
        Pat::Tuple { pats } => {
            let mut sub_pats = Vec::with_capacity(pats.len());
            for &subpat_id in pats {
                let (sub_qpat, _) = lower_hir_pat(plan, ctx, subpat_id, out_map)?;
                sub_pats.push(sub_qpat);
            }
            Ok((Pattern::Tuple(sub_pats), plan.fresh_binder()))
        }
        Pat::TupleStruct { pats, .. } => {
            let mut sub_pats = Vec::with_capacity(pats.len());
            for &subpat_id in pats {
                let (sub_qpat, _) = lower_hir_pat(plan, ctx, subpat_id, out_map)?;
                sub_pats.push(sub_qpat);
            }
            Ok((Pattern::Tuple(sub_pats), plan.fresh_binder()))
        }
        Pat::Struct { fields, .. } => {
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let (sub_qpat, _) = lower_hir_pat(plan, ctx, field.pat, out_map)?;
                lowered_fields.push((field.ident.symbol, sub_qpat));
            }
            Ok((Pattern::Record(lowered_fields), plan.fresh_binder()))
        }
        _ => Ok((Pattern::Wild, plan.fresh_binder())),
    }
}

fn lower_lit(
    ctx: &super::ExtractCtxt<'_>,
    lit: &yelang_hir::hir::core::Lit,
) -> Result<QLit, LoweringError> {
    Ok(match lit {
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
    })
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

fn unit_ty() -> TyId {
    yelang_ty::ty::TyId::new(1)
}

fn hir_ty_to_ty(
    ctx: &super::ExtractCtxt<'_>,
    hir_ty_id: yelang_hir::ids::HirTyId,
) -> Option<TyId> {
    use yelang_resolve::lang_items::LangItem;
    use yelang_ty::primitive::{FloatTy, IntTy};

    let hir_ty = ctx.tcx.crate_hir().ty(hir_ty_id)?;
    match hir_ty {
        yelang_hir::hir::ty::Ty::Path { res, .. } => match res {
            Res::Def { def_id } => {
                let interner = ctx.tcx.interner();
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::I32) {
                    return Some(interner.mk_ty(Ty::Int(IntTy::I32)));
                }
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::I64) {
                    return Some(interner.mk_ty(Ty::Int(IntTy::I64)));
                }
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::F64) {
                    return Some(interner.mk_ty(Ty::Float(FloatTy::F64)));
                }
                Some(interner.mk_ty(Ty::Adt(
                    yelang_ty::ty::AdtDef { def_id: *def_id },
                    yelang_ty::list::List::empty(),
                )))
            }
            _ => None,
        },
        _ => None,
    }
}

fn classify_cast(
    interner: &yelang_ty::interner::Interner,
    _from: TyId,
    to: TyId,
) -> crate::expr::CastKind {
    use yelang_ty::ty::Ty;
    // Casts to a floating-point target are lowered as IntToFloat; the executor
    // passes Float values through unchanged. Likewise for Int targets. This is
    // robust against incomplete type inference for the source expression.
    match interner.ty(to) {
        Ty::Float(_) => crate::expr::CastKind::IntToFloat,
        Ty::Int(_) | Ty::Uint(_) => crate::expr::CastKind::FloatToInt,
        _ => crate::expr::CastKind::Numeric,
    }
}
