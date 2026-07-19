//! Cascades-style goal-driven physical planner.

use crate::backend::capability::BackendCapability;
use crate::errors::PlanError;
use crate::ids::{LirId, PirId};
use crate::logical::LogicalPlan;
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::PhysicalProps;

/// A group of logically equivalent expressions in the memo.
#[derive(Debug, Default)]
pub struct Group {
    pub logical: Option<LirId>,
    pub physical: Vec<PirId>,
    pub props: PhysicalProps,
}

/// Cascades memo structure.
#[derive(Debug, Default)]
pub struct Memo {
    pub groups: Vec<Group>,
}

/// Physical planner.
pub struct Planner<'a> {
    pub logical: &'a LogicalPlan,
    pub backend: &'a dyn BackendCapability,
    pub memo: Memo,
    pub plan: PhysicalPlan,
}

impl<'a> Planner<'a> {
    pub fn new(logical: &'a LogicalPlan, backend: &'a dyn BackendCapability) -> Self {
        Self {
            logical,
            backend,
            memo: Memo::default(),
            plan: PhysicalPlan::empty(),
        }
    }

    /// Plan the logical plan into a physical plan.
    pub fn plan(mut self) -> Result<PhysicalPlan, PlanError> {
        let root = self.logical.root.ok_or(PlanError::NoValidPlan)?;
        let phys_root = self.optimize_group(root, PhysicalProps::any())?;
        self.plan.set_root(phys_root);
        Ok(self.plan)
    }

    fn optimize_group(
        &mut self,
        _lir: LirId,
        _required: PhysicalProps,
    ) -> Result<crate::ids::PirId, PlanError> {
        // TODO: implement Cascades explore/optimize tasks.
        Err(PlanError::NoValidPlan)
    }
}

/// Top-level entry point.
pub fn plan_logical(
    logical: &LogicalPlan,
    backend: &dyn BackendCapability,
) -> Result<PhysicalPlan, PlanError> {
    Planner::new(logical, backend).plan()
}
