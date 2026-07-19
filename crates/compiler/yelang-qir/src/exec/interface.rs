//! Execution interface for physical QIR plans.

use crate::errors::PlanError;
pub use crate::exec::value::Value;
use crate::pir::PhysicalPlan;

/// Trait implemented by query executors.
pub trait QueryExecutor {
    type Error;

    /// Execute a physical plan and return its result value.
    fn execute(&self, plan: &PhysicalPlan) -> Result<Value, Self::Error>;
}

impl From<PlanError> for Value {
    fn from(err: PlanError) -> Self {
        Value::Error(err.to_string())
    }
}
