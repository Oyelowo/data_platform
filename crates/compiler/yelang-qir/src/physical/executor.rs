//! Executor trait and backend implementations.
//!
//! Three executor backends:
//! - `InMemoryExecutor`: data in Vec<T>, no pushdown, no Exchange
//! - `SingleNodeExecutor`: storage engine with index scan, filter pushdown
//! - `DistributedExecutor`: sharded storage with Exchange = real shuffle

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

// ---------------------------------------------------------------------------
// InMemoryExecutor
// ---------------------------------------------------------------------------

/// In-memory executor: data lives in Vec<T>, no storage engine.
///
/// No filter/projection pushdown (filters evaluated in execution loop).
/// No Exchange nodes (single-threaded or rayon-style parallelism).
/// Hash join for equi-joins, nested loop for non-equi.
#[derive(Debug, Clone, Copy)]
pub struct InMemoryExecutor;

impl Executor for InMemoryExecutor {
    fn supports_filter_pushdown(&self, _source: &SourceRef, _pred: &ExprRef) -> bool {
        false
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

    fn choose_sort(&self, _est_card: Option<usize>, has_limit: bool) -> SortAlgorithm {
        if has_limit {
            SortAlgorithm::TopN { n: 0 } // n filled by planner
        } else {
            SortAlgorithm::InMemory
        }
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

// ---------------------------------------------------------------------------
// SingleNodeExecutor
// ---------------------------------------------------------------------------

/// Single-node executor with a storage engine (LSM-tree, B-tree, etc.).
///
/// Supports filter and projection pushdown into the storage engine.
/// Index scans when a suitable index exists.
/// External merge sort for large datasets (spill to disk).
#[derive(Debug, Clone)]
pub struct SingleNodeExecutor {
    /// Whether the storage engine supports index scans.
    pub has_indexes: bool,
    /// Estimated memory budget (bytes) for sort/aggregate buffers.
    pub memory_budget: usize,
}

impl SingleNodeExecutor {
    pub fn new(has_indexes: bool, memory_budget: usize) -> Self {
        Self {
            has_indexes,
            memory_budget,
        }
    }
}

impl Executor for SingleNodeExecutor {
    fn supports_filter_pushdown(&self, source: &SourceRef, _pred: &ExprRef) -> bool {
        // Can push filters into table scans.
        matches!(source, SourceRef::Table { .. })
    }

    fn supports_projection_pushdown(&self, source: &SourceRef) -> bool {
        matches!(source, SourceRef::Table { .. })
    }

    fn choose_join(
        &self,
        left_card: Option<usize>,
        right_card: Option<usize>,
        _kind: JoinKind,
        is_equi: bool,
    ) -> JoinAlgorithm {
        if !is_equi {
            return JoinAlgorithm::NestedLoop;
        }
        // Use cardinality estimates to choose between hash and merge join.
        match (left_card, right_card) {
            (Some(l), Some(r)) if l > 0 && r > 0 => {
                // If both sides fit in memory, use hash join.
                // Otherwise, use sort-merge (better for large datasets).
                let estimated_size = (l + r) * 64; // rough: 64 bytes per row
                if estimated_size < self.memory_budget {
                    JoinAlgorithm::HashBuildProbe
                } else {
                    JoinAlgorithm::SortMerge
                }
            }
            _ => JoinAlgorithm::HashBuildProbe,
        }
    }

    fn choose_sort(&self, est_card: Option<usize>, has_limit: bool) -> SortAlgorithm {
        if has_limit {
            return SortAlgorithm::TopN { n: 0 };
        }
        match est_card {
            Some(card) if card * 64 > self.memory_budget => SortAlgorithm::ExternalMerge,
            _ => SortAlgorithm::InMemory,
        }
    }

    fn choose_aggregate(
        &self,
        est_card: Option<usize>,
        num_groups: Option<usize>,
    ) -> AggAlgorithm {
        match (est_card, num_groups) {
            (Some(card), Some(groups)) if groups > 0 && card * 64 > self.memory_budget => {
                AggAlgorithm::SortBased
            }
            _ => AggAlgorithm::HashTable,
        }
    }

    fn choose_scan(&self, source: &SourceRef, has_filter: bool) -> ScanStrategy {
        if self.has_indexes && has_filter {
            ScanStrategy::Auto // Let the storage engine decide
        } else {
            ScanStrategy::Sequential
        }
    }

    fn is_distributed(&self) -> bool {
        false
    }

    fn num_nodes(&self) -> usize {
        1
    }
}

// ---------------------------------------------------------------------------
// DistributedExecutor
// ---------------------------------------------------------------------------

/// Distributed executor with sharded storage and Raft consensus.
///
/// Exchange nodes become real network shuffles.
/// Aggregation uses PartialMerge (shard-local + coordinator merge).
/// Joins use CoLocatedHash (if co-partitioned) or ShuffleHash/BroadcastHash.
#[derive(Debug, Clone)]
pub struct DistributedExecutor {
    /// Number of shard nodes.
    pub num_shards: usize,
    /// Whether data is co-partitioned by join keys.
    pub co_partitioned: bool,
}

impl DistributedExecutor {
    pub fn new(num_shards: usize, co_partitioned: bool) -> Self {
        Self {
            num_shards,
            co_partitioned,
        }
    }
}

impl Executor for DistributedExecutor {
    fn supports_filter_pushdown(&self, source: &SourceRef, _pred: &ExprRef) -> bool {
        matches!(source, SourceRef::Table { .. })
    }

    fn supports_projection_pushdown(&self, source: &SourceRef) -> bool {
        matches!(source, SourceRef::Table { .. })
    }

    fn choose_join(
        &self,
        left_card: Option<usize>,
        right_card: Option<usize>,
        _kind: JoinKind,
        is_equi: bool,
    ) -> JoinAlgorithm {
        if !is_equi {
            return JoinAlgorithm::NestedLoop;
        }
        if self.co_partitioned {
            // Data already co-partitioned by join key: no shuffle needed.
            return JoinAlgorithm::CoLocatedHash;
        }
        // If one side is small, broadcast it.
        match (left_card, right_card) {
            (Some(l), Some(r)) => {
                let broadcast_threshold = 10_000; // rows
                if l < broadcast_threshold || r < broadcast_threshold {
                    JoinAlgorithm::BroadcastHash
                } else {
                    JoinAlgorithm::ShuffleHash
                }
            }
            _ => JoinAlgorithm::ShuffleHash,
        }
    }

    fn choose_sort(&self, _est_card: Option<usize>, has_limit: bool) -> SortAlgorithm {
        if has_limit {
            SortAlgorithm::TopN { n: 0 }
        } else {
            // Distributed sort: local sort per shard + merge at coordinator.
            SortAlgorithm::LocalMerge
        }
    }

    fn choose_aggregate(
        &self,
        _est_card: Option<usize>,
        _num_groups: Option<usize>,
    ) -> AggAlgorithm {
        // Distributed aggregation: partial agg per shard + merge at coordinator.
        AggAlgorithm::PartialMerge
    }

    fn choose_scan(&self, _source: &SourceRef, _has_filter: bool) -> ScanStrategy {
        ScanStrategy::Auto
    }

    fn is_distributed(&self) -> bool {
        true
    }

    fn num_nodes(&self) -> usize {
        self.num_shards
    }
}
