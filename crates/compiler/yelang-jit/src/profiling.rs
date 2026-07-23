//! Profiling counters for the "flying start" JIT model.
//!
//! Following the Umbra approach, code starts life being interpreted by the
//! [`yelang_vm::Vm`]. Every time a function runs we bump its execution
//! counter; once a function becomes *hot* (its counter crosses a
//! configurable threshold) it is queued for native compilation. The
//! interpreter keeps running the cold paths while the JIT warms up the hot
//! ones, so there is never a pause-to-compile cliff.

use std::collections::{HashMap, HashSet, VecDeque};

/// Default number of interpreted executions before a function is considered
/// hot enough to JIT-compile.
pub const DEFAULT_JIT_THRESHOLD: u64 = 64;

/// Tracks per-function execution counts and decides when to JIT-compile.
///
/// The profiler is intentionally cheap: recording an execution is a single
/// hash-map increment, so it can sit in the interpreter's hot loop without
/// measurably slowing interpretation down.
#[derive(Debug, Clone)]
pub struct Profiler {
    /// Interpreted execution count per function id.
    counts: HashMap<u64, u64>,
    /// Functions that have already been compiled to native code.
    jitted: HashSet<u64>,
    /// Functions queued for compilation (hot but not yet compiled).
    queue: VecDeque<u64>,
    /// Execution count at/above which a function is considered hot.
    threshold: u64,
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Profiler {
    /// Create a profiler with the [`DEFAULT_JIT_THRESHOLD`].
    pub fn new() -> Self {
        Self::with_threshold(DEFAULT_JIT_THRESHOLD)
    }

    /// Create a profiler with a custom hotness threshold.
    ///
    /// A threshold of `0` means "JIT everything on first sight"; a very large
    /// threshold effectively disables JIT compilation.
    pub fn with_threshold(threshold: u64) -> Self {
        Self {
            counts: HashMap::new(),
            jitted: HashSet::new(),
            queue: VecDeque::new(),
            threshold,
        }
    }

    /// The configured hotness threshold.
    pub fn threshold(&self) -> u64 {
        self.threshold
    }

    /// Record one interpreted execution of `func_id`.
    ///
    /// Returns `true` if this execution pushed the function across the
    /// hotness threshold for the first time (i.e. it was just queued for
    /// compilation). Callers can use this as the trigger to kick off a
    /// background compile.
    pub fn record(&mut self, func_id: u64) -> bool {
        let count = self.counts.entry(func_id).or_insert(0);
        *count += 1;

        let just_went_hot = *count == self.threshold
            && !self.jitted.contains(&func_id)
            && !self.queue.contains(&func_id);
        if just_went_hot {
            self.queue.push_back(func_id);
        }
        just_went_hot
    }

    /// Whether `func_id` is hot enough to JIT-compile and has not been
    /// compiled yet.
    pub fn should_jit(&self, func_id: u64) -> bool {
        let count = self.counts.get(&func_id).copied().unwrap_or(0);
        count >= self.threshold && !self.jitted.contains(&func_id)
    }

    /// Mark `func_id` as successfully compiled to native code.
    ///
    /// Removes it from the pending queue (if present) so it is not compiled
    /// twice.
    pub fn mark_jitted(&mut self, func_id: u64) {
        self.jitted.insert(func_id);
        if let Some(pos) = self.queue.iter().position(|&id| id == func_id) {
            self.queue.remove(pos);
        }
    }

    /// Whether `func_id` has already been compiled to native code.
    pub fn is_jitted(&self, func_id: u64) -> bool {
        self.jitted.contains(&func_id)
    }

    /// Pop the next function id waiting to be compiled, if any.
    pub fn next_to_compile(&mut self) -> Option<u64> {
        self.queue.pop_front()
    }

    /// Number of functions currently queued for compilation.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The recorded execution count for `func_id` (0 if never seen).
    pub fn count(&self, func_id: u64) -> u64 {
        self.counts.get(&func_id).copied().unwrap_or(0)
    }

    /// Forget all profiling state (counts, queue, and compiled set).
    pub fn reset(&mut self) {
        self.counts.clear();
        self.jitted.clear();
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_function_is_not_jitted() {
        let mut p = Profiler::with_threshold(3);
        p.record(1);
        p.record(1);
        assert!(!p.should_jit(1));
        assert_eq!(p.count(1), 2);
    }

    #[test]
    fn hot_function_is_queued_exactly_once() {
        let mut p = Profiler::with_threshold(3);
        assert!(!p.record(7));
        assert!(!p.record(7));
        // Third execution crosses the threshold.
        assert!(p.record(7));
        assert!(p.should_jit(7));
        // Further executions do not re-queue.
        assert!(!p.record(7));
        assert_eq!(p.queued(), 1);
        assert_eq!(p.next_to_compile(), Some(7));
        assert_eq!(p.next_to_compile(), None);
    }

    #[test]
    fn mark_jitted_clears_should_jit() {
        let mut p = Profiler::with_threshold(1);
        p.record(42);
        assert!(p.should_jit(42));
        p.mark_jitted(42);
        assert!(!p.should_jit(42));
        assert!(p.is_jitted(42));
        // Recording again does not re-queue an already-compiled function.
        assert!(!p.record(42));
        assert_eq!(p.queued(), 0);
    }

    #[test]
    fn zero_threshold_jits_immediately() {
        let mut p = Profiler::with_threshold(0);
        // count starts at 0 >= 0, so it is hot before any execution.
        assert!(p.should_jit(5));
    }
}
