//! Predicate pushdown through joins and set operations.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct PredicatePushdownPass;

impl RewritePass for PredicatePushdownPass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement predicate pushdown through joins.
        Ok(false)
    }
}
