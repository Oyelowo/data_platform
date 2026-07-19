//! Configuration options for the geospatial storage engine.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// WAL durability policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalSyncPolicy {
    /// Fsync every individual append.
    EveryWrite,
    /// Fsync at most once per batch interval.
    Batch(Duration),
    /// Only fsync when `Engine::sync` is called.
    #[default]
    SyncOnEngineSync,
}

impl WalSyncPolicy {
    /// Return the `storage_wal::Durability` value used for normal appends.
    pub fn durability(&self) -> storage_wal::Durability {
        match self {
            WalSyncPolicy::EveryWrite => storage_wal::Durability::Immediate,
            WalSyncPolicy::Batch(_) | WalSyncPolicy::SyncOnEngineSync => {
                storage_wal::Durability::Buffered
            }
        }
    }
}

/// Supported geometry kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeometryKind {
    /// A single point.
    Point,
    /// A line string.
    LineString,
    /// A polygon.
    Polygon,
    /// A multi-point.
    MultiPoint,
    /// A multi-line-string.
    MultiLineString,
    /// A multi-polygon.
    MultiPolygon,
    /// A heterogeneous geometry collection.
    GeometryCollection,
}

/// Top-level options for [`GeoEngine`](crate::engine::GeoEngine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoOptions {
    /// Whether coordinates are assumed to be WGS84.
    ///
    /// This is always `true` in v1.
    pub use_wgs84: bool,

    /// Soft limit on the number of un-synced features kept in the mutable
    /// in-memory index.
    pub max_memtable_features: usize,

    /// WAL durability policy.
    pub wal_sync_policy: WalSyncPolicy,

    /// Maximum length of a feature id in bytes.
    pub max_key_len: usize,

    /// Maximum length of a single property value in bytes.
    pub max_value_len: usize,

    /// Trigger compaction when the ratio of deleted/updated bytes to live bytes
    /// exceeds this threshold.
    pub compaction_threshold: f64,
}

impl GeoOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if !self.use_wgs84 {
            return Err(crate::Error::invalid_argument(
                "only WGS84 coordinates are supported",
            ));
        }
        if self.max_memtable_features == 0 {
            return Err(crate::Error::invalid_argument(
                "max_memtable_features must be greater than zero",
            ));
        }
        if self.max_key_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_key_len must be greater than zero",
            ));
        }
        if self.max_value_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_value_len must be greater than zero",
            ));
        }
        if !(0.0..=1.0).contains(&self.compaction_threshold) {
            return Err(crate::Error::invalid_argument(
                "compaction_threshold must be in [0, 1]",
            ));
        }
        Ok(())
    }
}

impl Default for GeoOptions {
    fn default() -> Self {
        Self {
            use_wgs84: true,
            max_memtable_features: 10_000,
            wal_sync_policy: WalSyncPolicy::default(),
            max_key_len: 4096,
            max_value_len: 1024 * 1024,
            compaction_threshold: 0.25,
        }
    }
}
