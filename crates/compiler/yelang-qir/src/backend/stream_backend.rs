//! Streaming / event-bus backend.

use crate::backend::capability::{BackendCapability, Cardinality, Support};
use crate::expr::{AggregateClass, QExprId};
use crate::logical::operator::ScanSource;
use crate::pir::operator::ExchangeKind;

/// Backend for an unbounded event stream.
#[derive(Debug, Default, Clone, Copy)]
pub struct StreamBackend;

impl StreamBackend {
    /// Create a new stream backend capability.
    pub fn new() -> Self {
        Self
    }
}

impl BackendCapability for StreamBackend {
    fn can_push_down_filter(&self, _source: &ScanSource) -> bool {
        true
    }

    fn can_push_down_order(&self, _source: &ScanSource) -> bool {
        false
    }

    fn can_push_down_limit(&self, _source: &ScanSource) -> bool {
        false
    }

    fn supports_index_lookup(&self, _source: &ScanSource, _key: &[QExprId]) -> bool {
        false
    }

    fn supports_hash_join(&self) -> Support {
        Support::No
    }

    fn supports_merge_join(&self) -> Support {
        Support::No
    }

    fn supports_nested_loop_join(&self) -> Support {
        Support::WithFallback
    }

    fn supports_exchange(&self, kind: &ExchangeKind) -> bool {
        matches!(kind, ExchangeKind::Single | ExchangeKind::RepartitionBy(_))
    }

    fn supports_aggregation(&self, class: AggregateClass) -> bool {
        // Streaming prefers distributive/algebraic; holistic is expensive.
        matches!(class, AggregateClass::Distributive | AggregateClass::Algebraic)
    }

    fn estimated_cardinality(&self, _source: &ScanSource) -> Cardinality {
        Cardinality::Large
    }
}
