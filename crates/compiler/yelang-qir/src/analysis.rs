//! Column-reference analysis.
//!
//! Answers two questions the optimizer needs:
//! 1. **Which fields does an expression reference?** (for predicate pushdown)
//! 2. **Which fields does a plan node produce?** (for projection pruning)
//!
//! All expression walking goes through the THIR expression arena stored
//! in [`PlanArena::thir_exprs`] — the analysis never touches the HIR.

use yelang_arena::FxHashSet;
use yelang_interner::Symbol;
use yelang_thir::ThirExpr;

use crate::logical::plan::{ExprRef, GroupKey, JoinKey, Plan, PlanArena, PlanId, SortKey};

// ---------------------------------------------------------------------------
// Expression-level analysis
// ---------------------------------------------------------------------------

/// Collect all field names referenced by a THIR expression.
pub fn referenced_fields(expr: ExprRef, arena: &PlanArena) -> FxHashSet<Symbol> {
    let mut fields = FxHashSet::new();
    collect_fields(expr, arena, &mut fields);
    fields
}

fn collect_fields(expr: ExprRef, arena: &PlanArena, out: &mut FxHashSet<Symbol>) {
    let Some(expr_node) = arena.thir_expr(expr) else {
        return;
    };

    match expr_node {
        ThirExpr::Field { base, field } => {
            out.insert(*field);
            collect_fields(*base, arena, out);
        }

        ThirExpr::Binary { left, right, .. } => {
            collect_fields(*left, arena, out);
            collect_fields(*right, arena, out);
        }
        ThirExpr::Unary { expr: inner, .. } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::Call { func, args } => {
            collect_fields(*func, arena, out);
            for &arg in args {
                collect_fields(arg, arena, out);
            }
        }
        ThirExpr::Intrinsic { args, .. } => {
            for &arg in args {
                collect_fields(arg, arena, out);
            }
        }
        ThirExpr::Index { base, index } => {
            collect_fields(*base, arena, out);
            collect_fields(*index, arena, out);
        }
        ThirExpr::Cast { expr: inner, .. } | ThirExpr::TypeAscription { expr: inner, .. } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::Assign { left, right } | ThirExpr::AssignOp { left, right, .. } => {
            collect_fields(*left, arena, out);
            collect_fields(*right, arena, out);
        }
        ThirExpr::If { cond, .. } => {
            collect_fields(*cond, arena, out);
            // then_branch / else_branch are ThirBodyId — not in the expr arena.
        }
        ThirExpr::Match { scrutinee, .. } => {
            collect_fields(*scrutinee, arena, out);
            // Arm bodies are ThirBodyId — not in the expr arena.
        }
        ThirExpr::Block { tail, .. } => {
            if let Some(tail_expr) = tail {
                collect_fields(*tail_expr, arena, out);
            }
        }
        ThirExpr::Tuple { fields } | ThirExpr::Array { exprs: fields } => {
            for &e in fields {
                collect_fields(e, arena, out);
            }
        }
        ThirExpr::ArrayRepeat { value, count } => {
            collect_fields(*value, arena, out);
            collect_fields(*count, arena, out);
        }
        ThirExpr::Struct { fields, rest, .. } => {
            for &(_, field_expr) in fields {
                collect_fields(field_expr, arena, out);
            }
            if let Some(rest_expr) = rest {
                collect_fields(*rest_expr, arena, out);
            }
        }
        ThirExpr::Object { fields } => {
            for &(_, field_expr) in fields {
                collect_fields(field_expr, arena, out);
            }
        }
        ThirExpr::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_fields(*s, arena, out);
            }
            if let Some(e) = end {
                collect_fields(*e, arena, out);
            }
        }
        ThirExpr::Ref { expr: inner, .. } | ThirExpr::Deref { expr: inner } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::Try { expr: inner } | ThirExpr::Await { expr: inner } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::IsType { expr: inner, .. } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::Break { expr: Some(inner), .. } => {
            collect_fields(*inner, arena, out);
        }
        ThirExpr::Return { expr: Some(inner) } => {
            collect_fields(*inner, arena, out);
        }

        // Leaves.
        ThirExpr::Literal(_)
        | ThirExpr::Var(_)
        | ThirExpr::Local(_)
        | ThirExpr::Closure { .. }
        | ThirExpr::Loop { .. }
        | ThirExpr::Break { expr: None, .. }
        | ThirExpr::Continue { .. }
        | ThirExpr::Return { expr: None }
        | ThirExpr::Query(_)
        | ThirExpr::Err => {}
    }
}

// ---------------------------------------------------------------------------
// Plan-level analysis
// ---------------------------------------------------------------------------

