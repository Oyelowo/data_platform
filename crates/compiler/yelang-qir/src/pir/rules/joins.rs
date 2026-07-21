//! Join algorithm selection.

use crate::pir::capability::BackendCapability;
use crate::errors::PlanError;
use crate::ids::PirId;
use crate::lir::operator::JoinKind;
use crate::pir::operator::PirOp;
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Cost, PhysicalProps};

/// Pick a physical join implementation for a logical join.
pub fn plan_join(
    plan: &mut PhysicalPlan,
    kind: JoinKind,
    left: PirId,
    right: PirId,
    predicate: Option<crate::ids::QExprId>,
    _backend: &dyn BackendCapability,
) -> Result<PirId, PlanError> {
    // Simple heuristic: use hash join for equi-predicates; nested loop otherwise.
    // Real implementation inspects predicate shape and backend capabilities.
    let props = PhysicalProps::any();
    let cost = Cost::zero();

    if let Some(pred) = predicate {
        if is_equi_predicate(pred) {
            // Placeholder: build on smaller side.
            return Ok(plan.alloc(
                PirOp::HashJoin {
                    build: right,
                    probe: left,
                    build_key: pred,
                    probe_key: pred,
                    kind,
                },
                props,
                cost,
            ));
        }
    }

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

fn is_equi_predicate(_predicate: crate::ids::QExprId) -> bool {
    // TODO: analyze QExpr to detect equality.
    false
}
