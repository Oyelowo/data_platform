//! Dependent join elimination: unnesting + finalization.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

use crate::logical::plan::{DepJoinKind, ExprRef, JoinKind, Plan, PlanArena, PlanId};

use super::eliminate::eliminate_recursive;
use super::equivalences::add_predicate_equivalences;
use super::outer_refs::compute_outer_refs;
use super::state::{UnnestingInfo, UnnestingState};
use super::union_find::UnionFind;

pub(super) fn eliminate_dependent_join(
    node: PlanId,
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    // Step 1: Compute outer refs (A(outer) ∩ F(inner)).
    let outer_refs = compute_outer_refs(outer, inner, pred, arena);

    // Step 2: Try simple elimination.
    //
    // If the predicate only references inner columns (no correlation),
    // the dependent join is trivially a regular join.
    if outer_refs.is_empty() {
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    // Step 3: Create the unnesting state.
    let info = UnnestingInfo {
        join_id: node,
        outer_refs: outer_refs.clone(),
        cclasses: UnionFind::new(),
        repr: FxHashMap::default(),
        parent: if state.stack.is_empty() {
            None
        } else {
            Some(state.stack.len() - 1)
        },
    };
    let _info_idx = state.push(info);

    // BTW 2025: merge parent's outer_refs into this unnesting.
    // "Never push different D sets across dependent joins."
    state.merge_parent_outer_refs();

    // Step 4: Unnest the LEFT (outer) side first.
    //
    // This makes outer columns available for the inner side's unnesting.
    let new_outer = eliminate_recursive(outer, state, arena);

    // Step 5: Add equivalences from the join predicate to the union-find.
    if let Some(pred_expr) = pred {
        add_predicate_equivalences(pred_expr, arena, state);
    }

    // Step 6: Unnest the RIGHT (inner) side under this unnesting's umbrella.
    let new_inner = eliminate_recursive(inner, state, arena);

    // Step 7: Finalize — build the replacement plan.
    //
    // For each outer ref, try substitution via union-find first.
    // If substitution works, no domain join is needed.
    // Otherwise, create a domain join: D = Π_{outer_refs}(outer).
    let result = finalize_unnesting(
        new_outer,
        new_inner,
        pred,
        kind,
        &outer_refs,
        state,
        arena,
    );

    state.pop();
    result
}

/// Convert a trivial dependent join (no correlation) to a regular join.
fn convert_to_regular_join(
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    arena: &mut PlanArena,
) -> PlanId {
    let join_kind = match kind {
        DepJoinKind::Join | DepJoinKind::Single => JoinKind::Inner,
        DepJoinKind::Semi => JoinKind::Semi,
        DepJoinKind::Anti => JoinKind::Anti,
        DepJoinKind::LeftOuter => JoinKind::Left,
    };

    arena.alloc(Plan::Join {
        left: outer,
        right: inner,
        kind: join_kind,
        on: vec![],
        filter: pred,
    })
}

/// Finalize the unnesting of a dependent join.
///
/// For each outer ref, check if it can be substituted via the union-find.
/// If all can be substituted, no domain join is needed. Otherwise, create a
/// domain projection and join.
fn finalize_unnesting(
    outer: PlanId,
    inner: PlanId,
    pred: Option<ExprRef>,
    kind: DepJoinKind,
    outer_refs: &[Symbol],
    state: &UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    let info = state.current().expect("unnesting info must exist");

    // Check if all outer refs can be substituted.
    let all_substitutable = outer_refs
        .iter()
        .all(|r| info.repr.contains_key(r));

    if all_substitutable && outer_refs.is_empty() {
        // No outer refs: simple regular join.
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    if all_substitutable {
        // All outer refs can be substituted: no domain join needed.
        // The inner plan has already been rewritten with substitutions.
        // Just create a regular join with the predicate.
        return convert_to_regular_join(outer, inner, pred, kind, arena);
    }

    // Some outer refs cannot be substituted: create a domain join.
    //
    // D = Π_{outer_refs}(outer)  — duplicate-free projection
    // result = outer ⋈_{IS NOT DISTINCT FROM} (D ⋈ inner)
    //
    // For now, we create a simplified version:
    // Project(outer, outer_refs) → Join with inner
    let domain = arena.alloc(Plan::Project {
        input: outer,
        exprs: outer_refs
            .iter()
            .map(|&name| (name, ExprRef::default()))
            .collect(),
    });

    let join_kind = match kind {
        DepJoinKind::Join | DepJoinKind::Single => JoinKind::Inner,
        DepJoinKind::Semi => JoinKind::Semi,
        DepJoinKind::Anti => JoinKind::Anti,
        DepJoinKind::LeftOuter => JoinKind::Left,
    };

    // Join the domain with the inner plan.
    let domain_join = arena.alloc(Plan::Join {
        left: domain,
        right: inner,
        kind: JoinKind::Inner,
        on: vec![],
        filter: None,
    });

    // Join the outer with the domain-joined inner.
    arena.alloc(Plan::Join {
        left: outer,
        right: domain_join,
        kind: join_kind,
        on: vec![],
        filter: pred,
    })
}
