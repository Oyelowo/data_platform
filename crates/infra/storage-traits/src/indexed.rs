//! Indexed engine trait (placeholder for Phase 2).

use crate::engine::Engine;
use crate::error::Result;

/// An engine that supports secondary indexes.
pub trait IndexedEngine: Engine {
    /// Opaque index handle.
    type IndexId: Clone + Send + Sync;

    /// Create a secondary index named `name` over `columns`.
    fn create_index(&self, name: &str, columns: &[&str]) -> Result<Self::IndexId, Self::Error>;

    /// Drop a secondary index.
    fn drop_index(&self, id: Self::IndexId) -> Result<(), Self::Error>;
}
