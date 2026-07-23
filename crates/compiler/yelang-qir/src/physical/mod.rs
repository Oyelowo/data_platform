//! Physical plan — the execution-level operator tree.
//!
//! The physical plan is produced by the physical planner from the optimized
//! logical plan. It differs from the logical plan in three ways:
//!
//! 1. **Concrete algorithms**: each operator has a chosen implementation
//!    (hash join vs merge join, sequential scan vs index scan, …).
//! 2. **Exchange nodes**: distribution boundaries are explicit. In-memory
//!    execution removes them; distributed execution materializes them as
//!    real network shuffles.
//! 3. **Parallelism hints**: operators are annotated with parallelism
//!    and pipelining information.
//!
//! The physical plan is the **contract** between the compiler and the
//! storage/execution backends. Each backend (in-memory, single-node,
//! distributed) implements the [`Executor`] trait to interpret it.

pub mod planner;

use std::sync::Arc;

use yelang_arena::{Id, IndexVec};
use yelang_interner::Symbol;

use crate::plan::{AggCall, ExprRef, JoinKind, OrderSpec, PlanRange, SourceRef, TraversePath};

// ---------------------------------------------------------------------------
// PhysId
// ---------------------------------------------------------------------------

/// Tag type for [`PhysId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagPhys;

/// Dense key into [`PhysArena::nodes`].
pub type PhysId = Id<TagPhys>;

// ---------------------------------------------------------------------------
// PhysArena
// ---------------------------------------------------------------------------

/// Arena-allocated physical plan tree.
#[derive(Debug, Clone)]
pub struct PhysArena {
    pub nodes: IndexVec<PhysId, PhysOp>,
}

impl PhysArena {
    pub fn new() -> Self {
        Self {
            nodes: IndexVec::new(),
        }
    }

    pub fn alloc(&mut self, op: PhysOp) -> PhysId {
        self.nodes.push(op)
    }

    pub fn get(&self, id: PhysId) -> Option<&PhysOp> {
        self.nodes.get(id)
    }

    pub fn op(&self, id: PhysId) -> &PhysOp {
        &self.nodes[id]
    }

    pub fn iter(&self) -> impl Iterator<Item = (PhysId, &PhysOp)> {
        self.nodes.iter_enumerated()
    }
}

impl Default for PhysArena {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PhysOp
// ---------------------------------------------------------------------------

/// A physical operator in the execution plan.
#[derive(Debug, Clone)]
pub enum PhysOp {
    // ── Scans ──────────────────────────────────────────────────────────
    /// Read from a storage source with a concrete scan strategy.
    Scan {
        source: SourceRef,
        strategy: ScanStrategy,
        /// Pushed-down filter (evaluated inside the storage engine).
        filter: Option<ExprRef>,
        /// Pushed-down projection (read only these columns).
        projection: Option<Vec<Symbol>>,
        range: Option<PlanRange>,
    },

    // ── Transforms ─────────────────────────────────────────────────────
    Filter {
        input: PhysId,
        pred: ExprRef,
    },

    Project {
        input: PhysId,
        exprs: Vec<(Symbol, ExprRef)>,
    },

    Map {
        input: PhysId,
        func: ExprRef,
        flatten_depth: usize,
    },

    // ── Joins (with chosen algorithm) ──────────────────────────────────
    Join {
        left: PhysId,
        right: PhysId,
        kind: JoinKind,
        algorithm: JoinAlgorithm,
        on: Vec<(ExprRef, ExprRef)>,
        filter: Option<ExprRef>,
    },

    // ── Aggregation (with chosen algorithm) ────────────────────────────
    Aggregate {
        input: PhysId,
        keys: Vec<(Symbol, ExprRef)>,
        aggs: Vec<AggCall>,
        into: Symbol,
        algorithm: AggAlgorithm,
    },

    // ── Sort ───────────────────────────────────────────────────────────
    Sort {
        input: PhysId,
        specs: Vec<OrderSpec>,
        algorithm: SortAlgorithm,
    },

    // ── Limit / Distinct ───────────────────────────────────────────────
    Limit {
        input: PhysId,
        skip: Option<ExprRef>,
        fetch: Option<ExprRef>,
    },

    Distinct {
        input: PhysId,
        on: Option<Vec<ExprRef>>,
    },

    // ── Set operations ─────────────────────────────────────────────────
    Union {
        inputs: Vec<PhysId>,
    },

    // ── Graph traversal ────────────────────────────────────────────────
    Traverse {
        input: PhysId,
        paths: Vec<TraversePath>,
        strategy: TraverseStrategy,
    },

    // ── Distribution boundary ──────────────────────────────────────────
    /// Exchange is the distribution seam. In-memory: removed entirely.
    /// Single-node: planning boundary only. Distributed: real network
    /// shuffle coordinated by the consensus layer.
    Exchange {
        input: PhysId,
        kind: ExchangeKind,
    },

    // ── Iteration ──────────────────────────────────────────────────────
    Repeat {
        input: PhysId,
        func: ExprRef,
        max_iters: Option<usize>,
    },

    // ── User-defined / opaque ──────────────────────────────────────────
    Extension {
        node: Arc<dyn crate::plan::UserDefinedPlanNode>,
    },

    // ── Leaves ─────────────────────────────────────────────────────────
    Constant {
        value: ExprRef,
    },

    Empty {
        produce_one_row: bool,
    },
}

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
    Merge(Vec<OrderSpec>),
    /// Range-partition by these keys.
    RangeBy(Vec<Symbol>),
}

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
