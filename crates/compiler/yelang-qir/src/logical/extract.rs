//! Main THIR → LIR extraction pass.

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_thir::{ThirBodyId, ThirExpr, ThirExprId};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::TypeckResults;
use yelang_ty::ty::{Ty, TyId};

use crate::errors::{LoweringError, QirResult};
use crate::expr::{Direction, OrderKey, QExpr};
use crate::ids::{LirId, QExprId};
use crate::lir::plan::LogicalPlan;
use crate::rewrite;

use super::aggregate::{lower_aggregate_with_config, resolve_aggregate_config};
use super::context::{ExtractCtxt, ThirView};
use super::convert::{expr_to_lir, lower_scalar_expr};
use super::intrinsic::QueryableIntrinsic;

/// Lower a typed THIR body to a QIR logical plan.
pub fn lower_thir_body(
    tcx: &TyCtxt,
    thir: ThirView<'_>,
    body_id: ThirBodyId,
    results: &TypeckResults,
) -> QirResult<LogicalPlan> {
    let mut plan = LogicalPlan::empty();
    let mut ctx = ExtractCtxt::new(tcx, thir, results)?;
    let body = ctx
        .thir
        .bodies
        .bodies
        .get(body_id)
        .ok_or(LoweringError::UnsupportedExpr)?;

    // Introduce binders for the body's parameters. Queryable parameters become
    // the roots of pipeline scans.
    for &param in &body.params {
        let binder = plan.fresh_binder();
        ctx.insert_binder(param, binder);
    }

    let root_expr = extract_expr(&mut plan, &mut ctx, body.value)?;
    let root_lir = expr_to_lir(&mut plan, &mut ctx, root_expr)?;
    plan.set_root(root_lir);
    rewrite::apply_rewrites(&mut plan)?;
    Ok(plan)
}

/// Lower a single THIR expression to a QExpr.
pub fn extract_expr(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    expr: ThirExprId,
) -> Result<QExprId, LoweringError> {
    match ctx.thir.exprs.get(expr) {
        Some(ThirExpr::Call { func, args }) => {
            if let Some(method_def_id) = queryable_method_callee(ctx, *func) {
                return lower_queryable_call(plan, ctx, method_def_id, args);
            }
            lower_scalar_call(plan, ctx, *func, args)
        }
        Some(ThirExpr::Intrinsic { name, args }) => {
            lower_intrinsic(plan, ctx, *name, args)
        }
        Some(ThirExpr::Query(query_id)) => {
            super::query_syntax::lower_query_syntax(plan, ctx, *query_id)
        }
        Some(_) | None => lower_scalar_expr(plan, ctx, expr),
    }
}

/// If `func` is a direct reference to a `Queryable` method, return its `DefId`.
pub(crate) fn queryable_method_callee(ctx: &ExtractCtxt<'_>, func: ThirExprId) -> Option<DefId> {
    let func_expr = ctx.thir.exprs.get(func)?;
    let ThirExpr::Var(def_id) = func_expr else {
        return None;
    };
    if ctx.queryable_methods.contains_key(def_id) {
        Some(*def_id)
    } else {
        None
    }
}

fn lower_queryable_call(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    method_def_id: DefId,
    args: &[ThirExprId],
) -> Result<QExprId, LoweringError> {
    let info = ctx
        .queryable_methods
        .get(&method_def_id)
        .cloned()
        .ok_or(LoweringError::UnsupportedExpr)?;

    let receiver = args
        .get(info.self_index)
        .copied()
        .ok_or(LoweringError::UnsupportedExpr)?;
    let input_expr = extract_expr(plan, ctx, receiver)?;
    let input_lir = expr_to_lir(plan, ctx, input_expr)?;

    match info.intrinsic {
        Some(QueryableIntrinsic::Map) => lower_map(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Filter) => lower_filter(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::FlatMap) => lower_flat_map(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::OrderBy) => lower_order_by(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::GroupBy) => lower_group_by(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Distinct) => lower_distinct(plan, ctx, input_lir),
        Some(QueryableIntrinsic::Take) => lower_take(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Skip) => lower_skip(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Aggregate) => {
            let config_def_id = if let Some(marker) = info.sugar_marker {
                marker
            } else {
                let agg_arg_index = ctx
                    .aggregate_method_name()
                    .and_then(|name| info.arg_index.get(&name).copied())
                    .unwrap_or(1);
                let agg_arg = args
                    .get(agg_arg_index)
                    .copied()
                    .ok_or(LoweringError::UnsupportedExpr)?;
                resolve_aggregate_config(ctx, agg_arg)?.0
            };
            lower_aggregate_with_config(plan, ctx, input_lir, config_def_id)
        }
        Some(QueryableIntrinsic::Fold) => lower_fold(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Reduce) => lower_reduce(plan, ctx, input_lir, args, &info),
        Some(QueryableIntrinsic::Execute) => lower_execute(plan, input_lir),
        None => {
            // No recognized intrinsic: try to inline the method body. This path
            // is primarily used for default sugar bodies when THIR is available
            // for them.
            let inlined = super::inline::inline_method_body(ctx, method_def_id, args)?;
            extract_expr(plan, ctx, inlined)
        }
    }
}

