//! Logical column types and Arrow/Parquet type mappings.

use arrow_schema::DataType;
use serde::{Deserialize, Serialize};

/// Logical type of a column in a `TableSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    /// Boolean values.
    Bool,
    /// 64-bit signed integer.
    Int64,
    /// 64-bit IEEE floating point.
    Float64,
    /// UTF-8 string.
    Utf8,
    /// Opaque byte sequence.
    Binary,
    /// Timestamp with microsecond precision and no timezone.
    TimestampMicros,
}

impl ColumnType {
    /// Return the Arrow `DataType` that stores this logical type.
    pub fn to_arrow(&self) -> DataType {
        match self {
            ColumnType::Bool => DataType::Boolean,
            ColumnType::Int64 => DataType::Int64,
            ColumnType::Float64 => DataType::Float64,
            ColumnType::Utf8 => DataType::Utf8,
            ColumnType::Binary => DataType::Binary,
            ColumnType::TimestampMicros => {
                DataType::Timestamp(arrow_schema::TimeUnit::Microsecond, None)
            }
        }
    }

    /// Try to interpret an Arrow `DataType` as a supported logical type.
    pub fn try_from_arrow(data_type: &DataType) -> crate::Result<Self> {
        match data_type {
            DataType::Boolean => Ok(ColumnType::Bool),
            DataType::Int64 => Ok(ColumnType::Int64),
            DataType::Float64 => Ok(ColumnType::Float64),
            DataType::Utf8 | DataType::LargeUtf8 => Ok(ColumnType::Utf8),
            DataType::Binary | DataType::LargeBinary => Ok(ColumnType::Binary),
            DataType::Timestamp(arrow_schema::TimeUnit::Microsecond, _) => {
                Ok(ColumnType::TimestampMicros)
            }
            other => Err(crate::Error::Schema(format!(
                "unsupported Arrow type for logical type mapping: {other:?}"
            ))),
        }
    }
}
