//! Per-operator unnesting rules (BTW 2025 §3.3, Ne24 Lemmas 4.3–4.18).
//!
//! Each rule follows the same pattern:
//! 1. If accessing set is empty → finalize (domain join or substitution)
//! 2. Add equivalences (for selections/joins)
//! 3. Recurse to input(s) with updated accessing set
//! 4. Rewrite columns on the way back
//! 5. For aggregates/windows: add outer refs to group/partition keys

use yelang_arena::FxHashSet;
use yelang_interner::Symbol;

use crate::logical::plan::{GroupKey, Plan, PlanArena, PlanId, SortKey};

use super::dependent_join::eliminate_dependent_join;
use super::domain::{build_domain, build_domain_join_keys};
use super::equivalences::add_predicate_equivalences;
use super::state::UnnestingState;

// ---------------------------------------------------------------------------
// Main dispatch
// ---------------------------------------------------------------------------

/// Unnest an operator: push the domain D down through it.
///
/// `accessing` is the set of operators below `node` that access the
/// dependent join's left side. When it becomes empty, we finalize.
pub(super) fn unnest(
    node: PlanId,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    // DependentJoin: always eliminate, regardless of accessing set.
    // Nested DependentJoins have their own correlation context.
    if let Plan::DependentJoin { outer, inner, pred, kind } = arena.plan(node).clone() {
        return eliminate_dependent_join(node, outer, inner, pred, kind, state, arena);
    }

    // If no accessing operators below → finalize.
    if accessing.is_empty() {
        return finalize_leaf(node, state, arena);
    }

    // Filter accessing to only those in this subtree.
    let subtree_accessing = filter_accessing_to_subtree(node, accessing, arena);
    if subtree_accessing.is_empty() {
        return finalize_leaf(node, state, arena);
    }

    match arena.plan(node).clone() {
        // Leaf operators → finalize.
        Plan::Scan { .. } | Plan::Constant { .. } | Plan::Empty { .. } => {
            finalize_leaf(node, state, arena)
        }

        // Selection (Lemma 4.8): add equivalences, recurse, rewrite.
        Plan::Filter { input, pred } => {
            unnest_selection(node, input, pred, state, &subtree_accessing, arena)
        }

        // Map (Lemma 4.9): recurse, rewrite.
        Plan::Map { input, func, flatten_depth } => {
            unnest_map(node, input, func, flatten_depth, state, &subtree_accessing, arena)
        }

        // Project (Lemma 4.3/4.4): recurse, rewrite.
        Plan::Project { input, exprs } => {
            unnest_project(node, input, exprs, state, &subtree_accessing, arena)
        }

        // Aggregate (Lemma 4.14): recurse, add outer refs to group keys.
        Plan::Aggregate { input, keys, aggs, into } => {
            unnest_aggregate(node, input, keys, aggs, into, state, &subtree_accessing, arena)
        }

        // Window: recurse, add outer refs to PARTITION BY.
        Plan::Window { input, funcs } => {
            unnest_window(node, input, funcs, state, &subtree_accessing, arena)
        }

        // Join (Lemmas 4.11-4.13): split accessing, handle per-side.
        Plan::Join { left, right, kind, on, filter } => {
            unnest_join(node, left, right, kind, on, filter, state, &subtree_accessing, arena)
        }

        // GroupJoin: recurse to both sides.
        Plan::GroupJoin { left, right, on, aggs } => {
            unnest_group_join(node, left, right, on, aggs, state, &subtree_accessing, arena)
        }

        // Union (Lemma 4.5): replicate D on both sides.
        Plan::Union { inputs } => {
            unnest_union(node, inputs, state, &subtree_accessing, arena)
        }

        // Sort: recurse, rewrite sort specs.
        Plan::Sort { input, specs } => {
            unnest_sort(node, input, specs, state, &subtree_accessing, arena)
        }

        // Limit: recurse.
        Plan::Limit { input, skip, fetch } => {
            unnest_limit(node, input, skip, fetch, state, &subtree_accessing, arena)
        }

        // Distinct: recurse.
        Plan::Distinct { input, on } => {
            unnest_distinct(node, input, on, state, &subtree_accessing, arena)
        }

        // Traverse: recurse.
        Plan::Traverse { input, paths } => {
            unnest_traverse(node, input, paths, state, &subtree_accessing, arena)
        }

        // ScalarSubquery / Exists: should have been converted to DependentJoin.
        Plan::ScalarSubquery { .. } | Plan::Exists { .. } => {
            // Leave as-is — will be handled by a later pass.
            node
        }

        // Extension / Repeat: opaque barriers.
        Plan::Extension { .. } | Plan::Repeat { .. } => node,

        // DependentJoin: handled before the match (above).
        Plan::DependentJoin { .. } => unreachable!("DependentJoin handled before match"),
    }
}

