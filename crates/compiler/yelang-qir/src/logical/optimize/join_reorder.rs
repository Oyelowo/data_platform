//! Cost-based join reordering — greedy algorithm.
//!
//! Reorders join chains to minimize intermediate result sizes using
//! cardinality estimates from [`PlanMeta::est_cardinality`](crate::logical::plan::PlanMeta::est_cardinality).
//!
//! # Algorithm
//!
//! 1. **Flatten** a chain of inner/cross joins into a list of leaf
//!    relations and their join predicates.
//! 2. **Sort** relations by estimated cardinality (smallest first).
//! 3. **Greedy build**: start with the smallest relation, then at each
//!    step join with the smallest *compatible* relation — one that
//!    shares a join predicate with the accumulated result. If no
//!    compatible relation exists, fall back to the smallest remaining
//!    (cross join).
//! 4. **Rebuild** the join tree from the greedy ordering.
//!
//! Only inner and cross joins are reordered; outer/semi/anti joins
//! are treated as opaque barriers because reordering them would
//! change semantics.

use yelang_arena::FxHashSet;
use yelang_interner::Symbol;

use crate::analysis::{plan_output_fields, referenced_fields};
use crate::optimize::{ApplyOrder, OptRule};
use crate::logical::plan::{ExprRef, JoinKind, Plan, PlanArena, PlanId, PlanMeta};
use crate::tree::Transformed;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// A leaf relation extracted from a join chain.
struct Relation {
    /// The plan node id.
    id: PlanId,
    /// Estimated row count (unknown → `usize::MAX` so it sorts last).
    cardinality: usize,
    /// Output field names (for predicate compatibility checks).
    output_fields: FxHashSet<Symbol>,
}

/// A join predicate collected during flattening.
struct JoinPredicate {
    kind: JoinKind,
    on: Vec<(ExprRef, ExprRef)>,
    filter: Option<ExprRef>,
    /// All field names referenced by `on` and `filter`.
    referenced: FxHashSet<Symbol>,
}

// ---------------------------------------------------------------------------
// Rule
// ---------------------------------------------------------------------------

/// Greedy cost-based join reordering.
///
/// Flattens inner/cross join chains and rebuilds them in cardinality
/// order, starting with the smallest relation and greedily joining
/// with the smallest compatible relation at each step.
pub struct JoinReorder;

impl OptRule for JoinReorder {
    fn name(&self) -> &str {
        "join_reorder"
    }

    fn apply_order(&self) -> ApplyOrder {
        ApplyOrder::BottomUp
    }

    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed {
        // Only process Join nodes.
        if !matches!(arena.plan(id), Plan::Join { .. }) {
            return Transformed::no(id);
        }

        // Step 1: Flatten the join chain.
        let mut relations: Vec<Relation> = Vec::new();
        let mut predicates: Vec<JoinPredicate> = Vec::new();
        flatten_joins(id, arena, &mut relations, &mut predicates);

        // Need at least 3 relations for reordering to be worthwhile.
        if relations.len() < 3 {
            return Transformed::no(id);
        }

        // Step 2: Sort by cardinality (smallest first).
        relations.sort_by_key(|r| r.cardinality);

        // Step 3+4: Greedy reorder and rebuild.
        match greedy_reorder(relations, &predicates, arena) {
            Some(new_id) if new_id != id => Transformed::yes(new_id),
            _ => Transformed::no(id),
        }
    }
}

// ---------------------------------------------------------------------------
// Flattening
// ---------------------------------------------------------------------------

/// Recursively flatten a join chain into leaf relations and predicates.
///
/// Only inner/cross joins are flattened; other join kinds and non-join
/// nodes are treated as opaque leaf relations.
fn flatten_joins(
    id: PlanId,
    arena: &PlanArena,
    relations: &mut Vec<Relation>,
    predicates: &mut Vec<JoinPredicate>,
) {
    let plan = arena.plan(id).clone();

    match &plan {
        Plan::Join {
            left,
            right,
            kind,
            on,
            filter,
        } if matches!(kind, JoinKind::Inner | JoinKind::Cross) => {
            // Recurse into both sides.
            flatten_joins(*left, arena, relations, predicates);
            flatten_joins(*right, arena, relations, predicates);

            // Collect predicate field references.
            let mut referenced = FxHashSet::new();
            for &(l, r) in on {
                for f in referenced_fields(l, arena).iter() {
                    referenced.insert(*f);
                }
                for f in referenced_fields(r, arena).iter() {
                    referenced.insert(*f);
                }
            }
            if let Some(f) = filter {
                for sym in referenced_fields(*f, arena).iter() {
                    referenced.insert(*sym);
                }
            }

            predicates.push(JoinPredicate {
                kind: *kind,
                on: on.clone(),
                filter: *filter,
                referenced,
            });
        }
        _ => {
            // Leaf or non-reorderable node.
            push_relation(id, arena, relations);
        }
    }
}

