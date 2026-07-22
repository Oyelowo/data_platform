//! Resolution of `Aggregate` trait impls from THIR aggregate arguments.

use yelang_arena::DefId;
use yelang_hir::hir::core::ImplItemKind;
use yelang_hir::hir::expr::Expr;
use yelang_hir::ids::{BodyId, ExprId, PatId};
use yelang_hir::res::Res;
use yelang_thir::{ThirExpr, ThirExprId};
use yelang_ty::generic::{GenericArg, Substitution};
use yelang_ty::ty::{ParamTy, Ty, TyId};

use crate::errors::LoweringError;
use crate::expr::{AggregateClass, QExpr, QLit};
use crate::ids::{BinderId, LirId, QExprId};
use crate::lir::operator::AggregateOp;
use crate::lir::plan::LogicalPlan;

use super::context::AggregateImplInfo;

/// Resolve an aggregate config expression (e.g. `Sum {}`) to its config def_id.
pub fn resolve_aggregate_config(
    ctx: &super::ExtractCtxt<'_>,
    agg_arg: ThirExprId,
) -> Result<(DefId, ThirExprId), LoweringError> {
    let expr = ctx
        .thir
        .exprs
        .get(agg_arg)
        .ok_or(LoweringError::UnsupportedExpr)?;
    match expr {
        ThirExpr::Struct { path, .. } => {
            let def_id = match path {
                yelang_hir::res::Res::Def { def_id } => *def_id,
                _ => return Err(LoweringError::UnsupportedExpr),
            };
            Ok((def_id, agg_arg))
        }
        ThirExpr::Call { func, .. } => {
            // `Sum {}` may be desugared to a call to the struct constructor.
            let func_expr = ctx
                .thir
                .exprs
                .get(*func)
                .ok_or(LoweringError::UnsupportedExpr)?;
            if let ThirExpr::Var(def_id) = func_expr {
                Ok((*def_id, agg_arg))
            } else {
                Err(LoweringError::UnsupportedExpr)
            }
        }
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

/// Extract an `AggregateOp` from an aggregate config argument.
pub fn lower_aggregate(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    input: LirId,
    agg_arg: ThirExprId,
) -> Result<QExprId, LoweringError> {
    let (agg_def, _) = resolve_aggregate_config(ctx, agg_arg)?;
    lower_aggregate_with_config(plan, ctx, input, agg_def)
}

/// Extract an `AggregateOp` from a resolved aggregate config `DefId`.
pub fn lower_aggregate_with_config(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    input: LirId,
    agg_def: DefId,
) -> Result<QExprId, LoweringError> {
    let input_ty = plan.props[input].output_ty;
    let elem_ty = ctx.queryable_element_ty(input_ty).unwrap_or(input_ty);

    let impl_info = find_or_resolve_aggregate_impl(ctx, agg_def, elem_ty)?;

    // The per-row input is the element produced by the upstream operator.
    let _input_binder = plan.props[input]
        .output_binder
        .unwrap_or_else(|| plan.fresh_binder());
    // Wrap the per-row element as a closure `|row| row` so the executor can
    // apply it to each input row uniformly with init/step/merge/finish.
    let row_binder = plan.fresh_binder();
    let per_row_body = plan.alloc_expr(QExpr::Column(row_binder, elem_ty));
    let per_row = plan.alloc_expr(QExpr::Closure {
        params: vec![row_binder],
        body: per_row_body,
        captures: vec![],
        ty: elem_ty,
    });

    let init = build_aggregate_closure(plan, ctx, impl_info.init, &[], impl_info.acc_ty)?;
    let step = build_aggregate_closure(
        plan,
        ctx,
        impl_info.step,
        &[("acc", impl_info.acc_ty), ("item", elem_ty)],
        impl_info.acc_ty,
    )?;
    let merge = build_aggregate_closure(
        plan,
        ctx,
        impl_info.merge,
        &[("a", impl_info.acc_ty), ("b", impl_info.acc_ty)],
        impl_info.acc_ty,
    )?;
    let finish = build_aggregate_closure(
        plan,
        ctx,
        impl_info.finish,
        &[("acc", impl_info.acc_ty)],
        impl_info.out_ty,
    )?;

    let config_qexpr = plan.alloc_expr(QExpr::Call(agg_def, vec![], impl_info.acc_ty));

    let agg_op = AggregateOp {
        agg_def: impl_info.agg_def,
        impl_def: impl_info.impl_def,
        class: impl_info.class,
        per_row,
        init,
        step,
        merge,
        finish,
        config: config_qexpr,
        acc_ty: impl_info.acc_ty,
        out_ty: impl_info.out_ty,
    };

    let id = plan.aggregate(input, agg_op, impl_info.out_ty);
    Ok(plan.alloc_expr(QExpr::Subplan(id, impl_info.out_ty)))
}

/// Find the `Aggregate` impl for `Config` with the given input element type.
/// Caches the result in `ctx.aggregate_impls`.
fn find_or_resolve_aggregate_impl(
    ctx: &mut super::ExtractCtxt<'_>,
    agg_def: DefId,
    input_ty: TyId,
) -> Result<AggregateImplInfo, LoweringError> {
    let cached = ctx.aggregate_impl_info(agg_def).cloned();
    if let Some(info) = cached {
        return Ok(info);
    }

    let aggregate_trait = ctx
        .lang_traits
        .aggregate
        .ok_or(LoweringError::UnsupportedExpr)?;

    let interner = ctx.tcx.interner();

    for impl_def_id in ctx.tcx.trait_impls(aggregate_trait) {
        let impl_data = ctx.tcx.impl_def(*impl_def_id);
        let Some(trait_ref) = impl_data.trait_ref else {
            continue;
        };

        // Match self type against the config type.
        let self_ty = interner.ty(impl_data.self_ty);
        let Ty::Adt(self_adt, _) = self_ty else {
            continue;
        };
        if self_adt.def_id != agg_def {
            continue;
        }

        // The trait ref has four type args: Self, In, Acc, Out.
        if trait_ref.args.len() != 4 {
            continue;
        }
        let mut iter = trait_ref.args.iter().skip(1);
        let GenericArg::Type(in_arg) = *iter.next().unwrap() else {
            continue;
        };
        let GenericArg::Type(acc_arg) = *iter.next().unwrap() else {
            continue;
        };
        let GenericArg::Type(out_arg) = *iter.next().unwrap() else {
            continue;
        };

        // Build a substitution mapping impl generic params to concrete types.
        let mut subst = Substitution::empty();
        collect_type_bindings(interner, in_arg, input_ty, &mut subst)?;

        let in_ty = yelang_ty::subst::substitute(interner, in_arg, &subst);
        if in_ty != input_ty {
            continue;
        }
        let acc_ty = yelang_ty::subst::substitute(interner, acc_arg, &subst);
        let out_ty = yelang_ty::subst::substitute(interner, out_arg, &subst);

        // Locate the five required Aggregate methods.
        let mut init = None;
        let mut step = None;
        let mut merge = None;
        let mut finish = None;
        let mut class = None;
        for item in &impl_data.items {
            let item_def_id = item.def_id();
            let name = ctx
                .tcx
                .crate_hir()
                .definition(item_def_id)
                .and_then(|d| ctx.tcx.resolve_symbol(d.name));
            match name {
                Some("init") => init = Some(item_def_id),
                Some("step") => step = Some(item_def_id),
                Some("merge") => merge = Some(item_def_id),
                Some("finish") => finish = Some(item_def_id),
                Some("class") => class = Some(item_def_id),
                _ => {}
            }
        }

        let init = init.ok_or_else(|| LoweringError::MissingAggregate("init".to_string()))?;
        let step = step.ok_or_else(|| LoweringError::MissingAggregate("step".to_string()))?;
        let merge = merge.ok_or_else(|| LoweringError::MissingAggregate("merge".to_string()))?;
        let finish = finish.ok_or_else(|| LoweringError::MissingAggregate("finish".to_string()))?;
        let class = class.ok_or_else(|| LoweringError::MissingAggregate("class".to_string()))?;

        let class = extract_aggregate_class(ctx, class)?;

        let info = AggregateImplInfo {
            impl_def: impl_data.def_id,
            agg_def,
            input_ty,
            acc_ty,
            out_ty,
            init,
            step,
            merge,
            finish,
            class,
        };
        ctx.insert_aggregate_impl_info(info.clone());
        return Ok(info);
    }

    Err(LoweringError::MissingAggregate(format!(
        "no Aggregate impl for config {:?} with input type {:?}",
        agg_def, input_ty
    )))
}

fn collect_type_bindings(
    interner: &yelang_ty::interner::Interner,
    pattern: TyId,
    concrete: TyId,
    subst: &mut Substitution,
) -> Result<(), LoweringError> {
    let pat_ty = interner.ty(pattern);

    if let Ty::Param(ParamTy { index, .. }) = pat_ty {
        let idx = index as usize;
        while subst.args.len() <= idx {
            subst.args.push(GenericArg::Type(concrete));
        }
        subst.args[idx] = GenericArg::Type(concrete);
        return Ok(());
    }

    let conc_ty = interner.ty(concrete);
    match (pat_ty, conc_ty) {
        (Ty::Adt(pat_adt, pat_args), Ty::Adt(conc_adt, conc_args)) if pat_adt == conc_adt => {
            for (pa, ca) in pat_args.iter().zip(conc_args.iter()) {
                let GenericArg::Type(pat_arg_ty) = *pa else { continue };
                let GenericArg::Type(conc_arg_ty) = *ca else { continue };
                collect_type_bindings(interner, pat_arg_ty, conc_arg_ty, subst)?;
            }
            Ok(())
        }
        _ if pat_ty == conc_ty => Ok(()),
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

/// Read the body of `Aggregate::class()` and map it to an `AggregateClass`.
fn extract_aggregate_class(
    ctx: &super::ExtractCtxt<'_>,
    class_method_def_id: DefId,
) -> Result<AggregateClass, LoweringError> {
    let body_id = find_impl_method_body(ctx, class_method_def_id)?;
    let hir = ctx.tcx.crate_hir();
    let body = hir.body(body_id).ok_or(LoweringError::UnsupportedExpr)?;
    let expr = hir.expr(body.value).ok_or(LoweringError::UnsupportedExpr)?;
    let path_res = match expr {
        Expr::Block { block } => block
            .expr
            .and_then(|e| hir.expr(e))
            .and_then(|e| path_res_of_expr(hir, e)),
        _ => path_res_of_expr(hir, expr),
    };
    let Some(Res::Def { def_id }) = path_res else {
        return Err(LoweringError::UnsupportedExpr);
    };
    let name = hir
        .definition(*def_id)
        .and_then(|d| ctx.tcx.resolve_symbol(d.name));
    match name {
        Some("Distributive") => Ok(AggregateClass::Distributive),
        Some("Algebraic") => Ok(AggregateClass::Algebraic),
        Some("Holistic") => Ok(AggregateClass::Holistic),
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

fn path_res_of_expr<'hir>(hir: &'hir yelang_hir::Crate, expr: &'hir Expr) -> Option<&'hir Res> {
    match expr {
        Expr::Path { res } => Some(res),
        Expr::Field { expr, .. } => hir.expr(*expr).and_then(|e| path_res_of_expr(hir, e)),
        _ => None,
    }
}

fn find_impl_method_body(
    ctx: &super::ExtractCtxt<'_>,
    method_def_id: DefId,
) -> Result<BodyId, LoweringError> {
    for imp in &ctx.tcx.crate_hir().impls {
        for item in &imp.items {
            if item.def_id == method_def_id {
                if let ImplItemKind::Fn { body, .. } = &item.kind {
                    return Ok(*body);
                }
            }
        }
    }
    Err(LoweringError::UnsupportedExpr)
}

/// Build a QIR closure from an aggregate method body.
fn build_aggregate_closure(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    method_def_id: DefId,
    params: &[(&str, TyId)],
    ret_ty: TyId,
) -> Result<QExprId, LoweringError> {
    let body_id = find_impl_method_body(ctx, method_def_id)?;
    let hir = ctx.tcx.crate_hir();
    let body = hir.body(body_id).ok_or(LoweringError::UnsupportedExpr)?;

    let mut param_binders = Vec::with_capacity(params.len());
    let mut pat_to_binder: std::collections::HashMap<PatId, BinderId> =
        std::collections::HashMap::new();

    // Map HIR body params to fresh QIR binders by position. Aggregate method
    // bodies have `self` as the first parameter; the supplied `params` are the
    // non-`self` parameters, so we skip the HIR `self` param.
    for (hir_param, (_name, _ty)) in body.params.iter().skip(1).zip(params.iter()) {
        let binder = plan.fresh_binder();
        param_binders.push(binder);
        pat_to_binder.insert(hir_param.pat, binder);
    }

    let qexpr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, body.value, ret_ty, &mut pat_to_binder)?;

    Ok(plan.alloc_expr(QExpr::Closure {
        params: param_binders,
        body: qexpr,
        captures: vec![],
        ty: ret_ty,
    }))
}

/// Minimal HIR → QExpr translator for the bodies found in built-in aggregate
/// impls. Types are read from the aggregate method's `TypeckResults` when
/// available; otherwise they are approximated from the surrounding context.
fn lower_hir_expr_to_qexpr(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    method_def_id: DefId,
    expr_id: ExprId,
    ty: TyId,
    pat_to_binder: &mut std::collections::HashMap<PatId, BinderId>,
) -> Result<QExprId, LoweringError> {
    let hir = ctx.tcx.crate_hir();
    let expr = hir.expr(expr_id).ok_or(LoweringError::UnsupportedExpr)?;

    match expr {
        Expr::Lit { lit } => Ok(plan.alloc_expr(QExpr::Lit(lower_lexer_lit(ctx, lit), ty))),
        Expr::Path { res } => match res {
            Res::Local { pat_id } => {
                if let Some(&binder) = pat_to_binder.get(pat_id) {
                    Ok(plan.alloc_expr(QExpr::Column(binder, ty)))
                } else {
                    // `self` is not used as a closure parameter for built-ins.
                    Ok(plan.alloc_expr(QExpr::Tuple(vec![], ty)))
                }
            }
            Res::Def { def_id } => Ok(plan.alloc_expr(QExpr::Call(*def_id, vec![], ty))),
            _ => Err(LoweringError::UnsupportedExpr),
        },
        Expr::Call { func, args } => {
            let mut lowered_args = Vec::with_capacity(args.len());
            for &arg in args {
                let arg_ty = expr_ty_or_fallback(ctx, method_def_id, arg, ty);
                lowered_args.push(lower_hir_expr_to_qexpr(
                    plan, ctx, method_def_id, arg, arg_ty, pat_to_binder,
                )?);
            }
            match hir.expr(*func) {
                Some(Expr::Path { res: Res::Def { def_id } }) => {
                    Ok(plan.alloc_expr(QExpr::Call(*def_id, lowered_args, ty)))
                }
                Some(Expr::Path { .. }) => Err(LoweringError::UnsupportedExpr),
                _ => {
                    // General function calls are not needed for built-in
                    // aggregate bodies; all constructors are path calls.
                    Err(LoweringError::UnsupportedExpr)
                }
            }
        }
        Expr::Field { expr: base, field } => {
            let base_ty = expr_ty_or_fallback(ctx, method_def_id, *base, ty);
            let base_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *base, base_ty, pat_to_binder)?;
            Ok(plan.alloc_expr(QExpr::Field(base_expr, field.symbol, ty)))
        }
        Expr::Binary { op, left, right } => {
            let left_ty = expr_ty_or_fallback(ctx, method_def_id, *left, ty);
            let right_ty = expr_ty_or_fallback(ctx, method_def_id, *right, ty);
            let left_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *left, left_ty, pat_to_binder)?;
            let right_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *right, right_ty, pat_to_binder)?;
            let qop = match op {
                yelang_ast::BinaryOp::Add => crate::expr::QBinaryOp::Add,
                yelang_ast::BinaryOp::Subtract => crate::expr::QBinaryOp::Sub,
                yelang_ast::BinaryOp::Multiply => crate::expr::QBinaryOp::Mul,
                yelang_ast::BinaryOp::Divide => crate::expr::QBinaryOp::Div,
                yelang_ast::BinaryOp::Modulo => crate::expr::QBinaryOp::Mod,
                yelang_ast::BinaryOp::Eq => crate::expr::QBinaryOp::Eq,
                yelang_ast::BinaryOp::Ne => crate::expr::QBinaryOp::Ne,
                yelang_ast::BinaryOp::Lt => crate::expr::QBinaryOp::Lt,
                yelang_ast::BinaryOp::Lte => crate::expr::QBinaryOp::Lte,
                yelang_ast::BinaryOp::Gt => crate::expr::QBinaryOp::Gt,
                yelang_ast::BinaryOp::Gte => crate::expr::QBinaryOp::Gte,
                _ => crate::expr::QBinaryOp::Add,
            };
            Ok(plan.alloc_expr(QExpr::Binary(qop, left_expr, right_expr, ty)))
        }
        Expr::Unary { op, expr } => {
            let operand_ty = expr_ty_or_fallback(ctx, method_def_id, *expr, ty);
            let operand = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *expr, operand_ty, pat_to_binder)?;
            let qop = match op {
                yelang_ast::UnaryOp::Neg => crate::expr::QUnaryOp::Neg,
                yelang_ast::UnaryOp::Not => crate::expr::QUnaryOp::Not,
                _ => crate::expr::QUnaryOp::Not,
            };
            Ok(plan.alloc_expr(QExpr::Unary(qop, operand, ty)))
        }
        Expr::Struct { path: _path, fields, rest } => {
            if rest.is_some() {
                return Err(LoweringError::UnsupportedExpr);
            }
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let field_ty = expr_ty_or_fallback(ctx, method_def_id, field.expr, ty);
                let field_expr =
                    lower_hir_expr_to_qexpr(plan, ctx, method_def_id, field.expr, field_ty, pat_to_binder)?;
                lowered_fields.push((field.ident.symbol, field_expr));
            }
            Ok(plan.alloc_expr(QExpr::Record(lowered_fields, ty)))
        }
        Expr::Tuple { exprs } => {
            let mut lowered = Vec::with_capacity(exprs.len());
            for &e in exprs {
                let elem_ty = expr_ty_or_fallback(ctx, method_def_id, e, ty);
                lowered.push(lower_hir_expr_to_qexpr(plan, ctx, method_def_id, e, elem_ty, pat_to_binder)?);
            }
            Ok(plan.alloc_expr(QExpr::Tuple(lowered, ty)))
        }
        Expr::Match { expr: scrutinee, arms } => {
            let scrut_ty = expr_ty_or_fallback(ctx, method_def_id, *scrutinee, ty);
            let scrut_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *scrutinee, scrut_ty, pat_to_binder)?;
            let mut lowered_arms = Vec::with_capacity(arms.len());
            for arm in arms {
                let (qpat, arm_pat_to_binder) =
                    lower_hir_pat(plan, ctx, method_def_id, arm.pat, pat_to_binder)?;
                let body_expr =
                    lower_hir_expr_to_qexpr(plan, ctx, method_def_id, arm.body, ty, &mut arm_pat_to_binder.clone())?;
                // Merge any newly bound pattern variables into the outer scope
                // so later arms and the rest of the expression can see them if
                // needed (not required for aggregate bodies, but keeps the map
                // consistent).
                for (pat_id, binder) in arm_pat_to_binder.iter() {
                    if !pat_to_binder.contains_key(pat_id) {
                        pat_to_binder.insert(*pat_id, *binder);
                    }
                }
                lowered_arms.push(crate::expr::MatchArm {
                    pat: qpat,
                    guard: None,
                    body: body_expr,
                });
            }
            Ok(plan.alloc_expr(QExpr::Match {
                scrutinee: scrut_expr,
                arms: lowered_arms,
                ty,
            }))
        }
        Expr::If { cond, then_branch, else_branch } => {
            let cond_ty = expr_ty_or_fallback(ctx, method_def_id, *cond, ctx.tcx.interner().mk_ty(yelang_ty::ty::Ty::Bool));
            let cond_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *cond, cond_ty, pat_to_binder)?;
            let then_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *then_branch, ty, pat_to_binder)?;
            let else_expr = if let Some(else_id) = else_branch {
                lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *else_id, ty, pat_to_binder)?
            } else {
                plan.alloc_expr(QExpr::Tuple(vec![], ty))
            };
            Ok(plan.alloc_expr(QExpr::If(cond_expr, then_expr, else_expr, ty)))
        }
        Expr::Cast { expr: inner, ty: target_hir_ty } => {
            let inner_ty = expr_ty_or_fallback(ctx, method_def_id, *inner, ty);
            let inner_expr = lower_hir_expr_to_qexpr(plan, ctx, method_def_id, *inner, inner_ty, pat_to_binder)?;
            let target_ty = hir_ty_to_ty(ctx, *target_hir_ty).unwrap_or(ty);
            let kind = classify_cast(ctx.tcx.interner(), inner_ty, target_ty);
            Ok(plan.alloc_expr(QExpr::Cast(inner_expr, kind, target_ty)))
        }
        Expr::Block { block } => {
            // Only handle single-expression block tails.
            if let Some(tail) = block.expr {
                lower_hir_expr_to_qexpr(plan, ctx, method_def_id, tail, ty, pat_to_binder)
            } else {
                Err(LoweringError::UnsupportedExpr)
            }
        }
        _ => Err(LoweringError::UnsupportedExpr),
    }
}

