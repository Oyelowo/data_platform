//! Debug and pretty-printing helpers for QIR plans.

use crate::logical::LogicalPlan;
use crate::physical::PhysicalPlan;

/// Pretty-print a logical plan to a string.
pub fn print_logical(plan: &LogicalPlan) -> String {
    format!("{plan:?}")
}

/// Pretty-print a physical plan to a string.
pub fn print_physical(plan: &PhysicalPlan) -> String {
    format!("{plan:?}")
}