/// Add a plan node as a leaf relation.
fn push_relation(id: PlanId, arena: &PlanArena, relations: &mut Vec<Relation>) {
    let cardinality = arena
        .meta(id)
        .and_then(|m| m.est_cardinality)
        .unwrap_or(usize::MAX);
    let output_fields = plan_output_fields(arena.plan(id), arena);
    relations.push(Relation {
        id,
        cardinality,
        output_fields,
    });
}

// ---------------------------------------------------------------------------
// Greedy reordering
// ---------------------------------------------------------------------------

/// Build a join tree using the greedy algorithm.
///
/// Start with the smallest relation, then repeatedly join with the
/// smallest compatible remaining relation.
fn greedy_reorder(
    mut relations: Vec<Relation>,
    predicates: &[JoinPredicate],
    arena: &mut PlanArena,
) -> Option<PlanId> {
    if relations.is_empty() {
        return None;
    }

    // Start with the smallest relation.
    let first = relations.remove(0);
    let mut current_id = first.id;
    let mut current_card = first.cardinality;
    let mut current_fields = first.output_fields;

    while !relations.is_empty() {
        // Find the smallest compatible relation.
        let best_idx = find_best_compatible(&relations, &current_fields, predicates);
        let next = relations.remove(best_idx);

        // Find the connecting predicate (if any).
        let (kind, on, filter) =
            find_connecting_predicate(&current_fields, &next.output_fields, predicates);

        // Allocate the join node.
        let join_id = arena.alloc(Plan::Join {
            left: current_id,
            right: next.id,
            kind,
            on,
            filter,
        });

        // Estimate output cardinality and attach metadata.
        let est_card = estimate_join_cardinality(current_card, next.cardinality);
        let mut meta = PlanMeta::default();
        meta.est_cardinality = Some(est_card);
        arena.set_meta(join_id, meta);

        // Accumulate fields.
        for f in next.output_fields.iter() {
            current_fields.insert(*f);
        }
        current_id = join_id;
        current_card = est_card;
    }

    Some(current_id)
}

/// Find the index of the smallest relation compatible with the
/// accumulated result.
///
/// Relations are pre-sorted by cardinality, so the first compatible
/// one is the cheapest. Falls back to index 0 (cross join) when no
/// predicate-based compatibility exists.
fn find_best_compatible(
    relations: &[Relation],
    current_fields: &FxHashSet<Symbol>,
    predicates: &[JoinPredicate],
) -> usize {
    for (i, rel) in relations.iter().enumerate() {
        if is_compatible(current_fields, &rel.output_fields, predicates) {
            return i;
        }
    }
    0
}

/// Two sides are compatible if a predicate references fields from
/// both, or if there are no predicates at all (cross join).
fn is_compatible(
    current_fields: &FxHashSet<Symbol>,
    rel_fields: &FxHashSet<Symbol>,
    predicates: &[JoinPredicate],
) -> bool {
    if predicates.is_empty() {
        return true;
    }
    predicates.iter().any(|pred| {
        let touches_current = pred.referenced.iter().any(|f| current_fields.contains(f));
        let touches_rel = pred.referenced.iter().any(|f| rel_fields.contains(f));
        touches_current && touches_rel
    })
}

/// Find a predicate that connects the accumulated fields with the
/// next relation's fields. Returns a cross join if none is found.
fn find_connecting_predicate(
    current_fields: &FxHashSet<Symbol>,
    rel_fields: &FxHashSet<Symbol>,
    predicates: &[JoinPredicate],
) -> (JoinKind, Vec<(ExprRef, ExprRef)>, Option<ExprRef>) {
    for pred in predicates {
        let touches_current = pred.referenced.iter().any(|f| current_fields.contains(f));
        let touches_rel = pred.referenced.iter().any(|f| rel_fields.contains(f));
        if touches_current && touches_rel {
            return (pred.kind, pred.on.clone(), pred.filter);
        }
    }
    (JoinKind::Cross, vec![], None)
}

