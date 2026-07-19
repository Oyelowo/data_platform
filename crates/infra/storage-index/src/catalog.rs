//! Index catalog: durable metadata about secondary indexes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Opaque identifier returned by `create_index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexId(pub u32);

/// Lifecycle state of an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexState {
    /// Index is active and must be maintained by writes.
    Active,
    /// Index is being dropped; cleanup is in progress.
    Dropping,
}

/// Metadata for a single secondary index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDef {
    /// Unique id of this index.
    pub id: IndexId,
    /// Column names covered by this index.
    pub columns: Vec<String>,
    /// Current lifecycle state.
    pub state: IndexState,
}

/// The full set of indexes known to the engine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexCatalog {
    /// Name → index definition for all known indexes.
    pub indexes: HashMap<String, IndexDef>,
}

impl IndexCatalog {
    /// Create an empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return active indexes keyed by name.
    pub fn active(&self) -> impl Iterator<Item = (&String, &IndexDef)> {
        self.indexes.iter().filter(|(_, def)| def.state == IndexState::Active)
    }

    /// Return all indexes, including those being dropped.
    pub fn all(&self) -> &HashMap<String, IndexDef> {
        &self.indexes
    }
}
