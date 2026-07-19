//! Exchange insertion rules.

use crate::ids::PirId;
use crate::pir::operator::{ExchangeKind, PirOp};
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Cost, PhysicalProps};

/// Insert an exchange operator.
pub fn insert_exchange(
    plan: &mut PhysicalPlan,
    input: PirId,
    kind: ExchangeKind,
) -> PirId {
    plan.alloc(
        PirOp::Exchange { input, kind },
        PhysicalProps::any(),
        Cost::zero(),
    )
}

/// Decide whether to broadcast or repartition a join input based on cardinality.
pub fn choose_join_distribution(
    _build_rows: u64,
    _probe_rows: u64,
) -> ExchangeKind {
    // TODO: use backend stats and cost model.
    ExchangeKind::RepartitionBy(vec![])
}
