//! Exchange insertion for physical plans.

use crate::backend::capability::BackendCapability;
use crate::errors::PlanError;
use crate::physical::PhysicalPlan;

/// Insert required exchange operators into a physical plan.
///
/// The skeleton returns the plan unchanged.
pub fn insert_exchanges(
    plan: PhysicalPlan,
    _backend: &dyn BackendCapability,
) -> Result<PhysicalPlan, PlanError> {
    Ok(plan)
}
