//! Rewrite-pass driver.

use crate::errors::LoweringError;
use crate::lir::plan::LogicalPlan;

/// A single logical rewrite pass.
pub trait RewritePass {
    /// Apply the pass to `plan`. Return `true` if the plan changed.
    fn run(&self, plan: &mut LogicalPlan) -> Result<bool, LoweringError>;
}

/// Apply a pass repeatedly until it makes no further progress.
pub fn apply_to_fixpoint<P: RewritePass>(pass: &P, plan: &mut LogicalPlan) -> Result<(), LoweringError> {
    while pass.run(plan)? {}
    Ok(())
}
