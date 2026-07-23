//! Column-reference analysis.
//!
//! Answers two questions the optimizer needs:
//! 1. **Which fields does an expression reference?** (for predicate pushdown)
//! 2. **Which fields does a plan node produce?** (for projection pruning)

use yelang_arena::FxHashSet;
use yelang_hir::hir::expr::Expr;
use yelang_hir::ids::ExprId;
use yelang_hir::Crate;
use yelang_interner::Symbol;

use crate::plan::{Plan, PlanArena, PlanId};

// ---------------------------------------------------------------------------
// Expression-level analysis
// ---------------------------------------------------------------------------

/// Collect all field names referenced by an expression.
pub fn referenced_fields(expr: ExprId, hir: &Crate) -> FxHashSet<Symbol> {
    let mut fields = FxHashSet::new();
    collect_fields(expr, hir, &mut fields);
    fields
}

fn collect_fields(expr: ExprId, hir: &Crate, out: &mut FxHashSet<Symbol>) {
    let Some(expr_node) = hir.expr(expr) else {
        return;
    };

    match expr_node {
        Expr::Field { expr: base, field } => {
            out.insert(field.symbol);
            collect_fields(*base, hir, out);
        }

        Expr::Binary { left, right, .. } => {
            collect_fields(*left, hir, out);
            collect_fields(*right, hir, out);
        }
        Expr::Unary { expr: inner, .. } => {
            collect_fields(*inner, hir, out);
        }
        Expr::Call { func, args } => {
            collect_fields(*func, hir, out);
            for &arg in args {
                collect_fields(arg, hir, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_fields(*receiver, hir, out);
            for &arg in args {
                collect_fields(arg, hir, out);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_fields(*base, hir, out);
            collect_fields(*index, hir, out);
        }
        Expr::Cast { expr: inner, .. } | Expr::TypeAscription { expr: inner, .. } => {
            collect_fields(*inner, hir, out);
        }
        Expr::Assign { left, right } | Expr::AssignOp { left, right, .. } => {
            collect_fields(*left, hir, out);
            collect_fields(*right, hir, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_fields(*cond, hir, out);
            collect_fields(*then_branch, hir, out);
            if let Some(else_expr) = else_branch {
                collect_fields(*else_expr, hir, out);
            }
        }
        Expr::Match { expr: scrutinee, arms } => {
            collect_fields(*scrutinee, hir, out);
            for arm in arms {
                collect_fields(arm.body, hir, out);
                if let Some(guard) = arm.guard {
                    collect_fields(guard, hir, out);
                }
            }
        }
        Expr::Block { block } => {
            if let Some(tail) = block.expr {
                collect_fields(tail, hir, out);
            }
        }
        Expr::Tuple { exprs } | Expr::Array { exprs } => {
            for &e in exprs {
                collect_fields(e, hir, out);
            }
        }
        Expr::Struct { fields, rest, .. } => {
            for field_expr in fields {
                collect_fields(field_expr.expr, hir, out);
            }
            if let Some(rest_expr) = rest {
                collect_fields(*rest_expr, hir, out);
            }
        }
        Expr::Object { fields } => {
            for field_expr in fields {
                collect_fields(field_expr.expr, hir, out);
            }
        }
        Expr::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_fields(*s, hir, out);
            }
            if let Some(e) = end {
                collect_fields(*e, hir, out);
            }
        }
        Expr::DocumentAccess { base, projection } => {
            collect_fields(*base, hir, out);
            for proj in projection {
                if let yelang_hir::hir::expr::DocumentProjection::Field {
                    value: Some(v), ..
                } = proj
                {
                    collect_fields(*v, hir, out);
                }
            }
        }
        Expr::Comprehension {
            element,
            variables,
            condition,
            ..
        } => {
            collect_fields(*element, hir, out);
            for var in variables {
                collect_fields(var.source, hir, out);
            }
            if let Some(cond) = condition {
                collect_fields(*cond, hir, out);
            }
        }
        Expr::Intrinsic { args, .. } => {
            for &arg in args {
                collect_fields(arg, hir, out);
            }
        }

        // Leaves.
        Expr::Lit { .. }
        | Expr::Path { .. }
        | Expr::Closure { .. }
        | Expr::Loop { .. }
        | Expr::Break { .. }
        | Expr::Continue { .. }
        | Expr::Return { .. }
        | Expr::Try { .. }
        | Expr::Await { .. }
        | Expr::Async { .. }
        | Expr::Gen { .. }
        | Expr::Let { .. }
        | Expr::DestructureAssign { .. }
        | Expr::ArrayRepeat { .. }
        | Expr::IsType { .. }
        | Expr::Query(_)
        | Expr::Err => {}
    }
}

// ---------------------------------------------------------------------------
// Plan-level analysis
// ---------------------------------------------------------------------------

/// Collect all field names referenced by a plan node's own expressions.
pub fn plan_referenced_fields(plan: &Plan, hir: &Crate) -> FxHashSet<Symbol> {
    let mut fields = FxHashSet::new();

    match plan {
        Plan::Filter { pred, .. } => {
            fields = referenced_fields(*pred, hir);
        }
        Plan::Project { exprs, .. } => {
            for &(_, expr) in exprs {
                for f in referenced_fields(expr, hir).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Map { func, .. } => {
            fields = referenced_fields(*func, hir);
        }
        Plan::Aggregate { keys, aggs, .. } => {
            for &(_, key_expr) in keys {
                for f in referenced_fields(key_expr, hir).iter() {
                    fields.insert(*f);
                }
            }
            for agg in aggs {
                collect_agg_fields(&agg.kind, hir, &mut fields);
            }
        }
        Plan::Sort { specs, .. } => {
            for spec in specs {
                for f in referenced_fields(spec.expr, hir).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Limit { skip, fetch, .. } => {
            if let Some(s) = skip {
                for f in referenced_fields(*s, hir).iter() {
                    fields.insert(*f);
                }
            }
            if let Some(f_expr) = fetch {
                for f in referenced_fields(*f_expr, hir).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Distinct { on: Some(on), .. } => {
            for &expr in on {
                for f in referenced_fields(expr, hir).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Join { on, filter, .. } => {
            for &(left, right) in on {
                for f in referenced_fields(left, hir).iter() {
                    fields.insert(*f);
                }
                for f in referenced_fields(right, hir).iter() {
                    fields.insert(*f);
                }
            }
            if let Some(f) = filter {
                for sym in referenced_fields(*f, hir).iter() {
                    fields.insert(*sym);
                }
            }
        }
        Plan::Scan { filter, .. } => {
            if let Some(f) = filter {
                for sym in referenced_fields(*f, hir).iter() {
                    fields.insert(*sym);
                }
            }
        }
        Plan::Traverse { paths, .. } => {
            for path in paths {
                for seg in &path.segments {
                    if let Some(pred) = seg.edge_pred {
                        for f in referenced_fields(pred, hir).iter() {
                            fields.insert(*f);
                        }
                    }
                    if let Some(pred) = seg.target_pred {
                        for f in referenced_fields(pred, hir).iter() {
                            fields.insert(*f);
                        }
                    }
                }
            }
        }
        _ => {}
    }

    fields
}

fn collect_agg_fields(kind: &crate::plan::AggKind, hir: &Crate, out: &mut FxHashSet<Symbol>) {
    use crate::plan::AggKind;
    match kind {
        AggKind::Sum { expr }
        | AggKind::Avg { expr }
        | AggKind::Min { expr }
        | AggKind::Max { expr } => {
            for f in referenced_fields(*expr, hir).iter() {
                out.insert(*f);
            }
        }
        AggKind::UserAggregate { args, input_expr, .. } => {
            for &arg in args {
                for f in referenced_fields(arg, hir).iter() {
                    out.insert(*f);
                }
            }
            if let Some(expr) = input_expr {
                for f in referenced_fields(*expr, hir).iter() {
                    out.insert(*f);
                }
            }
        }
        AggKind::Count | AggKind::Opaque { .. } => {}
    }
}

/// Determine the output field names of a plan node.
///
/// Returns an empty set when the schema is unknown (meaning "assume all
/// fields" for safety).
pub fn plan_output_fields(plan: &Plan, arena: &PlanArena, hir: &Crate) -> FxHashSet<Symbol> {
    match plan {
        Plan::Project { exprs, .. } => {
            let mut s = FxHashSet::new();
            for &(name, _) in exprs {
                s.insert(name);
            }
            s
        }

        Plan::Aggregate { keys, aggs, .. } => {
            let mut s = FxHashSet::new();
            for &(name, _) in keys {
                s.insert(name);
            }
            for agg in aggs {
                s.insert(agg.output);
            }
            s
        }

        // Pass-through.
        Plan::Filter { input, .. }
        | Plan::Sort { input, .. }
        | Plan::Limit { input, .. }
        | Plan::Distinct { input, .. }
        | Plan::Map { input, .. }
        | Plan::Traverse { input, .. }
        | Plan::Repeat { input, .. } => {
            if let Some(child) = arena.get(*input) {
                plan_output_fields(child, arena, hir)
            } else {
                FxHashSet::new()
            }
        }

        // Join: union of both sides.
        Plan::Join { left, right, .. }
        | Plan::DependentJoin {
            outer: left,
            inner: right,
            ..
        }
        | Plan::GroupJoin { left, right, .. } => {
            let mut s = FxHashSet::new();
            if let Some(l) = arena.get(*left) {
                for f in plan_output_fields(l, arena, hir).iter() {
                    s.insert(*f);
                }
            }
            if let Some(r) = arena.get(*right) {
                for f in plan_output_fields(r, arena, hir).iter() {
                    s.insert(*f);
                }
            }
            s
        }

        Plan::Union { inputs } => {
            if let Some(&first) = inputs.first() {
                if let Some(child) = arena.get(first) {
                    return plan_output_fields(child, arena, hir);
                }
            }
            FxHashSet::new()
        }

        Plan::ScalarSubquery { plan, .. } | Plan::Exists { plan, .. } => {
            if let Some(child) = arena.get(*plan) {
                plan_output_fields(child, arena, hir)
            } else {
                FxHashSet::new()
            }
        }

        Plan::Extension { node } => {
            let mut s = FxHashSet::new();
            for f in node.output_fields() {
                s.insert(f);
            }
            s
        }

        // Unknown schema.
        Plan::Scan { .. } | Plan::Constant { .. } | Plan::Empty { .. } => FxHashSet::new(),
    }
}

/// Check whether a predicate's referenced fields are all available from
/// a given plan subtree.
pub fn predicate_can_evaluate_against(
    pred_fields: &FxHashSet<Symbol>,
    plan_id: PlanId,
    arena: &PlanArena,
    hir: &Crate,
) -> bool {
    let Some(plan) = arena.get(plan_id) else {
        return false;
    };

    let output = plan_output_fields(plan, arena, hir);

    // Empty output = unknown schema → conservatively allow.
    if output.is_empty() {
        return true;
    }

    pred_fields.iter().all(|f| output.contains(f))
}
