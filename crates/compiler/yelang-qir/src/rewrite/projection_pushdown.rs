//! Demand-driven projection pushdown.
//!
//! Propagates a `DemandSet` from consumers back to producers and trims
//! `Map`/`Construct` projections to only the fields that are actually needed.
//!
//! For the first cut the pass is conservative: any operator whose demand cannot
//! be precisely mapped to input fields simply demands everything (`DemandSet::all()`).
//! This is always correct and avoids mis-compilation; precision is added only
//! where it is locally safe.

use yelang_arena::FxHashMap;

use crate::demand::DemandSet;
use crate::errors::LoweringError;
use crate::expr::{QExpr, QExprId};
use crate::ids::{BinderId, LirId};
use crate::lir::operator::LirOp;
use crate::lir::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;
use crate::rewrite::reachable_ids;
use crate::util::subst::free_binders;

pub struct ProjectionPushdownPass;

impl RewritePass for ProjectionPushdownPass {
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        let Some(root) = plan.root else {
            return Ok(false);
        };

        // 1. Propagate demand top-down and remember the demand for each operator.
        let demands = {
            let mut demands: FxHashMap<LirId, DemandSet> = FxHashMap::default();
            propagate(plan, root, &DemandSet::all(), &mut demands);
            demands
        };

        // 2. Apply the computed demands: trim Map/Construct projections.
        let mut changed = false;
        for id in reachable_ids(plan) {
            let demand = demands.get(&id).cloned().unwrap_or_else(DemandSet::all);
            if let Some(new_op) = trim_operator(plan, id, &demand) {
                *plan.operator_mut(id) = new_op;
                changed = true;
            }
        }

        Ok(changed)
    }
}

/// Recursively propagate demand from parent to children.
/// `demands` is populated with the demand placed on each operator.
fn propagate(
    plan: &LogicalPlan,
    id: LirId,
    parent_demand: &DemandSet,
    demands: &mut FxHashMap<LirId, DemandSet>,
) {
    demands.insert(id, parent_demand.clone());

    match plan.operator(id) {
        LirOp::Map { input, projection } => {
            let child_demand = map_child_demand(plan, *projection, parent_demand);
            propagate(plan, *input, &child_demand, demands);
        }
        LirOp::Construct { fields, .. } => {
            for (name, child_id) in fields {
                let child_demand = if parent_demand.is_all() || parent_demand.contains(*name) {
                    DemandSet::all()
                } else {
                    DemandSet::none()
                };
                propagate(plan, *child_id, &child_demand, demands);
            }
        }
        LirOp::Filter { input, .. }
        | LirOp::FlatMap { input, .. }
        | LirOp::OrderBy { input, .. }
        | LirOp::Distinct { input, .. }
        | LirOp::GroupBy { input, .. }
        | LirOp::Aggregate { input, .. }
        | LirOp::AggregateGroupBy { input, .. }
        | LirOp::EdgeExpand { input, .. }
        | LirOp::AttachField { input, .. }
        | LirOp::Window { input, .. }
        | LirOp::Slice { input, .. } => {
            propagate(plan, *input, &DemandSet::all(), demands);
        }
        LirOp::Join { left, right, .. }
        | LirOp::DependentJoin {
            outer: left,
            inner: right,
            ..
        }
        | LirOp::SetOp { left, right, .. } => {
            propagate(plan, *left, &DemandSet::all(), demands);
            propagate(plan, *right, &DemandSet::all(), demands);
        }
        _ => {}
    }
}

/// Compute the demand to place on a `Map` input given the parent demand and the
/// projection expression.  If the projection is a record/tuple whose fields can
/// be matched against demanded symbols, only the needed input fields are
/// demanded; otherwise the whole input is demanded.
fn map_child_demand(
    plan: &LogicalPlan,
    projection: QExprId,
    parent_demand: &DemandSet,
) -> DemandSet {
    let (row_binder, body) = match projection_body(plan, projection) {
        Some(v) => v,
        None => return DemandSet::all(),
    };

    let mut child_demand = DemandSet::none();
    match plan.expr(body) {
        QExpr::Record(fields, _) => {
            for (name, expr) in fields {
                if parent_demand.is_all() || parent_demand.contains(*name) {
                    child_demand.union(&demand_from_expr(plan, *expr, row_binder));
                }
            }
        }
        QExpr::Tuple(elems, _) => {
            // DemandSet is keyed by Symbol, so we cannot address tuple indices
            // precisely yet.  Be conservative.
            let _ = elems;
            return DemandSet::all();
        }
        _ => {
            // Scalar or opaque projection: demand whatever the body needs.
            child_demand.union(&demand_from_expr(plan, body, row_binder));
        }
    }
    child_demand
}

/// Extract the row binder and projection body from a plain expression or a
/// single-parameter closure.  Returns `None` when we cannot identify a unique
/// row binder, in which case we fall back to demanding all input fields.
fn projection_body(
    plan: &LogicalPlan,
    projection: QExprId,
) -> Option<(BinderId, QExprId)> {
    match plan.expr(projection) {
        QExpr::Closure { params, body, .. } if params.len() == 1 => Some((params[0], *body)),
        _ => {
            let free = free_binders(plan, projection);
            if free.len() == 1 {
                let b = *free.iter().next().unwrap();
                Some((b, projection))
            } else {
                None
            }
        }
    }
}

/// Compute which input fields are needed to evaluate `expr` for the output row,
/// assuming `row_binder` represents the input row.
fn demand_from_expr(plan: &LogicalPlan, expr: QExprId, row_binder: BinderId) -> DemandSet {
    let mut demand = DemandSet::none();
    collect_expr_demand(plan, expr, row_binder, &mut demand);
    demand
}

