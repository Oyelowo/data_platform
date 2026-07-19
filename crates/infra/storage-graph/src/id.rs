//! Internal dense identifiers for nodes and edges.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Dense internal identifier for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InternalNodeId(pub u64);

/// Dense internal identifier for an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InternalEdgeId(pub u64);

/// Maps user-provided byte-string ids to dense internal node ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeIdMap {
    next: u64,
    map: BTreeMap<Vec<u8>, InternalNodeId>,
}

impl NodeIdMap {
    /// Create a new empty id map.
    pub fn new() -> Self {
        Self {
            next: 0,
            map: BTreeMap::new(),
        }
    }

    /// Return the internal id for an external id if it exists.
    pub fn get(&self, external: &[u8]) -> Option<InternalNodeId> {
        self.map.get(external).copied()
    }

    /// Insert a new external id and assign a fresh internal id.
    ///
    /// Returns the assigned internal id. If the external id already exists,
    /// the existing internal id is returned and `next` is not advanced.
    pub fn get_or_insert(&mut self, external: Vec<u8>) -> InternalNodeId {
        match self.map.get(&external) {
            Some(id) => *id,
            None => {
                let id = InternalNodeId(self.next);
                self.next += 1;
                self.map.insert(external, id);
                id
            }
        }
    }

    /// Assign a specific internal id for an external id.
    ///
    /// Used during recovery to restore exact ids.
    pub fn insert(&mut self, external: Vec<u8>, internal: InternalNodeId) {
        self.map.insert(external, internal);
        if internal.0 >= self.next {
            self.next = internal.0 + 1;
        }
    }

    /// Remove an external id mapping.
    pub fn remove(&mut self, external: &[u8]) -> Option<InternalNodeId> {
        self.map.remove(external)
    }

    /// Return the next internal id that would be assigned.
    pub fn next_id(&self) -> InternalNodeId {
        InternalNodeId(self.next)
    }

    /// Set the next internal id counter.
    pub fn set_next_id(&mut self, next: InternalNodeId) {
        self.next = next.0;
    }

    /// Iterate over all mappings.
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &InternalNodeId)> {
        self.map.iter()
    }

    /// Number of tracked ids.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Maps user-provided byte-string ids to dense internal edge ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeIdMap {
    next: u64,
    map: BTreeMap<Vec<u8>, InternalEdgeId>,
}

impl EdgeIdMap {
    /// Create a new empty id map.
    pub fn new() -> Self {
        Self {
            next: 0,
            map: BTreeMap::new(),
        }
    }

    /// Return the internal id for an external id if it exists.
    pub fn get(&self, external: &[u8]) -> Option<InternalEdgeId> {
        self.map.get(external).copied()
    }

    /// Insert a new external id and assign a fresh internal id.
    pub fn get_or_insert(&mut self, external: Vec<u8>) -> InternalEdgeId {
        match self.map.get(&external) {
            Some(id) => *id,
            None => {
                let id = InternalEdgeId(self.next);
                self.next += 1;
                self.map.insert(external, id);
                id
            }
        }
    }

    /// Assign a specific internal id for an external id.
    pub fn insert(&mut self, external: Vec<u8>, internal: InternalEdgeId) {
        self.map.insert(external, internal);
        if internal.0 >= self.next {
            self.next = internal.0 + 1;
        }
    }

    /// Remove an external id mapping.
    pub fn remove(&mut self, external: &[u8]) -> Option<InternalEdgeId> {
        self.map.remove(external)
    }

    /// Return the next internal id that would be assigned.
    pub fn next_id(&self) -> InternalEdgeId {
        InternalEdgeId(self.next)
    }

    /// Set the next internal id counter.
    pub fn set_next_id(&mut self, next: InternalEdgeId) {
        self.next = next.0;
    }

    /// Iterate over all mappings.
    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &InternalEdgeId)> {
        self.map.iter()
    }

    /// Number of tracked ids.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_map_assigns_sequential_ids() {
        let mut map = NodeIdMap::new();
        let a = map.get_or_insert(b"a".to_vec());
        let b = map.get_or_insert(b"b".to_vec());
        let a2 = map.get_or_insert(b"a".to_vec());
        assert_eq!(a.0, 0);
        assert_eq!(b.0, 1);
        assert_eq!(a, a2);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn edge_id_map_recovers_next_counter() {
        let mut map = EdgeIdMap::new();
        map.insert(b"x".to_vec(), InternalEdgeId(7));
        assert_eq!(map.next_id().0, 8);
    }
}
