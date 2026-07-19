//! Normalization rewrite batch.
//!
//! Performs simple structural normalizations:
//! - flatten nested constructs
//! - convert `select projection from source` into Scan -> Map
//! - ensure a root exists

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct NormalizePass;

impl RewritePass for NormalizePass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement normalization rules.
        Ok(false)
    }
}