// ---------------------------------------------------------------------------
// Finalize at leaves
// ---------------------------------------------------------------------------

/// Finalize unnesting at a leaf: create domain join or substitution.
fn finalize_leaf(
    node: PlanId,
    state: &mut UnnestingState,
    arena: &mut PlanArena,
) -> PlanId {
    let current = match state.current() {
        Some(c) => c,
        None => return node,
    };

    // If all outer refs are substitutable → no domain join needed.
    if current.all_substitutable(state) {
        return node;
    }

    // Build domain join: D ⋈ node.
    let join_keys = build_domain_join_keys(current, state);
    let info_idx = current.info_idx;
    let domain = build_domain(info_idx, state, arena);

    arena.alloc(Plan::Join {
        left: domain,
        right: node,
        kind: crate::logical::plan::JoinKind::Inner,
        on: join_keys,
        filter: None,
    })
}

// ---------------------------------------------------------------------------
// Per-operator rules
// ---------------------------------------------------------------------------

/// Selection (Lemma 4.8): D ⋈ (σ_p(R)) ≡ σ_p(D ⋈ R)
fn unnest_selection(
    node: PlanId,
    input: PlanId,
    pred: yelang_thir::ids::ThirExprId,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    // Add equivalences from the predicate.
    if let Some(current) = state.current_mut() {
        add_predicate_equivalences(pred, arena, current);
    }

    // Remove self from accessing, recurse to input.
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    // Rebuild the filter with the new input.
    arena.alloc(Plan::Filter {
        input: new_input,
        pred,
    })
}

/// Map (Lemma 4.9): D ⋈ (χ_{a:f}(R)) ≡ χ_{a:f}(D ⋈ R)
fn unnest_map(
    node: PlanId,
    input: PlanId,
    func: yelang_thir::ids::ThirExprId,
    flatten_depth: usize,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    arena.alloc(Plan::Map {
        input: new_input,
        func,
        flatten_depth,
    })
}

/// Project (Lemma 4.3/4.4): D ⋈ (Π_A(R)) ≡ Π_{A∪A(D)}(D ⋈ R)
fn unnest_project(
    node: PlanId,
    input: PlanId,
    exprs: Vec<(Symbol, yelang_thir::ids::ThirExprId)>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    // Add outer refs to the projection (so they're available after the project).
    let mut new_exprs = exprs;
    if let Some(current) = state.current() {
        let info = current.info(state);
        for &outer_ref in &info.outer_refs {
            if !new_exprs.iter().any(|(name, _)| *name == outer_ref) {
                // Add a pass-through for the outer ref column.
                let placeholder = arena.alloc_thir_expr(yelang_thir::ThirExpr::Literal(
                    yelang_hir::hir::core::Lit::Unit,
                ));
                new_exprs.push((outer_ref, placeholder));
            }
        }
    }

    arena.alloc(Plan::Project {
        input: new_input,
        exprs: new_exprs,
    })
}

