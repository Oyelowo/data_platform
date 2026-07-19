//! Logical-to-physical operator mapping.

use crate::backend::capability::BackendCapability;
use crate::errors::PlanError;
use crate::logical::LogicalPlan;
use crate::physical::PhysicalPlan;

/// Plan a logical QIR plan into a physical plan.
///
/// The skeleton returns an empty plan.
pub fn plan(logical: &LogicalPlan, backend: &dyn BackendCapability) -> Result<PhysicalPlan, PlanError> {
    PhysicalPlan::plan(logical, backend)
}
