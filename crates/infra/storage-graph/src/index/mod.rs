//! In-memory graph indexes.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::format::RecordAddress;
use crate::id::{EdgeIdMap, InternalEdgeId, InternalNodeId, NodeIdMap};
use crate::index::adjacency::{AdjacencyIndex, Direction};
use crate::index::label::LabelIndex;

pub mod adjacency;
pub mod label;

/// Entry tracking a live node in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEntry {
    /// User-provided node id.
    pub id: Vec<u8>,
    /// Address of the node record in the node store.
    pub address: RecordAddress,
    /// Current labels.
    pub labels: BTreeSet<String>,
}

/// Entry tracking a live edge in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeEntry {
    /// User-provided edge id.
    pub id: Vec<u8>,
    /// Address of the edge record in the edge store.
    pub address: RecordAddress,
    /// Internal source node id.
    pub from: InternalNodeId,
    /// Internal target node id.
    pub to: InternalNodeId,
    /// Edge label.
    pub label: String,
}

/// In-memory index over nodes, edges, adjacency, and labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphIndex {
    node_id_map: NodeIdMap,
    edge_id_map: EdgeIdMap,
    nodes: BTreeMap<InternalNodeId, NodeEntry>,
    edges: BTreeMap<InternalEdgeId, EdgeEntry>,
    adjacency: AdjacencyIndex,
    labels: LabelIndex,
    deleted_nodes: usize,
    deleted_edges: usize,
}

impl GraphIndex {
    /// Create an empty graph index.
    pub fn new() -> Self {
        Self {
            node_id_map: NodeIdMap::new(),
            edge_id_map: EdgeIdMap::new(),
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            adjacency: AdjacencyIndex::new(),
            labels: LabelIndex::new(),
            deleted_nodes: 0,
            deleted_edges: 0,
        }
    }

    /// Return the next internal node id.
    pub fn next_node_id(&self) -> InternalNodeId {
        self.node_id_map.next_id()
    }

    /// Return the next internal edge id.
    pub fn next_edge_id(&self) -> InternalEdgeId {
        self.edge_id_map.next_id()
    }

    /// Insert or replace a node.
    ///
    /// If the node already exists, its old labels and adjacency references are
    /// updated.
    pub fn insert_node(
        &mut self,
        id: Vec<u8>,
        address: RecordAddress,
        labels: BTreeSet<String>,
    ) -> InternalNodeId {
        let internal = self.node_id_map.get_or_insert(id.clone());
        if let Some(old) = self.nodes.remove(&internal) {
            self.labels
                .delete_node(internal, &old.labels.iter().cloned().collect::<Vec<_>>());
            self.deleted_nodes += 1;
        }
        self.labels
            .insert_node(internal, &labels.iter().cloned().collect::<Vec<_>>());
        self.nodes.insert(
            internal,
            NodeEntry {
                id,
                address,
                labels,
            },
        );
        internal
    }

    /// Insert or replace an edge.
    pub fn insert_edge(
        &mut self,
        id: Vec<u8>,
        from: InternalNodeId,
        to: InternalNodeId,
        address: RecordAddress,
        label: String,
    ) -> InternalEdgeId {
        let internal = self.edge_id_map.get_or_insert(id.clone());
        if let Some(old) = self.edges.remove(&internal) {
            self.adjacency.delete(old.from, old.to, internal);
            self.labels.delete_edge(internal, &old.label);
            self.deleted_edges += 1;
        }
        self.adjacency.insert(from, to, internal);
        self.labels.insert_edge(internal, &label);
        self.edges.insert(
            internal,
            EdgeEntry {
                id,
                address,
                from,
                to,
                label,
            },
        );
        internal
    }

