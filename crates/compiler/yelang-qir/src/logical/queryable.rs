//! Lowering of `Queryable` trait methods into logical operators.

use yelang_hir::res::Res;
use yelang_hir::hir::expr::Expr;
use yelang_hir::ids::{DefId, ExprId};
use yelang_interner::Symbol;
use yelang_ty::ty::{Ty, TyId};

use crate::errors::LoweringError;
use crate::expr::{AggregateClass, OrderKey, QExpr, QExprId};
use crate::ids::LirId;
use crate::logical::aggregate_impl;
use crate::logical::lower::LoweringCtxt;
use crate::logical::lower_expr::lower_hir_expr;
use crate::logical::operator::{AggregateOp, ScanSource};
use crate::logical::plan::LogicalPlan;

/// Known `Queryable` methods that the lowering layer recognizes.
///
/// In the future this table will be replaced by trait-introspection: the
/// compiler will read the `Queryable` trait definition and infer the operator
/// shape from each method's signature and body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryableMethod {
    Filter,
    Map,
    FlatMap,
    Take,
    Skip,
    OrderBy,
    Distinct,
    GroupBy,
    Aggregate,
    Sum,
    Product,
    Avg,
    Count,
    Execute,
}

/// Lower a `Queryable` method call to a LIR operator.
pub fn lower(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    _expr_id: ExprId,
    method_def_id: Option<DefId>,
    receiver: ExprId,
    args: &[ExprId],
    ty: TyId,
) -> Result<LirId, LoweringError> {
    let method = method_def_id
        .and_then(|id| ctx.queryable_method(id))
        .ok_or(LoweringError::UnsupportedExpr)?;

    let input = lir_input(plan, ctx, receiver)?;

    match method {
        QueryableMethod::Filter => lower_filter(plan, ctx, input, args),
        QueryableMethod::Map => lower_map(plan, ctx, input, args),
        QueryableMethod::FlatMap => lower_flat_map(plan, ctx, input, args),
        QueryableMethod::Take => lower_take(plan, ctx, input, args),
        QueryableMethod::Skip => lower_skip(plan, ctx, input, args),
        QueryableMethod::OrderBy => lower_order_by(plan, ctx, input, args),
        QueryableMethod::Distinct => Ok(plan.distinct(input, None, plan.props[input].output_ty)),
        QueryableMethod::GroupBy => lower_group_by(plan, ctx, input, args, ty),
        QueryableMethod::Aggregate => lower_aggregate_call(plan, ctx, input, args, ty),
        QueryableMethod::Sum | QueryableMethod::Avg | QueryableMethod::Count | QueryableMethod::Product => {
            let class = method_def_id
                .and_then(|id| ctx.aggregate_class(id))
                .ok_or_else(|| LoweringError::MissingAggregate(format!("{:?}", method)))?;
            lower_aggregate_sugar(plan, ctx, input, method_def_id.unwrap(), class, ty)
        }
        QueryableMethod::Execute => Ok(input),
    }
}

fn lir_input(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    receiver: ExprId,
) -> Result<LirId, LoweringError> {
    let recv_expr = lower_hir_expr(plan, ctx, receiver)?;
    if let QExpr::Subplan(lir, _) = plan.expr(recv_expr) {
        return Ok(*lir);
    }
    let recv_ty = ctx.results.expr_ty(receiver).unwrap_or_else(|| plan.expr(recv_expr).ty());
    let elem_ty = element_ty(ctx, recv_ty);
    Ok(plan.scan(ScanSource::Expr(recv_expr), elem_ty))
}

pub(crate) fn element_ty(ctx: &LoweringCtxt<'_>, ty: TyId) -> TyId {
    let interner = ctx.tcx.interner();
    if !interner.has_ty(ty) {
        return ty;
    }
    match interner.ty(ty) {
        Ty::Array(elem, _) | Ty::Slice(elem) => elem,
        Ty::Adt(adt, args) => {
            if ctx.tcx.lang_item(yelang_resolve::lang_items::LangItem::Array) == Some(adt.def_id) {
                args.iter().next().map(|a| a.expect_type()).unwrap_or(ty)
            } else {
                ty
            }
        }
        _ => ty,
    }
}

fn lower_filter(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let pred = expect_one_arg(args)?;
    let pred_expr = lower_hir_expr(plan, ctx, pred)?;
    let out_ty = plan.props[input].output_ty;
    let input_binder = plan.props[input].output_binder;
    let id = plan.filter(input, pred_expr, out_ty);
    plan.props[id].output_binder = input_binder;
    Ok(id)
}

fn lower_map(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let proj = expect_one_arg(args)?;
    let proj_expr = lower_hir_expr(plan, ctx, proj)?;
    let out_ty = closure_return_ty(ctx, proj).unwrap_or_else(|| plan.props[input].output_ty);
    let id = plan.map(input, proj_expr, out_ty);
    if let Some((param, _)) = crate::rewrite::as_closure(plan, proj_expr) {
        plan.props[id].output_binder = Some(param);
    }
    Ok(id)
}

fn lower_flat_map(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let proj = expect_one_arg(args)?;
    let proj_expr = lower_hir_expr(plan, ctx, proj)?;
    let out_ty = closure_return_ty(ctx, proj).unwrap_or_else(|| plan.props[input].output_ty);
    let id = plan.flat_map(input, proj_expr, out_ty);
    if let Some((param, _)) = crate::rewrite::as_closure(plan, proj_expr) {
        plan.props[id].output_binder = Some(param);
    }
    Ok(id)
}

