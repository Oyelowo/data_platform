//! Configuration options for the graph storage engine.

use std::time::Duration;

use serde::{Deserialize, Serialize};

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

/// Top-level options for [`GraphEngine`](crate::engine::GraphEngine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphOptions {
    /// Soft limit on the number of un-synced mutations before an automatic
    /// flush is triggered.
    pub max_unsynced_records: usize,

    /// WAL durability policy.
    pub wal_sync_policy: WalSyncPolicy,

    /// Maximum length of a node or edge id in bytes.
    pub max_key_len: usize,

    /// Maximum length of a single property value in bytes.
    pub max_value_len: usize,

    /// Maximum number of labels on a single node.
    pub max_labels: usize,

    /// Maximum length of a label string in bytes.
    pub max_label_len: usize,

    /// Trigger compaction when the ratio of deleted records to total records
    /// exceeds this threshold.
    pub compaction_threshold: f64,
}

impl GraphOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if self.max_unsynced_records == 0 {
            return Err(crate::Error::invalid_argument(
                "max_unsynced_records must be greater than zero",
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
        if self.max_labels == 0 {
            return Err(crate::Error::invalid_argument(
                "max_labels must be greater than zero",
            ));
        }
        if self.max_label_len == 0 {
            return Err(crate::Error::invalid_argument(
                "max_label_len must be greater than zero",
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

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            max_unsynced_records: 10_000,
            wal_sync_policy: WalSyncPolicy::default(),
            max_key_len: 4096,
            max_value_len: 1024 * 1024,
            max_labels: 64,
            max_label_len: 256,
            compaction_threshold: 0.25,
        }
    }
}
