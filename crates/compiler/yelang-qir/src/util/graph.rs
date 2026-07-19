//! DAG utilities for QIR plans.

use std::collections::HashSet;

use crate::ids::QirId;
use crate::logical::LogicalPlan;

/// Return the set of operator ids reachable from `root`.
pub fn reachable(plan: &LogicalPlan, root: QirId) -> HashSet<QirId> {
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(op) = plan.operators.get(id) {
            crate::util::graph::collect_children(op, &mut stack);
        }
    }
    seen
}

fn collect_children(op: &crate::logical::operator::Operator, out: &mut Vec<QirId>) {
    use crate::logical::operator::Operator;
    match op {
        Operator::Scan { .. } | Operator::Expr(_) => {}
        Operator::Filter { input, .. }
        | Operator::Map { input, .. }
        | Operator::FlatMap { input, .. }
        | Operator::OrderBy { input, .. }
        | Operator::Range { input, .. }
        | Operator::Aggregate { input, .. }
        | Operator::Window { input, .. }
        | Operator::Distinct { input, .. }
        | Operator::AttachField { input, .. }
        | Operator::GroupBy { input, .. } => out.push(*input),
        Operator::SetOp { left, right, .. } => {
            out.push(*left);
            out.push(*right);
        }
        Operator::InnerJoin { left, right, .. }
        | Operator::LeftOuterJoin { left, right, .. }
        | Operator::SemiJoin { left, right, .. }
        | Operator::AntiJoin { left, right, .. }
        | Operator::MarkJoin { left, right, .. }
        | Operator::CrossJoin { left, right }
        | Operator::DependentJoin { outer: left, inner: right, .. } => {
            out.push(*left);
            out.push(*right);
        }
        Operator::Construct { fields, .. } => {
            for (_, id) in fields {
                out.push(*id);
            }
        }
    }
}
