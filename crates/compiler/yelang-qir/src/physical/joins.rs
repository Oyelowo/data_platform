//! Join algorithm selection.

use crate::expr::QExpr;
use crate::ids::PhysId;
use crate::physical::operator::PhysOperator;
use crate::physical::PhysicalPlan;

/// Choose a physical join implementation for a logical join predicate.
///
/// The skeleton always returns a `NestedLoopJoin`.
pub fn choose_join(
    plan: &mut PhysicalPlan,
    left: PhysId,
    right: PhysId,
    predicate: QExpr,
) -> PhysId {
    let op = PhysOperator::NestedLoopJoin {
        outer: left,
        inner: right,
        predicate,
    };
    let id = plan.operators.push(op);
    plan.root = Some(id);
    id
}
