/*! Search graph for cycle detection and caching.
 *
 * The search graph tracks in-progress goals (to detect cycles) and
 * completed goals (for caching). It is the key to handling coinductive
 * cycles correctly.
 *
 * See:
 * - <https://rustc-dev-guide.rust-lang.org/solve/caching.html>
 * - <https://rustc-dev-guide.rust-lang.org/solve/coinduction.html>
 * - <https://rust-lang.github.io/chalk/book/recursive/search_graph.html>
 */

use yelang_arena::FxHashMap;

use crate::response::{CanonicalGoal, CanonicalResponse};

/// The search graph tracks in-progress and completed goals.
#[derive(Debug, Default)]
pub struct SearchGraph {
    /// Stack of currently evaluating goals (for cycle detection).
    stack: Vec<StackEntry>,
    /// Cache of completed goals.
    cache: FxHashMap<CanonicalGoal, CacheEntry>,
}

/// An entry on the evaluation stack.
#[derive(Clone, Debug)]
pub struct StackEntry {
    pub goal: CanonicalGoal,
    /// The remaining depth budget when this goal was pushed.
    pub available_depth: usize,
    /// Whether this entry has been used to prove itself (coinductive cycle).
    pub coinductive: bool,
    /// Whether this entry is part of any cycle.
    pub has_cycle: bool,
    /// The provisional result (used during cycle resolution).
    pub provisional: Option<CanonicalResponse>,
}

/// A cached completed goal.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub result: CanonicalResponse,
    /// The remaining depth budget the solver had when proving this result.
    /// The cache entry is only reusable when the caller has at least this
    /// much budget left.
    pub available_depth: usize,
}

impl SearchGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a goal is currently being evaluated (cycle detection).
    pub fn is_in_stack(&self, goal: &CanonicalGoal) -> Option<usize> {
        self.stack.iter().position(|e| e.goal == *goal)
    }

    /// Check the cache for a completed goal that is reusable with the given
    /// remaining depth budget.
    pub fn lookup_cache(
        &self,
        goal: &CanonicalGoal,
        available_depth: usize,
    ) -> Option<&CacheEntry> {
        let entry = self.cache.get(goal)?;
        if entry.available_depth <= available_depth {
            Some(entry)
        } else {
            None
        }
    }

    /// Push a goal onto the evaluation stack.
    pub fn push(&mut self, goal: CanonicalGoal, available_depth: usize) {
        self.stack.push(StackEntry {
            goal,
            available_depth,
            coinductive: false,
            has_cycle: false,
            provisional: None,
        });
    }

    /// Pop a goal from the evaluation stack.
    pub fn pop(&mut self) -> Option<StackEntry> {
        self.stack.pop()
    }

    /// Mark a stack entry and everything above it as participating in a
    /// coinductive cycle.
    pub fn mark_coinductive(&mut self, stack_index: usize) {
        for entry in self.stack.iter_mut().skip(stack_index) {
            entry.coinductive = true;
            entry.has_cycle = true;
        }
    }

    /// Mark a stack entry and everything above it as participating in a cycle.
    pub fn mark_cycle(&mut self, stack_index: usize) {
        for entry in self.stack.iter_mut().skip(stack_index) {
            entry.has_cycle = true;
        }
    }

    /// Set a provisional result for a stack entry.
    pub fn set_provisional(&mut self, stack_index: usize, response: CanonicalResponse) {
        if let Some(entry) = self.stack.get_mut(stack_index) {
            entry.provisional = Some(response);
        }
    }

    /// Insert a completed goal into the cache.
    pub fn insert_cache(
        &mut self,
        goal: CanonicalGoal,
        result: CanonicalResponse,
        available_depth: usize,
    ) {
        self.cache.insert(
            goal,
            CacheEntry {
                result,
                available_depth,
            },
        );
    }

    /// Current evaluation depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Access the stack entry at the given index.
    pub fn stack_entry(&self, index: usize) -> Option<&StackEntry> {
        self.stack.get(index)
    }

    /// Mutable access to the stack entry at the given index.
    pub fn stack_entry_mut(&mut self, index: usize) -> Option<&mut StackEntry> {
        self.stack.get_mut(index)
    }
}
