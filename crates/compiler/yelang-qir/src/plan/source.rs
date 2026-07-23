//! [`SourceRef`], [`TraversePath`], [`TraverseSegment`], [`EdgeRef`], [`NodeRef`], [`Direction`].

use yelang_arena::DefId;
use yelang_interner::Symbol;

use super::keys::PlanRange;
use super::ExprRef;

/// Where a `Scan` reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// A `@table`-annotated struct — storage-backed.
    Table { def: DefId, name: Symbol },
    /// A local variable holding a collection value.
    Local { name: Symbol },
    /// A function/method call that returns a collection.
    Call { func: ExprRef },
}

/// A single traversal path in a `links` clause.
///
/// Multiple paths can share the same anchor. Each path is an arbitrary-length
/// chain of segments with potentially mixed directions.
#[derive(Debug, Clone)]
pub struct TraversePath {
    /// The collection label this path starts from (must be bound in `from`).
    pub anchor: Symbol,
    /// The chain of edge→node hops.
    pub segments: Vec<TraverseSegment>,
}

/// One hop in a traversal path: traverse an edge to reach a target node.
#[derive(Debug, Clone)]
pub struct TraverseSegment {
    /// The edge type to traverse.
    pub edge: EdgeRef,
    /// Direction of traversal.
    pub direction: Direction,
    /// The target node type reached via this edge.
    pub target: NodeRef,
    /// Optional predicate on the edge element.
    pub edge_pred: Option<ExprRef>,
    /// Optional predicate on the target node element.
    pub target_pred: Option<ExprRef>,
    /// For variable-length hops: `{1..3}`.
    pub hop_range: Option<PlanRange>,
}

/// Reference to an edge type in a traversal.
#[derive(Debug, Clone)]
pub struct EdgeRef {
    /// The `DefId` of the edge struct (must have `_from`/`_to` fields).
    pub def: DefId,
    /// The collection label (e.g. `writes`).
    pub label: Symbol,
    /// The `@binder` for the edge element (e.g. `w`).
    pub binder: Symbol,
}

/// Reference to a node type in a traversal.
#[derive(Debug, Clone)]
pub struct NodeRef {
    /// The `DefId` of the node struct.
    pub def: DefId,
    /// The collection label (e.g. `books`).
    pub label: Symbol,
    /// The `@binder` for the node element (e.g. `b`).
    pub binder: Symbol,
}

/// Traversal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `->` — follow `_from` → `_to`.
    Forward,
    /// `<-` — follow `_to` → `_from`.
    Backward,
    /// `<->` — both directions.
    Both,
}