/// Aggregate (Lemma 4.14): D ⋈ (Γ_{A;f}(R)) ≡ Γ_{A∪A(D);f}(D ⋈ R)
fn unnest_aggregate(
    node: PlanId,
    input: PlanId,
    mut keys: Vec<(Symbol, GroupKey)>,
    aggs: Vec<crate::logical::plan::AggCall>,
    into: Symbol,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let is_static = keys.is_empty();
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    // Add outer refs to group keys.
    if let Some(current) = state.current() {
        let info = current.info(state);
        for &outer_ref in &info.outer_refs {
            if !keys.iter().any(|(name, _)| *name == outer_ref) {
                let repr_col = current.repr.get(&outer_ref).copied().unwrap_or(outer_ref);
                keys.push((outer_ref, GroupKey::Column(repr_col)));
            }
        }
    }

    let new_agg = arena.alloc(Plan::Aggregate {
        input: new_input,
        keys,
        aggs,
        into,
    });

    // Static aggregation (no GROUP BY): use GroupJoin for COUNT bug.
    // The GroupJoin ensures a row is produced even when the input is empty.
    if is_static {
        let has_outer_refs = state.current().map_or(false, |c| !c.info(state).outer_refs.is_empty());
        if has_outer_refs {
            let info_idx = state.current().expect("unnesting on stack").info_idx;
            let join_keys = build_domain_join_keys(state.current().expect("unnesting on stack"), state);
            let domain = build_domain(info_idx, state, arena);

            return arena.alloc(Plan::GroupJoin {
                left: domain,
                right: new_agg,
                on: join_keys,
                aggs: vec![],
            });
        }
    }

    new_agg
}

/// Window: add outer refs to PARTITION BY.
fn unnest_window(
    node: PlanId,
    input: PlanId,
    mut funcs: Vec<crate::logical::plan::WindowFunc>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    // Add outer refs to PARTITION BY.
    if let Some(current) = state.current() {
        let info = current.info(state);
        for func in &mut funcs {
            for &outer_ref in &info.outer_refs {
                let repr_col = current.repr.get(&outer_ref).copied().unwrap_or(outer_ref);
                if !func.partition_by.contains(&repr_col) {
                    func.partition_by.push(repr_col);
                }
            }
        }
    }

    arena.alloc(Plan::Window {
        input: new_input,
        funcs,
    })
}

/// Join (Lemmas 4.11-4.13): split accessing, handle per-side.
fn unnest_join(
    node: PlanId,
    left: PlanId,
    right: PlanId,
    kind: crate::logical::plan::JoinKind,
    on: Vec<(crate::logical::plan::JoinKey, crate::logical::plan::JoinKey)>,
    filter: Option<yelang_thir::ids::ThirExprId>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    // Check for nested dependent join.
    if state.annotations.accessing(node).iter().any(|&a| accessing.contains(&a)) {
        // This join IS a dependent join — eliminate it recursively.
        if let Plan::DependentJoin { outer, inner, pred, kind: dj_kind } = arena.plan(node).clone() {
            return eliminate_dependent_join(node, outer, inner, pred, dj_kind, state, arena);
        }
    }

    // Split accessing into left/right subtrees.
    let left_accessing = filter_accessing_to_subtree(left, accessing, arena);
    let right_accessing = filter_accessing_to_subtree(right, accessing, arena);

    // Determine if each side outputs unmatched rows.
    let left_outputs_unmatched = matches!(
        kind,
        crate::logical::plan::JoinKind::Right | crate::logical::plan::JoinKind::Full
    );
    let right_outputs_unmatched = matches!(
        kind,
        crate::logical::plan::JoinKind::Left | crate::logical::plan::JoinKind::Full
    );

    // One side only: recurse to that side.
    if right_accessing.is_empty() && !right_outputs_unmatched {
        let new_left = unnest(left, state, &left_accessing, arena);
        return arena.alloc(Plan::Join {
            left: new_left,
            right,
            kind,
            on,
            filter,
        });
    }

    if left_accessing.is_empty() && !left_outputs_unmatched {
        let new_right = unnest(right, state, &right_accessing, arena);
        return arena.alloc(Plan::Join {
            left,
            right: new_right,
            kind,
            on,
            filter,
        });
    }

    // Both sides: unnest both.
    let new_left = unnest(left, state, &left_accessing, arena);
    let new_right = unnest(right, state, &right_accessing, arena);

    arena.alloc(Plan::Join {
        left: new_left,
        right: new_right,
        kind,
        on,
        filter,
    })
}

