//! Placeholder for distributed/storage backends.

use crate::backend::capability::{BackendCapability, Cardinality, Support};
use crate::expr::{AggregateClass, QExprId};
use crate::logical::operator::ScanSource;
use crate::pir::operator::ExchangeKind;

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

    fn supports_index_lookup(&self, _source: &ScanSource, _key: &[QExprId]) -> bool {
        true
    }

    fn supports_hash_join(&self) -> Support {
        Support::Yes
    }

    fn supports_merge_join(&self) -> Support {
        Support::No
    }

    fn supports_nested_loop_join(&self) -> Support {
        Support::WithFallback
    }

    fn supports_exchange(&self, kind: &ExchangeKind) -> bool {
        matches!(kind, ExchangeKind::RepartitionBy(_) | ExchangeKind::Gather)
    }

    fn supports_aggregation(&self, _class: AggregateClass) -> bool {
        true
    }

    fn estimated_cardinality(&self, _source: &ScanSource) -> Cardinality {
        Cardinality::Large
    }
}
