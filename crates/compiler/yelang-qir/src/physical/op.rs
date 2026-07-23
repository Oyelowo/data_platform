//! Physical operator definitions and arena.

use std::sync::Arc;

use yelang_arena::{Id, IndexVec};
use yelang_interner::Symbol;

use crate::plan::{AggCall, ExprRef, GroupKey, JoinKind, PlanRange, SortSpec, SourceRef, TraversePath};

use super::algorithm::{
    AggAlgorithm, ExchangeKind, JoinAlgorithm, ScanStrategy, SortAlgorithm, TraverseStrategy,
};

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
        keys: Vec<(Symbol, GroupKey)>,
        aggs: Vec<AggCall>,
        into: Symbol,
        algorithm: AggAlgorithm,
    },

    // ── Sort ───────────────────────────────────────────────────────────
    Sort {
        input: PhysId,
        specs: Vec<SortSpec>,
        algorithm: SortAlgorithm,
    },

    /// Fused Sort + Limit (DuckDB-style TopN).
    ///
    /// Uses a min/max heap to efficiently return the top-N rows
    /// without sorting the entire input. The physical planner
    /// detects `Sort → Limit` patterns and fuses them into this.
    TopN {
        input: PhysId,
        specs: Vec<SortSpec>,
        skip: Option<ExprRef>,
        fetch: ExprRef,
    },

    // ── Window ─────────────────────────────────────────────────────────
    Window {
        input: PhysId,
        funcs: Vec<crate::plan::WindowFunc>,
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
