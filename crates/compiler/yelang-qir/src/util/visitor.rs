//! Visitors over LIR and PIR operator trees.

use crate::ids::LirId;
use crate::lir::plan::LogicalPlan;

/// A visitor over logical operators.
pub trait LirVisitor {
    fn visit(&mut self, plan: &LogicalPlan, id: LirId);
}

/// Walk the logical plan from `root`, calling `visitor.visit` for each operator.
pub fn walk_lir(plan: &LogicalPlan, root: LirId, visitor: &mut dyn LirVisitor) {
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        visitor.visit(plan, id);
        if let Some(op) = plan.operators.get(id) {
            stack.extend(op.children());
        }
    }
}
