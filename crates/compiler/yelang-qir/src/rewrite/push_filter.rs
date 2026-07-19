//! Filter pushdown through Map, Join, and other operators.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct PushFilterPass;

impl RewritePass for PushFilterPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement filter pushdown.
        Ok(false)
    }
}
