//! Cost model and cardinality estimation.

use crate::ids::PirId;
use crate::pir::operator::PirOp;
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::Cost;

/// Cardinality estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cardinality {
    pub min: u64,
    pub max: u64,
    pub expected: f64,
}

impl Cardinality {
    pub fn exact(n: u64) -> Self {
        Self { min: n, max: n, expected: n as f64 }
    }

    pub fn unknown() -> Self {
        Self { min: 0, max: u64::MAX, expected: 1_000_000.0 }
    }
}

/// Estimate cardinality of an operator.
pub fn estimate_cardinality(plan: &PhysicalPlan, id: PirId) -> Cardinality {
    match plan.operator(id) {
        PirOp::TableScan { .. } | PirOp::Values { .. } => Cardinality::unknown(),
        PirOp::Filter { input, .. } => {
            let c = estimate_cardinality(plan, *input);
            Cardinality { expected: c.expected * 0.1, ..c }
        }
        PirOp::Project { input, .. }
        | PirOp::FlatMap { input, .. }
        | PirOp::Sort { input, .. }
        | PirOp::TopK { input, .. }
        | PirOp::Slice { input, .. }
        | PirOp::Distinct { input, .. }
        | PirOp::GroupBy { input, .. }
        | PirOp::Exchange { input, .. }
        | PirOp::LocalRepartition { input, .. }
        | PirOp::EdgeExpand { input, .. }
        | PirOp::AttachField { input, .. }
        | PirOp::Window { input, .. } => estimate_cardinality(plan, *input),
        PirOp::HashJoin { build: left, probe: right, .. }
        | PirOp::MergeJoin { left, right, .. }
        | PirOp::NestedLoopJoin { outer: left, inner: right, .. }
        | PirOp::Intersect { left, right }
        | PirOp::Except { left, right } => {
            let l = estimate_cardinality(plan, *left);
            let r = estimate_cardinality(plan, *right);
            Cardinality {
                min: 0,
                max: l.max.min(r.max),
                expected: (l.expected * r.expected).sqrt(),
            }
        }
        PirOp::HashAggregate { group_keys, input, .. }
        | PirOp::SortAggregate { group_keys, input, .. }
        | PirOp::StreamingAggregate { group_keys, input, .. } => {
            let c = estimate_cardinality(plan, *input);
            Cardinality {
                expected: c.expected / (group_keys.len().max(1) as f64 * 10.0),
                ..c
            }
        }
        PirOp::Union { inputs } | PirOp::UnionAll { inputs } => {
            let mut expected = 0.0;
            let mut max = 0;
            for id in inputs {
                let c = estimate_cardinality(plan, *id);
                expected += c.expected;
                max = max.max(c.max);
            }
            Cardinality { min: 0, max, expected }
        }
        PirOp::Construct { fields, .. } => {
            fields.first().map(|(_, id)| estimate_cardinality(plan, *id)).unwrap_or(Cardinality::exact(1))
        }
        PirOp::Expr(_) => Cardinality::exact(1),
    }
}

/// Compute cost of an operator.
pub fn compute_cost(plan: &PhysicalPlan, id: PirId) -> Cost {
    let card = estimate_cardinality(plan, id);
    match plan.operator(id) {
        PirOp::TableScan { .. } => Cost { startup: 0.0, per_row: 1.0 },
        PirOp::Filter { .. } => Cost { startup: 0.0, per_row: 1.0 },
        PirOp::Project { .. } => Cost { startup: 0.0, per_row: 1.0 },
        PirOp::HashJoin { .. } => Cost {
            startup: card.expected * 2.0,
            per_row: 1.0,
        },
        PirOp::HashAggregate { .. } => Cost {
            startup: card.expected * 1.5,
            per_row: 1.0,
        },
        PirOp::Sort { .. } => Cost {
            startup: card.expected * card.expected.log2().max(1.0),
            per_row: 1.0,
        },
        PirOp::Exchange { kind, .. } => match kind {
            ExchangeKind::Gather | ExchangeKind::Broadcast => Cost {
                startup: card.expected * 10.0,
                per_row: 1.0,
            },
            ExchangeKind::RepartitionBy(_) | ExchangeKind::RangePartition(_) => Cost {
                startup: card.expected * 5.0,
                per_row: 1.0,
            },
            ExchangeKind::Single => Cost::zero(),
        },
        _ => Cost::zero(),
    }
}

use crate::pir::operator::ExchangeKind;
