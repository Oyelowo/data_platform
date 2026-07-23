//! Executor trait and in-memory implementation.

use crate::plan::{ExprRef, JoinKind, SourceRef};

use super::algorithm::{AggAlgorithm, JoinAlgorithm, ScanStrategy, SortAlgorithm};

// ---------------------------------------------------------------------------
// Executor trait
// ---------------------------------------------------------------------------

/// The interface between the physical plan and a storage/execution backend.
///
/// Each backend (in-memory, single-node, distributed) implements this
/// trait. The physical planner uses it to make algorithm choices; the
/// execution engine uses it to actually run the plan.
pub trait Executor {
    /// Can this backend push a filter into the scan?
    fn supports_filter_pushdown(&self, source: &SourceRef, pred: &ExprRef) -> bool;

    /// Can this backend push a projection into the scan?
    fn supports_projection_pushdown(&self, source: &SourceRef) -> bool;

    /// Choose a join algorithm given the input properties.
    fn choose_join(
        &self,
        left_card: Option<usize>,
        right_card: Option<usize>,
        kind: JoinKind,
        is_equi: bool,
    ) -> JoinAlgorithm;

    /// Choose a sort algorithm.
    fn choose_sort(&self, est_card: Option<usize>, has_limit: bool) -> SortAlgorithm;

    /// Choose an aggregation algorithm.
    fn choose_aggregate(&self, est_card: Option<usize>, num_groups: Option<usize>) -> AggAlgorithm;

    /// Choose a scan strategy.
    fn choose_scan(&self, source: &SourceRef, has_filter: bool) -> ScanStrategy;

    /// Whether this backend needs Exchange nodes for distribution.
    fn is_distributed(&self) -> bool;

    /// Number of execution nodes (1 for single-node, N for distributed).
    fn num_nodes(&self) -> usize;
}

/// A simple in-memory executor for testing and local execution.
#[derive(Debug, Clone, Copy)]
pub struct InMemoryExecutor;

impl Executor for InMemoryExecutor {
    fn supports_filter_pushdown(&self, _source: &SourceRef, _pred: &ExprRef) -> bool {
        false // In-memory: evaluate filters in the execution loop
    }

    fn supports_projection_pushdown(&self, _source: &SourceRef) -> bool {
        false
    }

    fn choose_join(
        &self,
        _left_card: Option<usize>,
        _right_card: Option<usize>,
        _kind: JoinKind,
        is_equi: bool,
    ) -> JoinAlgorithm {
        if is_equi {
            JoinAlgorithm::HashBuildProbe
        } else {
            JoinAlgorithm::NestedLoop
        }
    }

    fn choose_sort(&self, _est_card: Option<usize>, _has_limit: bool) -> SortAlgorithm {
        SortAlgorithm::InMemory
    }

    fn choose_aggregate(
        &self,
        _est_card: Option<usize>,
        _num_groups: Option<usize>,
    ) -> AggAlgorithm {
        AggAlgorithm::HashTable
    }

    fn choose_scan(&self, _source: &SourceRef, _has_filter: bool) -> ScanStrategy {
        ScanStrategy::Sequential
    }

    fn is_distributed(&self) -> bool {
        false
    }

    fn num_nodes(&self) -> usize {
        1
    }
}
