//! Logical → physical planning.
//!
//! Converts the optimized logical [`Plan`] tree into a [`PhysOp`] tree
//! with concrete algorithm choices and Exchange nodes for distribution.
//!
//! The planner consults the [`Executor`] trait to make backend-specific
//! decisions (scan strategy, join algorithm, Exchange insertion).

use crate::physical::{
    AggAlgorithm, ExchangeKind, Executor, JoinAlgorithm, PhysArena, PhysId, PhysOp,
    TraverseStrategy,
};
use crate::plan::{Plan, PlanArena, PlanId};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert an optimized logical plan into a physical plan.
///
/// The `executor` parameter determines algorithm choices and whether
/// Exchange nodes are inserted.
pub fn plan_physical(
    logical_root: PlanId,
    logical: &PlanArena,
    executor: &dyn Executor,
    phys: &mut PhysArena,
) -> PhysId {
    lower_node(logical_root, logical, executor, phys)
}

// ---------------------------------------------------------------------------
// Recursive lowering
// ---------------------------------------------------------------------------

fn lower_node(
    id: PlanId,
    logical: &PlanArena,
    executor: &dyn Executor,
    phys: &mut PhysArena,
) -> PhysId {
    let plan = logical.plan(id);

    match plan {
        Plan::Scan {
            source,
            filter,
            projection,
            range,
        } => {
            let strategy = executor.choose_scan(source, filter.is_some());
            phys.alloc(PhysOp::Scan {
                source: source.clone(),
                strategy,
                filter: *filter,
                projection: projection.clone(),
                range: range.clone(),
            })
        }

        Plan::Filter { input, pred } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Filter {
                input: phys_input,
                pred: *pred,
            })
        }

        Plan::Project { input, exprs } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Project {
                input: phys_input,
                exprs: exprs.clone(),
            })
        }

        Plan::Map {
            input,
            func,
            flatten_depth,
        } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Map {
                input: phys_input,
                func: *func,
                flatten_depth: *flatten_depth,
            })
        }

        Plan::Join {
            left,
            right,
            kind,
            on,
            filter,
        } => {
            let phys_left = lower_node(*left, logical, executor, phys);
            let phys_right = lower_node(*right, logical, executor, phys);

            let is_equi = !on.is_empty();
            let left_card = logical
                .meta(*left)
                .and_then(|m| m.est_cardinality);
            let right_card = logical
                .meta(*right)
                .and_then(|m| m.est_cardinality);

            let algorithm = executor.choose_join(left_card, right_card, *kind, is_equi);

            // For distributed execution, insert Exchange nodes before
            // the join if the data isn't already co-partitioned.
            let (phys_left, phys_right) = if executor.is_distributed() {
                insert_join_exchanges(
                    phys_left,
                    phys_right,
                    algorithm,
                    on,
                    executor,
                    phys,
                )
            } else {
                (phys_left, phys_right)
            };

            phys.alloc(PhysOp::Join {
                left: phys_left,
                right: phys_right,
                kind: *kind,
                algorithm,
                on: on.clone(),
                filter: *filter,
            })
        }

        Plan::Aggregate {
            input,
            keys,
            aggs,
            into,
        } => {
            let phys_input = lower_node(*input, logical, executor, phys);

            let est_card = logical.meta(*input).and_then(|m| m.est_cardinality);

            // Choose algorithm based on aggregate metadata.
            // User-defined aggregates with `merge` support can be parallelized
            // (partial aggregation per shard, merge at coordinator).
            let all_parallelizable = aggs.iter().all(|agg| match &agg.kind {
                crate::plan::AggKind::Count
                | crate::plan::AggKind::Sum { .. }
                | crate::plan::AggKind::Avg { .. }
                | crate::plan::AggKind::Min { .. }
                | crate::plan::AggKind::Max { .. } => true,
                // User-defined aggregates: parallelizable if they have merge
                // (all Aggregate trait impls do, by definition).
                crate::plan::AggKind::UserAggregate { .. } => true,
                // Opaque aggregates: cannot be parallelized.
                crate::plan::AggKind::Opaque { .. } => false,
            });

            let algorithm = if executor.is_distributed() && all_parallelizable && !keys.is_empty()
            {
                AggAlgorithm::PartialMerge
            } else {
                executor.choose_aggregate(est_card, None)
            };

            // For distributed execution, insert Exchange before aggregation
            // (shuffle by group keys) unless already partitioned.
            let phys_input = if executor.is_distributed() && !keys.is_empty() {
                let key_symbols: Vec<_> = keys.iter().map(|&(name, _)| name).collect();
                let exchange = phys.alloc(PhysOp::Exchange {
                    input: phys_input,
                    kind: ExchangeKind::ShuffleBy(key_symbols),
                });
                exchange
            } else {
                phys_input
            };

            phys.alloc(PhysOp::Aggregate {
                input: phys_input,
                keys: keys.clone(),
                aggs: aggs.clone(),
                into: *into,
                algorithm,
            })
        }

        Plan::Sort { input, specs } => {
            let phys_input = lower_node(*input, logical, executor, phys);

            let est_card = logical.meta(*input).and_then(|m| m.est_cardinality);
            let algorithm = executor.choose_sort(est_card, false);

            // For distributed execution: local sort + Exchange::Merge.
            let phys_input = if executor.is_distributed() {
                let exchange = phys.alloc(PhysOp::Exchange {
                    input: phys_input,
                    kind: ExchangeKind::Merge(specs.clone()),
                });
                exchange
            } else {
                phys_input
            };

            phys.alloc(PhysOp::Sort {
                input: phys_input,
                specs: specs.clone(),
                algorithm,
            })
        }

        Plan::Limit {
            input,
            skip,
            fetch,
        } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Limit {
                input: phys_input,
                skip: *skip,
                fetch: *fetch,
            })
        }

        Plan::Distinct { input, on } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Distinct {
                input: phys_input,
                on: on.clone(),
            })
        }

        Plan::Union { inputs } => {
            let phys_inputs: Vec<PhysId> = inputs
                .iter()
                .map(|&inp| lower_node(inp, logical, executor, phys))
                .collect();
            phys.alloc(PhysOp::Union {
                inputs: phys_inputs,
            })
        }

        Plan::Traverse { input, paths } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            // Choose traversal strategy based on backend.
            let strategy = if executor.is_distributed() {
                TraverseStrategy::HashJoin
            } else {
                TraverseStrategy::NestedLoop
            };
            phys.alloc(PhysOp::Traverse {
                input: phys_input,
                paths: paths.clone(),
                strategy,
            })
        }

        Plan::Repeat {
            input,
            func,
            max_iters,
        } => {
            let phys_input = lower_node(*input, logical, executor, phys);
            phys.alloc(PhysOp::Repeat {
                input: phys_input,
                func: *func,
                max_iters: *max_iters,
            })
        }

        Plan::Extension { node } => phys.alloc(PhysOp::Extension {
            node: node.clone(),
        }),

        Plan::Constant { value } => phys.alloc(PhysOp::Constant { value: *value }),

        Plan::Empty { produce_one_row } => {
            phys.alloc(PhysOp::Empty {
                produce_one_row: *produce_one_row,
            })
        }

        // These should not survive decorrelation.
        Plan::DependentJoin { .. }
        | Plan::GroupJoin { .. }
        | Plan::ScalarSubquery { .. }
        | Plan::Exists { .. } => {
            // Fallback: treat as opaque. In a correct pipeline, these
            // are eliminated before physical planning.
            phys.alloc(PhysOp::Empty {
                produce_one_row: false,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Exchange insertion for joins
// ---------------------------------------------------------------------------

/// Insert Exchange nodes before a join for distributed execution.
///
/// The strategy depends on the join algorithm:
/// - `ShuffleHash`: shuffle both sides by join key
/// - `BroadcastHash`: broadcast the smaller side
/// - `CoLocatedHash`: no exchange needed (already co-partitioned)
fn insert_join_exchanges(
    left: PhysId,
    right: PhysId,
    algorithm: JoinAlgorithm,
    _on: &[(crate::plan::ExprRef, crate::plan::ExprRef)],
    _executor: &dyn Executor,
    phys: &mut PhysArena,
) -> (PhysId, PhysId) {
    match algorithm {
        JoinAlgorithm::ShuffleHash => {
            // Shuffle both sides by join key.
            // TODO: extract join key symbols from the `on` expressions.
            let left_exchange = phys.alloc(PhysOp::Exchange {
                input: left,
                kind: ExchangeKind::ShuffleBy(vec![]),
            });
            let right_exchange = phys.alloc(PhysOp::Exchange {
                input: right,
                kind: ExchangeKind::ShuffleBy(vec![]),
            });
            (left_exchange, right_exchange)
        }

        JoinAlgorithm::BroadcastHash => {
            // Broadcast the right (smaller) side.
            let right_exchange = phys.alloc(PhysOp::Exchange {
                input: right,
                kind: ExchangeKind::Broadcast,
            });
            (left, right_exchange)
        }

        JoinAlgorithm::CoLocatedHash => {
            // Already co-partitioned: no exchange needed.
            (left, right)
        }

        _ => (left, right),
    }
}