fn collect_expr_demand(
    plan: &LogicalPlan,
    expr: QExprId,
    row_binder: BinderId,
    demand: &mut DemandSet,
) {
    match plan.expr(expr) {
        QExpr::Column(b, _) => {
            if *b == row_binder {
                // Whole row is referenced; demand everything.
                *demand = DemandSet::all();
            }
        }
        QExpr::Field(base, field, _) => {
            if let QExpr::Column(b, _) = plan.expr(*base) {
                if *b == row_binder {
                    demand.insert(*field);
                    return;
                }
            }
            // Otherwise walk the base normally.
            collect_expr_demand(plan, *base, row_binder, demand);
        }
        QExpr::Index(base, idx, _) | QExpr::Binary(_, base, idx, _) => {
            collect_expr_demand(plan, *base, row_binder, demand);
            collect_expr_demand(plan, *idx, row_binder, demand);
        }
        QExpr::Unary(_, e, _) | QExpr::Cast(e, _, _) => {
            collect_expr_demand(plan, *e, row_binder, demand);
        }
        QExpr::Call(_, args, _) => {
            for a in args {
                collect_expr_demand(plan, *a, row_binder, demand);
            }
        }
        QExpr::MethodCall { receiver, args, .. } => {
            collect_expr_demand(plan, *receiver, row_binder, demand);
            for a in args {
                collect_expr_demand(plan, *a, row_binder, demand);
            }
        }
        QExpr::Record(fields, _) => {
            for (_, e) in fields {
                collect_expr_demand(plan, *e, row_binder, demand);
            }
        }
        QExpr::Tuple(elems, _) | QExpr::Array(elems, _) => {
            for e in elems {
                collect_expr_demand(plan, *e, row_binder, demand);
            }
        }
        QExpr::If(c, t, e, _) => {
            collect_expr_demand(plan, *c, row_binder, demand);
            collect_expr_demand(plan, *t, row_binder, demand);
            collect_expr_demand(plan, *e, row_binder, demand);
        }
        QExpr::Match { scrutinee, arms, .. } => {
            collect_expr_demand(plan, *scrutinee, row_binder, demand);
            for arm in arms {
                if let Some(g) = arm.guard {
                    collect_expr_demand(plan, g, row_binder, demand);
                }
                // Pattern bindings shadow the row binder inside the arm body,
                // but we are conservative and just collect from the body too.
                collect_expr_demand(plan, arm.body, row_binder, demand);
            }
        }
        QExpr::Closure { params, body, .. } => {
            if !params.contains(&row_binder) {
                collect_expr_demand(plan, *body, row_binder, demand);
            }
        }
        QExpr::Let { name, value, body, .. } => {
            collect_expr_demand(plan, *value, row_binder, demand);
            if *name != row_binder {
                collect_expr_demand(plan, *body, row_binder, demand);
            }
        }
        QExpr::AggregateCall(call, _) => {
            collect_expr_demand(plan, call.input, row_binder, demand);
            collect_expr_demand(plan, call.per_row, row_binder, demand);
        }
        QExpr::WindowCall {
            input,
            partition,
            order,
            ..
        } => {
            collect_expr_demand(plan, *input, row_binder, demand);
            for e in partition {
                collect_expr_demand(plan, *e, row_binder, demand);
            }
            for k in order {
                collect_expr_demand(plan, k.expr, row_binder, demand);
            }
        }
        QExpr::Subplan(_, _) | QExpr::Lit(_, _) | QExpr::Error(_) => {}
    }
}

/// Trim an operator's projection/construct fields based on the computed demand.
/// Returns `Some(new_op)` if the operator changed.
fn trim_operator(
    plan: &mut LogicalPlan,
    id: LirId,
    demand: &DemandSet,
) -> Option<LirOp> {
    let op = plan.operator(id).clone();
    match op {
        LirOp::Map { input, projection } => {
            trim_map_projection(plan, input, projection, demand)
        }
        LirOp::Construct { kind, fields } => {
            if demand.is_all() {
                return None;
            }
            let mut new_fields = Vec::new();
            let mut changed = false;
            for (name, child_id) in fields {
                if demand.contains(name) {
                    new_fields.push((name, child_id));
                } else {
                    changed = true;
                }
            }
            if !changed {
                None
            } else {
                Some(LirOp::Construct {
                    kind,
                    fields: new_fields,
                })
            }
        }
        _ => None,
    }
}

fn trim_map_projection(
    plan: &mut LogicalPlan,
    input: LirId,
    projection: QExprId,
    demand: &DemandSet,
) -> Option<LirOp> {
    if demand.is_all() {
        return None;
    }

    let (_, body) = projection_body(plan, projection)?;

    match plan.expr(body) {
        QExpr::Record(fields, ty) => {
            let mut new_fields = Vec::new();
            let mut changed = false;
            for (name, expr) in fields {
                if demand.contains(*name) {
                    new_fields.push((*name, *expr));
                } else {
                    changed = true;
                }
            }
            if !changed {
                return None;
            }
            let ty = *ty;
            let new_body = plan.alloc_expr(QExpr::Record(new_fields, ty));
            let new_proj = match plan.expr(projection) {
                QExpr::Closure { params, captures, ty, .. } => {
                    let params = params.clone();
                    let captures = captures.clone();
                    plan.alloc_expr(QExpr::Closure {
                        params,
                        body: new_body,
                        captures,
                        ty: *ty,
                    })
                }
                _ => new_body,
            };
            Some(LirOp::Map { input, projection: new_proj })
        }
        _ => None,
    }
}
