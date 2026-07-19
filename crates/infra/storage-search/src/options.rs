//! Configuration options for the search engine.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::schema::Schema;

/// Top-level options for [`SearchEngine`](crate::SearchEngine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Soft byte limit for the in-memory segment before it is flushed to disk.
    pub memtable_size_limit: usize,

    /// Maximum number of disk segments before a merge is triggered.
    pub max_segments: usize,

    /// Number of segments to merge together when the threshold is exceeded.
    pub merge_factor: usize,

    /// BM25 `k1` parameter.
    pub bm25_k1: f32,

    /// BM25 `b` parameter.
    pub bm25_b: f32,

    /// WAL durability policy.
    pub wal_sync_policy: WalSyncPolicy,

    /// Maximum length of a document id in bytes.
    pub max_key_len: usize,

    /// Maximum length of a field value in bytes.
    pub max_field_len: usize,

    /// Maximum number of query results returned when no explicit limit is given.
    pub default_top_k: usize,
}

impl SearchOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if self.memtable_size_limit == 0 {
            return Err(crate::Error::invalid_argument(
                "memtable_size_limit must be greater than zero",
            ));
        }
        if self.max_segments == 0 {
            return Err(crate::Error::invalid_argument(
                "max_segments must be greater than zero",
            ));
        }
        if self.merge_factor == 0 || self.merge_factor > self.max_segments {
            return Err(crate::Error::invalid_argument(
                "merge_factor must be in [1, max_segments]",
            ));
        }
        if !(0.0..=10.0).contains(&self.bm25_k1) {
            return Err(crate::Error::invalid_argument(
                "bm25_k1 must be in [0, 10]",
            ));
        }
        if !(0.0..=1.0).contains(&self.bm25_b) {
            return Err(crate::Error::invalid_argument(
                "bm25_b must be in [0, 1]",
            ));
        }
        if self.max_key_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_key_len must be greater than zero",
            ));
        }
        if self.max_field_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_field_len must be greater than zero",
            ));
        }
        if self.default_top_k == 0 {
            return Err(crate::Error::invalid_argument(
                "default_top_k must be greater than zero",
            ));
        }
        Ok(())
    }

    /// Return options with a default search schema.
    pub fn default_for(_schema: Schema) -> Self {
        Self {
            memtable_size_limit: 8 * 1024 * 1024,
            max_segments: 10,
            merge_factor: 5,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
            max_key_len: 4096,
            max_field_len: 128 * 1024,
            default_top_k: 100,
        }
    }
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            memtable_size_limit: 8 * 1024 * 1024,
            max_segments: 10,
            merge_factor: 5,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
            max_key_len: 4096,
            max_field_len: 128 * 1024,
            default_top_k: 100,
        }
    }
}

/// WAL durability policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalSyncPolicy {
    /// Fsync every individual append.
    EveryWrite,

    /// Fsync at most once per batch interval.
    Batch(Duration),

    /// Only fsync when `Engine::sync` is called.
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
