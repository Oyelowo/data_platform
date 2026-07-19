//! Physical QIR: operators, properties, planning, and exchanges.

pub mod aggregations;
pub mod exchanges;
pub mod joins;
pub mod operator;
pub mod planner;
pub mod properties;

pub use operator::{ExchangeKind, PhysOperator};

use crate::backend::capability::BackendCapability;
use crate::errors::PlanError;
use crate::ids::{PhysArena, PhysId};
use crate::logical::LogicalPlan;
use crate::physical::properties::Properties;

/// A physical QIR plan.
#[derive(Debug, Default)]
pub struct PhysicalPlan {
    /// Arena of physical operators.
    pub operators: PhysArena<PhysOperator>,
    /// Root operator id.
    pub root: Option<PhysId>,
    /// Properties derived for each operator.
    pub properties: PhysArena<Properties>,
}

impl PhysicalPlan {
    /// Create an empty physical plan.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Plan a logical QIR plan into a physical plan for the given backend.
    ///
    /// The skeleton returns an empty plan.
    pub fn plan(_logical: &LogicalPlan, _backend: &dyn BackendCapability) -> Result<Self, PlanError> {
        Ok(Self::empty())
    }
}
