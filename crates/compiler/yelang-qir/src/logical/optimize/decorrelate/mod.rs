//! Subquery decorrelation — BTW 2025 top-down holistic unnesting.
//!
//! Based on:
//! - Neumann, "Improving Unnesting of Complex Queries", BTW 2025
//! - Neumann, "A Formalization of Top-Down Unnesting", arXiv:2412.04294
//! - Neumann & Kemper, "Unnesting Arbitrary Queries", BTW 2015
//!
//! # Algorithm (3 phases)
//!
//! **Phase 1** — Identify non-trivial dependent joins:
//!   Annotate each DependentJoin with its accessing operators (operators
//!   below it that reference its left-hand side columns). Trivial
//!   DependentJoins (empty accessing set) are converted directly.
//!
//! **Phase 2** — Eliminate dependent joins top-to-bottom:
//!   Process from root to leaves. Never push different D sets across
//!   dependent joins. Try simple elimination first (merge selections,
//!   move maps). Full unnesting uses UnnestingInfo + Unnesting state
//!   with union-find for column equivalences.
//!
//! **Phase 3** — Per-operator rules:
//!   Selection → add equivalences + recurse.
//!   Map → recurse + rewrite.
//!   Aggregate → add outer refs to group keys, static agg uses GroupJoin.
//!   Window → add outer refs to PARTITION BY.
//!   Join → split accessing left/right, handle nested DJoin recursively.
//!   Union/Intersect/Except → replicate D on both sides.
//!
//! # Post-condition
//!
//! After `decorrelate()` returns, no `DependentJoin`, `ScalarSubquery`,
//! or `Exists` nodes may remain in the live plan tree.

mod annotate;
mod cte_dag;
mod dependent_join;
mod domain;
mod eliminate;
mod equivalences;
mod orderby_limit;
mod outer_refs;
mod recursive;
mod rewrite;
mod rules;
mod simple;
mod state;
mod union_find;

use crate::logical::plan::{PlanArena, PlanId};

use annotate::annotate_accessing;
use eliminate::eliminate_top_down;
use state::UnnestingState;

/// Run the full decorrelation pipeline on a plan tree.
///
/// Returns the new root PlanId. The arena is mutated in place
/// (new nodes are allocated; old correlated nodes become unreachable).
pub fn decorrelate(
    root: PlanId,
    arena: &mut PlanArena,
    interner: &yelang_interner::Interner,
) -> PlanId {
    // Phase 1: Annotate accessing operators.
    let annotations = annotate_accessing(root, arena);

    // Phase 2: Top-down elimination.
    let mut state = UnnestingState::new(annotations, interner.clone());
    let result = eliminate_top_down(root, &mut state, arena);

    result
}
