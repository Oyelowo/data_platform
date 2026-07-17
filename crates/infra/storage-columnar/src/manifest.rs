//! In-memory table manifest: schema and file metadata.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::schema::TableSchema;

/// Per-column statistics extracted from a Parquet file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    /// Minimum value in the same byte representation used for scan output.
    pub min: Bytes,
    /// Maximum value in the same byte representation used for scan output.
    pub max: Bytes,
    /// Number of null values in the column.
    pub null_count: usize,
}

impl ColumnStats {
    /// Create unknown statistics (keeps all files during pruning).
    pub fn unknown() -> Self {
        Self {
            min: Bytes::new(),
            max: Bytes::new(),
            null_count: 0,
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
