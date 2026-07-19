//! Logical rewrites over QIR plans.
//!
//! Rewrites are applied in a fixed-point loop until no rule makes progress.
//! Phase I includes placeholder modules for the rules listed in the design doc;
//! each rule will be implemented and tested before the phase is marked complete.

use crate::ids::QirId;
use crate::logical::LogicalPlan;

/// Apply all logical rewrites to `plan` and return the new root operator id.
///
/// The skeleton simply returns the existing root without modifying the plan.
pub fn apply_rewrites(plan: &mut LogicalPlan) -> QirId {
    plan.root.unwrap_or_else(|| {
        // If there is no root, allocate a no-op Expr(Error) so the plan is still
        // valid for downstream consumers.
        let id = plan.alloc_operator(crate::logical::operator::Operator::Expr(
            crate::expr::QExpr::Error,
        ));
        plan.set_root(id);
        id
    })
}