/// Look up the inferred type of an expression inside an aggregate method body.
/// Falls back to `fallback` when the per-method typeck results are unavailable.
fn expr_ty_or_fallback(
    ctx: &super::ExtractCtxt<'_>,
    method_def_id: DefId,
    expr_id: ExprId,
    fallback: TyId,
) -> TyId {
    ctx.tcx
        .typeck_results
        .get(method_def_id)
        .and_then(|r| r.expr_ty(expr_id))
        .unwrap_or(fallback)
}

/// Look up the inferred type of a pattern inside an aggregate method body.
fn pat_ty(
    ctx: &super::ExtractCtxt<'_>,
    method_def_id: DefId,
    pat_id: PatId,
) -> Option<TyId> {
    ctx.tcx
        .typeck_results
        .get(method_def_id)
        .and_then(|r| r.pat_ty(pat_id))
}

/// Convert a HIR type annotation to a `TyId`, when possible.
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
                // Primitive types are registered as lang items; map them to the
                // canonical primitive `Ty` instead of an ADT wrapper.
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::I32) {
                    return Some(interner.mk_ty(Ty::Int(IntTy::I32)));
                }
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::I64) {
                    return Some(interner.mk_ty(Ty::Int(IntTy::I64)));
                }
                if Some(*def_id) == ctx.tcx.lang_item(LangItem::F64) {
                    return Some(interner.mk_ty(Ty::Float(FloatTy::F64)));
                }
                Some(interner.mk_ty(yelang_ty::ty::Ty::Adt(
                    yelang_ty::ty::AdtDef { def_id: *def_id },
                    yelang_ty::list::List::empty(),
                )))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Classify a cast into a `CastKind` the executor can interpret.
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

