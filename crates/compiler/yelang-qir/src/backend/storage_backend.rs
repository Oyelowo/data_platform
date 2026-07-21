//! Embedded storage-engine backend (LSM, B-tree, etc.).

use crate::pir::capability::{BackendCapability, Cardinality, Support};
use crate::expr::{AggregateClass, QExprId};
use crate::lir::operator::ScanSource;
use crate::pir::operator::ExchangeKind;

/// Backend for an embedded storage engine.
#[derive(Debug, Default, Clone, Copy)]
pub struct StorageBackend;

impl StorageBackend {
    /// Create a new storage backend capability.
    pub fn new() -> Self {
        Self
    }
}

impl BackendCapability for StorageBackend {
    fn can_push_down_filter(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_order(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_limit(&self, _source: &ScanSource) -> bool {
        true
    }

    fn supports_index_lookup(&self, _source: &ScanSource, _key: &[QExprId]) -> bool {
        true
    }

    fn supports_hash_join(&self) -> Support {
        Support::WithFallback
    }

    fn supports_merge_join(&self) -> Support {
        Support::Yes
    }

    fn supports_nested_loop_join(&self) -> Support {
        Support::WithFallback
    }

    fn supports_exchange(&self, _kind: &ExchangeKind) -> bool {
        false
    }

    fn supports_aggregation(&self, _class: AggregateClass) -> bool {
        true
    }

    fn estimated_cardinality(&self, _source: &ScanSource) -> Cardinality {
        Cardinality::Medium
    }
}