    /// Delete a node and return the ids of all incident edges that must also be
    /// deleted.
    pub fn delete_node(&mut self, external_id: &[u8]) -> Option<Vec<InternalEdgeId>> {
        let internal = self.node_id_map.remove(external_id)?;
        let entry = self.nodes.remove(&internal)?;
        self.labels
            .delete_node(internal, &entry.labels.iter().cloned().collect::<Vec<_>>());
        self.adjacency.delete_node(internal);
        self.deleted_nodes += 1;

        // Collect and remove all edges that reference this node.
        let mut to_delete_internal = Vec::new();
        let edge_ids: Vec<InternalEdgeId> = self.edges.keys().copied().collect();
        for edge_id in edge_ids {
            if let Some(edge_entry) = self.edges.get(&edge_id)
                && (edge_entry.from == internal || edge_entry.to == internal)
            {
                to_delete_internal.push(edge_id);
            }
        }
        for edge_id in &to_delete_internal {
            self.delete_edge_internal(*edge_id);
        }
        Some(to_delete_internal)
    }

    /// Delete an edge.
    pub fn delete_edge(&mut self, external_id: &[u8]) -> Option<InternalEdgeId> {
        let internal = self.edge_id_map.remove(external_id)?;
        let entry = self.edges.remove(&internal)?;
        self.adjacency.delete(entry.from, entry.to, internal);
        self.labels.delete_edge(internal, &entry.label);
        self.deleted_edges += 1;
        Some(internal)
    }

    /// Remove an edge by internal id without touching the id map.
    ///
    /// Used during node deletion cascade.
    fn delete_edge_internal(&mut self, internal: InternalEdgeId) {
        if let Some(entry) = self.edges.remove(&internal) {
            self.edge_id_map.remove(&entry.id);
            self.adjacency.delete(entry.from, entry.to, internal);
            self.labels.delete_edge(internal, &entry.label);
            self.deleted_edges += 1;
        }
    }

    /// Look up a node by external id.
    pub fn get_node(&self, external_id: &[u8]) -> Option<(InternalNodeId, &NodeEntry)> {
        let internal = self.node_id_map.get(external_id)?;
        self.nodes.get(&internal).map(|entry| (internal, entry))
    }

    /// Look up an edge by external id.
    pub fn get_edge(&self, external_id: &[u8]) -> Option<(InternalEdgeId, &EdgeEntry)> {
        let internal = self.edge_id_map.get(external_id)?;
        self.edges.get(&internal).map(|entry| (internal, entry))
    }

    /// Look up a node entry by internal id.
    pub fn get_node_entry(&self, internal: InternalNodeId) -> Option<&NodeEntry> {
        self.nodes.get(&internal)
    }

    /// Look up an edge entry by internal id.
    pub fn get_edge_entry(&self, internal: InternalEdgeId) -> Option<&EdgeEntry> {
        self.edges.get(&internal)
    }

    /// Return neighbor node ids for `node` in the requested direction.
    pub fn neighbors(
        &self,
        internal: InternalNodeId,
        direction: Direction,
    ) -> Vec<InternalNodeId> {
        match direction {
            Direction::Out => self
                .adjacency
                .outgoing(internal)
                .iter()
                .filter_map(|&edge_id| {
                    self.edges.get(&edge_id).map(|entry| entry.to)
                })
                .collect(),
            Direction::In => self
                .adjacency
                .incoming(internal)
                .iter()
                .filter_map(|&edge_id| {
                    self.edges.get(&edge_id).map(|entry| entry.from)
                })
                .collect(),
            Direction::Both => {
                let mut out = self.neighbors(internal, Direction::Out);
                out.extend(self.neighbors(internal, Direction::In));
                out
            }
        }
    }

    /// Return incident edge ids for `node` in the requested direction.
    pub fn edges(&self, internal: InternalNodeId, direction: Direction) -> Vec<InternalEdgeId> {
        self.adjacency.edges(internal, direction)
    }

    /// Return node ids with `label`.
    pub fn nodes_with_label(&self, label: &str) -> &[InternalNodeId] {
        self.labels.nodes_with_label(label)
    }

    /// Return edge ids with `label`.
    pub fn edges_with_label(&self, label: &str) -> &[InternalEdgeId] {
        self.labels.edges_with_label(label)
    }

