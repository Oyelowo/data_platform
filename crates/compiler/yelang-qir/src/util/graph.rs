//! DAG utilities for QIR plans.

use std::collections::HashSet;

use crate::ids::LirId;
use crate::lir::LogicalPlan;

/// Return the set of operator ids reachable from `root`.
pub fn reachable(plan: &LogicalPlan, root: LirId) -> HashSet<LirId> {
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(op) = plan.operators.get(id) {
            collect_children(op, &mut stack);
        }
    }
    seen
}

fn collect_children(op: &crate::lir::operator::LirOp, out: &mut Vec<LirId>) {
    use crate::lir::operator::LirOp;
    match op {
        LirOp::Scan { .. } | LirOp::Values { .. } | LirOp::Expr(_) => {}
        LirOp::Filter { input, .. }
        | LirOp::Map { input, .. }
        | LirOp::FlatMap { input, .. }
        | LirOp::OrderBy { input, .. }
        | LirOp::Slice { input, .. }
        | LirOp::Distinct { input, .. }
        | LirOp::GroupBy { input, .. }
        | LirOp::Aggregate { input, .. }
        | LirOp::AggregateGroupBy { input, .. }
        | LirOp::EdgeExpand { input, .. }
        | LirOp::AttachField { input, .. }
        | LirOp::Window { input, .. } => out.push(*input),
        LirOp::Join { left, right, .. } | LirOp::DependentJoin { outer: left, inner: right, .. } | LirOp::SetOp { left, right, .. } => {
            out.push(*left);
            out.push(*right);
        }
        LirOp::Construct { fields, .. } => {
            for (_, id) in fields {
                out.push(*id);
            }
        }
    }
}
