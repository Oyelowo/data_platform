//! Subquery unnesting: convert scalar/EXISTS subqueries into joins.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct UnnestSubqueriesPass;

impl RewritePass for UnnestSubqueriesPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement subquery-to-join unnesting.
        Ok(false)
    }
}