/// Lower a HIR pattern to a QIR pattern, collecting any variable bindings into
/// a copy of `pat_to_binder`.
fn lower_hir_pat(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    method_def_id: DefId,
    pat_id: PatId,
    parent_map: &std::collections::HashMap<PatId, BinderId>,
) -> Result<(crate::expr::Pattern, std::collections::HashMap<PatId, BinderId>), LoweringError> {
    let hir = ctx.tcx.crate_hir();
    let pat = hir.pat(pat_id).ok_or(LoweringError::UnsupportedExpr)?;
    let mut map = parent_map.clone();

    match pat {
        yelang_hir::hir::pat::Pat::Wild => Ok((crate::expr::Pattern::Wild, map)),
        yelang_hir::hir::pat::Pat::Binding { name: _, subpat, .. } => {
            let pat_ty = pat_ty(ctx, method_def_id, pat_id).unwrap_or_else(|| {
                ctx.tcx.interner().mk_ty(yelang_ty::ty::Ty::Error)
            });
            let binder = plan.fresh_binder();
            map.insert(pat_id, binder);
            if let Some(subpat_id) = subpat {
                let (_sub_qpat, sub_map) =
                    lower_hir_pat(plan, ctx, method_def_id, *subpat_id, &map)?;
                map.extend(sub_map.into_iter());
                Ok((
                    crate::expr::Pattern::Bind(binder, pat_ty),
                    map,
                ))
            } else {
                Ok((crate::expr::Pattern::Bind(binder, pat_ty), map))
            }
        }
        yelang_hir::hir::pat::Pat::Path { res } => match res {
            Res::Def { def_id } => {
                let name = ctx
                    .tcx
                    .crate_hir()
                    .definition(*def_id)
                    .and_then(|d| ctx.tcx.resolve_symbol(d.name));
                match name {
                    Some("None") => Ok((crate::expr::Pattern::Tuple(vec![]), map)),
                    _ => Ok((crate::expr::Pattern::Wild, map)),
                }
            }
            _ => Ok((crate::expr::Pattern::Wild, map)),
        },
        yelang_hir::hir::pat::Pat::TupleStruct { res: _res, pats } => {
            let mut sub_pats = Vec::with_capacity(pats.len());
            for subpat_id in pats {
                let (sub_qpat, sub_map) =
                    lower_hir_pat(plan, ctx, method_def_id, *subpat_id, &map)?;
                map.extend(sub_map.into_iter());
                sub_pats.push(sub_qpat);
            }
            Ok((crate::expr::Pattern::Tuple(sub_pats), map))
        }
        yelang_hir::hir::pat::Pat::Tuple { pats } => {
            let mut sub_pats = Vec::with_capacity(pats.len());
            for subpat_id in pats {
                let (sub_qpat, sub_map) =
                    lower_hir_pat(plan, ctx, method_def_id, *subpat_id, &map)?;
                map.extend(sub_map.into_iter());
                sub_pats.push(sub_qpat);
            }
            Ok((crate::expr::Pattern::Tuple(sub_pats), map))
        }
        yelang_hir::hir::pat::Pat::Struct { fields, .. } => {
            let mut lowered_fields = Vec::with_capacity(fields.len());
            for field in fields {
                let (sub_qpat, sub_map) =
                    lower_hir_pat(plan, ctx, method_def_id, field.pat, &map)?;
                map.extend(sub_map.into_iter());
                lowered_fields.push((field.ident.symbol, sub_qpat));
            }
            Ok((crate::expr::Pattern::Record(lowered_fields), map))
        }
        _ => Ok((crate::expr::Pattern::Wild, map)),
    }
}

fn lower_lexer_lit(ctx: &super::ExtractCtxt<'_>, lit: &yelang_hir::hir::core::Lit) -> QLit {
    match lit {
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
        _ => QLit::Unit,
    }
}
