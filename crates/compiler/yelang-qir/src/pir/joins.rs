//! Join algorithm selection.

use crate::backend::capability::BackendCapability;
use crate::errors::PlanError;
use crate::ids::PirId;
use crate::logical::operator::JoinKind;
use crate::pir::operator::PirOp;
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Cost, PhysicalProps};

/// Pick a physical join implementation for a logical join.
pub fn plan_join(
    plan: &mut PhysicalPlan,
    kind: JoinKind,
    left: PirId,
    right: PirId,
    predicate: crate::ids::QExprId,
    _backend: &dyn BackendCapability,
) -> Result<PirId, PlanError> {
    // Simple heuristic: use hash join for equi-predicates; nested loop otherwise.
    // Real implementation inspects predicate shape and backend capabilities.
    let props = PhysicalProps::any();
    let cost = Cost::zero();

    if is_equi_predicate(predicate) {
        // Placeholder: build on smaller side.
        Ok(plan.alloc(
            PirOp::HashJoin {
                build: right,
                probe: left,
                build_key: predicate,
                probe_key: predicate,
                kind,
            },
            props,
            cost,
        ))
    } else {
        Ok(plan.alloc(
            PirOp::NestedLoopJoin {
                outer: left,
                inner: right,
                predicate,
                kind,
            },
            props,
            cost,
        ))
    }
}

fn is_equi_predicate(_predicate: crate::ids::QExprId) -> bool {
    // TODO: analyze QExpr to detect equality.
    false
}
