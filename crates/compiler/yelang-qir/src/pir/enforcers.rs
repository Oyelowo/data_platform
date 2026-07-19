//! Property enforcers: insert Sort / Exchange / Gather to satisfy physical properties.

use crate::errors::PlanError;
use crate::ids::PirId;
use crate::pir::operator::{ExchangeKind, PirOp};
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Partitioning, PhysicalOrdering, PhysicalProps};

/// Insert enforcers between `input` and its consumer so that `input` satisfies `required`.
pub fn enforce(
    plan: &mut PhysicalPlan,
    input: PirId,
    required: &PhysicalProps,
    actual: &PhysicalProps,
) -> Result<PirId, PlanError> {
    let mut current = input;

    // Enforce location/partitioning.
    if !partitioning_satisfies(&actual.partitioning, &required.partitioning) {
        current = enforce_partitioning(plan, current, &required.partitioning)?;
    }

    // Enforce ordering.
    if !ordering_satisfies(&actual.ordering, &required.ordering) {
        current = enforce_ordering(plan, current, &required.ordering)?;
    }

    Ok(current)
}

fn enforce_partitioning(
    plan: &mut PhysicalPlan,
    input: PirId,
    required: &Partitioning,
) -> Result<PirId, PlanError> {
    let kind = match required {
        Partitioning::Singleton => ExchangeKind::Gather,
        Partitioning::Hash(keys) => ExchangeKind::RepartitionBy(keys.clone()),
        Partitioning::Range(keys) => ExchangeKind::RangePartition(keys.clone()),
        Partitioning::Replicated => ExchangeKind::Broadcast,
        Partitioning::Any => return Ok(input),
    };
    let props = PhysicalProps::any();
    let cost = crate::pir::props::Cost::zero();
    Ok(plan.alloc(PirOp::Exchange { input, kind }, props, cost))
}

fn enforce_ordering(
    plan: &mut PhysicalPlan,
    input: PirId,
    required: &PhysicalOrdering,
) -> Result<PirId, PlanError> {
    if required.keys.is_empty() {
        return Ok(input);
    }
    let keys: Vec<_> = required.keys.iter().cloned().collect();
    let props = PhysicalProps::any();
    let cost = crate::pir::props::Cost::zero();
    Ok(plan.alloc(PirOp::Sort { input, keys }, props, cost))
}

fn ordering_satisfies(actual: &PhysicalOrdering, required: &PhysicalOrdering) -> bool {
    if required.keys.is_empty() {
        return true;
    }
    actual.keys.starts_with(&required.keys)
}

fn partitioning_satisfies(actual: &Partitioning, required: &Partitioning) -> bool {
    match (actual, required) {
        (_, Partitioning::Any) => true,
        (a, b) => a == b,
    }
}
