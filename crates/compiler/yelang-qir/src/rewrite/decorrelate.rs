//! Neumann-style top-down decorrelation.
//!
//! Transforms correlated subqueries (DependentJoin) into flat joins or
//! semantically-equivalent lateral plans.

use crate::errors::LoweringError;
use crate::logical::plan::LogicalPlan;
use crate::rewrite::pass::RewritePass;

pub struct DecorrelatePass;

impl RewritePass for DecorrelatePass {
    fn run(&self, _plan: &mut LogicalPlan) -> Result<bool, LoweringError> {
        // TODO: implement Neumann top-down decorrelation.
        Ok(false)
    }
}

/// Decorrelation state: which binders are free (outer) vs bound (inner).
#[derive(Clone, Debug, Default)]
pub struct CorrelationContext {
    pub outer_binders: Vec<crate::ids::BinderId>,
}
