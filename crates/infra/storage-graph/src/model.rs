//! Data model for the labeled property graph engine.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Opaque property map stored with nodes and edges.
pub type PropertyMap = BTreeMap<String, Vec<u8>>;

/// Set of labels attached to a node.
pub type LabelSet = BTreeSet<String>;

/// A node in the property graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// User-provided primary key.
    pub id: Vec<u8>,
    /// Ordered set of labels.
    pub labels: LabelSet,
    /// Opaque key-value properties.
    pub properties: PropertyMap,
}

impl Node {
    /// Create a new node.
    pub fn new(
        id: impl Into<Vec<u8>>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        properties: PropertyMap,
    ) -> Self {
        Self {
            id: id.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            properties,
        }
    }
}

/// An edge in the property graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// User-provided primary key.
    pub id: Vec<u8>,
    /// Source node id.
    pub from: Vec<u8>,
    /// Target node id.
    pub to: Vec<u8>,
    /// Edge label.
    pub label: String,
    /// Opaque key-value properties.
    pub properties: PropertyMap,
}

impl Edge {
    /// Create a new edge.
    pub fn new(
        id: impl Into<Vec<u8>>,
        from: impl Into<Vec<u8>>,
        to: impl Into<Vec<u8>>,
        label: impl Into<String>,
        properties: PropertyMap,
    ) -> Self {
        Self {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            label: label.into(),
            properties,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{EdgeRecord, NodeRecord};

    #[test]
    fn node_record_roundtrip() {
        let node = Node::new(
            b"n1".to_vec(),
            ["User", "Admin"],
            PropertyMap::from([("name".into(), b"Ada".to_vec())]),
        );
        let record = NodeRecord::from(&node);
        let back: Node = record.into();
        assert_eq!(node, back);
    }

    #[test]
    fn edge_record_roundtrip() {
        let edge = Edge::new(
            b"e1".to_vec(),
            b"n1".to_vec(),
            b"n2".to_vec(),
            "FOLLOWS",
            PropertyMap::from([("since".into(), b"2024".to_vec())]),
        );
        let record = EdgeRecord::from(&edge);
        let back: Edge = record.into();
        assert_eq!(edge, back);
    }
}
