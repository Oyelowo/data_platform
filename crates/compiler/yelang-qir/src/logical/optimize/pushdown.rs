//! Predicate pushdown — push `Filter` nodes below joins, traversals,
//! and into scans.

use crate::analysis::{predicate_can_evaluate_against, referenced_fields};
use crate::optimize::{ApplyOrder, OptRule};
use crate::logical::plan::{JoinKind, Plan, PlanArena, PlanId};
use crate::tree::Transformed;

/// Push `Filter` nodes as far down the plan tree as possible.
pub struct PushDownFilter;

impl OptRule for PushDownFilter {
    fn name(&self) -> &str {
        "push_down_filter"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::TopDown
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        let plan = arena.plan(id).clone();

        let Plan::Filter { input, pred } = &plan else {
            return Transformed::no(id);
        };

        let input_plan = arena.plan(*input).clone();

        match &input_plan {
            // Filter → Scan: merge predicate into the scan.
            Plan::Scan {
                source,
                filter: None,
                projection,
                range,
            } => {
                let new_scan = Plan::Scan {
                    source: source.clone(),
                    filter: Some(*pred),
                    projection: projection.clone(),
                    range: range.clone(),
                };
                let new_id = arena.alloc(new_scan);
                if let Some(meta) = arena.meta(*input) {
                    arena.set_meta(new_id, meta.clone());
                }
                Transformed::yes(new_id)
            }

            // Filter → Join: push to the correct side using column analysis.
            Plan::Join {
                left,
                right,
                kind: kind @ (JoinKind::Cross | JoinKind::Inner),
                on,
                filter: join_filter,
            } => {
                let pred_fields = referenced_fields(*pred, arena);

                let can_push_left =
                    predicate_can_evaluate_against(&pred_fields, *left, arena);
                let can_push_right =
                    predicate_can_evaluate_against(&pred_fields, *right, arena);

                if can_push_left && !can_push_right {
                    let new_left = arena.alloc(Plan::Filter {
                        input: *left,
                        pred: *pred,
                    });
                    let new_join = Plan::Join {
                        left: new_left,
                        right: *right,
                        kind: *kind,
                        on: on.clone(),
                        filter: *join_filter,
                    };
                    Transformed::yes(arena.alloc(new_join))
                } else if can_push_right && !can_push_left {
                    let new_right = arena.alloc(Plan::Filter {
                        input: *right,
                        pred: *pred,
                    });
                    let new_join = Plan::Join {
                        left: *left,
                        right: new_right,
                        kind: *kind,
                        on: on.clone(),
                        filter: *join_filter,
                    };
                    Transformed::yes(arena.alloc(new_join))
                } else {
                    Transformed::no(id)
                }
            }

            _ => Transformed::no(id),
        }
    }
}
