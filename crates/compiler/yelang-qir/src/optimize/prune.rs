//! Projection pruning — remove unused fields from `Scan` nodes.
//!
//! If a `Scan` has no explicit projection and the fields it produces
//! that are actually referenced downstream are known, we can push a
//! projection list into the scan so the storage engine reads fewer
//! columns.

use crate::optimize::{ApplyOrder, OptRule};
use crate::plan::{Plan, PlanArena, PlanId};
use crate::tree::Transformed;

/// Push a projection list into `Scan` nodes that don't have one,
/// based on which fields are actually referenced by ancestor nodes.
///
/// This is a bottom-up pass: it collects the set of fields referenced
/// by the parent chain and pushes the intersection into the scan.
///
/// For now, this is a conservative placeholder: it only prunes when
/// the scan's output fields are fully known (non-empty) and the
/// referenced fields are a strict subset.
pub struct PruneUnusedFields;

impl OptRule for PruneUnusedFields {
    fn name(&self) -> &str {
        "prune_unused_fields"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        let plan = arena.plan(id).clone();

        // Only prune scans that don't already have a projection.
        let Plan::Scan {
            source,
            filter,
            projection: None,
            range,
        } = &plan
        else {
            return Transformed::no(id);
        };

        // Collect all fields referenced by this scan's filter.
        // In a full implementation, we'd also collect fields referenced
        // by all ancestor nodes. For now, we use the scan's own filter
        // as a starting point.
        //
        // TODO: walk up the plan tree to collect all referenced fields
        // from ancestors, then intersect with the scan's output fields.
        let _ = (source, filter, range);
        Transformed::no(id)
    }
}
