//! In-memory array/table backend.

use crate::backend::capability::{BackendCapability, Cardinality};
use crate::expr::QExpr;
use crate::logical::operator::{AggregateKind, ScanSource};
use crate::physical::operator::ExchangeKind;

/// A backend that executes against in-memory arrays.
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryBackend;

impl MemoryBackend {
    /// Create a new in-memory backend capability.
    pub fn new() -> Self {
        Self
    }
}

impl BackendCapability for MemoryBackend {
    fn can_push_down_filter(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_order(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_limit(&self, _source: &ScanSource) -> bool {
        true
    }

    fn supports_index_lookup(&self, _source: &ScanSource, _key: &[QExpr]) -> bool {
        false
    }

    fn supports_hash_join(&self) -> bool {
        true
    }

    fn supports_merge_join(&self) -> bool {
        true
    }

    fn supports_exchange(&self, _kind: ExchangeKind) -> bool {
        true
    }

    fn supports_aggregation(&self, _kind: AggregateKind) -> bool {
        true
    }

    fn estimated_cardinality(&self, _source: &ScanSource) -> Cardinality {
        Cardinality::Small
    }
}
