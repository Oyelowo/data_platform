//! Lower `ThirExpr::Query` to LIR using the HIR query side table.
//!
//! This module handles `select` query syntax by building the equivalent LIR
//! pipeline directly from the HIR `SelectQuery`. It reuses `hir_expr.rs` for
//! scalar expressions and closures, and registers HIR-pattern binders for the
//! row variables introduced by `from` and `group by ... into`.

use yelang_arena::FxHashMap;
use yelang_hir::hir::query::{QueryKind, SelectQuery};
use yelang_hir::ids::{PatId, QueryId};
use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::errors::LoweringError;
use crate::expr::{Direction, NullOrdering, OrderKey, QExpr, QExprId};
use crate::ids::{BinderId, LirId};
use crate::lir::operator::ScanSource;
use crate::lir::plan::LogicalPlan;

/// Lower a query-syntax expression (`select ... from ...`) to a QExpr subplan.
pub fn lower_query_syntax(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    query_id: QueryId,
) -> Result<QExprId, LoweringError> {
    let query = ctx
        .tcx
        .crate_hir()
        .query(query_id)
        .cloned()
        .ok_or(LoweringError::UnsupportedClause)?;

    match query.kind {
        QueryKind::Select(sq) => lower_select_query(plan, ctx, &sq),
        _ => Err(LoweringError::UnsupportedClause),
    }
}

fn lower_select_query(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    sq: &SelectQuery,
) -> Result<QExprId, LoweringError> {
    ctx.push_hir_binder_scope();

    // 1. Build a scan for each `from` root.
    let mut roots: Vec<LirId> = Vec::with_capacity(sq.from.len());
    for from in &sq.from {
        roots.push(lower_from_node(plan, ctx, from)?);
    }

    let mut input = match roots.len() {
        0 => return Err(LoweringError::UnsupportedClause),
        1 => roots[0],
        // Multi-root selects become a cross join for now. Decorrelation and
        // join reordering rewrites can improve this later.
        _ => {
            let mut combined = roots[0];
            for right in roots.into_iter().skip(1) {
                let out_ty = plan.props[right].output_ty;
                combined = plan.join(
                    crate::lir::operator::JoinKind::Cross,
                    combined,
                    right,
                    None,
                    out_ty,
                );
            }
            combined
        }
    };

    // 2. WHERE clause.
    if let Some(cond_id) = sq.where_clause {
        let input_binder = plan.props[input]
            .output_binder
            .ok_or(LoweringError::UnsupportedExpr)?;
        let mut binder_map = FxHashMap::default();
        binder_map.insert(from_binder_for_current_input(ctx, sq), input_binder);
        let cond = super::hir_expr::lower_hir_expr(plan, ctx, cond_id, &mut binder_map)?;
        let cond = wrap_as_closure(plan, input_binder, cond);
        let out_ty = plan.props[input].output_ty;
        input = plan.filter(input, cond, out_ty);
        plan.props[input].output_binder = Some(input_binder);
    }

    // 3. GROUP BY.
    if let Some(group) = &sq.group_by {
        let input_binder = plan.props[input]
            .output_binder
            .ok_or(LoweringError::UnsupportedExpr)?;
        let from_binder = from_binder_for_current_input(ctx, sq);
        let mut binder_map = FxHashMap::default();
        binder_map.insert(from_binder, input_binder);

        // Use the first key; composite keys are not yet supported here.
        let key_expr_id = group
            .keys
            .first()
            .map(|k| k.expr)
            .ok_or(LoweringError::UnsupportedClause)?;
        let key = super::hir_expr::lower_hir_expr(plan, ctx, key_expr_id, &mut binder_map)?;
        let key = wrap_as_closure(plan, input_binder, key);
        let key_ty = plan.expr(key).ty();
        let out_ty = ctx.results.pat_ty(group.into_binder).unwrap_or_else(unit_ty);

        let group_row_binder = plan.fresh_binder();
        ctx.insert_hir_binder(group.into_binder, group_row_binder);
        input = plan.group_by(input, key, key_ty, Symbol::from(1), out_ty);
        plan.props[input].output_binder = Some(group_row_binder);
    }

    // 4. ORDER BY.
    if !sq.order_by.is_empty() {
        let input_binder = plan.props[input]
            .output_binder
            .ok_or(LoweringError::UnsupportedExpr)?;
        let active_binder = active_binder_for_projection(ctx, sq);
        let mut binder_map = FxHashMap::default();
        binder_map.insert(active_binder, input_binder);

        let mut keys = Vec::with_capacity(sq.order_by.len());
        for part in &sq.order_by {
            let expr = super::hir_expr::lower_hir_expr(plan, ctx, part.expr, &mut binder_map)?;
            let expr = wrap_as_closure(plan, input_binder, expr);
            keys.push(OrderKey {
                expr,
                dir: match part.direction {
                    yelang_ast::query::SortDirection::Asc => Direction::Asc,
                    yelang_ast::query::SortDirection::Desc => Direction::Desc,
                },
                nulls: NullOrdering::Last,
            });
        }
        let out_ty = plan.props[input].output_ty;
        input = plan.order_by(input, keys, out_ty);
    }

    // 5. RANGE.
    if let Some(range) = &sq.range {
        let input_binder = plan.props[input]
            .output_binder
            .ok_or(LoweringError::UnsupportedExpr)?;
        let active_binder = active_binder_for_projection(ctx, sq);
        let mut binder_map = FxHashMap::default();
        binder_map.insert(active_binder, input_binder);

        let out_ty = plan.props[input].output_ty;
        let offset = range
            .start
            .map(|e| super::hir_expr::lower_hir_expr(plan, ctx, e, &mut binder_map))
            .transpose()?
            .unwrap_or_else(|| plan.alloc_expr(QExpr::Lit(crate::expr::QLit::Int(0), out_ty)));
        let limit = range
            .end
            .map(|e| super::hir_expr::lower_hir_expr(plan, ctx, e, &mut binder_map))
            .transpose()?;

        input = if plan.props[input].ordered {
            plan.slice(input, offset, limit, out_ty)?
        } else {
            plan.slice_unordered(input, offset, limit, out_ty)
        };
    }

    // 6. Projection.
    let input_binder = plan.props[input]
        .output_binder
        .ok_or(LoweringError::UnsupportedExpr)?;
    let active_binder = active_binder_for_projection(ctx, sq);
    let mut binder_map = FxHashMap::default();
    binder_map.insert(active_binder, input_binder);

    let projection = super::hir_expr::lower_hir_expr(plan, ctx, sq.projection, &mut binder_map)?;
    let out_ty = plan.expr(projection).ty();
    let projection_closure = wrap_as_closure(plan, input_binder, projection);
    input = plan.map(input, projection_closure, out_ty);
    plan.props[input].output_binder = Some(input_binder);

    ctx.pop_hir_binder_scope();

    Ok(plan.alloc_expr(QExpr::Subplan(input, out_ty)))
}

