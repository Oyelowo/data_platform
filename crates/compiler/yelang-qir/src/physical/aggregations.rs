//! Aggregate implementation selection.

use crate::ids::PhysId;
use crate::logical::operator::AggregateKind;
use crate::physical::operator::PhysOperator;
use crate::physical::PhysicalPlan;

/// Choose a physical aggregate implementation.
///
/// The skeleton always returns a single `Aggregate` operator.
pub fn choose_aggregation(plan: &mut PhysicalPlan, input: PhysId, kind: AggregateKind) -> PhysId {
    let op = PhysOperator::Aggregate { input, kind };
    let id = plan.operators.push(op);
    plan.root = Some(id);
    id
}
