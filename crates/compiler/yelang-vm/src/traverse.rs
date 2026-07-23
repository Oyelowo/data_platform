//! Link traversal specification for the Yelang VM.
//!
//! A [`TraverseSpec`] describes how to follow links (edges) from a source
//! collection to a target collection. The VM's `QueryTraverse` instruction
//! uses it to perform a nested-loop traversal: for every input row it finds
//! matching edges in the edge table, then resolves each edge to its target
//! row, collecting the matches into a nested array column.
//!
//! # Model
//!
//! An edge table holds links between two node tables. Each edge row carries a
//! *source* key (the node the edge leaves from) and a *target* key (the node
//! it points to). Given an input row, the traversal:
//!
//! 1. Reads the row's `source_key` field.
//! 2. Scans the edge table for edges whose `source_column` matches it
//!    (for [`TraverseDirection::Out`]; reversed for `In`, either for `Both`).
//! 3. For each matching edge, looks up the target row whose `target_key`
//!    field equals the edge's `target_column`.
//! 4. Collects all matched target rows into an array stored under `output`.

use yelang_interner::Symbol;

/// Direction of a link traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraverseDirection {
    /// Follow outgoing edges: match the input row against the edge's source
    /// column and resolve the edge's target column to a target row.
    Out,
    /// Follow incoming edges: match the input row against the edge's target
    /// column and resolve the edge's source column to a (source) row.
    In,
    /// Follow edges in either direction.
    Both,
}

/// A specification for traversing links from a source collection to a target
/// collection via an edge (link) table.
#[derive(Debug, Clone, PartialEq)]
pub struct TraverseSpec {
    /// The edge (link) table id.
    pub edge_table: u64,
    /// The edge-table column holding the source node key.
    pub source_column: Symbol,
    /// The edge-table column holding the target node key.
    pub target_column: Symbol,
    /// The target node table id.
    pub target_table: u64,
    /// Traversal direction.
    pub direction: TraverseDirection,
    /// The input-row field matched against the edge's `source_column`
    /// (or `target_column` for [`TraverseDirection::In`]).
    pub source_key: Symbol,
    /// The target-row field matched against the edge's `target_column`
    /// (or `source_column` for [`TraverseDirection::In`]).
    pub target_key: Symbol,
    /// The output field name for the nested array of matched target rows.
    pub output: Symbol,
}