/// GroupJoin: recurse to both sides.
fn unnest_group_join(
    _node: PlanId,
    left: PlanId,
    right: PlanId,
    on: Vec<(crate::logical::plan::JoinKey, crate::logical::plan::JoinKey)>,
    aggs: Vec<crate::logical::plan::AggCall>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let left_accessing = filter_accessing_to_subtree(left, accessing, arena);
    let right_accessing = filter_accessing_to_subtree(right, accessing, arena);

    let new_left = unnest(left, state, &left_accessing, arena);
    let new_right = unnest(right, state, &right_accessing, arena);

    arena.alloc(Plan::GroupJoin {
        left: new_left,
        right: new_right,
        on,
        aggs,
    })
}

/// Union (Lemma 4.5): D ⋈ (R1 ∪ R2) ≡ (D ⋈ R1) ∪ (D ⋈ R2)
fn unnest_union(
    _node: PlanId,
    inputs: Vec<PlanId>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let new_inputs: Vec<PlanId> = inputs
        .iter()
        .map(|&input| {
            let input_accessing = filter_accessing_to_subtree(input, accessing, arena);
            unnest(input, state, &input_accessing, arena)
        })
        .collect();

    arena.alloc(Plan::Union { inputs: new_inputs })
}

/// Sort: recurse, add outer refs as prefix sort keys.
fn unnest_sort(
    node: PlanId,
    input: PlanId,
    mut specs: Vec<crate::logical::plan::SortSpec>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    // Add outer refs as prefix sort keys (for deterministic ordering within groups).
    if let Some(current) = state.current() {
        let info = current.info(state);
        for &outer_ref in &info.outer_refs {
            let repr_col = current.repr.get(&outer_ref).copied().unwrap_or(outer_ref);
            specs.insert(0, crate::logical::plan::SortSpec {
                key: SortKey::Column(repr_col),
                desc: false,
            });
        }
    }

    arena.alloc(Plan::Sort {
        input: new_input,
        specs,
    })
}

/// Limit: recurse.
fn unnest_limit(
    node: PlanId,
    input: PlanId,
    skip: Option<yelang_thir::ids::ThirExprId>,
    fetch: Option<yelang_thir::ids::ThirExprId>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    arena.alloc(Plan::Limit {
        input: new_input,
        skip,
        fetch,
    })
}

/// Distinct: recurse.
fn unnest_distinct(
    node: PlanId,
    input: PlanId,
    on: Option<Vec<yelang_thir::ids::ThirExprId>>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    arena.alloc(Plan::Distinct { input: new_input, on })
}

/// Traverse: recurse.
fn unnest_traverse(
    node: PlanId,
    input: PlanId,
    paths: Vec<crate::logical::plan::TraversePath>,
    state: &mut UnnestingState,
    accessing: &[PlanId],
    arena: &mut PlanArena,
) -> PlanId {
    let child_accessing = remove_from_accessing(accessing, node);
    let new_input = unnest(input, state, &child_accessing, arena);

    arena.alloc(Plan::Traverse {
        input: new_input,
        paths,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Remove a node from the accessing set.
fn remove_from_accessing(accessing: &[PlanId], node: PlanId) -> Vec<PlanId> {
    accessing.iter().copied().filter(|&id| id != node).collect()
}

/// Filter accessing operators to those in a subtree.
fn filter_accessing_to_subtree(
    root: PlanId,
    accessing: &[PlanId],
    arena: &PlanArena,
) -> Vec<PlanId> {
    // Collect all node IDs in the subtree.
    let mut subtree_ids = FxHashSet::default();
    collect_subtree_ids(root, arena, &mut subtree_ids);

    accessing
        .iter()
        .copied()
        .filter(|id| subtree_ids.contains(id))
        .collect()
}

/// Collect all node IDs in a subtree.
fn collect_subtree_ids(node: PlanId, arena: &PlanArena, out: &mut FxHashSet<PlanId>) {
    if !out.insert(node) {
        return; // already visited
    }
    for child in crate::tree::children(arena.plan(node)) {
        collect_subtree_ids(child, arena, out);
    }
}