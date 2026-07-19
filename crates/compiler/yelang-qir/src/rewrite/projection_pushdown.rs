//! Demand-driven projection pushdown.
//!
//! Propagates `DemandSet` from consumers back to producers and trims Map/Scan
//! projections to only required fields.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct ProjectionPushdownPass;

impl RewritePass for ProjectionPushdownPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement demand propagation and projection pushdown.
        Ok(false)
    }
}