/// Collect all field names referenced by a plan node's own expressions.
pub fn plan_referenced_fields(plan: &Plan, arena: &PlanArena) -> FxHashSet<Symbol> {
    let mut fields = FxHashSet::new();

    match plan {
        Plan::Filter { pred, .. } => {
            fields = referenced_fields(*pred, arena);
        }
        Plan::Project { exprs, .. } => {
            for &(_, expr) in exprs {
                for f in referenced_fields(expr, arena).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Map { func, .. } => {
            fields = referenced_fields(*func, arena);
        }
        Plan::Aggregate { keys, aggs, .. } => {
            for (_, key) in keys {
                if let GroupKey::Expr(key_expr) = key {
                    for f in referenced_fields(*key_expr, arena).iter() {
                        fields.insert(*f);
                    }
                }
            }
            for agg in aggs {
                collect_agg_fields(&agg.kind, arena, &mut fields);
            }
        }
        Plan::Window { funcs, .. } => {
            for func in funcs {
                for &sym in &func.partition_by {
                    fields.insert(sym);
                }
                for spec in &func.order_by {
                    if let SortKey::Expr(key_expr) = &spec.key {
                        for f in referenced_fields(*key_expr, arena).iter() {
                            fields.insert(*f);
                        }
                    }
                }
            }
        }
        Plan::Sort { specs, .. } => {
            for spec in specs {
                if let SortKey::Expr(key_expr) = &spec.key {
                    for f in referenced_fields(*key_expr, arena).iter() {
                        fields.insert(*f);
                    }
                }
            }
        }
        Plan::Limit { skip, fetch, .. } => {
            if let Some(s) = skip {
                for f in referenced_fields(*s, arena).iter() {
                    fields.insert(*f);
                }
            }
            if let Some(f_expr) = fetch {
                for f in referenced_fields(*f_expr, arena).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Distinct { on: Some(on), .. } => {
            for &expr in on {
                for f in referenced_fields(expr, arena).iter() {
                    fields.insert(*f);
                }
            }
        }
        Plan::Join { on, filter, .. } => {
            for (left, right) in on {
                collect_join_key_fields(left, &mut fields);
                collect_join_key_fields(right, &mut fields);
            }
            if let Some(f) = filter {
                for sym in referenced_fields(*f, arena).iter() {
                    fields.insert(*sym);
                }
            }
        }
        Plan::Scan { filter, .. } => {
            if let Some(f) = filter {
                for sym in referenced_fields(*f, arena).iter() {
                    fields.insert(*sym);
                }
            }
        }
        Plan::Traverse { paths, .. } => {
            for path in paths {
                for seg in &path.segments {
                    if let Some(pred) = seg.edge_pred {
                        for f in referenced_fields(pred, arena).iter() {
                            fields.insert(*f);
                        }
                    }
                    if let Some(pred) = seg.target_pred {
                        for f in referenced_fields(pred, arena).iter() {
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

fn collect_agg_fields(kind: &crate::logical::plan::AggKind, arena: &PlanArena, out: &mut FxHashSet<Symbol>) {
    use crate::logical::plan::AggKind;
    match kind {
        AggKind::Sum { expr }
        | AggKind::Avg { expr }
        | AggKind::Min { expr }
        | AggKind::Max { expr } => {
            for f in referenced_fields(*expr, arena).iter() {
                out.insert(*f);
            }
        }
        AggKind::UserAggregate { args, input_expr, .. } => {
            for &arg in args {
                for f in referenced_fields(arg, arena).iter() {
                    out.insert(*f);
                }
            }
            if let Some(expr) = input_expr {
                for f in referenced_fields(*expr, arena).iter() {
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
pub fn plan_output_fields(plan: &Plan, arena: &PlanArena) -> FxHashSet<Symbol> {
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

        // Window: pass through input fields + add window func output columns.
        Plan::Window { input, funcs } => {
            let mut s = if let Some(child) = arena.get(*input) {
                plan_output_fields(child, arena)
            } else {
                FxHashSet::new()
            };
            for func in funcs {
                s.insert(func.output);
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
                plan_output_fields(child, arena)
            } else {
                FxHashSet::new()
            }
        }

        // Recursive CTE: output fields from the iteration side.
        Plan::Iterate { iteration, .. } => {
            if let Some(child) = arena.get(*iteration) {
                plan_output_fields(child, arena)
            } else {
                FxHashSet::new()
            }
        }

        // IterateScan: output fields unknown at analysis time.
        Plan::IterateScan { .. } => FxHashSet::new(),

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
                for f in plan_output_fields(l, arena).iter() {
                    s.insert(*f);
                }
            }
            if let Some(r) = arena.get(*right) {
                for f in plan_output_fields(r, arena).iter() {
                    s.insert(*f);
                }
            }
            s
        }

        Plan::Union { inputs } => {
            if let Some(&first) = inputs.first() {
                if let Some(child) = arena.get(first) {
                    return plan_output_fields(child, arena);
                }
            }
            FxHashSet::new()
        }

        Plan::ScalarSubquery { plan, .. } | Plan::Exists { plan, .. } => {
            if let Some(child) = arena.get(*plan) {
                plan_output_fields(child, arena)
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
) -> bool {
    let Some(plan) = arena.get(plan_id) else {
        return false;
    };

    let output = plan_output_fields(plan, arena);

    // Empty output = unknown schema → conservatively allow.
    if output.is_empty() {
        return true;
    }

    pred_fields.iter().all(|f| output.contains(f))
}

/// Collect field names from a JoinKey into a set.
fn collect_join_key_fields(key: &JoinKey, fields: &mut FxHashSet<Symbol>) {
    match key {
        JoinKey::Expr(expr) => {
            // We can't call referenced_fields here because we don't have
            // access to the arena. For Expr keys, the fields will be
            // collected when the expression is analyzed elsewhere.
            // For now, this is a no-op for Expr keys.
            let _ = expr;
        }
        JoinKey::Column(sym) => {
            fields.insert(*sym);
        }
    }
}
