//! Map fusion: merge adjacent Map operators when possible.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct MergeMapsPass;

impl RewritePass for MergeMapsPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement map fusion.
        Ok(false)
    }
}
