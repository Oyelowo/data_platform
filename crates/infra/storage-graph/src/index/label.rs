//! Label indexes for nodes and edges.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::{InternalEdgeId, InternalNodeId};

/// Index from label strings to nodes and edges.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelIndex {
    nodes_by_label: BTreeMap<String, Vec<InternalNodeId>>,
    edges_by_label: BTreeMap<String, Vec<InternalEdgeId>>,
}

impl LabelIndex {
    /// Create an empty label index.
    pub fn new() -> Self {
        Self {
            nodes_by_label: BTreeMap::new(),
            edges_by_label: BTreeMap::new(),
        }
    }

    /// Index a node under all of its labels.
    pub fn insert_node(&mut self, node: InternalNodeId, labels: &[String]) {
        for label in labels {
            self.nodes_by_label
                .entry(label.clone())
                .or_default()
                .push(node);
        }
    }

    /// Remove a node from all of its labels.
    pub fn delete_node(&mut self, node: InternalNodeId, labels: &[String]) {
        for label in labels {
            if let Some(list) = self.nodes_by_label.get_mut(label) {
                list.retain(|&n| n != node);
                if list.is_empty() {
                    self.nodes_by_label.remove(label);
                }
            }
        }
    }

    /// Index an edge under its label.
    pub fn insert_edge(&mut self, edge: InternalEdgeId, label: &str) {
        self.edges_by_label
            .entry(label.to_string())
            .or_default()
            .push(edge);
    }

    /// Remove an edge from its label.
    pub fn delete_edge(&mut self, edge: InternalEdgeId, label: &str) {
        if let Some(list) = self.edges_by_label.get_mut(label) {
            list.retain(|&e| e != edge);
            if list.is_empty() {
                self.edges_by_label.remove(label);
            }
        }
    }

    /// Return node ids with `label`.
    pub fn nodes_with_label(&self, label: &str) -> &[InternalNodeId] {
        self.nodes_by_label.get(label).map_or(&[], |v| v.as_slice())
    }

    /// Return edge ids with `label`.
    pub fn edges_with_label(&self, label: &str) -> &[InternalEdgeId] {
        self.edges_by_label.get(label).map_or(&[], |v| v.as_slice())
    }

    /// Iterate over all node labels.
    pub fn iter_node_labels(&self) -> impl Iterator<Item = (&String, &Vec<InternalNodeId>)> {
        self.nodes_by_label.iter()
    }

    /// Iterate over all edge labels.
    pub fn iter_edge_labels(&self) -> impl Iterator<Item = (&String, &Vec<InternalEdgeId>)> {
        self.edges_by_label.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_labels() {
        let mut idx = LabelIndex::new();
        let n = InternalNodeId(1);
        idx.insert_node(n, &["User".into(), "Admin".into()]);
        assert_eq!(idx.nodes_with_label("User"), &[n]);
        assert_eq!(idx.nodes_with_label("Admin"), &[n]);
        idx.delete_node(n, &["Admin".into()]);
        assert!(idx.nodes_with_label("Admin").is_empty());
        assert_eq!(idx.nodes_with_label("User"), &[n]);
    }

    #[test]
    fn edge_labels() {
        let mut idx = LabelIndex::new();
        let e = InternalEdgeId(3);
        idx.insert_edge(e, "FOLLOWS");
        assert_eq!(idx.edges_with_label("FOLLOWS"), &[e]);
        idx.delete_edge(e, "FOLLOWS");
        assert!(idx.edges_with_label("FOLLOWS").is_empty());
    }
}
