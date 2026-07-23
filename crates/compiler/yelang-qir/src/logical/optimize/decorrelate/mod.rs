//! Subquery decorrelation — eliminate correlated subqueries by rewriting
//! them into joins.
//!
//! Implements the top-down, one-pass algorithm from:
//! - Neumann & Kemper, "Unnesting Arbitrary Queries" (BTW 2015)
//! - Neumann, "Improving Unnesting of Complex Queries" (BTW 2025)
//!
//! # Algorithm overview
//!
//! 1. Convert `ScalarSubquery` / `Exists` nodes into `DependentJoin` nodes.
//! 2. Walk the plan tree **top-down, one pass**.
//! 3. For each `DependentJoin`:
//!    a. Try simple elimination (pull correlation predicate into the join).
//!    b. If nested, unnest the left side first (makes columns available).
//!    c. Build a union-find of column equivalences from join predicates.
//!    d. Unnest the right side under this unnesting's umbrella.
//!    e. At leaves: choose domain-join or substitution via union-find.
//! 4. Invariant: **never push different D sets across dependent joins.**
//!
//! After this pass, no `DependentJoin`, `ScalarSubquery`, or `Exists`
//! nodes remain in the plan tree.

mod dependent_join;
mod eliminate;
mod equivalences;
mod outer_refs;
mod state;
mod union_find;

pub use union_find::UnionFind;

use crate::plan::{DepJoinKind, Plan, PlanArena, PlanId};
use crate::tree::Transformed;

use eliminate::eliminate_recursive;
use state::UnnestingState;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Eliminate all correlated subqueries from the plan tree.
///
/// This is a **one-shot, top-down** pass. It must run before the
/// optimizer's fixpoint loop. After this pass, no `DependentJoin`,
/// `ScalarSubquery`, or `Exists` nodes remain.
///
/// Returns the new root [`PlanId`].
pub fn decorrelate(root: PlanId, arena: &mut PlanArena) -> PlanId {
    // Phase 1: Convert ScalarSubquery/Exists → DependentJoin.
    let root = convert_subqueries_to_dependent_joins(root, arena);

    // Phase 2: Top-down elimination of DependentJoin nodes.
    let mut state = UnnestingState::new();
    eliminate_recursive(root, &mut state, arena)
}

// ---------------------------------------------------------------------------
// Phase 1: Convert subqueries to dependent joins
// ---------------------------------------------------------------------------

/// Convert `ScalarSubquery` and `Exists` nodes into `DependentJoin` nodes.
///
/// This is done as a bottom-up pass before the main top-down elimination.
fn convert_subqueries_to_dependent_joins(
    root: PlanId,
    arena: &mut PlanArena,
) -> PlanId {
    crate::tree::transform_bottom_up(root, arena, &mut |id, arena| {
        let plan = arena.plan(id).clone();
        match &plan {
            Plan::ScalarSubquery { plan: inner, correlation: _ } => {
                // A scalar subquery becomes a dependent single join.
                // The outer side is the "current row" — represented as
                // an Empty node with one row. The actual outer context
                // is provided by the parent plan.
                //
                // For now, we create a DependentJoin with the inner plan
                // and mark it as a Single join (at most one match per
                // outer row).
                let outer = arena.alloc(Plan::Empty { produce_one_row: true });
                let dep_join = Plan::DependentJoin {
                    outer,
                    inner: *inner,
                    pred: None,
                    kind: DepJoinKind::Single,
                };
                let new_id = arena.alloc(dep_join);
                Transformed::yes(new_id)
            }

            Plan::Exists {
                plan: inner,
                correlation: _,
                negated,
            } => {
                let outer = arena.alloc(Plan::Empty { produce_one_row: true });
                let kind = if *negated {
                    DepJoinKind::Anti
                } else {
                    DepJoinKind::Semi
                };
                let dep_join = Plan::DependentJoin {
                    outer,
                    inner: *inner,
                    pred: None,
                    kind,
                };
                let new_id = arena.alloc(dep_join);
                Transformed::yes(new_id)
            }

            _ => Transformed::no(id),
        }
    })
    .id
}