fn lower_take(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let n = expect_one_arg(args)?;
    let n_expr = lower_hir_expr(plan, ctx, n)?;
    let n_ty = plan.expr(n_expr).ty();
    let zero = plan.alloc_expr(QExpr::Lit(crate::expr::QLit::Int(0), n_ty));
    let out_ty = plan.props[input].output_ty;
    Ok(plan.slice_unordered(input, zero, Some(n_expr), out_ty))
}

fn lower_skip(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let n = expect_one_arg(args)?;
    let n_expr = lower_hir_expr(plan, ctx, n)?;
    let out_ty = plan.props[input].output_ty;
    Ok(plan.slice_unordered(input, n_expr, None, out_ty))
}

fn lower_order_by(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
) -> Result<LirId, LoweringError> {
    let key = expect_one_arg(args)?;
    let key_expr = lower_hir_expr(plan, ctx, key)?;
    let order = OrderKey {
        expr: key_expr,
        dir: crate::expr::Direction::Asc,
        nulls: crate::expr::NullOrdering::Last,
    };
    let out_ty = plan.props[input].output_ty;
    Ok(plan.order_by(input, vec![order], out_ty))
}

fn lower_group_by(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
    ty: TyId,
) -> Result<LirId, LoweringError> {
    let key = expect_one_arg(args)?;
    let key_expr = lower_hir_expr(plan, ctx, key)?;
    let key_ty = plan.expr(key_expr).ty();
    let vals_label = Symbol::from(1); // "vals"
    Ok(plan.group_by(input, key_expr, key_ty, vals_label, ty))
}

fn lower_aggregate_call(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    args: &[ExprId],
    ty: TyId,
) -> Result<LirId, LoweringError> {
    let marker = expect_one_arg(args)?;
    let agg_def = resolve_aggregate_marker_def(plan, ctx, marker)?;
    let class = ctx
        .aggregate_class(agg_def)
        .ok_or_else(|| LoweringError::MissingAggregate(format!("agg#{}", agg_def.raw())))?;
    let per_row = identity_per_row(plan, input);
    let elem_ty = plan.expr(per_row).ty();
    let agg = aggregate_impl::build_builtin_aggregate(plan, ctx, agg_def, per_row, class, elem_ty, ty)
        .unwrap_or_else(|| placeholder_aggregate_op(plan, agg_def, class, per_row, elem_ty, ty));
    Ok(plan.aggregate(input, agg, ty))
}

fn lower_aggregate_sugar(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    input: LirId,
    method_def_id: DefId,
    class: AggregateClass,
    ty: TyId,
) -> Result<LirId, LoweringError> {
    let per_row = identity_per_row(plan, input);
    let elem_ty = plan.expr(per_row).ty();
    let agg_def = aggregate_impl::resolve_sugar_marker(ctx, method_def_id).unwrap_or(method_def_id);
    let agg = aggregate_impl::build_builtin_aggregate(plan, ctx, agg_def, per_row, class, elem_ty, ty)
        .unwrap_or_else(|| placeholder_aggregate_op(plan, agg_def, class, per_row, elem_ty, ty));
    Ok(plan.aggregate(input, agg, ty))
}

fn placeholder_aggregate_op(
    plan: &mut LogicalPlan,
    agg_def: DefId,
    class: AggregateClass,
    per_row: QExprId,
    acc_ty: TyId,
    out_ty: TyId,
) -> AggregateOp {
    let unit = plan.alloc_expr(QExpr::Record(vec![], acc_ty));
    AggregateOp {
        agg_def,
        impl_def: agg_def,
        class,
        per_row,
        init: unit,
        step: unit,
        merge: unit,
        finish: unit,
        config: unit,
        acc_ty,
        out_ty,
    }
}

fn identity_per_row(plan: &mut LogicalPlan, input: LirId) -> QExprId {
    let elem_ty = plan.props[input].output_ty;
    let binder = plan.props[input].output_binder.unwrap_or_else(|| plan.fresh_binder());
    let body = plan.alloc_expr(QExpr::Column(binder, elem_ty));
    plan.alloc_expr(QExpr::Closure {
        params: vec![binder],
        body,
        captures: vec![],
        ty: elem_ty,
    })
}

fn resolve_aggregate_marker_def(
    _plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    marker_expr_id: ExprId,
) -> Result<DefId, LoweringError> {
    let marker_expr = ctx
        .krate()
        .expr(marker_expr_id)
        .ok_or(LoweringError::UnsupportedExpr)?;
    match marker_expr {
        Expr::Path { res } => match res {
            Res::Def { def_id } => Ok(*def_id),
            _ => Err(LoweringError::UnsupportedExpr),
        },
        Expr::Struct { path, .. } => {
            let _ = path;
            // TODO: resolve path to def_id via TyCtxt item tables.
            Err(LoweringError::UnsupportedExpr)
        }
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

fn closure_return_ty(ctx: &LoweringCtxt<'_>, closure_expr_id: ExprId) -> Option<TyId> {
    let closure_ty = ctx.results.expr_ty(closure_expr_id)?;
    let interner = ctx.tcx.interner();
    if !interner.has_ty(closure_ty) {
        return None;
    }
    match interner.ty(closure_ty) {
        Ty::FnPtr(sig) => Some(sig.sig.output),
        _ => None,
    }
}

fn expect_one_arg(args: &[ExprId]) -> Result<ExprId, LoweringError> {
    args.first().copied().ok_or(LoweringError::UnsupportedExpr)
}
