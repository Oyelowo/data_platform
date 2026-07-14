/*! Search graph for cycle detection and caching.
 *
 * The search graph tracks in-progress goals (to detect cycles) and
 * completed goals (for caching). It is the key to handling coinductive
 * cycles correctly.
 */

use yelang_util::FxHashMap;

use crate::response::{CanonicalGoal, CanonicalResponse};

/// The search graph tracks in-progress and completed goals.
#[derive(Debug, Default)]
pub struct SearchGraph<'tcx> {
    /// Stack of currently evaluating goals (for cycle detection).
    stack: Vec<StackEntry<'tcx>>,
    /// Cache of completed goals.
    cache: FxHashMap<CanonicalGoal<'tcx>, CacheEntry<'tcx>>,
}

/// An entry on the evaluation stack.
#[derive(Clone, Debug)]
pub struct StackEntry<'tcx> {
    pub goal: CanonicalGoal<'tcx>,
    /// Whether this entry has been used to prove itself (coinductive cycle).
    pub coinductive: bool,
    /// The provisional result (used during cycle resolution).
    pub provisional: Option<CanonicalResponse<'tcx>>,
}

/// A cached completed goal.
#[derive(Clone, Debug)]
pub struct CacheEntry<'tcx> {
    pub result: CanonicalResponse<'tcx>,
    /// The depth at which this was proven.
    pub depth: usize,
}

impl<'tcx> SearchGraph<'tcx> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a goal is currently being evaluated (cycle detection).
    pub fn is_in_stack(&self, goal: &CanonicalGoal<'tcx>) -> Option<usize> {
        self.stack.iter().position(|e| e.goal == *goal)
    }

    /// Check the cache for a completed goal.
    pub fn lookup_cache(&self, goal: &CanonicalGoal<'tcx>) -> Option<&CacheEntry<'tcx>> {
        self.cache.get(goal)
    }

    /// Push a goal onto the evaluation stack.
    pub fn push(&mut self, goal: CanonicalGoal<'tcx>) {
        self.stack.push(StackEntry {
            goal,
            coinductive: false,
            provisional: None,
        });
    }

    /// Pop a goal from the evaluation stack.
    pub fn pop(&mut self) -> Option<StackEntry<'tcx>> {
        self.stack.pop()
    }

    /// Mark a stack entry as coinductive (used in its own proof).
    pub fn mark_coinductive(&mut self, stack_index: usize) {
        if let Some(entry) = self.stack.get_mut(stack_index) {
            entry.coinductive = true;
        }
    }

    /// Set a provisional result for a stack entry.
    pub fn set_provisional(&mut self, stack_index: usize, response: CanonicalResponse<'tcx>) {
        if let Some(entry) = self.stack.get_mut(stack_index) {
            entry.provisional = Some(response);
        }
    }

    /// Insert a completed goal into the cache.
    pub fn insert_cache(&mut self, goal: CanonicalGoal<'tcx>, result: CanonicalResponse<'tcx>) {
        self.cache.insert(
            goal,
            CacheEntry {
                result,
                depth: self.stack.len(),
            },
        );
    }

    /// Current evaluation depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
}
