//! Query API for the graph engine.

use crate::model::{Edge, Node};
use crate::query::pattern::PatternStep;

pub mod pattern;
pub mod traversal;

/// Direction of traversal across directed edges.
pub use crate::index::adjacency::Direction;

/// A query against the graph engine.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphQuery {
    /// Fetch a node by id.
    NodeById(Vec<u8>),
    /// Fetch an edge by id.
    EdgeById(Vec<u8>),
    /// Neighbors of a node in a direction, optionally filtered by edge label.
    Neighbors {
        /// Node id.
        node: Vec<u8>,
        /// Direction.
        direction: Direction,
        /// Optional edge label filter.
        edge_label: Option<String>,
    },
    /// Nodes with the given label.
    NodesByLabel(String),
    /// Edges with the given label.
    EdgesByLabel(String),
    /// Find a path between two nodes.
    Path {
        /// Start node id.
        from: Vec<u8>,
        /// End node id.
        to: Vec<u8>,
        /// Maximum number of edges in the path.
        max_depth: usize,
    },
    /// Match a chain pattern.
    Pattern(Vec<PatternStep>),
}

/// Result of a graph query.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    /// Nodes returned by the query.
    pub nodes: Vec<Node>,
    /// Edges returned by the query.
    pub edges: Vec<Edge>,
    /// Paths returned by the query, each path is a sequence of node ids.
    pub paths: Vec<Vec<Vec<u8>>>,
}

impl QueryResult {
    /// Create an empty query result.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            paths: Vec::new(),
        }
    }

    /// Create a result containing only nodes.
    pub fn nodes(nodes: Vec<Node>) -> Self {
        Self {
            nodes,
            edges: Vec::new(),
            paths: Vec::new(),
        }
    }

    /// Create a result containing only edges.
    pub fn edges(edges: Vec<Edge>) -> Self {
        Self {
            nodes: Vec::new(),
            edges,
            paths: Vec::new(),
        }
    }

    /// Create a result containing only paths.
    pub fn paths(paths: Vec<Vec<Vec<u8>>>) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            paths,
        }
    }
}

impl Default for QueryResult {
    fn default() -> Self {
        Self::new()
    }
}
