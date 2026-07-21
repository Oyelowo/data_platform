//! Aggregate physical planning: partial/final/full modes and exchange insertion.

use crate::errors::PlanError;
use crate::expr::AggregateClass;
use crate::ids::{PirId, QExprId};
use crate::pir::operator::{AggMode, ExchangeKind, PhysicalAggregateOp, PirOp};
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::{Cost, PhysicalProps};

/// Plan a scalar aggregate.
pub fn plan_scalar_aggregate(
    plan: &mut PhysicalPlan,
    input: PirId,
    agg: PhysicalAggregateOp,
) -> Result<PirId, PlanError> {
    match agg.class {
        AggregateClass::Distributive | AggregateClass::Algebraic => {
            let partial = plan.alloc(
                PirOp::HashAggregate {
                    input,
                    group_keys: vec![],
                    aggregates: vec![agg.clone()],
                    mode: AggMode::Partial,
                },
                PhysicalProps::any(),
                Cost::zero(),
            );
            let gathered = plan.alloc(
                PirOp::Exchange { input: partial, kind: ExchangeKind::Gather },
                PhysicalProps::any(),
                Cost::zero(),
            );
            Ok(plan.alloc(
                PirOp::HashAggregate {
                    input: gathered,
                    group_keys: vec![],
                    aggregates: vec![agg],
                    mode: AggMode::Final,
                },
                PhysicalProps::any(),
                Cost::zero(),
            ))
        }
        AggregateClass::Holistic => {
            let gathered = plan.alloc(
                PirOp::Exchange { input, kind: ExchangeKind::Gather },
                PhysicalProps::any(),
                Cost::zero(),
            );
            Ok(plan.alloc(
                PirOp::HashAggregate {
                    input: gathered,
                    group_keys: vec![],
                    aggregates: vec![agg],
                    mode: AggMode::Full,
                },
                PhysicalProps::any(),
                Cost::zero(),
            ))
        }
    }
}

/// Plan a grouped aggregate.
pub fn plan_grouped_aggregate(
    plan: &mut PhysicalPlan,
    input: PirId,
    group_keys: Vec<QExprId>,
    aggs: Vec<PhysicalAggregateOp>,
) -> Result<PirId, PlanError> {
    let needs_shuffle = aggs.iter().any(|a| a.class == AggregateClass::Holistic);

    if needs_shuffle {
        // For Holistic grouped, repartition raw data by key, then full agg.
        let shuffled = plan.alloc(
            PirOp::Exchange { input, kind: ExchangeKind::RepartitionBy(group_keys.clone()) },
            PhysicalProps::any(),
            Cost::zero(),
        );
        return Ok(plan.alloc(
            PirOp::HashAggregate {
                input: shuffled,
                group_keys,
                aggregates: aggs,
                mode: AggMode::Full,
            },
            PhysicalProps::any(),
            Cost::zero(),
        ));
    }

    // Distributive/Algebraic: partial per node, shuffle accumulators, final merge.
    let partial = plan.alloc(
        PirOp::HashAggregate {
            input,
            group_keys: group_keys.clone(),
            aggregates: aggs.clone(),
            mode: AggMode::Partial,
        },
        PhysicalProps::any(),
        Cost::zero(),
    );
    let shuffled = plan.alloc(
        PirOp::Exchange { input: partial, kind: ExchangeKind::RepartitionBy(group_keys.clone()) },
        PhysicalProps::any(),
        Cost::zero(),
    );
    Ok(plan.alloc(
        PirOp::HashAggregate {
            input: shuffled,
            group_keys,
            aggregates: aggs,
            mode: AggMode::Final,
        },
        PhysicalProps::any(),
        Cost::zero(),
    ))
}
