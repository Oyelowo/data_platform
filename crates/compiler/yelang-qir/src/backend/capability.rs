//! Backend capability and cost hints.

use crate::expr::QExpr;
use crate::logical::operator::{AggregateKind, ScanSource};
use crate::physical::operator::ExchangeKind;

/// Estimated cardinality of a scan source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Cardinality {
    /// Unknown cardinality.
    #[default]
    Unknown,
    /// Estimated small size (e.g. < 10k rows).
    Small,
    /// Estimated medium size.
    Medium,
    /// Estimated large size.
    Large,
}

/// Capability model for a storage or execution backend.
///
/// Physical planning consults this trait to decide whether operators can be
/// pushed down, which join algorithms are available, and how data must be
/// exchanged.
pub trait BackendCapability {
    /// Can the backend apply a filter predicate at the source?
    fn can_push_down_filter(&self, source: &ScanSource) -> bool;

    /// Can the backend apply ordering at the source?
    fn can_push_down_order(&self, source: &ScanSource) -> bool;

    /// Can the backend apply limit/slice at the source?
    fn can_push_down_limit(&self, source: &ScanSource) -> bool;

    /// Does the backend support an index lookup on the given fields?
    fn supports_index_lookup(&self, source: &ScanSource, key: &[QExpr]) -> bool;

    /// Does the backend support hash joins?
    fn supports_hash_join(&self) -> bool;

    /// Does the backend support merge joins?
    fn supports_merge_join(&self) -> bool;

    /// Does the backend support the given exchange kind?
    fn supports_exchange(&self, kind: ExchangeKind) -> bool;

    /// Does the backend support the given aggregate kind?
    fn supports_aggregation(&self, kind: AggregateKind) -> bool;

    /// Estimated cardinality of a scan source.
    fn estimated_cardinality(&self, source: &ScanSource) -> Cardinality;
}
