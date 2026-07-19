//! Lower HIR query constructs into LIR.

use yelang_hir::hir::query::{Query, QueryKind, SelectQuery};

use crate::errors::LoweringError;
use crate::ids::LirId;
use crate::logical::lower::LoweringCtxt;
use crate::logical::operator::ScanSource;
use crate::logical::plan::LogicalPlan;

/// Lower any HIR query into LIR.
pub fn lower_query(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    query: &Query,
) -> Result<LirId, LoweringError> {
    match &query.kind {
        QueryKind::Select(sq) => lower_select_query(plan, ctx, sq),
        _ => Err(LoweringError::UnsupportedClause),
    }
}

/// Lower a `select` query into LIR.
pub fn lower_select_query(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    sq: &SelectQuery,
) -> Result<LirId, LoweringError> {
    ctx.push_binder_scope();

    // 1. Lower FROM sources.
    let mut roots: Vec<LirId> = Vec::new();
    for from in &sq.from {
        roots.push(lower_from_node(plan, ctx, from)?);
    }

    // 2. Combine roots: single root -> identity, multiple -> cross product / Construct.
    let mut input = if roots.len() == 1 {
        roots[0]
    } else {
        // TODO: multi-root select -> Construct(Facet, roots) or cross join.
        roots[0]
    };

    // 3. WHERE clause -> Filter.
    if let Some(cond) = sq.where_clause {
        let pred = super::lower_expr::lower_hir_expr(plan, ctx, cond)?;
        let out_ty = plan.props[input].output_ty;
        input = plan.filter(input, pred, out_ty);
    }

    // 4. GROUP BY.
    if let Some(group) = &sq.group_by {
        let key_exprs: Result<Vec<_>, _> = group
            .keys
            .iter()
            .map(|k| super::lower_expr::lower_hir_expr(plan, ctx, k.expr))
            .collect();
        let key_exprs = key_exprs?;
        let out_ty = ctx.results.pat_ty(group.into_binder).unwrap_or(ty());
        // Register the group binder so the projection can reference it.
        let group_binder = plan.fresh_binder();
        ctx.insert_binder(group.into_binder, group_binder);
        input = plan.group_by(input, key_exprs[0], plan.expr(key_exprs[0]).ty(), yelang_interner::Symbol::from(1), out_ty);
    }

    // 5. ORDER BY.
    if !sq.order_by.is_empty() {
        let lowered: Result<Vec<_>, _> = sq
            .order_by
            .iter()
            .map(|part| {
                let expr = super::lower_expr::lower_hir_expr(plan, ctx, part.expr)?;
                Ok(crate::expr::OrderKey {
                    expr,
                    dir: crate::expr::Direction::Asc,
                    nulls: crate::expr::NullOrdering::Last,
                })
            })
            .collect();
        let out_ty = plan.props[input].output_ty;
        input = plan.order_by(input, lowered?, out_ty);
    }

    // 6. RANGE.
    if let Some(range) = &sq.range {
        let out_ty = plan.props[input].output_ty;
        let offset = range
            .start
            .map(|e| super::lower_expr::lower_hir_expr(plan, ctx, e))
            .transpose()?
            .unwrap_or_else(|| plan.alloc_expr(crate::expr::QExpr::Lit(crate::expr::QLit::Int(0), out_ty)));
        let limit = range
            .end
            .map(|e| super::lower_expr::lower_hir_expr(plan, ctx, e))
            .transpose()?;
        input = plan.slice(input, offset, limit, out_ty)?;
    }

    // 7. Projection.
    let projection = super::lower_expr::lower_hir_expr(plan, ctx, sq.projection)?;
    let out_ty = plan.expr(projection).ty();
    input = plan.map(input, projection, out_ty);

    ctx.pop_binder_scope();

    plan.set_root(input);
    Ok(input)
}

fn lower_from_node(
    plan: &mut LogicalPlan,
    ctx: &mut LoweringCtxt<'_>,
    from: &yelang_hir::hir::query::FromNode,
) -> Result<LirId, LoweringError> {
    let source_expr = super::lower_expr::lower_hir_expr(plan, ctx, from.source)?;
    let source_ty = plan.expr(source_expr).ty();

    // If the source expression is a path to a queryable collection, emit Scan.
    // Otherwise we rely on the expression's type to tell us it is Queryable.
    let elem_ty = super::queryable::element_ty(ctx, source_ty);
    let mut root = plan.scan(ScanSource::Expr(source_expr), elem_ty);

    // Register the element binder so filters/projections can reference it.
    let binder = plan.fresh_binder();
    ctx.insert_binder(from.binder, binder);

    if let Some(filter) = from.filter {
        let pred = super::lower_expr::lower_hir_expr(plan, ctx, filter)?;
        root = plan.filter(root, pred, elem_ty);
    }

    // TODO: per-root order_by and range.

    Ok(root)
}

fn ty() -> yelang_ty::ty::TyId {
    yelang_ty::ty::TyId::new(1)
}
