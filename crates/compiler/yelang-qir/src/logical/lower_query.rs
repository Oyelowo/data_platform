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
    ctx: &LoweringCtxt<'_>,
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
    ctx: &LoweringCtxt<'_>,
    sq: &SelectQuery,
) -> Result<LirId, LoweringError> {
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
    if let Some(_group) = &sq.group_by {
        // TODO: build grouping key expression and emit GroupBy or AggregateGroupBy.
    }

    // 5. ORDER BY.
    if !sq.order_by.is_empty() {
        // TODO: lower order keys.
        let out_ty = plan.props[input].output_ty;
        input = plan.order_by(input, vec![], out_ty);
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

    plan.set_root(input);
    Ok(input)
}

fn lower_from_node(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    from: &yelang_hir::hir::query::FromNode,
) -> Result<LirId, LoweringError> {
    let source_expr = super::lower_expr::lower_hir_expr(plan, ctx, from.source)?;
    let source_ty = plan.expr(source_expr).ty();

    // If the source expression is a path to a queryable collection, emit Scan.
    // Otherwise we rely on the expression's type to tell us it is Queryable.
    let mut root = plan.scan(ScanSource::Expr(source_expr), source_ty);

    if let Some(filter) = from.filter {
        let pred = super::lower_expr::lower_hir_expr(plan, ctx, filter)?;
        root = plan.filter(root, pred, source_ty);
    }

    // TODO: per-root order_by and range.

    Ok(root)
}
