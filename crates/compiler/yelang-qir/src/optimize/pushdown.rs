//! Predicate pushdown — push `Filter` nodes below joins, traversals,
//! and into scans.
//!
//! This is the highest-impact optimization: it reduces the number of
//! rows processed by downstream operators.

use crate::optimize::{ApplyOrder, OptRule};
use crate::plan::{JoinKind, Plan, PlanArena, PlanId};
use crate::tree::Transformed;

// ---------------------------------------------------------------------------
// PushDownFilter
// ---------------------------------------------------------------------------

/// Push `Filter` nodes as far down the plan tree as possible.
///
/// Rules:
/// - `Filter(p, Scan)` → merge `p` into `Scan.filter`.
/// - `Filter(p, Join(L, R))` → push `p` into `L` (conservative; full
///   column-reference analysis is TODO).
///
/// For now, this implements the Scan pushdown case (the simplest and
/// most common). Join pushdown requires column-reference analysis which
/// will be added with the metadata pass.
pub struct PushDownFilter;

impl OptRule for PushDownFilter {
    fn name(&self) -> &str {
        "push_down_filter"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::TopDown
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        // Clone the plan to release the immutable borrow on `arena`.
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

            // Filter → Join (cross or inner): push filter to left side.
            // TODO: analyze column references to decide left vs right.
            Plan::Join {
                left,
                right,
                kind: kind @ (JoinKind::Cross | JoinKind::Inner),
                on,
                filter: join_filter,
            } => {
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
                let new_id = arena.alloc(new_join);
                Transformed::yes(new_id)
            }

            _ => Transformed::no(id),
        }
    }
}