fn arg_by_name(
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
    name: &str,
    ctx: &super::ExtractCtxt<'_>,
) -> Result<ThirExprId, LoweringError> {
    let sym = ctx
        .tcx
        .intern_symbol(name)
        .ok_or(LoweringError::UnsupportedExpr)?;
    let idx = info
        .arg_index
        .get(&sym)
        .copied()
        .unwrap_or_else(|| {
            // Fallback positional index for the well-known Queryable API. The
            // name map is built from the HIR body when available; when it is
            // not, self is at 0 and the single non-self argument is at 1.
            1
        });
    args.get(idx).copied().ok_or(LoweringError::UnsupportedExpr)
}

fn lower_map(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let f = arg_by_name(args, info, "f", ctx)?;
    let f_expr = extract_expr(plan, ctx, f)?;
    let out_ty = closure_return_ty(plan, ctx, f_expr)?;
    let id = plan.map(input, f_expr, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_filter(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let pred = arg_by_name(args, info, "pred", ctx)?;
    let pred_expr = extract_expr(plan, ctx, pred)?;
    let out_ty = plan.props[input].output_ty;
    let id = plan.filter(input, pred_expr, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_flat_map(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let f = arg_by_name(args, info, "f", ctx)?;
    let f_expr = extract_expr(plan, ctx, f)?;
    let out_ty = closure_return_ty(plan, ctx, f_expr)?;
    let id = plan.flat_map(input, f_expr, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_order_by(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let key = arg_by_name(args, info, "key", ctx)?;
    let key_expr = extract_expr(plan, ctx, key)?;
    let _key_ty = plan.expr(key_expr).ty();
    let order_key = OrderKey {
        expr: key_expr,
        dir: Direction::Asc,
        nulls: crate::expr::NullOrdering::Last,
    };
    let out_ty = plan.props[input].output_ty;
    let id = plan.order_by(input, vec![order_key], out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_group_by(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let key = arg_by_name(args, info, "key", ctx)?;
    let key_expr = extract_expr(plan, ctx, key)?;
    let key_ty = plan.expr(key_expr).ty();
    let vals_label = ctx
        .tcx
        .intern_symbol("items")
        .ok_or(LoweringError::UnsupportedExpr)?;
    let out_ty = plan.props[input].output_ty;
    let id = plan.group_by(input, key_expr, key_ty, vals_label, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_distinct(
    plan: &mut LogicalPlan,
    _ctx: &mut ExtractCtxt<'_>,
    input: LirId,
) -> Result<QExprId, LoweringError> {
    let out_ty = plan.props[input].output_ty;
    let id = plan.distinct(input, None, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_take(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let n = arg_by_name(args, info, "n", ctx)?;
    let n_expr = extract_expr(plan, ctx, n)?;
    let zero_expr = plan.alloc_expr(QExpr::Lit(crate::expr::QLit::Int(0), plan.expr(n_expr).ty()));
    let out_ty = plan.props[input].output_ty;
    let id = plan.slice_unordered(input, zero_expr, Some(n_expr), out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_skip(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    let n = arg_by_name(args, info, "n", ctx)?;
    let n_expr = extract_expr(plan, ctx, n)?;
    let out_ty = plan.props[input].output_ty;
    let id = plan.slice_unordered(input, n_expr, None, out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, out_ty)))
}

fn lower_fold(
    plan: &mut LogicalPlan,
    _ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    // TODO(phase3): eager fold can be represented as an aggregate with a
    // user-supplied init and step/merge. For now, return the input unchanged so
    // that extraction does not crash; full lowering requires closure-to-agg
    // plumbing.
    let _ = (args, info);
    let out_ty = plan.props[input].output_ty;
    Ok(plan.alloc_expr(QExpr::Subplan(input, out_ty)))
}

fn lower_reduce(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    input: LirId,
    args: &[ThirExprId],
    info: &super::context::QueryableMethodInfo,
) -> Result<QExprId, LoweringError> {
    // TODO(phase3): eager reduce is an aggregate over pairs of elements.
    let _ = (args, info, ctx);
    let out_ty = plan.props[input].output_ty;
    Ok(plan.alloc_expr(QExpr::Subplan(input, out_ty)))
}

fn lower_execute(
    plan: &mut LogicalPlan,
    input: LirId,
) -> Result<QExprId, LoweringError> {
    // `execute` forces materialization. The LIR layer has no separate
    // materialization operator; the subplan itself is the executable fragment.
    let out_ty = plan.props[input].output_ty;
    Ok(plan.alloc_expr(QExpr::Subplan(input, out_ty)))
}

fn lower_intrinsic(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    name: Symbol,
    args: &[ThirExprId],
) -> Result<QExprId, LoweringError> {
    let name_str = ctx
        .tcx
        .resolve_symbol(name)
        .unwrap_or("");
    if name_str == "intrinsic" && !args.is_empty() {
        // The first argument of `@intrinsic(query_*(...))` is a call to the
        // placeholder query function. Treat it like a Queryable method call.
        let first_arg = args[0];
        if let Some(ThirExpr::Call { func, args: inner_args }) = ctx.thir.exprs.get(first_arg) {
            if let Some(method_def_id) = queryable_method_callee(ctx, *func) {
                return lower_queryable_call(plan, ctx, method_def_id, inner_args);
            }
        }
        return lower_scalar_expr(plan, ctx, first_arg);
    }
    match args.first() {
        Some(&arg) => lower_scalar_expr(plan, ctx, arg),
        None => Err(LoweringError::UnsupportedExpr),
    }
}

fn lower_scalar_call(
    plan: &mut LogicalPlan,
    ctx: &mut ExtractCtxt<'_>,
    func: ThirExprId,
    args: &[ThirExprId],
) -> Result<QExprId, LoweringError> {
    let _func_expr = extract_expr(plan, ctx, func)?;
    let mut lowered_args = Vec::with_capacity(args.len());
    for &arg in args {
        lowered_args.push(extract_expr(plan, ctx, arg)?);
    }
    let ty = expr_thir_ty(ctx, func)?;
    Ok(plan.alloc_expr(QExpr::Call(
        function_def_id(ctx, func).unwrap_or_else(|| DefId::new(0)),
        lowered_args,
        ty,
    )))
}

/// Extract the return type of a closure expression.
pub fn closure_return_ty(
    plan: &LogicalPlan,
    ctx: &ExtractCtxt<'_>,
    expr: QExprId,
) -> Result<TyId, LoweringError> {
    let ty = plan.expr(expr).ty();
    match ctx.tcx.interner().ty(ty) {
        Ty::FnPtr(sig) => Ok(sig.sig.output),
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

/// Extract the `DefId` of a function referenced by a THIR expression, if any.
fn function_def_id(ctx: &ExtractCtxt<'_>, expr: ThirExprId) -> Option<DefId> {
    let expr_data = ctx.thir.exprs.get(expr)?;
    match expr_data {
        ThirExpr::Var(def_id) => Some(*def_id),
        _ => None,
    }
}

/// Return the inferred type of a THIR expression from the type-check results.
fn expr_thir_ty(ctx: &ExtractCtxt<'_>, _expr: ThirExprId) -> Result<TyId, LoweringError> {
    // THIR does not store types directly. We rely on the caller to supply the
    // output type via the surrounding context; for now, return the unit type.
    let unit = ctx.tcx.interner().mk_ty(Ty::Tuple(yelang_ty::list::List::empty()));
    Ok(unit)
}
