//! Logical plan optimizer.
//!
//! The optimizer is an ordered list of rewrite rules applied to a fixpoint.
//! Each rule declares whether it traverses top-down or bottom-up. The loop
//! runs until no rule changes the plan.
//!
//! Decorrelation is special: it is a stateful top-down pass that runs
//! **once** before the fixpoint loop (see the BTW 2025 algorithm).

use crate::logical::plan::{PlanArena, PlanId};
use crate::tree::{transform_bottom_up, transform_top_down, Transformed};

// ---------------------------------------------------------------------------
// OptRule trait
// ---------------------------------------------------------------------------

/// Traversal direction for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOrder {
    /// Visit parents before children.
    TopDown,
    /// Visit children before parents.
    BottomUp,
}

/// A single optimizer rewrite rule.
pub trait OptRule {
    /// Human-readable name for logging / EXPLAIN.
    fn name(&self) -> &str;

    /// Traversal direction.
    fn apply_order(&self) -> ApplyOrder;

    /// Attempt to rewrite the node at `id`.
    fn rewrite(&self, id: PlanId, arena: &mut PlanArena) -> Transformed;
}

// ---------------------------------------------------------------------------
// Optimizer
// ---------------------------------------------------------------------------

/// The logical plan optimizer: an ordered rule list + fixpoint loop.
pub struct Optimizer {
    rules: Vec<Box<dyn OptRule>>,
    max_passes: usize,
}

impl Optimizer {
    /// Create an optimizer with the default rule set.
    pub fn new() -> Self {
        Self {
            rules: default_rules(),
            max_passes: 16,
        }
    }

    /// Create an optimizer with a custom rule set.
    pub fn with_rules(rules: Vec<Box<dyn OptRule>>, max_passes: usize) -> Self {
        Self { rules, max_passes }
    }

    /// Run the optimizer on a plan tree. Returns the new root.
    ///
    /// Decorrelation runs **once** before the fixpoint loop (it is
    /// stateful and top-down, not a fixpoint rule).
    pub fn optimize(&self, root: PlanId, arena: &mut PlanArena, interner: &yelang_interner::Interner) -> PlanId {
        // Phase 0: Decorrelation (one-shot, top-down).
        let mut current = crate::logical::optimize::decorrelate::decorrelate(root, arena, interner);

        // Invariant: decorrelation must eliminate all correlated nodes.
        debug_assert!(
            !arena.has_correlated_nodes(),
            "decorrelation left DependentJoin/ScalarSubquery/Exists nodes in the plan"
        );

        // Phase 1+: Fixpoint loop with rewrite rules.
        for _pass in 0..self.max_passes {
            let mut any_changed = false;

            for rule in &self.rules {
                let result = match rule.apply_order() {
                    ApplyOrder::TopDown => transform_top_down(current, arena, &mut |id, arena| {
                        rule.rewrite(id, arena)
                    }),
                    ApplyOrder::BottomUp => {
                        transform_bottom_up(current, arena, &mut |id, arena| {
                            rule.rewrite(id, arena)
                        })
                    }
                };
                current = result.id;
                any_changed |= result.changed;
            }

            if !any_changed {
                break;
            }
        }

        current
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Default rule set
// ---------------------------------------------------------------------------

/// The default optimizer rules, in application order.
///
/// Ordering constraints:
/// - Simplification first (remove trivial nodes before other rules see them)
/// - PushDownLimit before PushDownFilter (filters can't push past limits)
/// - Projection pruning last (other rules may add/remove expressions)
pub fn default_rules() -> Vec<Box<dyn OptRule>> {
    vec![
        // Phase 1: Simplification
        Box::new(crate::logical::optimize::simplify::EliminateTrivialFilter),
        Box::new(crate::logical::optimize::simplify::EliminateTrivialLimit),
        Box::new(crate::logical::optimize::simplify::MergeAdjacentFilters),
        // Phase 2: Pushdown
        Box::new(crate::logical::optimize::pushdown::PushDownFilter),
        // Phase 3: Join reordering (cost-based, greedy)
        Box::new(crate::logical::optimize::join_reorder::JoinReorder),
        // Phase 4: Projection pruning
        Box::new(crate::logical::optimize::prune::PruneUnusedFields),
    ]
}
