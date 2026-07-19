//! Physical properties: ordering, distribution, and streaming unit.

use crate::expr::QExpr;

/// Ordering property of an operator's output.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Ordering {
    /// No guaranteed ordering.
    #[default]
    Arbitrary,
    /// Ordered by the given keys.
    By(Vec<OrderKey>),
}

/// A sort key used in ordering properties.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderKey {
    pub expr: QExpr,
    pub descending: bool,
}

/// Distribution of an operator's output across workers.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Distribution {
    /// Single worker.
    #[default]
    Single,
    /// Broadcast to all workers.
    Broadcast,
    /// Hash-partitioned by the given expressions.
    Hash(Vec<QExpr>),
    /// Fully replicated on every worker.
    Replicate,
    /// Unknown / unconstrained.
    Unknown,
}

/// Streaming granularity of an operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StreamingUnit {
    /// One output per parent element (nested/correlated).
    #[default]
    PerParent,
    /// One output per input leaf element.
    Leaf,
    /// Single scalar output.
    Scalar,
}

/// Combined physical properties for an operator.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties {
    pub ordering: Ordering,
    pub distribution: Distribution,
    pub streaming: StreamingUnit,
}
