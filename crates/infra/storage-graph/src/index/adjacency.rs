//! Outgoing and incoming adjacency indexes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::{InternalEdgeId, InternalNodeId};

/// Direction of traversal across directed edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Outgoing edges.
    Out,
    /// Incoming edges.
    In,
    /// Both outgoing and incoming edges.
    Both,
}

/// Adjacency index mapping nodes to their incident edge ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdjacencyIndex {
    outgoing: BTreeMap<InternalNodeId, Vec<InternalEdgeId>>,
    incoming: BTreeMap<InternalNodeId, Vec<InternalEdgeId>>,
}

impl AdjacencyIndex {
    /// Create an empty adjacency index.
    pub fn new() -> Self {
        Self {
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
        }
    }

    /// Record that `edge` goes from `from` to `to`.
    pub fn insert(&mut self, from: InternalNodeId, to: InternalNodeId, edge: InternalEdgeId) {
        self.outgoing.entry(from).or_default().push(edge);
        self.incoming.entry(to).or_default().push(edge);
    }

    /// Remove `edge` from the adjacency lists of `from` and `to`.
    pub fn delete(&mut self, from: InternalNodeId, to: InternalNodeId, edge: InternalEdgeId) {
        if let Some(list) = self.outgoing.get_mut(&from) {
            list.retain(|&e| e != edge);
            if list.is_empty() {
                self.outgoing.remove(&from);
            }
        }
        if let Some(list) = self.incoming.get_mut(&to) {
            list.retain(|&e| e != edge);
            if list.is_empty() {
                self.incoming.remove(&to);
            }
        }
    }

    /// Remove a node and all references to its incident edges.
    ///
    /// The caller is responsible for deleting the actual edge records.
    pub fn delete_node(&mut self, node: InternalNodeId) {
        self.outgoing.remove(&node);
        self.incoming.remove(&node);
    }

    /// Return outgoing edge ids for `node`.
    pub fn outgoing(&self, node: InternalNodeId) -> &[InternalEdgeId] {
        self.outgoing.get(&node).map_or(&[], |v| v.as_slice())
    }

    /// Return incoming edge ids for `node`.
    pub fn incoming(&self, node: InternalNodeId) -> &[InternalEdgeId] {
        self.incoming.get(&node).map_or(&[], |v| v.as_slice())
    }

    /// Return edge ids for `node` in the requested direction.
    pub fn edges(&self, node: InternalNodeId, direction: Direction) -> Vec<InternalEdgeId> {
        match direction {
            Direction::Out => self.outgoing(node).to_vec(),
            Direction::In => self.incoming(node).to_vec(),
            Direction::Both => {
                let mut out = self.outgoing(node).to_vec();
                out.extend_from_slice(self.incoming(node));
                out
            }
        }
    }

    /// Return the out-degree of `node`.
    pub fn out_degree(&self, node: InternalNodeId) -> usize {
        self.outgoing(node).len()
    }

    /// Return the in-degree of `node`.
    pub fn in_degree(&self, node: InternalNodeId) -> usize {
        self.incoming(node).len()
    }

    /// Return all edges incident to `node` regardless of direction.
    pub fn degree(&self, node: InternalNodeId) -> usize {
        self.out_degree(node) + self.in_degree(node)
    }

    /// Iterate over all outgoing adjacencies.
    pub fn iter_outgoing(&self) -> impl Iterator<Item = (&InternalNodeId, &Vec<InternalEdgeId>)> {
        self.outgoing.iter()
    }

    /// Iterate over all incoming adjacencies.
    pub fn iter_incoming(&self) -> impl Iterator<Item = (&InternalNodeId, &Vec<InternalEdgeId>)> {
        self.incoming.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_delete_edge() {
        let mut idx = AdjacencyIndex::new();
        let n1 = InternalNodeId(1);
        let n2 = InternalNodeId(2);
        let e1 = InternalEdgeId(10);
        idx.insert(n1, n2, e1);
        assert_eq!(idx.out_degree(n1), 1);
        assert_eq!(idx.in_degree(n2), 1);
        idx.delete(n1, n2, e1);
        assert_eq!(idx.out_degree(n1), 0);
        assert_eq!(idx.in_degree(n2), 0);
    }

    #[test]
    fn self_loop_counts_in_both() {
        let mut idx = AdjacencyIndex::new();
        let n = InternalNodeId(1);
        let e = InternalEdgeId(5);
        idx.insert(n, n, e);
        assert_eq!(idx.degree(n), 2);
        let both = idx.edges(n, Direction::Both);
        assert_eq!(both.len(), 2);
    }
}
