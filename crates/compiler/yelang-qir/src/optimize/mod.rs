//! Logical plan optimizer.
//!
//! The optimizer is an ordered list of rewrite rules applied to a fixpoint.
//! Each rule declares whether it traverses top-down or bottom-up. The loop
//! runs until no rule changes the plan.
//!
//! Decorrelation is special: it is a stateful top-down pass that runs
//! **once** before the fixpoint loop (see the BTW 2025 algorithm).

pub mod pushdown;
pub mod simplify;

use crate::plan::{PlanArena, PlanId};
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
    ///
    /// Return [`Transformed::no`] if the rule does not apply.
    /// Return [`Transformed::yes`] with a new [`PlanId`] if the node was
    /// replaced (the new node must be allocated in `arena`).
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
    /// Decorrelation should be run **before** this (it is stateful and
    /// not a fixpoint rule).
    pub fn optimize(&self, root: PlanId, arena: &mut PlanArena) -> PlanId {
        let mut current = root;

        for _pass in 0..self.max_passes {
            let mut any_changed = false;

            for rule in &self.rules {
                let result = match rule.apply_order() {
                    ApplyOrder::TopDown => {
                        transform_top_down(current, arena, &mut |id, arena| {
                            rule.rewrite(id, arena)
                        })
                    }
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
        Box::new(simplify::EliminateTrivialFilter),
        Box::new(simplify::EliminateTrivialLimit),
        Box::new(simplify::MergeAdjacentFilters),
        // Phase 2: Pushdown
        Box::new(pushdown::PushDownFilter),
        // Phase 3: Projection pruning (add later)
    ]
}
