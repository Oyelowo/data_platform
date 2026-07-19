//! Options for `ArtMap` and the durable `ArtEngine`.

use crate::node::MAX_KEY_LEN;

/// Options controlling `ArtMap` behavior and limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtMapOptions {
    /// Maximum key length in bytes.
    pub max_key_len: usize,
    /// Maximum value length in bytes.
    pub max_value_len: usize,
    /// Optional hard limit on the number of entries.
    pub max_entries: Option<usize>,
}

impl Default for ArtMapOptions {
    fn default() -> Self {
        Self {
            max_key_len: MAX_KEY_LEN,
            max_value_len: 8 * 1024 * 1024, // 8 MiB
            max_entries: None,
        }
    }
}

/// Durability policy for the write-ahead log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WalSyncPolicy {
    /// Wait for fsync on every write. Safest, slowest.
    #[default]
    Immediate,
    /// Append to the OS page cache and return. Callers must `sync` to guarantee
    /// durability; this batches concurrent writes into a single fsync.
    Buffered,
}

/// Options controlling the durable `ArtEngine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtEngineOptions {
    /// Options forwarded to the in-memory `ArtMap`.
    pub map: ArtMapOptions,
    /// WAL durability policy.
    pub wal_sync_policy: WalSyncPolicy,
    /// Maximum size of a single WAL segment.
    pub wal_segment_size: u64,
    /// If true, `Engine::sync` writes a snapshot and truncates the WAL.
    /// If false, `sync` only flushes the WAL.
    pub snapshot_on_sync: bool,
}

impl Default for ArtEngineOptions {
    fn default() -> Self {
        Self {
            map: ArtMapOptions::default(),
            wal_sync_policy: WalSyncPolicy::default(),
            wal_segment_size: storage_wal::WalOptions::DEFAULT_SEGMENT_SIZE,
            snapshot_on_sync: true,
        }
    }
}

impl ArtEngineOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if self.map.max_key_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_key_len must be greater than 0",
            ));
        }
        if self.map.max_value_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_value_len must be greater than 0",
            ));
        }
        if self.wal_segment_size < storage_wal::WalOptions::MIN_SEGMENT_SIZE {
            return Err(crate::Error::invalid_argument(format!(
                "wal_segment_size {} is below minimum {}",
                self.wal_segment_size,
                storage_wal::WalOptions::MIN_SEGMENT_SIZE
            )));
        }
        Ok(())
    }

    /// Convert to the underlying `storage_wal::WalOptions`.
    pub(crate) fn wal_options(&self) -> storage_wal::WalOptions {
        storage_wal::WalOptions {
            segment_size: self.wal_segment_size,
            durability: match self.wal_sync_policy {
                WalSyncPolicy::Immediate => storage_wal::Durability::Immediate,
                WalSyncPolicy::Buffered => storage_wal::Durability::Buffered,
            },
        }
    }
}