fn lower_from_node(
    plan: &mut LogicalPlan,
    ctx: &mut super::ExtractCtxt<'_>,
    from: &yelang_hir::hir::query::FromNode,
) -> Result<LirId, LoweringError> {
    let mut binder_map = FxHashMap::default();
    let source = super::hir_expr::lower_hir_expr(plan, ctx, from.source, &mut binder_map)?;
    let source_ty = plan.expr(source).ty();

    // The element type is the type of the row binder.
    let elem_ty = ctx
        .results
        .pat_ty(from.binder)
        .or_else(|| ctx.results.local_ty(from.binder))
        .or_else(|| ctx.queryable_element_ty(source_ty))
        .unwrap_or(source_ty);

    let binder = plan.fresh_binder();
    ctx.insert_hir_binder(from.binder, binder);

    let root = plan.scan(ScanSource::Expr(source), elem_ty);
    plan.props[root].output_binder = Some(binder);

    // Per-root filter.
    if let Some(filter_id) = from.filter {
        let mut binder_map = FxHashMap::default();
        binder_map.insert(from.binder, binder);
        let pred = super::hir_expr::lower_hir_expr(plan, ctx, filter_id, &mut binder_map)?;
        let pred = wrap_as_closure(plan, binder, pred);
        let filtered = plan.filter(root, pred, elem_ty);
        plan.props[filtered].output_binder = Some(binder);
        return Ok(filtered);
    }

    Ok(root)
}

/// Wrap a scalar expression as a single-argument closure using `binder`.
fn wrap_as_closure(plan: &mut LogicalPlan, binder: BinderId, body: QExprId) -> QExprId {
    let ty = plan.expr(body).ty();
    plan.alloc_expr(QExpr::Closure {
        params: vec![binder],
        body,
        captures: vec![],
        ty,
    })
}

/// Return the HIR pattern id that names the current row in the projection.
/// For a query without `group by`, this is the first `from` binder. With
/// `group by`, it is the `into` binder.
fn active_binder_for_projection(_ctx: &super::ExtractCtxt<'_>, sq: &SelectQuery) -> PatId {
    if let Some(group) = &sq.group_by {
        return group.into_binder;
    }
    sq.from
        .first()
        .map(|f| f.binder)
        .expect("select query has no from root")
}

/// Return the HIR pattern id that names the current row before grouping.
fn from_binder_for_current_input(ctx: &super::ExtractCtxt<'_>, sq: &SelectQuery) -> PatId {
    let _ = ctx;
    sq.from
        .first()
        .map(|f| f.binder)
        .expect("select query has no from root")
}

fn unit_ty() -> TyId {
    yelang_ty::ty::TyId::new(1)
}
