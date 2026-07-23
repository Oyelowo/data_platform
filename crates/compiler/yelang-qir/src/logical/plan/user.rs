//! [`UserDefinedPlanNode`] trait.

use yelang_interner::Symbol;

use super::arena::PlanId;
use super::ExprRef;

/// Trait for user-defined logical operators.
///
/// The compiler cannot optimize through these, but they participate in
/// the plan tree and can declare which optimizations are safe.
pub trait UserDefinedPlanNode: std::fmt::Debug + Send + Sync {
    /// Display name for diagnostics and EXPLAIN output.
    fn name(&self) -> &str;

    /// Child plan inputs.
    fn inputs(&self) -> Vec<PlanId>;

    /// Output field names this operator produces.
    fn output_fields(&self) -> Vec<Symbol>;

    /// Can the optimizer push a filter below this node?
    fn supports_filter_pushdown(&self, _pred: &ExprRef) -> bool {
        false
    }

    /// Can the optimizer push a projection into this node?
    fn supports_projection_pushdown(&self, _fields: &[Symbol]) -> bool {
        false
    }
}