/// Estimate join output cardinality.
///
/// Heuristic: the larger of the two inputs (conservative for
/// equi-joins; favours joining small relations first).
fn estimate_join_cardinality(left_card: usize, right_card: usize) -> usize {
    left_card.max(right_card)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal arena with three scan leaves of known
    /// cardinality joined in a chain, then verify the reorder rule
    /// produces a valid plan.
    #[test]
    fn reorder_three_relations() {
        use crate::logical::plan::SourceRef;

        let mut arena = PlanArena::new();
        let interner = yelang_interner::Interner::new();

        // Create three scans with different cardinalities.
        let name_a = interner.intern("a");
        let name_b = interner.intern("b");
        let name_c = interner.intern("c");

        let scan_a = arena.alloc(Plan::Scan {
            source: SourceRef::Local { name: name_a },
            filter: None,
            projection: None,
            range: None,
        });
        let scan_b = arena.alloc(Plan::Scan {
            source: SourceRef::Local { name: name_b },
            filter: None,
            projection: None,
            range: None,
        });
        let scan_c = arena.alloc(Plan::Scan {
            source: SourceRef::Local { name: name_c },
            filter: None,
            projection: None,
            range: None,
        });

        // Attach cardinality metadata: C(10) < A(100) < B(1000).
        let mut meta_a = PlanMeta::default();
        meta_a.est_cardinality = Some(100);
        arena.set_meta(scan_a, meta_a);

        let mut meta_b = PlanMeta::default();
        meta_b.est_cardinality = Some(1000);
        arena.set_meta(scan_b, meta_b);

        let mut meta_c = PlanMeta::default();
        meta_c.est_cardinality = Some(10);
        arena.set_meta(scan_c, meta_c);

        // Build join chain: (A ⋈ B) ⋈ C  (deliberately worst order).
        let join_ab = arena.alloc(Plan::Join {
            left: scan_a,
            right: scan_b,
            kind: JoinKind::Cross,
            on: vec![],
            filter: None,
        });
        let join_abc = arena.alloc(Plan::Join {
            left: join_ab,
            right: scan_c,
            kind: JoinKind::Cross,
            on: vec![],
            filter: None,
        });

        // Apply the rule.
        let rule = JoinReorder;
        let result = rule.rewrite(join_abc, &mut arena);

        assert!(result.changed, "expected the join tree to be reordered");

        // The new root should be a Join.
        assert!(matches!(arena.plan(result.id), Plan::Join { .. }));

        // The leftmost leaf should be the smallest relation (C, card 10).
        let leftmost = leftmost_leaf(result.id, &arena);
        let leftmost_card = arena
            .meta(leftmost)
            .and_then(|m| m.est_cardinality)
            .unwrap_or(usize::MAX);
        assert_eq!(leftmost_card, 10, "smallest relation should be leftmost");
    }

    /// Walk down the left spine to find the leftmost leaf.
    fn leftmost_leaf(id: PlanId, arena: &PlanArena) -> PlanId {
        let mut current = id;
        loop {
            match arena.plan(current) {
                Plan::Join { left, .. } => current = *left,
                _ => return current,
            }
        }
    }

    #[test]
    fn two_relations_not_reordered() {
        use crate::logical::plan::SourceRef;

        let mut arena = PlanArena::new();
        let interner = yelang_interner::Interner::new();

        let name_a = interner.intern("a");
        let name_b = interner.intern("b");

        let scan_a = arena.alloc(Plan::Scan {
            source: SourceRef::Local { name: name_a },
            filter: None,
            projection: None,
            range: None,
        });
        let scan_b = arena.alloc(Plan::Scan {
            source: SourceRef::Local { name: name_b },
            filter: None,
            projection: None,
            range: None,
        });

        let join = arena.alloc(Plan::Join {
            left: scan_a,
            right: scan_b,
            kind: JoinKind::Cross,
            on: vec![],
            filter: None,
        });

        let rule = JoinReorder;
        let result = rule.rewrite(join, &mut arena);
        assert!(!result.changed, "two relations should not trigger reorder");
    }
}
