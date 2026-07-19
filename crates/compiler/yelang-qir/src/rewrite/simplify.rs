//! Simplification rewrite: constant folding and boolean simplification.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct SimplifyPass;

impl RewritePass for SimplifyPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement constant folding and boolean simplification.
        Ok(false)
    }
}
