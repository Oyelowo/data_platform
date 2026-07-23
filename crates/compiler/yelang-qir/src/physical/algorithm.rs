//! Algorithm choice enums for physical operators.

use yelang_interner::Symbol;

use crate::logical::plan::SortSpec;

// ---------------------------------------------------------------------------
// Algorithm choices
// ---------------------------------------------------------------------------

/// How to read from a storage source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStrategy {
    /// Full sequential scan.
    Sequential,
    /// Index scan on a specific key.
    Index { key_column: Symbol },
    /// Bitmap index scan (multiple predicates combined).
    Bitmap,
    /// Let the storage engine decide.
    Auto,
}

/// How to execute a join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinAlgorithm {
    /// Build hash table on one side, probe with the other.
    HashBuildProbe,
    /// Sort both sides by join key, merge.
    SortMerge,
    /// Nested loop (for small inputs or non-equi joins).
    NestedLoop,
    /// Co-located hash join (distributed: both sides already
    /// partitioned by the join key — no shuffle needed).
    CoLocatedHash,
    /// Shuffle both sides by join key, then hash join.
    ShuffleHash,
    /// Broadcast the smaller side to all nodes, hash join locally.
    BroadcastHash,
}

/// How to execute an aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggAlgorithm {
    /// In-memory hash table.
    HashTable,
    /// Sort by group keys, then aggregate sequentially.
    SortBased,
    /// Partial aggregation per shard, merge at coordinator.
    PartialMerge,
}

/// How to execute a sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortAlgorithm {
    /// In-memory sort.
    InMemory,
    /// External merge sort (spill to disk).
    ExternalMerge,
    /// Local sort per shard + merge at coordinator.
    LocalMerge,
    /// Top-N heap (for LIMIT after SORT).
    TopN { n: usize },
}

/// How to execute a graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraverseStrategy {
    /// Nested loop per parent element.
    NestedLoop,
    /// Decorrelated: hash join on _from/_to keys.
    HashJoin,
    /// Batch edge lookups per shard.
    BatchLookup,
}

/// Distribution boundary type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeKind {
    /// Gather all partitions to a single node.
    Gather,
    /// Replicate data to all nodes.
    Broadcast,
    /// Hash-partition by these keys.
    ShuffleBy(Vec<Symbol>),
    /// Merge pre-sorted partitions (preserves order).
    Merge(Vec<SortSpec>),
    /// Range-partition by these keys.
    RangeBy(Vec<Symbol>),
}
