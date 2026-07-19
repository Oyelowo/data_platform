//! In-memory interpreter for physical QIR plans.

use crate::errors::PlanError;
use crate::exec::interface::{QueryExecutor, Value};
use crate::exec::kernels::KernelRegistry;
use crate::pir::PhysicalPlan;

/// In-memory query executor.
#[derive(Debug, Default)]
pub struct MemoryExecutor {
    #[allow(dead_code)] // used once scalar evaluation is wired up
    kernels: KernelRegistry,
}

impl MemoryExecutor {
    /// Create a new in-memory executor.
    pub fn new() -> Self {
        Self {
            kernels: KernelRegistry::new(),
        }
    }
}

impl QueryExecutor for MemoryExecutor {
    type Error = PlanError;

    fn execute(&self, _plan: &PhysicalPlan) -> Result<Value, Self::Error> {
        // Skeleton: return an empty array.
        Ok(Value::Array(vec![]))
    }
}
