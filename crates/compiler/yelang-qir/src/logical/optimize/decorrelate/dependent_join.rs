//! Dependent join elimination (BTW 2025 Fig 6, Ne24 Theorem 4.1).
//!
//! Two paths:
//! 1. **Simple elimination** (Fig 3): merge selections, move maps, empty accessing set.
//! 2. **Full unnesting** (Fig 6): create UnnestingInfo/Unnesting, walk inner plan,
//!    push domain D down via per-operator rules, finalize at leaves.

use yelang_interner::Symbol;

use crate::logical::plan::{DepJoinKind, ExprRef, JoinKind, Plan, PlanArena, PlanId};

use super::domain::{build_domain, build_domain_join_keys};
use super::equivalences::add_predicate_equivalences;
use super::outer_refs::compute_outer_refs;
use super::rules::unnest;
use super::simple::try_simple_elimination;
use super::state::{Unnesting, UnnestingInfo, UnnestingState};

// ---------------------------------------------------------------------------
// Main entry: eliminate a single DependentJoin
// ---------------------------------------------------------------------------

pub(super) fn eliminate_dependent_join(
    node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    // Step 1: Compute outer refs (A(outer) ∩ F(inner ∪ pred)).
    let outer_refs = compute_outer_refs(outer, inner, pred, arena);

    // Step 2: Trivial — no correlation.
    // Still need to eliminate any nested DependentJoins in the inner side.
    if outer_refs.is_empty() {
        let new_inner = super::eliminate::eliminate_top_down(inner, state, arena);
        return convert_to_regular_join(outer, new_inner, pred, kind, arena);
    }

    // Step 3: Try simple elimination (BTW 2025 Fig 3).
    if let Some(result) = try_simple_elimination(node, outer, inner, pred, kind, &outer_refs, state, arena) {
        return result;
    }

    // Step 4: Full unnesting (BTW 2025 Fig 6).
    djoin_elimination(node, outer, inner, pred, kind, outer_refs, state, arena)
}

// ---------------------------------------------------------------------------
// Full unnesting (BTW 2025 Fig 6)
// ---------------------------------------------------------------------------

fn djoin_elimination(
    node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    outer_refs: Vec<Symbol>,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    // Create UnnestingInfo (global, shared).
    let parent_idx = if state.stack.is_empty() {
        None
    } else {
        Some(state.stack.len() - 1)
    };

    let info = UnnestingInfo {
        join_id: node,
        outer_refs,
        domain: None,
        parent: parent_idx,
    };
    let info_idx = state.alloc_info(info);

    // Create Unnesting (per-fragment).
    let unnesting = Unnesting::new(info_idx);
    state.push(unnesting);

    // BTW 2025: merge parent's outer_refs.
    // "Never push different D sets across dependent joins."
    state.merge_parent_outer_refs();

    // Get the accessing operators for this dependent join.
    let accessing: Vec<PlanId> = state.annotations.accessing(node).to_vec();

    // Add equivalences from the join predicate.
    if let Some(pred_expr) = pred {
        if let Some(current) = state.current_mut() {
            add_predicate_equivalences(pred_expr, arena, current);
        }
    }

    // Populate repr from cclasses: for each outer ref with an equivalence
    // to a non-outer column, add it to repr (enables substitution).
    {
        let outer_refs: Vec<Symbol> = state.infos[info_idx].outer_refs.clone();
        if let Some(current) = state.current_mut() {
            super::equivalences::populate_repr(current, &outer_refs);
        }
    }

    // Unnest the RIGHT (inner) side under this unnesting's umbrella.
    let new_inner = unnest(inner, state, &accessing, arena);

    // Finalize: build the replacement plan.
    let result = finalize(node, outer, new_inner, pred, kind, state, arena);

    state.pop();
    result
}

// ---------------------------------------------------------------------------
// Finalize: domain join or substitution
// ---------------------------------------------------------------------------

/// Finalize the unnesting of a dependent join.
///
/// BTW 2025 / Ne24 Lemma 4.2:
/// - If all outer refs have substitutions in repr → regular join (no domain).
/// - Otherwise → domain join: D ⋈ inner, then outer ⋈_{natural_D} result.
fn finalize(
    _node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    let current = state.current().expect("unnesting must be on stack");

    // Check if all outer refs can be substituted.
    if current.all_substitutable(state) {
        // All outer refs substituted: no domain join needed.
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    // Build domain join keys (natural join on outer refs).
    let join_keys = build_domain_join_keys(current, state);

    // Build the domain projection D = Π_{outer_refs}(outer).
    let info_idx = current.info_idx;
    let domain = build_domain(info_idx, state, arena);

    let join_kind = dep_join_kind_to_join_kind(kind);

    // D ⋈ inner (domain join — makes outer columns available in inner).
    let domain_inner = arena.alloc(Plan::Join {
        left: domain,
        right: inner,
        kind: JoinKind::Inner,
        on: join_keys.clone(),
        filter: None,
    });

    // outer ⋈_{natural_D} (D ⋈ inner)
    // The natural join condition is on the outer ref columns.
    arena.alloc(Plan::Join {
        left: outer,
        right: domain_inner,
        kind: join_kind,
        on: join_keys,
        filter: pred,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a trivial dependent join (no correlation) to a regular join.
pub(super) fn convert_to_regular_join(
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    arena: &mut PlanArena,
) -> PlanId {
    arena.alloc(Plan::Join {
        left: outer,
        right: inner,
        kind: dep_join_kind_to_join_kind(kind),
        on: vec![],
        filter: pred,
    })
}

fn dep_join_kind_to_join_kind(kind: DepJoinKind) -> JoinKind {
    match kind {
        DepJoinKind::Join | DepJoinKind::Single => JoinKind::Inner,
        DepJoinKind::Semi => JoinKind::Semi,
        DepJoinKind::Anti => JoinKind::Anti,
        DepJoinKind::LeftOuter => JoinKind::Left,
    }
}