    /// Add a label to a node.
    pub fn add_node_label(&mut self, internal: InternalNodeId, label: String) -> bool {
        let entry = match self.nodes.get_mut(&internal) {
            Some(e) => e,
            None => return false,
        };
        if entry.labels.insert(label.clone()) {
            self.labels.insert_node(internal, &[label]);
            true
        } else {
            false
        }
    }

    /// Remove a label from a node.
    pub fn remove_node_label(&mut self, internal: InternalNodeId, label: &str) -> bool {
        let entry = match self.nodes.get_mut(&internal) {
            Some(e) => e,
            None => return false,
        };
        if entry.labels.remove(label) {
            self.labels.delete_node(internal, &[label.to_string()]);
            true
        } else {
            false
        }
    }

    /// Set a node property (no-op for index).
    pub fn set_node_property(&mut self, _internal: InternalNodeId, _key: &str) {}

    /// Delete a node property (no-op for index).
    pub fn delete_node_property(&mut self, _internal: InternalNodeId, _key: &str) {}

    /// Set an edge property (no-op for index).
    pub fn set_edge_property(&mut self, _internal: InternalEdgeId, _key: &str) {}

    /// Delete an edge property (no-op for index).
    pub fn delete_edge_property(&mut self, _internal: InternalEdgeId, _key: &str) {}

    /// Number of live nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of live edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Number of deleted node records.
    pub fn deleted_nodes(&self) -> usize {
        self.deleted_nodes
    }

    /// Number of deleted edge records.
    pub fn deleted_edges(&self) -> usize {
        self.deleted_edges
    }

    /// Estimate total records (live + deleted).
    pub fn total_records(&self) -> usize {
        self.node_count() + self.edge_count() + self.deleted_nodes + self.deleted_edges
    }

    /// Estimate the ratio of deleted records to total records.
    pub fn deletion_ratio(&self) -> f64 {
        let total = self.total_records();
        if total == 0 {
            return 0.0;
        }
        let deleted = self.deleted_nodes + self.deleted_edges;
        deleted as f64 / total as f64
    }

    /// Iterate over all live node entries.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (&InternalNodeId, &NodeEntry)> {
        self.nodes.iter()
    }

    /// Iterate over all live edge entries.
    pub fn iter_edges(&self) -> impl Iterator<Item = (&InternalEdgeId, &EdgeEntry)> {
        self.edges.iter()
    }
}

impl Default for GraphIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> RecordAddress {
        RecordAddress::new(0, 0, 0)
    }

    #[test]
    fn insert_and_lookup() {
        let mut idx = GraphIndex::new();
        let n1 = idx.insert_node(b"n1".to_vec(), addr(), ["User".into()].into());
        let n2 = idx.insert_node(b"n2".to_vec(), addr(), ["User".into()].into());
        idx.insert_edge(b"e1".to_vec(), n1, n2, addr(), "FOLLOWS".into());

        assert_eq!(idx.node_count(), 2);
        assert_eq!(idx.edge_count(), 1);
        assert_eq!(idx.neighbors(n1, Direction::Out), vec![n2]);
        assert_eq!(idx.neighbors(n2, Direction::In), vec![n1]);
    }

    #[test]
    fn delete_node_cascades_edges() {
        let mut idx = GraphIndex::new();
        let n1 = idx.insert_node(b"n1".to_vec(), addr(), ["User".into()].into());
        let n2 = idx.insert_node(b"n2".to_vec(), addr(), ["User".into()].into());
        idx.insert_edge(b"e1".to_vec(), n1, n2, addr(), "FOLLOWS".into());

        let removed = idx.delete_node(b"n1").unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(idx.node_count(), 1);
        assert_eq!(idx.edge_count(), 0);
    }

    #[test]
    fn label_indexing() {
        let mut idx = GraphIndex::new();
        let n = idx.insert_node(b"n1".to_vec(), addr(), ["User".into(), "Admin".into()].into());
        idx.add_node_label(n, "Moderator".into());
        assert_eq!(idx.nodes_with_label("Admin"), &[n]);
        assert_eq!(idx.nodes_with_label("Moderator"), &[n]);
        idx.remove_node_label(n, "Admin");
        assert!(idx.nodes_with_label("Admin").is_empty());
    }
}
