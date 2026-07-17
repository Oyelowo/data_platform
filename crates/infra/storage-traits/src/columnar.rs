//! Columnar engine trait (placeholder for Phase 2).

use bytes::Bytes;

use crate::error::Result;

/// A column-oriented batch of rows.
///
/// Each tuple is `(column_name, values)` where `None` represents a SQL NULL.
pub type ColumnBatch = Vec<(String, Vec<Option<Bytes>>)>;

/// Result of a columnar scan: projected columns in the requested order.
pub type ScanResult = Vec<(String, Vec<Option<Bytes>>)>;

/// A predicate expression for pushdown.
///
/// This is intentionally a minimal enum. More operators are added as engines
/// grow.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Predicate {
    /// Always true.
    #[default]
    True,
    /// Equality on a named column.
    Eq {
        /// Column name.
        column: String,
        /// Value to compare against.
        value: Bytes,
    },
    /// Range comparison on a named column.
    Range {
        /// Column name.
        column: String,
        /// Lower bound, if any.
        lower: Option<Bytes>,
        /// Whether the lower bound is inclusive.
        lower_inclusive: bool,
        /// Upper bound, if any.
        upper: Option<Bytes>,
        /// Whether the upper bound is inclusive.
        upper_inclusive: bool,
    },
    /// Logical conjunction.
    And(Vec<Predicate>),
    /// Logical disjunction.
    Or(Vec<Predicate>),
}

/// Column-oriented storage engine for analytical workloads.
///
/// Phase 0 only defines the trait. The Arrow/Parquet implementation is Phase 2.
pub trait ColumnarEngine: Send + Sync + 'static {
    /// Error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Ingest a batch of rows.
    ///
    /// `None` represents a SQL-style NULL. `Some(Bytes::new())` is a valid empty
    /// string or binary value.
    fn ingest(&self, columns: ColumnBatch) -> Result<(), Self::Error>;

    /// Scan columns matching `projection`, filtering by `predicate`.
    fn scan(&self, projection: &[&str], predicate: &Predicate) -> Result<ScanResult, Self::Error>;
}
