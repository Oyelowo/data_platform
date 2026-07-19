//! In-memory table manifest: schema and file metadata.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::schema::TableSchema;
use crate::types::ColumnType;

/// A typed statistic value used for min/max bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StatsValue {
    /// Unknown / no bound (keeps all files during pruning).
    Unknown,
    /// Boolean value.
    Bool(bool),
    /// 64-bit signed integer.
    Int64(i64),
    /// 64-bit IEEE floating point.
    Float64(f64),
    /// UTF-8 string.
    Utf8(String),
    /// Opaque byte sequence.
    Binary(Bytes),
    /// Timestamp with microsecond precision.
    TimestampMicros(i64),
}

impl StatsValue {
    /// Return true if this value is unknown.
    pub fn is_unknown(&self) -> bool {
        matches!(self, StatsValue::Unknown)
    }
}

/// Per-column statistics extracted from a Parquet file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    /// Minimum value as a typed statistic.
    pub min: StatsValue,
    /// Maximum value as a typed statistic.
    pub max: StatsValue,
    /// Number of null values in the column.
    pub null_count: usize,
}

impl ColumnStats {
    /// Create unknown statistics (keeps all files during pruning).
    pub fn unknown() -> Self {
        Self {
            min: StatsValue::Unknown,
            max: StatsValue::Unknown,
            null_count: 0,
        }
    }

    /// Update min/max in place using the column's logical type.
    pub fn update(&mut self, min: StatsValue, max: StatsValue) {
        if self.min.is_unknown() || self.max.is_unknown() {
            self.min = min;
            self.max = max;
            return;
        }

        self.min = min_of(&self.min, &min);
        self.max = max_of(&self.max, &max);
    }
}

fn min_of(a: &StatsValue, b: &StatsValue) -> StatsValue {
    use std::cmp::Ordering;
    match a.partial_cmp(b) {
        Some(Ordering::Greater) => b.clone(),
        _ => a.clone(),
    }
}

fn max_of(a: &StatsValue, b: &StatsValue) -> StatsValue {
    use std::cmp::Ordering;
    match a.partial_cmp(b) {
        Some(Ordering::Less) => b.clone(),
        _ => a.clone(),
    }
}

impl PartialOrd for StatsValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (StatsValue::Unknown, _) | (_, StatsValue::Unknown) => None,
            (StatsValue::Bool(a), StatsValue::Bool(b)) => a.partial_cmp(b),
            (StatsValue::Int64(a), StatsValue::Int64(b)) => a.partial_cmp(b),
            (StatsValue::TimestampMicros(a), StatsValue::TimestampMicros(b)) => a.partial_cmp(b),
            (StatsValue::Int64(a), StatsValue::TimestampMicros(b))
            | (StatsValue::TimestampMicros(a), StatsValue::Int64(b)) => a.partial_cmp(b),
            (StatsValue::Float64(a), StatsValue::Float64(b)) => a.partial_cmp(b),
            (StatsValue::Utf8(a), StatsValue::Utf8(b)) => a.partial_cmp(b),
            (StatsValue::Binary(a), StatsValue::Binary(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl StatsValue {
    /// Parse a scan-time byte value into a typed statistic for the given logical type.
    pub fn from_bytes(bytes: &Bytes, ty: ColumnType) -> crate::Result<Self> {
        match ty {
            ColumnType::Bool => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for Bool: {e}")))?;
                match s.to_ascii_lowercase().as_str() {
                    "true" | "1" => Ok(StatsValue::Bool(true)),
                    "false" | "0" => Ok(StatsValue::Bool(false)),
                    _ => Err(crate::Error::Predicate(format!("invalid Bool value: {s}"))),
                }
            }
            ColumnType::Int64 => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for Int64: {e}")))?;
                s.parse::<i64>()
                    .map(StatsValue::Int64)
                    .map_err(|e| crate::Error::Predicate(format!("invalid Int64 '{s}': {e}")))
            }
            ColumnType::Float64 => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for Float64: {e}")))?;
                s.parse::<f64>()
                    .map(StatsValue::Float64)
                    .map_err(|e| crate::Error::Predicate(format!("invalid Float64 '{s}': {e}")))
            }
            ColumnType::Utf8 => Ok(StatsValue::Utf8(
                std::str::from_utf8(bytes)
                    .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for Utf8: {e}")))?
                    .to_string(),
            )),
            ColumnType::Binary => Ok(StatsValue::Binary(bytes.clone())),
            ColumnType::TimestampMicros => {
                let s = std::str::from_utf8(bytes)
                    .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for Timestamp: {e}")))?;
                s.parse::<i64>()
                    .map(StatsValue::TimestampMicros)
                    .map_err(|e| crate::Error::Predicate(format!("invalid Timestamp '{s}': {e}")))
            }
        }
    }
}

/// Metadata for a single immutable Parquet file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMeta {
    /// Absolute path to the Parquet file.
    pub path: PathBuf,
    /// Partition directory name.
    pub partition: String,
    /// Number of rows in the file.
    pub row_count: usize,
    /// Creation time.
    pub created_at: SystemTime,
    /// Per-column statistics.
    pub column_stats: HashMap<String, ColumnStats>,
}

/// Snapshot of table state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Current table schema.
    pub schema: TableSchema,
    /// Live files in insertion order.
    pub files: Vec<FileMeta>,
}

impl Manifest {
    /// Create an empty manifest.
    pub fn empty() -> Self {
        Self {
            schema: TableSchema::empty(),
            files: Vec::new(),
        }
    }
}
