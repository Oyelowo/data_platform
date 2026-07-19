//! Configuration options for the time-series engine.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Top-level options for [`TimeSeriesEngine`](crate::TimeSeriesEngine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesOptions {
    /// Optional retention policy applied during `sync`.
    pub retention: Option<RetentionPolicy>,

    /// Soft byte limit for the in-memory memtable before it is flushed to chunks.
    pub memtable_size_limit: usize,

    /// Target uncompressed size for a single chunk file in bytes.
    pub chunk_size_target: usize,

    /// Maximum number of decoded chunks kept in memory.
    pub max_open_chunks: usize,

    /// Compression codec used for values inside chunks.
    pub compression: CompressionKind,

    /// WAL durability policy.
    pub wal_sync_policy: WalSyncPolicy,

    /// Default value kind for samples inserted through the byte-key API.
    pub value_kind: ValueKind,

    /// Maximum length of a series key in bytes.
    pub max_key_len: usize,

    /// Maximum uncompressed chunk size in bytes; larger chunks are rejected.
    pub max_chunk_size: usize,
}

impl TimeSeriesOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if self.memtable_size_limit == 0 {
            return Err(crate::Error::invalid_argument(
                "memtable_size_limit must be greater than zero",
            ));
        }
        if self.chunk_size_target < 1024 {
            return Err(crate::Error::invalid_argument(
                "chunk_size_target must be at least 1024 bytes",
            ));
        }
        if self.max_open_chunks == 0 {
            return Err(crate::Error::invalid_argument(
                "max_open_chunks must be greater than zero",
            ));
        }
        if self.max_key_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_key_len must be greater than zero",
            ));
        }
        if self.max_chunk_size < self.chunk_size_target {
            return Err(crate::Error::invalid_argument(
                "max_chunk_size must be >= chunk_size_target",
            ));
        }
        Ok(())
    }

    /// Return conservative defaults for the given value kind.
    pub fn default_for(value_kind: ValueKind) -> Self {
        Self {
            retention: None,
            memtable_size_limit: 16 * 1024 * 1024,
            chunk_size_target: 64 * 1024,
            max_open_chunks: 128,
            compression: CompressionKind::Gorilla,
            wal_sync_policy: WalSyncPolicy::SyncOnEngineSync,
            value_kind,
            max_key_len: 4096,
            max_chunk_size: 16 * 1024 * 1024,
        }
    }
}

impl Default for TimeSeriesOptions {
    fn default() -> Self {
        Self::default_for(ValueKind::F64)
    }
}

/// Retention policy controlling how long samples are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetentionPolicy {
    /// Retain samples newer than `Duration` ago.
    Duration(Duration),
    /// Retain at most this many newest samples per series.
    MaxSamples(usize),
}

/// Kind of value stored in a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ValueKind {
    /// 64-bit floating point scalar.
    #[default]
    F64,
    /// Opaque byte payload.
    Bytes,
}

/// Compression kind for chunk values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CompressionKind {
    /// No compression.
    None,
    /// Gorilla XOR compression for `f64` values and delta-of-delta timestamps.
    #[default]
    Gorilla,
    /// Zstd block compression for byte payloads.
    Zstd,
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
