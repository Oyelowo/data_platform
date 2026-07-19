//! Placeholder for distributed/storage backends.

use crate::backend::capability::{BackendCapability, Cardinality};
use crate::expr::QExpr;
use crate::logical::operator::{AggregateKind, ScanSource};
use crate::physical::operator::ExchangeKind;

/// Placeholder backend for remote/distributed storage.
#[derive(Debug, Default, Clone, Copy)]
pub struct RemoteBackend;

impl RemoteBackend {
    /// Create a new remote backend capability placeholder.
    pub fn new() -> Self {
        Self
    }
}

impl BackendCapability for RemoteBackend {
    fn can_push_down_filter(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_order(&self, _source: &ScanSource) -> bool {
        false
    }

    fn can_push_down_limit(&self, _source: &ScanSource) -> bool {
        true
    }

    fn supports_index_lookup(&self, _source: &ScanSource, _key: &[QExpr]) -> bool {
        true
    }

    fn supports_hash_join(&self) -> bool {
        true
    }

    fn supports_merge_join(&self) -> bool {
        false
    }

    fn supports_exchange(&self, kind: ExchangeKind) -> bool {
        matches!(kind, ExchangeKind::RepartitionBy(_) | ExchangeKind::Gather)
    }

    fn supports_aggregation(&self, _kind: AggregateKind) -> bool {
        true
    }

    fn estimated_cardinality(&self, _source: &ScanSource) -> Cardinality {
        Cardinality::Large
    }
}
