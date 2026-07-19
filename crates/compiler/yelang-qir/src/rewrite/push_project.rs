//! Projection pushdown through compatible operators.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct PushProjectPass;

impl RewritePass for PushProjectPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement projection pushdown.
        Ok(false)
    }
}
